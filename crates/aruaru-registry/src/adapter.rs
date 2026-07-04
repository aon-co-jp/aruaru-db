//! 取り込み/接続アダプタ
//!
//! capability(ワイヤプロトコル)単位で実装する。
//! 1 つの実装で、その互換群すべて(数十DB)をカバーするのが狙い。
//!
//! - `PgWireAdapter`  : PostgreSQL ワイヤ互換 (実装済み・tokio-postgres)
//!   → CockroachDB / YugabyteDB / Redshift / AlloyDB / Materialize / Citus /
//!     Greenplum / RisingWave / QuestDB / CrateDB / Supabase / Neon … を一括カバー
//! - その他のワイヤ(MySQL/Mongo/...)は今後 capability ごとに追加。

use async_trait::async_trait;

/// 取り込み元のテーブル概要
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub estimated_rows: i64,
}

/// 接続テスト結果
#[derive(Debug, Clone)]
pub struct ConnTest {
    pub ok: bool,
    pub message: String,
    pub server_version: Option<String>,
}

/// 取り込み元アダプタの共通インタフェース
#[async_trait]
pub trait SourceAdapter: Send + Sync {
    /// この実装が対応するワイヤ名 (表示用)
    fn wire_name(&self) -> &'static str;

    /// 接続テスト
    async fn test(&self, uri: &str) -> ConnTest;

    /// テーブル一覧
    async fn list_tables(&self, uri: &str) -> anyhow::Result<Vec<TableInfo>>;

    /// 1 テーブルを文字列行で読み出す (列名, 行)
    async fn read_table(
        &self,
        uri: &str,
        schema: &str,
        table: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)>;
}

// ── PostgreSQL ワイヤ互換アダプタ (実接続) ──────────────────────

pub struct PgWireAdapter;

#[async_trait]
impl SourceAdapter for PgWireAdapter {
    fn wire_name(&self) -> &'static str {
        "PostgreSQL wire"
    }

    async fn test(&self, uri: &str) -> ConnTest {
        match tokio_postgres::connect(uri, tokio_postgres::NoTls).await {
            Ok((client, connection)) => {
                // connection は別タスクで駆動する必要がある
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                let version = client
                    .query_one("SHOW server_version", &[])
                    .await
                    .ok()
                    .and_then(|row| row.try_get::<_, String>(0).ok());
                ConnTest {
                    ok: true,
                    message: "接続成功".to_string(),
                    server_version: version,
                }
            }
            Err(e) => ConnTest {
                ok: false,
                message: format!("接続失敗: {e}"),
                server_version: None,
            },
        }
    }

    async fn list_tables(&self, uri: &str) -> anyhow::Result<Vec<TableInfo>> {
        let (client, connection) = tokio_postgres::connect(uri, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let rows = client
            .query(
                "SELECT schemaname, relname, n_live_tup \
                 FROM pg_stat_user_tables ORDER BY n_live_tup DESC",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| TableInfo {
                schema: r.get::<_, String>(0),
                name: r.get::<_, String>(1),
                estimated_rows: r.get::<_, i64>(2),
            })
            .collect())
    }

    async fn read_table(
        &self,
        uri: &str,
        schema: &str,
        table: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
        let (client, connection) = tokio_postgres::connect(uri, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });

        // 識別子はホワイトリスト的にクオート (簡易インジェクション対策)
        let safe = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !safe(schema) || !safe(table) {
            anyhow::bail!("不正な識別子: {schema}.{table}");
        }
        let sql = format!("SELECT * FROM \"{schema}\".\"{table}\" LIMIT {limit}");
        let rows = client.query(sql.as_str(), &[]).await?;

        let columns: Vec<String> = rows
            .first()
            .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
            .unwrap_or_default();

        // 値は文字列化 (型を問わず text 表現に寄せる)
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut vals = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                // text として取得を試み、無理なら型別フォールバック
                let v = row
                    .try_get::<_, Option<String>>(i)
                    .ok()
                    .flatten()
                    .or_else(|| row.try_get::<_, Option<i64>>(i).ok().flatten().map(|n| n.to_string()))
                    .or_else(|| row.try_get::<_, Option<f64>>(i).ok().flatten().map(|n| n.to_string()))
                    .or_else(|| row.try_get::<_, Option<bool>>(i).ok().flatten().map(|b| b.to_string()))
                    .unwrap_or_default();
                vals.push(v);
            }
            out.push(vals);
        }
        Ok((columns, out))
    }
}

/// ワイヤ種別から適切なアダプタを返す (未実装ワイヤは None)
pub fn adapter_for(wire: crate::types::Wire) -> Option<Box<dyn SourceAdapter>> {
    use crate::types::Wire;
    match wire {
        Wire::Postgres => Some(Box::new(PgWireAdapter)),
        Wire::MySQL => Some(Box::new(MySqlAdapter)),
        Wire::Mongo => Some(Box::new(MongoAdapter)),
        Wire::Cql => Some(Box::new(CqlAdapter)),
        _ => None,
    }
}

