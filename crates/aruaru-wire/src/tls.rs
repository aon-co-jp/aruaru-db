//! TLS証明書ロード・rustls::ServerConfig構築ヘルパー
//!
//! TCPリスナー・QUICリスナーの両方から共通で使う。
//! 【第2層】相互認証の一部として、クライアント証明書検証(mTLS)にも対応する。

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use rustls_pemfile::{certs, pkcs8_private_keys};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// TLS設定。`client_ca_path` を指定するとmTLS(クライアント証明書必須)になる。
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
    pub client_ca_path: Option<String>,
}

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).map_err(|e| anyhow::anyhow!("failed to open cert {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("failed to parse cert {path}: {e}"))
}

fn load_private_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let file = File::open(path).map_err(|e| anyhow::anyhow!("failed to open key {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    let mut keys = pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("failed to parse private key {path}: {e}"))?;
    if keys.is_empty() {
        anyhow::bail!("no PKCS8 private key found in {path}");
    }
    Ok(PrivateKeyDer::from(keys.remove(0)))
}

/// rustls::ServerConfig を構築する (TCP+TLS・QUIC共用)。
/// `client_ca_path` が設定されている場合はクライアント証明書の提示を必須化する(mTLS)。
pub fn build_server_config(tls: &TlsConfig) -> anyhow::Result<rustls::ServerConfig> {
    let cert_chain = load_certs(&tls.cert_path)?;
    let key = load_private_key(&tls.key_path)?;

    let builder = rustls::ServerConfig::builder();

    let mut config = if let Some(ca_path) = &tls.client_ca_path {
        let ca_certs = load_certs(ca_path)?;
        let mut roots = rustls::RootCertStore::empty();
        for c in ca_certs {
            roots.add(c)?;
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|e| anyhow::anyhow!("client cert verifier build failed: {e}"))?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert_chain, key)?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)?
    };

    // QUIC(0-RTT)は非冪等なStartupメッセージ(認証)には使わせない設計のため、
    // early dataはトランスポート層では常に許可しつつ、認証ハンドラ側で
    // early-dataストリームでのStartupメッセージ処理を制御する(auth.rs参照)。
    config.max_early_data_size = u32::MAX;

    Ok(config)
}

/// TCP用 TlsAcceptor を構築する。
pub fn build_tls_acceptor(tls: &TlsConfig) -> anyhow::Result<tokio_rustls::TlsAcceptor> {
    let config = build_server_config(tls)?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}
