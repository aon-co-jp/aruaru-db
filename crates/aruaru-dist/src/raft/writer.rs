//! Raft経由の複製書き込みを型消去して公開するアダプタ
//!
//! `aruaru-wire` (pgwireサーバ) はクエリ実行時に、統合先の具体的な
//! `Applier` 実装を知る必要なく「書き込みをRaftに提案し、過半数コミット+適用
//! されるまで待つ」ことができるよう、この object-safe なトレイトを介して
//! `RaftNode<A>` を利用する。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::command::Command;
use super::node::{Applier, RaftNode};

#[cfg(feature = "disaster_email_backup")]
use crate::disaster_email_backup::DisasterEmailBackup;
#[cfg(feature = "disaster_email_backup")]
use parking_lot::Mutex;

/// デフォルトの複製書き込み待ちタイムアウト (単一ノードでは即時完了)
pub const DEFAULT_COMMIT_TIMEOUT: Duration = Duration::from_secs(5);

#[async_trait]
pub trait ReplicatedWriter: Send + Sync {
    /// 書き込みSQLをRaft経由で提案し、過半数コミット+適用完了まで待つ。
    async fn write_sql(&self, sql: &str) -> Result<String, String>;

    /// バージョンコミット(aruaru_commit)をRaft経由で提案し、完了まで待つ。
    async fn write_commit(&self, message: &str) -> Result<String, String>;

    /// 稼働中(既に`Arc`で共有済み)のインスタンスへ、スタンドアロン・メール
    /// ディザスタバックアップを後から注入する(2026-07-25追記、gap (b) 対応)。
    /// `RaftWriter::with_disaster_email_backup`が構築時専用のconsumingビルダー
    /// なのに対し、こちらは`&self`のみで呼べる実行時セッター——管理API
    /// (`POST /admin/disaster-email-backup`)が、既に起動しpgwireサーバへ
    /// 渡し済みの`Arc<dyn ReplicatedWriter>`に対して呼び出すためのもの。
    #[cfg(feature = "disaster_email_backup")]
    fn set_disaster_email_backup(&self, backup: Arc<DisasterEmailBackup>);
}

/// `RaftNode<A>` を型消去して `ReplicatedWriter` として公開するラッパー
pub struct RaftWriter<A: Applier + 'static> {
    node: Arc<RaftNode<A>>,
    timeout: Duration,
    /// スタンドアロン・メールディザスタバックアップ(任意設定、
    /// `disaster_email_backup` feature有効時のみ存在するフィールド)。
    /// 未設定(`None`)なら既存動作から一切変わらない
    /// (「補助機能の失敗/未設定が本流の書き込み経路をブロックしない」
    /// という本エコシステム共通の設計方針、`open-web-server`の
    /// DDNS/ACME補助機能と同じ扱い)。
    /// `Mutex`による内部可変性(2026-07-25追記、gap (b) 対応)。構築時の
    /// `with_disaster_email_backup`ビルダーに加え、既に`Arc`共有済みの
    /// 生存インスタンスへ実行時に注入する`set_disaster_email_backup`
    /// (`ReplicatedWriter`トレイト経由)からも書き換えられるようにするため、
    /// 単純な`Option<Arc<..>>`ではなく`Mutex`で包む。
    #[cfg(feature = "disaster_email_backup")]
    disaster_backup: Mutex<Option<Arc<DisasterEmailBackup>>>,
}

