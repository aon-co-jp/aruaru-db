//! スタンドアロンのメール・ディザスタバックアップ(`disaster_email_backup`
//! feature、任意有効化)。
//!
//! **設計方針(ユーザー指示、2026-07-25)**: VPS間の分散同期(`multi_raft`の
//! 複数ノード構成)・ZFSスナップショット連携(`snapshot_pairing`、
//! `open_raid_z`機能)のいずれも設定していない状態でも、**メールアドレス
//! ひとつだけ**で有効化できる、最小構成のディザスタ・セーフティネットを
//! 提供する。SATA/USB/LAN/WiFi等の物理的な断線・ネットワーク障害により、
//! Raft複製書き込み(`RaftWriter::write_sql`/`write_commit`)が過半数コミット
//! に到達できず失敗した場合に、**失われかけている書き込みコマンド自体を
//! メールで退避する**、最後の砦。
//!
//! **再利用方針(車輪の再発明をしない)**: メール送信ロジック自体は
//! 姉妹リポジトリ`open-raid-z`が実装・テスト済みの
//! `open_raid_z_core::offsite_backup::EmailBackupTarget`をそのまま
//! path依存で再利用する(`open-web-server`側の
//! `open-web-server-ledger::disaster_email_backup`と同じ構成)。
//! このモジュールが新規に持つのは、(a) `aruaru-dist`固有の型
//! (`raft::Command`)をバックアップセグメントへ変換する薄い橋渡し、
//! (b) 失敗を握りつぶさず記録するベストエフォートの送信ラッパー、の
//! 2点のみ。
//!
//! **正直な開示**: (1) 実SMTPサーバー・実メールアカウントへの接続は
//! このモジュールのテストでは一切行っていない(`open-raid-z`側の
//! `tests/offsite_backup_integration.rs`と同じ「ローカルモックSMTPのみ」
//! 方針)。(2) 実際の物理断線(SATA/USB/LAN/WiFiケーブル抜去)を検知する
//! 専用のハードウェアイベントフックはこのリポジトリには無い——
//! `RaftWriter::propose_and_wait`が過半数コミット待ち(`wait_for_commit`)
//! でタイムアウト・失敗した時点を「断線・障害相当」のシグナルとして扱う。
//! 送信のみでリプレイ機構は持たない——スコープは「消えかけている
//! コマンドをメールで見える化する」ことに限定した安全側の設計。

use anyhow::Context;
use open_raid_z_core::offsite_backup::{EmailBackupTarget, EmailBackupTargetConfig, OffsiteBackupTarget};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::raft::Command;

/// 管理API経由で受け取る設定。`EmailBackupTargetConfig`をそのまま
/// 包むだけ(このリポジトリ固有のフィールドは追加しない——「メール
/// アドレスひとつだけで有効化できる」という要件に沿い、必須項目は
/// `open_raid_z_core`側が既に定義済みの最小限のSMTP設定のみ)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasterEmailBackupConfig {
    #[serde(flatten)]
    pub email: EmailBackupTargetConfig,
}

impl DisasterEmailBackupConfig {
    /// 宣言的設定(`aruaru.yaml: disaster_backup.email`)からの構築ヘルパー。
    /// 呼び出し側が `open_raid_z_core::offsite_backup::EmailBackupTargetConfig`
    /// を直接名前で参照せずに済むようにする。
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        smtp_host: String,
        smtp_port: u16,
        smtp_username: String,
        smtp_password_env: String,
        from_address: String,
        to_address: String,
        allow_plaintext_for_testing: bool,
    ) -> Self {
        Self {
            email: EmailBackupTargetConfig {
                smtp_host,
                smtp_port,
                smtp_username,
                smtp_password_env,
                from_address,
                to_address,
                allow_plaintext_for_testing,
            },
        }
    }
}

/// 分散レイヤーから独立して使えるスタンドアロンのメール退避ラッパー。
/// `ClusterTopology`/`multi_raft`/`snapshot_pairing`の登録有無に一切
/// 依存しない。
pub struct DisasterEmailBackup {
    target: EmailBackupTarget,
}

impl DisasterEmailBackup {
    pub fn new(config: DisasterEmailBackupConfig) -> Self {
        Self { target: EmailBackupTarget::new(config.email) }
    }