// ── MySQL ワイヤ互換アダプタ (実接続) ──────────────────────────
//
// MariaDB / TiDB / SingleStore / StarRocks / Apache Doris / Vitess /
// OceanBase / PolarDB / Percona / Aurora MySQL … を一括カバー。

pub struct MySqlAdapter;

/// mysql_async::Value を文字列へ
fn mysql_value_to_string(v: &mysql_async::Value) -> String {
    use mysql_async::Value as V;
    match v {
        V::NULL => String::new(),
        V::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        V::Int(n) => n.to_string(),
        V::UInt(n) => n.to_string(),
        V::Float(f) => f.to_string(),
        V::Double(d) => d.to_string(),
        V::Date(y, mo, d, h, mi, s, _us) => {
            format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
        }
        V::Time(neg, d, h, mi, s, _us) => {
            let sign = if *neg { "-" } else { "" };
            format!("{sign}{}:{h:02}:{mi:02}:{s:02}", *d * 24 + *h as u32)
        }
    }
}

#[async_trait]
impl SourceAdapter for MySqlAdapter {
    fn wire_name(&self) -> &'static str {
        "MySQL wire"
    }

    async fn test(&self, uri: &str) -> ConnTest {
        match mysql_async::Pool::from_url(uri) {
            Ok(pool) => match pool.get_conn().await {
                Ok(mut conn) => {
                    use mysql_async::prelude::Queryable;
                    let version: Option<String> =
                        conn.query_first("SELECT VERSION()").await.ok().flatten();
                    drop(conn);
                    let _ = pool.disconnect().await;
                    ConnTest {
                        ok: true,
                        message: "接続成功".to_string(),
                        server_version: version,
                    }
                }
                Err(e) => ConnTest {
                    ok: false,
                    message: format!("接続失敗: {e}"),
                    server_version: None,
                },
            },
            Err(e) => ConnTest {
                ok: false,
                message: format!("URI 解析失敗: {e}"),
                server_version: None,
            },
        }
    }

    async fn list_tables(&self, uri: &str) -> anyhow::Result<Vec<TableInfo>> {
        use mysql_async::prelude::Queryable;
        let pool = mysql_async::Pool::from_url(uri)?;
        let mut conn = pool.get_conn().await?;
        let rows: Vec<(String, String, Option<i64>)> = conn
            .query(
                "SELECT table_schema, table_name, table_rows \
                 FROM information_schema.tables \
                 WHERE table_type = 'BASE TABLE' \
                 ORDER BY table_rows DESC",
            )
            .await?;
        drop(conn);
        let _ = pool.disconnect().await;
        Ok(rows
            .into_iter()
            .map(|(schema, name, rows)| TableInfo {
                schema,
                name,
                estimated_rows: rows.unwrap_or(0),
            })
            .collect())
    }

    async fn read_table(
        &self,
        uri: &str,
        schema: &str,
        table: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
        use mysql_async::prelude::Queryable;
        use mysql_async::Row;

        let safe = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !safe(schema) || !safe(table) {
            anyhow::bail!("不正な識別子: {schema}.{table}");
        }

        let pool = mysql_async::Pool::from_url(uri)?;
        let mut conn = pool.get_conn().await?;
        let sql = format!("SELECT * FROM `{schema}`.`{table}` LIMIT {limit}");
        let rows: Vec<Row> = conn.query(sql).await?;
        drop(conn);
        let _ = pool.disconnect().await;

        let columns: Vec<String> = rows
            .first()
            .map(|r| r.columns_ref().iter().map(|c| c.name_str().to_string()).collect())
            .unwrap_or_default();

        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut vals = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                let v = row
                    .as_ref(i)
                    .map(mysql_value_to_string)
                    .unwrap_or_default();
                vals.push(v);
            }
            out.push(vals);
        }
        Ok((columns, out))
    }
}

// ── MongoDB アダプタ (実接続) ──────────────────────────────────
//
// MongoDB / Amazon DocumentDB / Azure Cosmos DB(Mongo API) をカバー。
// スキーマレスのため、コレクション=テーブル、ドキュメントの最上位キー=列とする。

pub struct MongoAdapter;

/// bson::Bson を文字列へ
fn bson_to_string(b: &mongodb::bson::Bson) -> String {
    use mongodb::bson::Bson;
    match b {
        Bson::String(s) => s.clone(),
        Bson::Int32(n) => n.to_string(),
        Bson::Int64(n) => n.to_string(),
        Bson::Double(d) => d.to_string(),
        Bson::Boolean(b) => b.to_string(),
        Bson::Null => String::new(),
        Bson::ObjectId(oid) => oid.to_hex(),
        other => other.to_string(), // 拡張JSON表現
    }
}