impl<A: Applier + 'static> RaftWriter<A> {
    pub fn new(node: Arc<RaftNode<A>>) -> Self {
        Self {
            node,
            timeout: DEFAULT_COMMIT_TIMEOUT,
            #[cfg(feature = "disaster_email_backup")]
            disaster_backup: Mutex::new(None),
        }
    }

    /// スタンドアロン・メールディザスタバックアップを配線する(任意、構築時)。
    /// 過半数コミットに失敗した(=quorum未達/タイムアウト)`Command`だけを
    /// メールへ退避する。設定しなければ`propose_and_wait`の挙動は完全に
    /// 従来通り。
    #[cfg(feature = "disaster_email_backup")]
    pub fn with_disaster_email_backup(self, backup: Arc<DisasterEmailBackup>) -> Self {
        *self.disaster_backup.lock() = Some(backup);
        self
    }

    /// テスト・特殊用途向けにコミット待ちタイムアウトを上書きする
    /// (feature構成によらず一貫して使えるよう、構造体リテラルの代わりに
    /// このビルダーを使う)。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 単一ノード構成では propose 直後にローカルで commit+apply を進める。
    /// 複数ノードでは `RaftDriver::run` の背景ループが複製・多数決commitを進める。
    async fn propose_and_wait(&self, command: Command) -> Result<String, String> {
        let idx = self.node.propose(&command)?;
        if self.node.peers().is_empty() {
            self.node.try_commit_to(idx);
            self.node.maybe_commit();
            self.node.apply_committed();
        }
        match self.node.wait_for_commit(idx, self.timeout).await {
            Ok(resp) => {
                if resp.ok {
                    Ok(resp.message)
                } else {
                    // Raftとしては(過半数)コミット+適用まで到達しているが、
                    // アプリケーション層(Applier)が拒否したケース(例: 無効な
                    // コマンド)。quorum障害ではないため、ディザスタバック
                    // アップの対象にはしない。
                    Err(resp.message)
                }
            }
            Err(reason) => {
                // `wait_for_commit`が`Err`を返すのは、タイムアウトまでに
                // 過半数コミットへ到達できなかった場合のみ(実装は
                // `RaftNode::wait_for_commit`参照)——すなわち真の
                // quorum障害(ネットワーク断・ノード不足等)。
                self.trigger_disaster_backup_if_configured(&command, &reason);
                Err(reason)
            }
        }
    }

    /// quorum障害で失われかけている`Command`を、設定されていれば
    /// バックグラウンドタスクとしてメールへ退避する(非ブロッキング・
    /// ベストエフォート)。呼び出し元(`propose_and_wait`)はこの完了を
    /// 一切待たない——`EmailBackupTarget::upload_segment`はブロッキング
    /// SMTP I/Oであり、遅い/到達不能なSMTPサーバーが実際のRaft失敗を
    /// 既に受け取った呼び出し元をさらに待たせてはならないため。
    #[cfg(feature = "disaster_email_backup")]
    fn trigger_disaster_backup_if_configured(&self, command: &Command, reason: &str) {
        let Some(backup) = self.disaster_backup.lock().clone() else { return };
        let command = command.clone();
        let reason = reason.to_string();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || backup.backup_failed_command(&command, &reason)).await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "disaster email backup task failed for a quorum-lost command");
                }
                Err(join_err) => {
                    tracing::error!(error = %join_err, "disaster email backup blocking task panicked/was cancelled");
                }
            }
        });
    }

    #[cfg(not(feature = "disaster_email_backup"))]
    fn trigger_disaster_backup_if_configured(&self, _command: &Command, _reason: &str) {}
}

