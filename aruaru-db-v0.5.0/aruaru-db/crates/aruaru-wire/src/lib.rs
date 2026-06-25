//! aruaru-wire: PostgreSQL ワイヤプロトコルサーバ (pgwire)
//!
//! psql / DBeaver / Tableau / 各言語の PostgreSQL ドライバが
//! そのまま接続できる。Simple Query プロトコルを実装し、
//! Simple Query + Extended Query(プリペアドステートメント)に対応し、
//! クエリを aruaru-query の QueryEngine に委譲する。
//!
//! ## 対応 pgwire バージョン
//! `pgwire = "0.27"` を想定。pgwire は版間で trait 構成が変化するため、
//! 初回コンパイル時にハンドラ trait のシグネチャ微調整が必要な場合がある
//! (その際は docs.rs/pgwire の該当版を参照)。

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use tokio::net::TcpListener;

use pgwire::api::auth::noop::NoopStartupHandler;
use pgwire::api::portal::Portal;
use pgwire::api::query::{ExtendedQueryHandler, SimpleQueryHandler};
use pgwire::api::results::{
    DataRowEncoder, DescribePortalResponse, DescribeStatementResponse, FieldFormat, FieldInfo,
    QueryResponse, Response, Tag,
};
use pgwire::api::stmt::{QueryParser, StoredStatement};
use pgwire::api::{ClientInfo, PgWireServerHandlers, Type};
use pgwire::error::{PgWireError, PgWireResult};
use pgwire::tokio::process_socket;

use aruaru_query::{QueryEngine, QueryResponse as EngineResponse, Value};

/// pgwire サーバ設定
#[derive(Debug, Clone)]
pub struct WireServerConfig {
    pub bind_addr: String,
    pub database_name: String,
}

impl Default for WireServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:5432".to_string(),
            database_name: "aruaru".to_string(),
        }
    }
}

/// クエリ処理ハンドラ: QueryEngine を保持し、SQL を委譲する
pub struct AruaruHandler {
    engine: Arc<QueryEngine>,
}

impl AruaruHandler {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self { engine }
    }
}

/// エンジンのエラー文字列を pgwire エラーに変換
fn user_error(e: String) -> PgWireError {
    PgWireError::UserError(Box::new(pgwire::error::ErrorInfo::new(
        "ERROR".to_owned(),
        "42601".to_owned(), // syntax_error
        e,
    )))
}

#[async_trait]
impl SimpleQueryHandler for AruaruHandler {
    async fn do_query<'a, C>(
        &self,
        _client: &mut C,
        query: &'a str,
    ) -> PgWireResult<Vec<Response<'a>>>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        tracing::debug!(sql = query, "pgwire simple query");

        // 複数文 (; 区切り) に対応
        let mut responses = Vec::new();
        for stmt in query.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let resp = self
                .engine
                .execute_async(stmt)
                .await
                .map_err(user_error)?;
            responses.push(engine_to_pgwire(resp)?);
        }
        Ok(responses)
    }
}

// ── Extended Query (プリペアドステートメント) ───────────────────
//
// Parse/Bind/Describe/Execute に対応。プレースホルダ $1.. を
// テキスト表現で SQL に展開してエンジンへ委譲する。
//
// 注意 (v0.4 ベースライン):
//  - パラメータはテキスト形式として展開する (バイナリ形式は将来対応)
//  - Describe は動的スキーマのため空を返す (RowDescription は Execute 時に付与)
//  - pgwire 0.27 の trait シグネチャを想定。版差で要微調整。

/// SQL 文字列をそのまま保持する簡易パーサ
pub struct AruaruQueryParser;

#[async_trait]
impl QueryParser for AruaruQueryParser {
    type Statement = String;

    async fn parse_sql(&self, sql: &str, _types: &[Type]) -> PgWireResult<Self::Statement> {
        Ok(sql.to_string())
    }
}

/// SQL 中の最大プレースホルダ番号 ($1, $2, ...) を返す
fn max_placeholder(sql: &str) -> usize {
    let bytes = sql.as_bytes();
    let mut max = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let mut j = i + 1;
            let mut num = 0usize;
            let mut has = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num = num * 10 + (bytes[j] - b'0') as usize;
                has = true;
                j += 1;
            }
            if has && num > max {
                max = num;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    max
}

/// Portal のパラメータをテキストで取り出し、$N を SQL に展開する
fn substitute_params(sql: &str, portal: &Portal<String>) -> PgWireResult<String> {
    let n = max_placeholder(sql);
    let mut out = sql.to_string();
    // $10 が $1 にマッチしないよう、大きい番号から置換
    for idx in (1..=n).rev() {
        let val: Option<String> = portal
            .parameter::<String>(idx - 1, &Type::VARCHAR)
            .ok()
            .flatten();
        let literal = match val {
            Some(s) => format!("'{}'", s.replace('\'', "''")),
            None => "NULL".to_string(),
        };
        out = out.replace(&format!("${idx}"), &literal);
    }
    Ok(out)
}

