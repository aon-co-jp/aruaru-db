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

pub mod auth;
pub mod payload_crypto;
pub mod tls;

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
use pgwire::api::{ClientInfo, PgWireHandlerFactory, Type};
use pgwire::error::{PgWireError, PgWireResult};
use pgwire::tokio::process_socket;

use aruaru_query::{parser, QueryEngine, QueryResponse as EngineResponse, Value};

/// pgwire サーバ設定
#[derive(Clone)]
pub struct WireServerConfig {
    pub bind_addr: String,
    pub database_name: String,
    /// 【第1層】伝送路暗号化。None の場合は平文TCP(開発用、警告ログを出す)。
    pub tls: Option<tls::TlsConfig>,
    /// 【課金アイテムの権利消失防止】設定時、書き込み文はRaft経由の
    /// 過半数コミットを待ってからACKを返す。
    pub replicator: Option<Arc<dyn aruaru_dist::ReplicatedWriter>>,
}

impl std::fmt::Debug for WireServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WireServerConfig")
            .field("bind_addr", &self.bind_addr)
            .field("database_name", &self.database_name)
            .field("tls", &self.tls)
            .field("replicator", &self.replicator.is_some())
            .finish()
    }
}

impl Default for WireServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:5432".to_string(),
            database_name: "aruaru".to_string(),
            replicator: None,
            tls: None,
        }
    }
}

/// クエリ処理ハンドラ: QueryEngine を保持し、SQL を委譲する。
/// 【課金アイテムの権利消失防止】`replicator` が設定されている場合、
/// 書き込み文(SELECT以外)はRaft経由で3ノード複製・過半数コミットされる
/// まで待ってからクライアントへACKを返す。
pub struct AruaruHandler {
    engine: Arc<QueryEngine>,
    replicator: Option<Arc<dyn aruaru_dist::ReplicatedWriter>>,
}

impl AruaruHandler {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self { engine, replicator: None }
    }

    pub fn with_replicator(mut self, replicator: Arc<dyn aruaru_dist::ReplicatedWriter>) -> Self {
        self.replicator = Some(replicator);
        self
    }

    /// 1文を実行する。書き込み文かつレプリケータ設定済みならRaft経由、
    /// それ以外(SELECT等の読み取り、またはレプリケータ未設定)はローカル直接実行。
    async fn execute_one(&self, stmt: &str) -> Result<EngineResponse, String> {
        if is_write_statement(stmt) {
            if let Some(replicator) = &self.replicator {
                let tag = replicator.write_sql(stmt).await?;
                return Ok(EngineResponse::Command { tag });
            }
        }
        self.engine.execute_async(stmt).await
    }
}