#[async_trait]
impl SourceAdapter for MongoAdapter {
    fn wire_name(&self) -> &'static str {
        "MongoDB wire"
    }

    async fn test(&self, uri: &str) -> ConnTest {
        match mongodb::Client::with_uri_str(uri).await {
            Ok(client) => match client.list_database_names(None, None).await {
                Ok(dbs) => ConnTest {
                    ok: true,
                    message: format!("接続成功 ({} データベース)", dbs.len()),
                    server_version: None,
                },
                Err(e) => ConnTest { ok: false, message: format!("接続失敗: {e}"), server_version: None },
            },
            Err(e) => ConnTest { ok: false, message: format!("URI 解析失敗: {e}"), server_version: None },
        }
    }

    async fn list_tables(&self, uri: &str) -> anyhow::Result<Vec<TableInfo>> {
        let client = mongodb::Client::with_uri_str(uri).await?;
        let db = client
            .default_database()
            .ok_or_else(|| anyhow::anyhow!("URI にデータベース名がありません"))?;
        let names = db.list_collection_names(None).await?;
        let mut out = Vec::new();
        for name in names {
            let coll = db.collection::<mongodb::bson::Document>(&name);
            let count = coll.estimated_document_count(None).await.unwrap_or(0);
            out.push(TableInfo {
                schema: db.name().to_string(),
                name,
                estimated_rows: count as i64,
            });
        }
        Ok(out)
    }

    async fn read_table(
        &self,
        uri: &str,
        _schema: &str,
        table: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
        use futures::stream::TryStreamExt;
        use mongodb::bson::Document;

        let client = mongodb::Client::with_uri_str(uri).await?;
        let db = client
            .default_database()
            .ok_or_else(|| anyhow::anyhow!("URI にデータベース名がありません"))?;
        let coll = db.collection::<Document>(table);

        let find_opts = mongodb::options::FindOptions::builder()
            .limit(limit as i64)
            .build();
        let mut cursor = coll.find(None, find_opts).await?;

        // 最上位キーの和集合を列にする
        let mut columns: Vec<String> = Vec::new();
        let mut docs: Vec<Document> = Vec::new();
        while let Some(doc) = cursor.try_next().await? {
            for k in doc.keys() {
                if !columns.iter().any(|c| c == k) {
                    columns.push(k.clone());
                }
            }
            docs.push(doc);
        }

        let rows: Vec<Vec<String>> = docs
            .iter()
            .map(|doc| {
                columns
                    .iter()
                    .map(|c| doc.get(c).map(bson_to_string).unwrap_or_default())
                    .collect()
            })
            .collect();

        Ok((columns, rows))
    }
}

// ── Cassandra / ScyllaDB (CQL) アダプタ (実接続) ────────────────
//
// Apache Cassandra / ScyllaDB をカバー。uri は "host:9042" 形式。

pub struct CqlAdapter;

#[async_trait]
impl SourceAdapter for CqlAdapter {
    fn wire_name(&self) -> &'static str {
        "CQL (Cassandra/Scylla)"
    }

    async fn test(&self, uri: &str) -> ConnTest {
        match scylla::SessionBuilder::new().known_node(uri).build().await {
            Ok(_session) => ConnTest {
                ok: true,
                message: "接続成功".to_string(),
                server_version: None,
            },
            Err(e) => ConnTest { ok: false, message: format!("接続失敗: {e}"), server_version: None },
        }
    }

    async fn list_tables(&self, uri: &str) -> anyhow::Result<Vec<TableInfo>> {
        let session = scylla::SessionBuilder::new().known_node(uri).build().await?;
        let rows = session
            .query("SELECT keyspace_name, table_name FROM system_schema.tables", &[])
            .await?
            .rows
            .unwrap_or_default();
        let mut out = Vec::new();
        for row in rows {
            let (ks, tbl) = row.into_typed::<(String, String)>()?;
            // システムキースペースは除外
            if ks.starts_with("system") {
                continue;
            }
            out.push(TableInfo {
                schema: ks,
                name: tbl,
                estimated_rows: 0, // CQL はテーブル行数の概算が安価に取れない
            });
        }
        Ok(out)
    }

    async fn read_table(
        &self,
        uri: &str,
        schema: &str,
        table: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
        let safe = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !safe(schema) || !safe(table) {
            anyhow::bail!("不正な識別子: {schema}.{table}");
        }
        let session = scylla::SessionBuilder::new().known_node(uri).build().await?;
        let cql = format!("SELECT * FROM {schema}.{table} LIMIT {limit}");
        let result = session.query(cql, &[]).await?;

        // 列名
        let columns: Vec<String> = result
            .col_specs
            .iter()
            .map(|c| c.name.clone())
            .collect();

        let rows = result.rows.unwrap_or_default();
        let out: Vec<Vec<String>> = rows
            .into_iter()
            .map(|row| {
                row.columns
                    .iter()
                    .map(|cell| match cell {
                        Some(v) => format!("{v:?}"), // CqlValue を Debug 表現で文字列化
                        None => String::new(),
                    })
                    .collect()
            })
            .collect();

        Ok((columns, out))
    }
}
