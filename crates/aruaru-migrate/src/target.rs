//! 移行先 (aruaru-DB / PostgreSQL ワイヤ互換) への接続・取り込みヘルパ。
//!
//! `MigrateConfig::target_uri` は aruaru-DB (または他の PostgreSQL ワイヤ
//! 互換サーバ) を指す。tokio-postgres クライアントで接続し、テーブルごとに
//! `CREATE TABLE IF NOT EXISTS` → 行単位 `INSERT` で取り込む。
//! aruaru-query の SQL サブセットは全列 TEXT 前提のため、型変換は行わない。

use crate::sql_build::{build_create_table_sql, build_insert_sql};

/// 移行先への接続。テーブル作成・行挿入を行う薄いラッパ。
pub struct TargetClient {
    client: tokio_postgres::Client,
}

impl TargetClient {
    /// `target_uri` へ接続する。コネクションを駆動するバックグラウンドタスクを spawn する。
    pub async fn connect(target_uri: &str) -> anyhow::Result<Self> {
        let (client, connection) =
            tokio_postgres::connect(target_uri, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "aruaru-migrate: target connection closed with error");
            }
        });
        Ok(Self { client })
    }

    /// テーブルが無ければ作成する (全列 TEXT 前提)
    pub async fn ensure_table(&self, table: &str, columns: &[String]) -> anyhow::Result<()> {
        let sql = build_create_table_sql(table, columns);
        self.client.batch_execute(&sql).await?;
        Ok(())
    }

    /// 1行を挿入する
    pub async fn insert_row(&self, table: &str, row: &[String]) -> anyhow::Result<()> {
        let sql = build_insert_sql(table, row);
        self.client.batch_execute(&sql).await?;
        Ok(())
    }

    /// 複数行をまとめて挿入する。行ごとの失敗はログに残しつつ継続し、
    /// 成功した行数を返す。
    pub async fn insert_rows(&self, table: &str, rows: &[Vec<String>]) -> usize {
        let mut ok = 0;
        for row in rows {
            match self.insert_row(table, row).await {
                Ok(()) => ok += 1,
                Err(e) => {
                    tracing::warn!(table, error = %e, "aruaru-migrate: row insert failed")
                }
            }
        }
        ok
    }
}
