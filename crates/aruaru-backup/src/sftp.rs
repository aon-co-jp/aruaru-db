//! SFTP-compatible backup destination (`BackupDestination::Sftp`) — closes
//! the second half of the "S3/SFTP destination not yet connected" gap
//! flagged in `CLAUDE.md`'s handoff notes (S3 was connected on 2026-07-12
//! in `s3.rs`; SFTP was explicitly scoped out of that pass and is
//! implemented here).
//!
//! **Design**: [`russh`](https://docs.rs/russh) is a pure-Rust SSH client
//! implementation (no `libssh2`/OpenSSL C dependency, unlike the `ssh2`
//! crate) — matching this workspace's existing preference for pure-Rust
//! crates over C-binding wrappers (e.g. `rustls` over openssl elsewhere in
//! this workspace's dependency table). [`russh_sftp`] layers the SFTP
//! subsystem protocol on top of a `russh` channel.
//!
//! Unlike `s3.rs`'s presigned-URL approach (stateless, one HTTP request
//! per operation), SFTP is a stateful protocol over a single SSH
//! connection. Rather than keeping a long-lived connection alive across
//! [`SftpClient`] method calls (more moving parts: reconnect-on-drop,
//! keepalives, concurrent-access rules), each public method here opens a
//! fresh SSH connection, does its one operation, and disconnects — a
//! deliberate simplicity/efficiency tradeoff appropriate for a backup
//! destination that's touched a handful of times per backup/restore run,
//! not a hot path.
//!
//! **Credentials**: like the S3 destination's AWS credentials, the actual
//! secret (a password, or an encrypted private key's passphrase) is never
//! stored in [`crate::BackupDestination::Sftp`]/[`SftpAuth`] — those get
//! serialized to config/manifest files — and is instead read from the
//! `SFTP_PASSWORD` / `SFTP_KEY_PASSPHRASE` environment variables at
//! connect time. A private key *path* is not itself a secret, so (like
//! the S3 destination's bucket/region/endpoint) it's fine to store
//! directly on [`SftpAuth::PrivateKey`].
//!
//! **Verification note**: this sandbox has no live SSH/SFTP server to
//! test against (same class of limitation as `s3.rs`'s live-S3-server
//! note, and `poem-cosmo-tauri`'s `RedisCache`). `SftpClient::new` and the
//! pure path-joining logic (`full_path`) are unit tested directly; the
//! functions that actually open a network connection
//! (`put_object`/`get_object`/`list_objects`) are not covered by an
//! automated test here.
//!
//! **Known limitation (host key verification)**: [`ClientHandler`]
//! currently accepts *any* server host key rather than checking it
//! against a known-hosts store — there's no host-key-pinning config
//! surfaced on [`crate::BackupDestination::Sftp`] yet. This is a real gap
//! (accepting any host key means no protection against a
//! man-in-the-middle on the first connection), recorded here honestly
//! rather than silently; adding a `known_hosts_path` or pinned
//! fingerprint field to the destination config is a reasonable follow-up.

use std::sync::Arc;

use russh::client::{self, AuthResult};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh_sftp::client::SftpSession;

use crate::SftpAuth;

/// Minimal `russh` client handler. The only behavior that matters for a
/// backup destination is host key handling -- see the module doc's "Known
/// limitation" note for why this currently accepts every server key.
struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Everything needed to talk to one SFTP-compatible remote directory: the
/// connection target (host/port/username, from
/// [`crate::BackupDestination::Sftp`]), how to authenticate ([`SftpAuth`]),
/// and the remote root directory that every key is relative to.
pub struct SftpClient {
    host: String,
    port: u16,
    username: String,
    auth: SftpAuth,
    /// Remote root directory (from `BackupDestination::Sftp::path`),
    /// trailing slashes stripped so [`Self::full_path`] can join it with
    /// exactly one `/`.
    remote_dir: String,
}

impl SftpClient {
    /// Build a client for `host:port`, authenticating as `username` via
    /// `auth`, with every key relative to `remote_dir`. Unlike
    /// [`crate::s3::S3Client::new`], this doesn't touch the environment or
    /// validate anything up front -- there's nothing to validate locally
    /// (no URL parsing, no bucket-name shape) until an actual connection
    /// is attempted, which only happens in `put_object`/`get_object`/
    /// `list_objects`.
    pub fn new(host: &str, port: u16, username: &str, remote_dir: &str, auth: &SftpAuth) -> anyhow::Result<Self> {
        Ok(Self {
            host: host.to_string(),
            port,
            username: username.to_string(),
            auth: auth.clone(),
            remote_dir: remote_dir.trim_end_matches('/').to_string(),
        })
    }