    /// SMTP接続の疎通確認のみ(実送信は行わない)。
    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        self.target
            .ensure_ready()
            .map_err(|e| anyhow::anyhow!("disaster email backup not ready: {e}"))
    }

    /// Raft複製書き込み(過半数コミット待ち)が実際に失敗した`Command`を
    /// メールで退避する。**ベストエフォート**——このメソッド自体が
    /// 失敗しても呼び出し元(`RaftWriter::propose_and_wait`)の失敗理由を
    /// 上書きしない設計とする(呼び出し側でログに残すに留める使い方を
    /// 想定)。
    pub fn backup_failed_command(&self, command: &Command, reason: &str) -> anyhow::Result<()> {
        let label = format!("disaster-fallback-{}.json", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let payload = serde_json::json!({
            "reason": reason,
            "command": command,
            "backed_up_at": chrono::Utc::now().to_rfc3339(),
        });
        let bytes = serde_json::to_vec_pretty(&payload).context("failed to serialize command for email backup")?;

        match self.target.upload_segment(&label, &bytes) {
            Ok(()) => {
                info!(reason, "disaster email backup: raft command emailed as fallback");
                Ok(())
            }
            Err(e) => {
                error!(reason, error = %e, "disaster email backup: failed to email fallback segment");
                Err(anyhow::anyhow!("disaster email backup failed: {e}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// `open-raid-z`側`tests/offsite_backup_integration.rs`と同じ
    /// 最小限の偽SMTPサーバー(EHLO/AUTH LOGIN/MAIL FROM/RCPT TO/DATA/QUIT)。
    /// 実SMTPサーバーへは一切接続しない。
    fn spawn_fake_smtp_server(received: Arc<Mutex<Vec<String>>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                handle_smtp_client(stream, Arc::clone(&received));
                break;
            }
        });
        port
    }

    fn handle_smtp_client(mut stream: TcpStream, received: Arc<Mutex<Vec<String>>>) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ = stream.write_all(b"220 localhost fake smtp ready\r\n");
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let cmd = line.trim_end();
            if cmd.to_ascii_uppercase().starts_with("EHLO") {
                let _ = stream.write_all(b"250-localhost\r\n250-AUTH LOGIN\r\n250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("AUTH LOGIN") {
                let _ = stream.write_all(b"334 VXNlcm5hbWU6\r\n");
                line.clear();
                reader.read_line(&mut line).unwrap();
                let _ = stream.write_all(b"334 UGFzc3dvcmQ6\r\n");
                line.clear();
                reader.read_line(&mut line).unwrap();
                let _ = stream.write_all(b"235 Authentication successful\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("MAIL FROM") {
                let _ = stream.write_all(b"250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("RCPT TO") {
                let _ = stream.write_all(b"250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("DATA") {
                let _ = stream.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n");
                let mut body = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line == ".\r\n" {
                        break;
                    }
                    body.push_str(&line);
                }
                received.lock().unwrap().push(body);
                let _ = stream.write_all(b"250 OK: queued\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("QUIT") {
                let _ = stream.write_all(b"221 Bye\r\n");
                return;
            } else {
                let _ = stream.write_all(b"250 OK\r\n");
            }
        }
    }

    fn make_backup(port: u16, password_env: &str) -> DisasterEmailBackup {
        DisasterEmailBackup::new(DisasterEmailBackupConfig {
            email: EmailBackupTargetConfig {
                smtp_host: "127.0.0.1".to_string(),
                smtp_port: port,
                smtp_username: "backup@example.com".to_string(),
                smtp_password_env: password_env.to_string(),
                from_address: "backup@example.com".to_string(),
                to_address: "admin@example.com".to_string(),
                allow_plaintext_for_testing: true,
            },
        })
    }

    #[test]
    fn backup_failed_command_sends_json_payload_via_mock_smtp() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let port = spawn_fake_smtp_server(Arc::clone(&received));
        std::env::set_var("ARUARU_TEST_SMTP_PASSWORD_1", "test-password");

        let backup = make_backup(port, "ARUARU_TEST_SMTP_PASSWORD_1");
        let command = Command::Exec("INSERT INTO items VALUES (1, 'sword')".to_string());

        backup
            .backup_failed_command(&command, "raft quorum commit timed out")
            .expect("email backup should succeed against the mock smtp server");

        let bodies = received.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("INSERT INTO items"));
    }

    /// マルチノードRaftクラスタ・ZFSスナップショット連携のいずれも構築
    /// せず、`DisasterEmailBackup`単体だけを構築・使用できることを確認
    /// する(要件どおり「メールアドレスひとつだけ」で完結すること)。
    #[test]
    fn disaster_email_backup_requires_no_cluster_or_snapshot_setup() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let port = spawn_fake_smtp_server(Arc::clone(&received));
        std::env::set_var("ARUARU_TEST_SMTP_PASSWORD_2", "test-password");

        // ClusterTopology/multi_raft/SnapshotPairingRegistryのいずれも
        // 生成していない——これがコンパイル・実行できること自体が
        // 「独立して動く」ことの検証になる。
        let backup = make_backup(port, "ARUARU_TEST_SMTP_PASSWORD_2");
        let command = Command::Commit("disaster fallback commit".to_string());

        backup
            .backup_failed_command(&command, "simulated disconnection")
            .expect("standalone email backup should work with no cluster/snapshot setup");
    }

    #[test]
    fn ensure_ready_reports_missing_password_env_honestly() {
        let backup = make_backup(65500, "ARUARU_TEST_SMTP_PASSWORD_DOES_NOT_EXIST_ANYWHERE");
        // SMTP接続を試みる前に環境変数チェックで失敗するはずなので、
        // ポート65500に実サーバーが無くてもテストは決定的に失敗を返す。
        let err = backup.ensure_ready().expect_err("missing secret env var must fail honestly");
        assert!(err.to_string().contains("ARUARU_TEST_SMTP_PASSWORD_DOES_NOT_EXIST_ANYWHERE"));
    }
}