#[async_trait]
impl<A: Applier + 'static> ReplicatedWriter for RaftWriter<A> {
    async fn write_sql(&self, sql: &str) -> Result<String, String> {
        self.propose_and_wait(Command::Exec(sql.to_string())).await
    }

    async fn write_commit(&self, message: &str) -> Result<String, String> {
        self.propose_and_wait(Command::Commit(message.to_string())).await
    }

    #[cfg(feature = "disaster_email_backup")]
    fn set_disaster_email_backup(&self, backup: Arc<DisasterEmailBackup>) {
        *self.disaster_backup.lock() = Some(backup);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::time::Duration;

    struct RecordingApplier {
        applied: Mutex<Vec<String>>,
    }
    impl Applier for RecordingApplier {
        fn apply(&self, command: &Command) -> super::super::command::CommandResponse {
            if let Command::Exec(sql) = command {
                self.applied.lock().push(sql.clone());
            }
            super::super::command::CommandResponse::ok()
        }
    }

    #[tokio::test]
    async fn test_single_node_write_completes_without_peers() {
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![]));
        node.become_leader();
        let writer = RaftWriter::new(node);
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_write_times_out_if_quorum_never_reached() {
        // peers を持つが複製が来ない (ネットワーク層なし) ため、過半数コミットに到達しない
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter::new(node).with_timeout(Duration::from_millis(100));
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        assert!(result.is_err(), "quorum未達なら書き込みは確定応答してはならない");
    }

    #[tokio::test]
    async fn test_write_completes_once_quorum_replication_is_simulated() {
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter::new(node.clone()).with_timeout(Duration::from_secs(2));

        // 別タスクで、あたかも2/3ノードから複製ACKが届いたかのように match_index を更新し、
        // maybe_commit + apply_committed を進める (実運用では RaftDriver::run が定期的に行う)
        let bg_node = node.clone();
        let bg = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            bg_node.update_match(2, 1); // peer 2 の複製ACKを模擬
            bg_node.maybe_commit();
            bg_node.apply_committed();
        });

        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        bg.await.unwrap();
        assert!(result.is_ok(), "2/3(過半数)の複製ACKが揃えばコミット確定するはず");
    }

    #[tokio::test]
    async fn test_quorum_failure_without_disaster_backup_configured_behaves_as_before() {
        // disaster_email_backup feature が有効でも、`with_disaster_email_backup`
        // を呼ばず未設定のままなら、quorum障害時の挙動(Errを返す)は
        // 従来と完全に一致することを確認する(既存呼び出し元への無影響)。
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: Mutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter::new(node).with_timeout(Duration::from_millis(100));
        let start = std::time::Instant::now();
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "quorum未達なら書き込みは確定応答してはならない");
        assert!(
            elapsed < Duration::from_secs(1),
            "disaster backup未設定時に余計な遅延が発生してはならない(実測: {elapsed:?})"
        );
    }
}

/// `disaster_email_backup` feature有効時のみコンパイルされる、
/// quorum障害からの実際のメール退避配線を検証するテスト。
/// 既存の`test_write_times_out_if_quorum_never_reached`と同じ
/// quorum失敗シミュレーション(peersを持つがネットワーク層が無いため
/// 複製ACKが来ない)を再利用する——新しい障害シミュレーション機構は
/// 発明しない。
#[cfg(all(test, feature = "disaster_email_backup"))]
mod disaster_backup_wiring_tests {
    use super::*;
    use crate::disaster_email_backup::{DisasterEmailBackup, DisasterEmailBackupConfig};
    use open_raid_z_core::offsite_backup::EmailBackupTargetConfig;
    use parking_lot::Mutex as PlMutex;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct RecordingApplier {
        applied: PlMutex<Vec<String>>,
    }
    impl Applier for RecordingApplier {
        fn apply(&self, command: &Command) -> super::super::command::CommandResponse {
            if let Command::Exec(sql) = command {
                self.applied.lock().push(sql.clone());
            }
            super::super::command::CommandResponse::ok()
        }
    }

    /// `disaster_email_backup.rs`のテストと同じ最小限の偽SMTPサーバー
    /// (EHLO/AUTH LOGIN/MAIL FROM/RCPT TO/DATA/QUIT)。実SMTPには接続しない。
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

    fn make_backup(port: u16, password_env: &str) -> Arc<DisasterEmailBackup> {
        Arc::new(DisasterEmailBackup::new(DisasterEmailBackupConfig {
            email: EmailBackupTargetConfig {
                smtp_host: "127.0.0.1".to_string(),
                smtp_port: port,
                smtp_username: "backup@example.com".to_string(),
                smtp_password_env: password_env.to_string(),
                from_address: "backup@example.com".to_string(),
                to_address: "admin@example.com".to_string(),
                allow_plaintext_for_testing: true,
            },
        }))
    }