    /// Join this client's remote root with `key` (e.g.
    /// `"<backup_id>/MANIFEST.json"`), producing an absolute-or-relative
    /// remote path exactly one `/` apart -- mirrors
    /// [`crate::s3::S3Client::full_key`].
    fn full_path(&self, key: &str) -> String {
        if self.remote_dir.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.remote_dir, key)
        }
    }

    /// Open a fresh SSH connection, authenticate, and start an SFTP
    /// subsystem session on it. See the module doc for why this is done
    /// per-operation rather than once and reused.
    async fn connect(&self) -> anyhow::Result<SftpSession> {
        let config = Arc::new(client::Config::default());
        let mut session = client::connect(config, (self.host.as_str(), self.port), ClientHandler)
            .await
            .map_err(|e| anyhow::anyhow!("SFTP: failed to connect to {}:{}: {e}", self.host, self.port))?;

        let auth_result: AuthResult = match &self.auth {
            SftpAuth::Password => {
                let password = std::env::var("SFTP_PASSWORD").map_err(|_| {
                    anyhow::anyhow!("SFTP_PASSWORD is not set; required for password-authenticated SFTP backup destination")
                })?;
                session.authenticate_password(self.username.clone(), password).await?
            }
            SftpAuth::PrivateKey { key_path } => {
                let passphrase = std::env::var("SFTP_KEY_PASSPHRASE").ok();
                let key = load_secret_key(key_path, passphrase.as_deref())
                    .map_err(|e| anyhow::anyhow!("SFTP: failed to load private key {}: {e}", key_path.display()))?;
                let key = PrivateKeyWithHashAlg::new(Arc::new(key), None);
                session.authenticate_publickey(self.username.clone(), key).await?
            }
        };
        if !auth_result.success() {
            anyhow::bail!("SFTP: authentication rejected by {} for user {}", self.host, self.username);
        }

        let channel = session.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| anyhow::anyhow!("SFTP: failed to start SFTP subsystem: {e}"))?;
        Ok(sftp)
    }

    /// `mkdir -p`-equivalent: create every missing directory component of
    /// `dir`, one path segment at a time. SFTP servers generally return a
    /// generic "failure" status for `create_dir` on an already-existing
    /// directory (not a distinct "already exists" status this crate
    /// exposes), so a `create_dir` error here is treated as "probably
    /// already exists" rather than surfaced -- a genuine permissions/path
    /// problem still gets caught when the caller subsequently tries to
    /// actually read/write a file under this directory.
    async fn ensure_dir(sftp: &SftpSession, dir: &str) -> anyhow::Result<()> {
        if dir.is_empty() {
            return Ok(());
        }
        let mut built = if let Some(rest) = dir.strip_prefix('/') {
            let _ = rest;
            String::from("/")
        } else {
            String::new()
        };
        for segment in dir.split('/').filter(|s| !s.is_empty()) {
            if !built.is_empty() && !built.ends_with('/') {
                built.push('/');
            }
            built.push_str(segment);
            let _ = sftp.create_dir(built.clone()).await;
        }
        Ok(())
    }

    /// Upload `bytes` to `key` (relative to this client's remote root),
    /// creating any missing parent directories first.
    pub async fn put_object(&self, key: &str, bytes: Vec<u8>) -> anyhow::Result<()> {
        let sftp = self.connect().await?;
        let full_path = self.full_path(key);
        if let Some((dir, _)) = full_path.rsplit_once('/') {
            Self::ensure_dir(&sftp, dir).await?;
        }
        let result = sftp
            .write(full_path.clone(), &bytes)
            .await
            .map_err(|e| anyhow::anyhow!("SFTP PUT {key} failed: {e}"));
        let _ = sftp.close().await;
        result
    }

    /// Download the object at `key` (relative to this client's remote
    /// root).
    pub async fn get_object(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let sftp = self.connect().await?;
        let full_path = self.full_path(key);
        let result = sftp
            .read(full_path)
            .await
            .map_err(|e| anyhow::anyhow!("SFTP GET {key} failed: {e}"));
        let _ = sftp.close().await;
        result
    }

    /// List every object key under this client's remote root, one level
    /// of subdirectory deep (`<backup_id>/<file>`) -- mirrors the shape
    /// [`crate::s3::S3Client::list_objects`] returns (flat keys, not a
    /// recursive tree), which is all `list_backups` needs. Returns an
    /// empty list rather than an error if the remote root doesn't exist
    /// yet (no backups have been written there), matching the `Local`
    /// destination's `list_backups` behavior for a missing directory.
    pub async fn list_objects(&self) -> anyhow::Result<Vec<String>> {
        let sftp = self.connect().await?;

        let top_entries = match sftp.read_dir(self.remote_dir.clone()).await {
            Ok(entries) => entries,
            Err(_) => {
                let _ = sftp.close().await;
                return Ok(Vec::new());
            }
        };

        let subdirs: Vec<String> = top_entries
            .filter(|entry| entry.file_type().is_dir())
            .map(|entry| entry.file_name())
            .filter(|name| name != "." && name != "..")
            .collect();

        let mut keys = Vec::new();
        for subdir in subdirs {
            let subdir_path = self.full_path(&subdir);
            let Ok(entries) = sftp.read_dir(subdir_path).await else { continue };
            for entry in entries {
                if entry.file_type().is_file() {
                    keys.push(format!("{subdir}/{}", entry.file_name()));
                }
            }
        }

        let _ = sftp.close().await;
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_client() -> SftpClient {
        SftpClient::new("backup.example.com", 22, "aruaru", "/backups", &SftpAuth::Password).unwrap()
    }

    #[test]
    fn full_path_joins_remote_dir_and_key_with_exactly_one_slash() {
        let client = test_client();
        assert_eq!(client.full_path("backup-1/MANIFEST.json"), "/backups/backup-1/MANIFEST.json");
    }

    #[test]
    fn full_path_strips_trailing_slash_from_remote_dir() {
        let client = SftpClient::new("h", 22, "u", "/backups/", &SftpAuth::Password).unwrap();
        assert_eq!(client.full_path("x"), "/backups/x");
    }

    #[test]
    fn full_path_with_empty_remote_dir_is_passthrough() {
        let client = SftpClient::new("h", 22, "u", "", &SftpAuth::Password).unwrap();
        assert_eq!(client.full_path("x"), "x");
    }

    #[test]
    fn new_accepts_password_auth() {
        let client = SftpClient::new("h", 22, "u", "/backups", &SftpAuth::Password);
        assert!(client.is_ok());
    }

    #[test]
    fn new_accepts_private_key_auth() {
        let auth = SftpAuth::PrivateKey { key_path: PathBuf::from("/home/aruaru/.ssh/id_ed25519") };
        let client = SftpClient::new("h", 22, "u", "/backups", &auth);
        assert!(client.is_ok());
    }
}