#[async_trait]
impl ExtendedQueryHandler for AruaruHandler {
    type Statement = String;
    type QueryParser = AruaruQueryParser;

    fn query_parser(&self) -> Arc<Self::QueryParser> {
        Arc::new(AruaruQueryParser)
    }

    async fn do_query<'a, 'b: 'a, C>(
        &'b self,
        _client: &mut C,
        portal: &'a Portal<Self::Statement>,
        _max_rows: usize,
    ) -> PgWireResult<Response<'a>>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        let template = portal.statement.statement.clone();
        let sql = substitute_params(&template, portal)?;
        tracing::debug!(sql = %sql, "pgwire extended query");
        let resp = self.engine.execute_async(&sql).await.map_err(user_error)?;
        engine_to_pgwire(resp)
    }

    async fn do_describe_statement<C>(
        &self,
        _client: &mut C,
        _stmt: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        // 動的スキーマのためパラメータ型・列は未確定で返す
        Ok(DescribeStatementResponse::new(vec![], vec![]))
    }

    async fn do_describe_portal<C>(
        &self,
        _client: &mut C,
        _portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        // 列は Execute 時の RowDescription で確定する
        Ok(DescribePortalResponse::new(vec![]))
    }
}

/// EngineResponse を pgwire の Response に変換
fn engine_to_pgwire<'a>(resp: EngineResponse) -> PgWireResult<Response<'a>> {
    match resp {
        EngineResponse::Command { tag } => {
            // "INSERT 0 1" / "CREATE TABLE" などをタグに
            Ok(Response::Execution(Tag::new(&tag)))
        }
        EngineResponse::Rows { columns, rows } => {
            // フィールド定義 (すべて TEXT 型として返す簡易版)
            let fields: Vec<FieldInfo> = columns
                .iter()
                .map(|name| {
                    FieldInfo::new(
                        name.clone(),
                        None,
                        None,
                        Type::VARCHAR,
                        FieldFormat::Text,
                    )
                })
                .collect();
            let fields = Arc::new(fields);

            // 各行をエンコード
            let schema_ref = fields.clone();
            let data_rows = rows.into_iter().map(move |row| {
                let mut encoder = DataRowEncoder::new(schema_ref.clone());
                for value in &row {
                    let encoded = match value {
                        Value::Null => encoder.encode_field(&None::<&str>),
                        v => encoder.encode_field(&Some(v.as_text())),
                    };
                    if let Err(e) = encoded {
                        return Err(e);
                    }
                }
                encoder.finish()
            });

            let row_stream = stream::iter(data_rows);
            Ok(Response::Query(QueryResponse::new(fields, row_stream)))
        }
    }
}

/// PgWireServerHandlers 実装: 各種ハンドラをまとめて提供する
pub struct AruaruHandlerFactory {
    handler: Arc<AruaruHandler>,
    startup: Arc<NoopStartupHandler>,
}

impl AruaruHandlerFactory {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self {
            handler: Arc::new(AruaruHandler::new(engine)),
            startup: Arc::new(NoopStartupHandler),
        }
    }
}

impl PgWireServerHandlers for AruaruHandlerFactory {
    type StartupHandler = NoopStartupHandler;
    type SimpleQueryHandler = AruaruHandler;
    // Extended Query (プリペアドステートメント) も同じハンドラで処理
    type ExtendedQueryHandler = AruaruHandler;
    type CopyHandler = pgwire::api::copy::NoopCopyHandler;
    type ErrorHandler = pgwire::api::NoopErrorHandler;

    fn simple_query_handler(&self) -> Arc<Self::SimpleQueryHandler> {
        self.handler.clone()
    }

    fn extended_query_handler(&self) -> Arc<Self::ExtendedQueryHandler> {
        self.handler.clone()
    }

    fn startup_handler(&self) -> Arc<Self::StartupHandler> {
        self.startup.clone()
    }

    fn copy_handler(&self) -> Arc<Self::CopyHandler> {
        Arc::new(pgwire::api::copy::NoopCopyHandler)
    }

    fn error_handler(&self) -> Arc<Self::ErrorHandler> {
        Arc::new(pgwire::api::NoopErrorHandler)
    }
}

/// pgwire サーバ起動。指定エンジンに接続を委譲する。
pub async fn start_wire_server(
    config: WireServerConfig,
    engine: Arc<QueryEngine>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(
        addr = %config.bind_addr,
        db   = %config.database_name,
        "PostgreSQL wire server listening"
    );

    let factory = Arc::new(AruaruHandlerFactory::new(engine));

    loop {
        let (socket, peer) = listener.accept().await?;
        let factory = factory.clone();
        tracing::debug!(?peer, "client connected");
        tokio::spawn(async move {
            if let Err(e) = process_socket(socket, None, factory).await {
                tracing::error!("connection error: {e}");
            }
        });
    }
}
