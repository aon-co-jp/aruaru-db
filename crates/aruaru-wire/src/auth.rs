//! 【第2層】相互認証: SCRAM-SHA-256パスワード認証
//!
//! クライアント証明書(mTLS)は`tls.rs`側(rustls WebPkiClientVerifier)で
//! 伝送路レベルで強制されるため、ここではユーザー知識ベースの認証
//! (SCRAM-SHA-256)のみを扱う。両方が有効な運用では、
//! 「クライアント証明書(所有)」+「SCRAMパスワード(知識)」の二要素になる。
//!
//! ユーザーストアは初期実装として環境変数 `ARUARU_USERS`
//! ("user1:pass1,user2:pass2") から読み込む簡易実装。
//! 本番運用では aruaru-core 側のユーザーテーブルに置き換える拡張ポイント。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rand::RngCore;

use pgwire::api::auth::scram::{gen_salted_password, SASLScramAuthStartupHandler};
use pgwire::api::auth::{AuthSource, DefaultServerParameterProvider, LoginInfo, Password};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};

/// RFC 5802 の推奨最小値
pub const SCRAM_ITERATIONS: usize = 4096;

fn random_salt() -> Vec<u8> {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    salt.to_vec()
}

fn auth_error(code: &str, message: String) -> PgWireError {
    PgWireError::UserError(Box::new(ErrorInfo::new(
        "FATAL".to_owned(),
        code.to_owned(),
        message,
    )))
}

/// ユーザー名→平文パスワードのシンプルなインメモリストア
pub struct AruaruAuthSource {
    users: HashMap<String, String>,
}

impl AruaruAuthSource {
    pub fn from_env() -> Self {
        let mut users = HashMap::new();
        if let Ok(raw) = std::env::var("ARUARU_USERS") {
            for pair in raw.split(',') {
                if let Some((user, pass)) = pair.split_once(':') {
                    users.insert(user.trim().to_string(), pass.trim().to_string());
                }
            }
        }
        if users.is_empty() {
            tracing::warn!(
                "ARUARU_USERS is not set (format: \"user:pass,user2:pass2\"); \
                 no users configured, all SCRAM logins will fail"
            );
        }
        Self { users }
    }
}

#[async_trait]
impl AuthSource for AruaruAuthSource {
    async fn get_password(&self, login: &LoginInfo) -> PgWireResult<Password> {
        let user = login
            .user()
            .ok_or_else(|| auth_error("28000", "no user provided in startup message".to_string()))?;
        let password = self.users.get(user).ok_or_else(|| {
            auth_error("28P01", format!("password authentication failed for user \"{user}\""))
        })?;
        let salt = random_salt();
        let hashed = gen_salted_password(password, &salt, SCRAM_ITERATIONS);
        Ok(Password::new(Some(salt), hashed))
    }
}

pub type AruaruStartupHandler =
    SASLScramAuthStartupHandler<AruaruAuthSource, DefaultServerParameterProvider>;

/// SCRAM-SHA-256スタートアップハンドラを構築する。
/// `cert_pem` を渡すとSCRAM-SHA-256-PLUS(TLSチャネルバインディング)が有効になる。
pub fn build_startup_handler(cert_pem: Option<&[u8]>) -> anyhow::Result<AruaruStartupHandler> {
    let mut handler = SASLScramAuthStartupHandler::new(
        Arc::new(AruaruAuthSource::from_env()),
        Arc::new(DefaultServerParameterProvider::default()),
    );
    handler.set_iterations(SCRAM_ITERATIONS);
    if let Some(cert) = cert_pem {
        handler
            .configure_certificate(cert)
            .map_err(|e| anyhow::anyhow!("failed to configure SCRAM channel binding: {e}"))?;
    }
    Ok(handler)
}