/// SELECT以外を書き込み文とみなす簡易判定。
/// 書き込み文はレプリケータ経由(設定時)でRaft複製・過半数コミットを待つ対象になる。
fn is_write_statement(stmt: &str) -> bool {
    !stmt
        .trim_start()
        .get(..6)
        .map(|s| s.eq_ignore_ascii_case("select"))
        .unwrap_or(false)
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
            let resp = self.execute_one(stmt).await.map_err(user_error)?;
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

/// テキストリテラルとして安全にクォートする。
/// シングルクォートは `''` に二重化し、NULバイトは除去する
/// (下流の軽量パーサ・ストレージ層での解釈違いによる抜け道を防ぐ)。
/// このエンジンの SQL サブセットパーサはバックスラッシュを
/// エスケープ文字として扱わないため、バックスラッシュは変更しない。
fn quote_text_literal(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('\'');
    for ch in s.chars() {
        match ch {
            '\'' => escaped.push_str("''"),
            '\0' => {} // NULバイトは無害化のため除去
            other => escaped.push(other),
        }
    }
    escaped.push('\'');
    escaped
}

/// 数値として安全な文字列か検証する (符号・数字・小数点・指数表記のみ許可)。
/// SQLメタ文字を一切含まないことを保証してから初めて非クォートで埋め込む。
fn is_safe_numeric_literal(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars().peekable();
    if matches!(chars.peek(), Some('+') | Some('-')) {
        chars.next();
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    let mut saw_exp = false;
    while let Some(&c) = chars.peek() {
        match c {
            '0'..='9' => {
                saw_digit = true;
                chars.next();
            }
            '.' if !saw_dot && !saw_exp => {
                saw_dot = true;
                chars.next();
            }
            'e' | 'E' if !saw_exp && saw_digit => {
                saw_exp = true;
                chars.next();
                if matches!(chars.peek(), Some('+') | Some('-')) {
                    chars.next();
                }
            }
            _ => return false,
        }
    }
    saw_digit
}

/// パラメータ1個分を、宣言された型に応じたSQLリテラルに変換する。
/// pgwire 0.27 の `Portal::parameter::<T>()` はバイナリ形式前提のデコードしか
/// 行わないため(テキスト形式のパラメータで誤動作しうる)、生バイト列を自前で
/// UTF-8テキストとして読み、型ごとに安全性を検証してから整形する。
fn format_param_literal(raw: &Option<bytes::Bytes>, pg_type: &Type) -> String {
    let Some(bytes) = raw else {
        return "NULL".to_string();
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            // 非UTF-8 (真のバイナリ形式パラメータ)。安全側に倒し、
            // ロスレスではないがクォート済みテキストとして扱う。
            let lossy = String::from_utf8_lossy(bytes).into_owned();
            return quote_text_literal(&lossy);
        }
    };

    match *pg_type {
        Type::BOOL => match text {
            "t" | "true" | "TRUE" | "1" => "TRUE".to_string(),
            "f" | "false" | "FALSE" | "0" => "FALSE".to_string(),
            other => quote_text_literal(other),
        },
        Type::INT2 | Type::INT4 | Type::INT8 | Type::FLOAT4 | Type::FLOAT8 | Type::NUMERIC => {
            if is_safe_numeric_literal(text) {
                text.to_string()
            } else {
                quote_text_literal(text)
            }
        }
        _ => quote_text_literal(text),
    }
}

/// Portal のパラメータを取り出し、$N を SQL に展開する。
/// 【SQLインジェクション対策】型ごとの安全なリテラル化を行う (暫定強化。
/// 真のプリペアドバインディングは follow-up 対応)。
fn substitute_params(sql: &str, portal: &Portal<String>) -> PgWireResult<String> {
    let n = max_placeholder(sql);
    let mut out = sql.to_string();
    // $10 が $1 にマッチしないよう、大きい番号から置換
    for idx in (1..=n).rev() {
        let zero_idx = idx - 1;
        let pg_type = portal
            .statement
            .parameter_types
            .get(zero_idx)
            .cloned()
            .unwrap_or(Type::UNKNOWN);
        let raw = portal.parameters.get(zero_idx).cloned().flatten();
        let literal = format_param_literal(&raw, &pg_type);
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
        let resp = self.execute_one(&sql).await.map_err(user_error)?;
        engine_to_pgwire(resp)
    }

    /// **拡張プロトコル(prepared statement)対応の実用性ギャップを解消**
    /// (open-web-server連携の実用性調査で指摘、2026-07-14)。
    ///
    /// このサーバーは動的スキーマ(テーブル定義がクライアントの
    /// `CREATE TABLE`次第で変わる)のため、以前は`Describe`で列情報を
    /// 一切返せず常に空列リストだった。psqlはSimple Queryプロトコルを
    /// 使うため影響しないが、`sqlx`をはじめ多くのORM/ドライバは既定で
    /// Extendedプロトコルを使い、`Parse`直後(パラメータがまだ`Bind`
    /// されていない段階)に送る`Describe(Statement)`の応答で得た列形状を
    /// 使って後続の`Execute`結果をデコードするため、行データを持つ
    /// SELECTは`ColumnIndexOutOfBounds`で必ず失敗していた(この
    /// ワークスペースの`AruaruDbBackend`自身が`raw_sql`=Simple Query
    /// プロトコルへ回避策として切り替えていた実例が、まさにこの制約の
    /// 証拠)。
    ///
    /// **修正方針**: クエリを実行せず、`aruaru_query::parser::parse`の
    /// 構文解析結果と`QueryEngine::table_columns`(テーブルスキーマの
    /// 参照のみ、行は読まない)だけから列名を解決する
    /// (`describe_columns`)。`Bind`前で実パラメータ値が無くても動作
    /// する(列名はWHERE句の値に依存しないため)——**副作用の心配が
    /// そもそも無い設計**であり、実行を伴う代替案(クエリを1回
    /// 事前実行して列を確定する等)より安全。書き込み文やGit-on-SQL
    /// 関数呼び出し(`Statement::AruaruFn`——`SELECT aruaru_commit(...)`
    /// のように構文上は`SELECT`で始まるが実際には副作用を持つ)は
    /// `describe_columns`が`None`を返し空列リストのまま
    /// (コマンドタグのみの応答なので実害無し)。
    async fn do_describe_statement<C>(
        &self,
        _client: &mut C,
        stmt: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        let fields = describe_columns(&self.engine, &stmt.statement).unwrap_or_default();
        let field_infos: Vec<FieldInfo> = fields
            .into_iter()
            .map(|name| FieldInfo::new(name, None, None, Type::VARCHAR, FieldFormat::Text))
            .collect();
        // パラメータ型は動的型付けのため引き続き未確定(空)のまま返す。
        Ok(DescribeStatementResponse::new(vec![], field_infos))
    }

    /// `Describe(Portal)`(`Bind`後、実パラメータ値が判明している段階)。
    /// 列名の解決ロジックは`do_describe_statement`と同一
    /// (`describe_columns`、パラメータ値には依存しない)——`Bind`前後
    /// どちらでDescribeが呼ばれても正しい応答を返せる。
    async fn do_describe_portal<C>(
        &self,
        _client: &mut C,
        portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        let fields = describe_columns(&self.engine, &portal.statement.statement).unwrap_or_default();
        let field_infos: Vec<FieldInfo> = fields
            .into_iter()
            .map(|name| FieldInfo::new(name, None, None, Type::VARCHAR, FieldFormat::Text))
            .collect();
        Ok(DescribePortalResponse::new(field_infos))
    }
}

/// `sql`(バインド前の生テンプレート、`$1`等のプレースホルダを含んでも
/// よい——列名はパラメータ値に依存しないため)が返す列名を、**クエリを
/// 実行せずに**解決する。`aruaru_query::parser::parse`の構文解析結果と
/// `QueryEngine::table_columns`(スキーマ参照のみ)だけから求める。
///
/// 副作用の有無で判定を分けているわけではなく、そもそも実行しないため
/// 副作用の心配が構造的に存在しない。書き込み文・Git-on-SQL関数呼び出し
/// (`AruaruFn`)・未知テーブルへの`SELECT *`など、列名を静的に解決
/// できない場合は`None`(呼び出し側は空列リストにフォールバック)。
fn describe_columns(engine: &QueryEngine, sql: &str) -> Option<Vec<String>> {
    match parser::parse(sql).ok()? {
        parser::Statement::Select { table, columns, .. } => match columns {
            Some(cols) => Some(cols),
            None => engine.table_columns(&table),
        },
        // `select_as_of`は要求された列リストを無視し常にテーブルの
        // フルROWを返す(`AruaruDbBackend::get_at_commit`のdoc comment
        // 参照)ため、実行時と同じ列形状(テーブル全列)を返す。
        parser::Statement::SelectAsOf { table, .. } => engine.table_columns(&table),
        parser::Statement::AruaruLog { .. } => Some(vec![
            "commit_id".to_string(),
            "author".to_string(),
            "message".to_string(),
            "timestamp".to_string(),
        ]),
        // Git-on-SQL関数呼び出し。副作用を持つため実行はしないが、
        // 各関数が返す列の形は`aruaru_query::engine::QueryEngine::
        // aruaru_fn`にハードコードされた固定の単一列
        // (`columns: vec!["<関数名>".into()]`)であり、実行結果を見ずに
        // 静的に分かる——それをそのままミラーする。
        parser::Statement::AruaruFn { name, .. }
            if matches!(name.as_str(), "aruaru_branch" | "aruaru_checkout" | "aruaru_commit" | "aruaru_merge") =>
        {
            Some(vec![name])
        }
        _ => None,
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

/// 開発用: 認証なしスタートアップハンドラ (ARUARU_USERS未設定時のフォールバックには使わない。
/// 明示的にauthを無効化したいテスト・ローカル検証専用)
pub struct AruaruNoopStartupHandler;

impl NoopStartupHandler for AruaruNoopStartupHandler {}

/// PgWireHandlerFactory 実装: 各種ハンドラをまとめて提供する。
/// 【第2層】起動時に SCRAM-SHA-256 スタートアップハンドラを構築する。
pub struct AruaruHandlerFactory {
    handler: Arc<AruaruHandler>,
    /// SCRAM-SHA-256-PLUS (TLSチャネルバインディング) 用のサーバ証明書 (PEM)
    tls_cert_pem: Option<Vec<u8>>,
}

impl AruaruHandlerFactory {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self {
            handler: Arc::new(AruaruHandler::new(engine)),
            tls_cert_pem: None,
        }
    }

    /// TLS証明書(PEM)を紐付ける。SCRAM-SHA-256-PLUSのチャネルバインディングに使う。
    pub fn with_tls_cert(mut self, cert_pem: Vec<u8>) -> Self {
        self.tls_cert_pem = Some(cert_pem);
        self
    }

    /// 【課金アイテムの権利消失防止】Raft複製書き込みレプリケータを紐付ける。
    /// 設定すると、SELECT以外の書き込み文はRaft経由の過半数コミットを待ってから
    /// クライアントへACKを返すようになる。construct直後 (他にArcの参照が
    /// 増える前) に呼ぶこと。
    pub fn with_replicator(mut self, replicator: Arc<dyn aruaru_dist::ReplicatedWriter>) -> Self {
        match Arc::get_mut(&mut self.handler) {
            Some(handler) => handler.replicator = Some(replicator),
            None => panic!("with_replicator must be called before the handler is shared"),
        }
        self
    }
}

impl PgWireHandlerFactory for AruaruHandlerFactory {
    type StartupHandler = auth::AruaruStartupHandler;
    type SimpleQueryHandler = AruaruHandler;
    // Extended Query (プリペアドステートメント) も同じハンドラで処理
    type ExtendedQueryHandler = AruaruHandler;
    type CopyHandler = pgwire::api::copy::NoopCopyHandler;

    fn simple_query_handler(&self) -> Arc<Self::SimpleQueryHandler> {
        self.handler.clone()
    }

    fn extended_query_handler(&self) -> Arc<Self::ExtendedQueryHandler> {
        self.handler.clone()
    }

    fn startup_handler(&self) -> Arc<Self::StartupHandler> {
        let handler = auth::build_startup_handler(self.tls_cert_pem.as_deref())
            .expect("failed to build SCRAM startup handler");
        Arc::new(handler)
    }

    fn copy_handler(&self) -> Arc<Self::CopyHandler> {
        Arc::new(pgwire::api::copy::NoopCopyHandler)
    }
}

/// pgwire サーバ起動。指定エンジンに接続を委譲する。
pub async fn start_wire_server(
    config: WireServerConfig,
    engine: Arc<QueryEngine>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&config.bind_addr).await?;

    let mut factory = AruaruHandlerFactory::new(engine);
    if let Some(replicator) = config.replicator.clone() {
        factory = factory.with_replicator(replicator);
    }

    let tls_acceptor = match &config.tls {
        Some(tls_config) => {
            tracing::info!(
                addr = %config.bind_addr,
                db   = %config.database_name,
                mtls = tls_config.client_ca_path.is_some(),
                "PostgreSQL wire server listening (TLS enabled)"
            );
            // SCRAM-SHA-256-PLUS のチャネルバインディング用にサーバ証明書を渡す
            if let Ok(cert_pem) = std::fs::read(&tls_config.cert_path) {
                factory = factory.with_tls_cert(cert_pem);
            }
            Some(Arc::new(tls::build_tls_acceptor(tls_config)?))
        }
        None => {
            tracing::warn!(
                addr = %config.bind_addr,
                db   = %config.database_name,
                "PostgreSQL wire server listening WITHOUT TLS (plaintext) — development use only, do not expose to untrusted networks"
            );
            None
        }
    };

    let factory = Arc::new(factory);

    loop {
        let (socket, peer) = listener.accept().await?;
        let factory = factory.clone();
        let tls_acceptor = tls_acceptor.clone();
        tracing::debug!(?peer, "client connected");
        tokio::spawn(async move {
            if let Err(e) = process_socket(socket, tls_acceptor, factory).await {
                tracing::error!("connection error: {e}");
            }
        });
    }
}

#[cfg(test)]
mod injection_tests {
    use super::*;

    #[test]
    fn test_quote_text_literal_escapes_single_quote() {
        assert_eq!(
            quote_text_literal("it's a trap"),
            "'it''s a trap'"
        );
    }

    #[test]
    fn test_quote_text_literal_strips_nul_byte() {
        assert_eq!(quote_text_literal("a\0b"), "'ab'");
    }

    #[test]
    fn test_classic_injection_payload_is_neutralized() {
        let payload = "'; DROP TABLE users; --";
        let literal = format_param_literal(&Some(bytes::Bytes::from(payload)), &Type::VARCHAR);
        // 埋め込み後も単一のクォート済み文字列リテラルのままであること
        // (シングルクォートは全て '' に二重化され、文の外に脱出できない)
        assert_eq!(literal, "'''; DROP TABLE users; --'");
    }

    #[test]
    fn test_numeric_literal_is_unquoted_when_safe() {
        let literal = format_param_literal(&Some(bytes::Bytes::from("123")), &Type::INT4);
        assert_eq!(literal, "123");
    }

    #[test]
    fn test_numeric_type_with_injection_payload_falls_back_to_quoted() {
        let payload = "1; DROP TABLE users; --";
        let literal = format_param_literal(&Some(bytes::Bytes::from(payload)), &Type::INT4);
        assert!(literal.starts_with('\'') && literal.ends_with('\''));
        assert!(!literal.contains("; DROP") || literal.starts_with('\''));
    }

    #[test]
    fn test_bool_literal() {
        assert_eq!(
            format_param_literal(&Some(bytes::Bytes::from("true")), &Type::BOOL),
            "TRUE"
        );
        assert_eq!(
            format_param_literal(&Some(bytes::Bytes::from("f")), &Type::BOOL),
            "FALSE"
        );
    }

    #[test]
    fn test_null_parameter() {
        assert_eq!(format_param_literal(&None, &Type::VARCHAR), "NULL");
    }
}