    #[tokio::test]
    async fn quorum_failure_with_disaster_backup_configured_emails_the_failed_command() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let port = spawn_fake_smtp_server(Arc::clone(&received));
        std::env::set_var("ARUARU_TEST_WRITER_SMTP_PASSWORD", "test-password");
        let backup = make_backup(port, "ARUARU_TEST_WRITER_SMTP_PASSWORD");

        // 既存の quorum 失敗シミュレーション(peers はいるがネットワーク層が
        // 無いため複製 ACK が来ない)を再利用。
        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: PlMutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter::new(node)
            .with_timeout(Duration::from_millis(100))
            .with_disaster_email_backup(backup);

        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        assert!(result.is_err(), "quorum未達なら書き込みは確定応答してはならない");

        // バックグラウンドタスク(tokio::spawn + spawn_blocking)の完了を
        // 待つ(実運用のRaft失敗応答自体はこれを待たない設計だが、テストの
        // 検証のためにここでだけポーリングする)。
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if !received.lock().unwrap().is_empty() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "disaster backup email was never sent");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let bodies = received.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("INSERT INTO items"));
    }

    /// EHLO/AUTHへの応答を数秒遅延させる「本物の低速SMTP」偽サーバー。
    /// 到達不能(接続自体が即失敗)ケースとは異なり、TCP接続は確立するが
    /// アプリケーション層(SMTP応答)がダラダラ遅い、真のスロー・スロー・
    /// ロリスシナリオを再現する。`disaster_email_backup.rs`と同じ最小限の
    /// 偽SMTPサーバー実装に、EHLO受信後の`delay`だけを追加した。
    fn spawn_slow_fake_smtp_server(received: Arc<Mutex<Vec<String>>>, delay: Duration) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                handle_slow_smtp_client(stream, Arc::clone(&received), delay);
                break;
            }
        });
        port
    }

    fn handle_slow_smtp_client(mut stream: TcpStream, received: Arc<Mutex<Vec<String>>>, delay: Duration) {
        // 接続(TCP)自体はすぐ確立するが、EHLO/AUTHへの応答をわざと遅延させる。
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ = stream.write_all(b"220 localhost slow fake smtp ready\r\n");
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let cmd = line.trim_end();
            if cmd.to_ascii_uppercase().starts_with("EHLO") {
                std::thread::sleep(delay);
                let _ = stream.write_all(b"250-localhost\r\n250-AUTH LOGIN\r\n250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("AUTH LOGIN") {
                std::thread::sleep(delay);
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

    /// Gap (a) を埋めるテスト: 「TCP接続は確立するがSMTP応答がダラダラ遅い」
    /// 真のスロー・スロー・ロリスシナリオでも、`propose_and_wait`(`write_sql`)
    /// 自体は遅延に引きずられず、設定タイムアウト通りに素早く応答を返す
    /// ことを実証する。遅延(3秒、EHLO+AUTHの2箇所で発生するため合計6秒相当)
    /// より十分小さい`with_timeout`(100ms)で、書き込み自体の所要時間を検証する。
    /// Gap (b) の土台となる実行時セッターの検証: 既に`Arc<dyn ReplicatedWriter>`
    /// として共有済み(=生存中のサーバーが保持しているのと同じ状態)の
    /// インスタンスに対し、構築時ビルダーを使わず`set_disaster_email_backup`
    /// (トレイト経由)で後から注入しても、quorum障害時に実際にメールされる
    /// ことを確認する。`aruaru-server`の管理API(`POST /admin/
    /// disaster-email-backup`)がまさにこの経路(`&self`のみ、生存インスタンス
    /// への注入)で呼ぶことを想定している。
    #[tokio::test]
    async fn set_disaster_email_backup_after_arc_sharing_still_wires_up_correctly() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let port = spawn_fake_smtp_server(Arc::clone(&received));
        std::env::set_var("ARUARU_TEST_WRITER_SMTP_PASSWORD_RUNTIME", "test-password");
        let backup = make_backup(port, "ARUARU_TEST_WRITER_SMTP_PASSWORD_RUNTIME");

        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: PlMutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        // 構築時は disaster backup 未設定のまま Arc<dyn ReplicatedWriter> として共有
        // (main.rs が pgwire サーバへ渡すのと同じ形)。
        let writer: Arc<dyn ReplicatedWriter> =
            Arc::new(RaftWriter::new(node).with_timeout(Duration::from_millis(100)));

        // ここで後から注入する(管理APIハンドラが行うのと同じ操作)。
        writer.set_disaster_email_backup(backup);

        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        assert!(result.is_err(), "quorum未達なら書き込みは確定応答してはならない");

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if !received.lock().unwrap().is_empty() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "実行時注入したdisaster backupがメールしなかった");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(received.lock().unwrap()[0].contains("INSERT INTO items"));
    }

    #[tokio::test]
    async fn quorum_failure_does_not_block_on_disaster_backup_even_when_smtp_is_genuinely_slow() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let slow_delay = Duration::from_secs(3);
        let port = spawn_slow_fake_smtp_server(Arc::clone(&received), slow_delay);
        std::env::set_var("ARUARU_TEST_WRITER_SMTP_PASSWORD_SLOW", "test-password");
        let backup = make_backup(port, "ARUARU_TEST_WRITER_SMTP_PASSWORD_SLOW");

        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: PlMutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter::new(node)
            .with_timeout(Duration::from_millis(100))
            .with_disaster_email_backup(backup);

        let start = std::time::Instant::now();
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "quorum未達なら書き込みは確定応答してはならない");
        assert!(
            elapsed < Duration::from_secs(1),
            "本物の低速SMTP(EHLO/AUTH応答を{slow_delay:?}遅延)に引きずられて\
             write_sql自体がブロックしてはならない(実測: {elapsed:?}、\
             SMTP側の遅延より遥かに短いはず)"
        );

        // 背景タスク側では、遅延はあってもいずれメールが届くはず(非ブロッキングは
        // 「呼び出し元を待たせない」であって「送信を諦める」ではないことの確認)。
        let deadline = std::time::Instant::now() + slow_delay * 3 + Duration::from_secs(2);
        loop {
            if !received.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "低速SMTPでも(遅延はしても)最終的にはメールが送られるはず"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let bodies = received.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("INSERT INTO items"));
    }

    #[tokio::test]
    async fn quorum_failure_does_not_block_on_disaster_backup_even_if_smtp_is_unreachable() {
        // SMTPサーバーを一切起動しない(到達不能なアドレス相当)。
        // それでも write_sql 自体はタイムアウト設定通りに素早く Err を
        // 返すこと(非ブロッキング配線であることの直接証明)を確認する。
        std::env::set_var("ARUARU_TEST_WRITER_SMTP_PASSWORD_UNREACHABLE", "test-password");
        let backup = make_backup(1, "ARUARU_TEST_WRITER_SMTP_PASSWORD_UNREACHABLE"); // port 1: 到達不能

        let node = Arc::new(RaftNode::new(1, RecordingApplier { applied: PlMutex::new(vec![]) }, vec![2, 3]));
        node.become_leader();
        let writer = RaftWriter::new(node)
            .with_timeout(Duration::from_millis(100))
            .with_disaster_email_backup(backup);

        let start = std::time::Instant::now();
        let result = writer.write_sql("INSERT INTO items VALUES (1, 'sword')").await;
        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_secs(1),
            "到達不能なSMTPに引きずられてwrite_sql自体がブロックしてはならない(実測: {elapsed:?})"
        );
    }
}
