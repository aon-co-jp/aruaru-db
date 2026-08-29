//! `KeyGuardian` — aruaru-db専用のAPIキー自動ライフサイクル管理。
//!
//! 【2026-08-29(続き)移設】元々`aruaru-server`クレート内に実装していたが、
//! GraphQL側(`aruaru-graphql::admin_resolvers`)から`keys`(revoke/status)
//! 操作をREST実データへ接続するには、REST(`aruaru-server::admin::
//! AdminState`)とGraphQL(`AdminCtx`)の両方が同一の`KeyGuardian`
//! インスタンスを参照できる必要がある。両クレートとも依存できる
//! `aruaru-dist`(`admin_shared.rs`の`ClusterTopology`共有と同じ理由)
//! へ移設し、`aruaru-server`側は`aruaru_dist::keyring::KeyGuardian`を
//! 再利用する形に変更した。ロジック自体は無変更(ファイル移動のみ)。
//!
//! ユーザー指示(2026-08-29、open-english経由)「aruaru-dbとの連携を
//! 強化し、特にREST APIを不要にして。APIキーを自動管理で、自動発行・
//! 自動承認・自動破棄・自動削除」への対応。
//!
//! **設計方針(このエコシステムの既存方針を踏襲)**: RPoem
//! (`open-runo-router::keyring::KeyGuardian`)と全く同じ設計思想
//! (自己発行・自動失効・期限切れ自動削除)だが、Cargo依存として
//! RPoemへ結合するのではなく、このリポジトリ側で独立に再実装した
//! ——RPoem自身が"WunderGraph Cosmoをパッケージ依存させず概念だけ
//! 自前実装する"という方針を取っており、open-web-serverも同じ
//! `KeyGuardian`設計を無関係な独立実装として持つ(RPoemとopen-web-server
//! 間にCargo依存は無い)。この慣習に倣い、aruaru-dbも3つ目の独立実装
//! として持つ。
//!
//! - **自動発行(auto-issue)**: 認証不要の`self_issue`が、既定で
//!   低権限(`viewer`ロール)・短命(既定24時間)のキーを即座に発行する。
//! - **自動承認(auto-approve)**: 上記の「認証を要求しない即時発行」
//!   自体が承認手続きそのもの——人間の承認待ちキューは存在しない
//!   (RPoemの`POST /api/keys/self-issue`と同じ設計)。
//! - **自動破棄(auto-revoke)**: `revoke_owner`で特定オーナーの
//!   全キーを即座に失効させる。
//! - **自動削除(auto-clean)**: 期限切れのキーは`verify`時に検知され
//!   その場でレジストリから削除される(明示的なcronジョブ等は不要)。
//!
//! **正直な開示**: RPoemの`KeyGuardian`が持つEWMAベースの異常検知
//! (急激なリクエストレート上昇によるキー隔離)は、今回のユーザー指示
//! (4つの具体的なライフサイクル操作)に含まれないため実装していない
//! ——必要になった場合はRPoem側の実装(`crates/open-runo-router/src/
//! keyring.rs`)を参考に追加できる。また、キーはプロセスメモリ上にのみ
//! 保持し(既存の`ARUARU_DB_ADMIN_TOKEN`静的トークンと同様、永続化は
//! しない)、プロセス再起動で全て失効する——複数レプリカ間でのキー
//! 共有(分散レジストリ)も今回のスコープ外。

use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// 登録済みキー1件(平文キーのSHA-256ハッシュをキーとして保持する)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    pub owner: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

/// 検証結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDecision {
    /// レジストリが空(まだ1件も発行されていない)——既存の
    /// `ARUARU_DB_ADMIN_TOKEN`静的トークンのみで運用中の後方互換の
    /// ため、この状態では静的トークン検証の結果に委ねる。
    RegistryEmpty,
    /// 検証成功。
    Ok { owner: String, role: String },
    /// 未知・失効済み・期限切れのキー。
    Rejected,
}

/// 平文キーのSHA-256 16進ハッシュ。
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// 自己発行キーの既定TTL(24時間、RPoem側の既定値と同じ)。
pub const DEFAULT_SELF_ISSUE_TTL_HOURS: i64 = 24;

#[derive(Debug, Default)]
pub struct KeyGuardian {
    /// ハッシュ化済みキー -> レコード。
    keys: Mutex<HashMap<String, KeyRecord>>,
    /// 「レジストリが1件でも発行済みか」を高速判定するキャッシュ。
    known_nonempty: AtomicBool,
}

impl KeyGuardian {
    pub fn new() -> Self {
        Self::default()
    }

    /// `owner`向けにキーを自動発行する。返り値の平文キーはこの1回しか
    /// 存在しない(以降はハッシュのみ保持)。
    pub fn issue(&self, owner: &str, role: &str, ttl: Option<Duration>) -> String {
        let plaintext = format!(
            "adb_{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let record = KeyRecord {
            owner: owner.to_string(),
            role: role.to_string(),
            created_at: Utc::now(),
            expires_at: ttl.map(|d| Utc::now() + d),
            revoked: false,
        };
        self.keys.lock().insert(hash_key(&plaintext), record);
        self.known_nonempty.store(true, Ordering::Relaxed);
        plaintext
    }

    /// `owner`が持つ全キーを即座に失効させる(自動破棄)。失効させた件数を返す。
    pub fn revoke_owner(&self, owner: &str) -> usize {
        let mut revoked = 0;
        for record in self.keys.lock().values_mut() {
            if record.owner == owner && !record.revoked {
                record.revoked = true;
                revoked += 1;
            }
        }
        revoked
    }

    /// 平文キーを検証する。期限切れのキーはここで検知され、レジストリ
    /// から削除される(自動削除)。
    pub fn verify(&self, key: &str) -> KeyDecision {
        if !self.known_nonempty.load(Ordering::Relaxed) {
            return KeyDecision::RegistryEmpty;
        }
        let hashed = hash_key(key);
        let mut keys = self.keys.lock();
        let Some(record) = keys.get(&hashed) else {
            return KeyDecision::Rejected;
        };
        if record.revoked {
            return KeyDecision::Rejected;
        }
        if let Some(expiry) = record.expires_at {
            if Utc::now() >= expiry {
                keys.remove(&hashed);
                return KeyDecision::Rejected;
            }
        }
        KeyDecision::Ok { owner: record.owner.clone(), role: record.role.clone() }
    }

    /// 現在登録されているキー件数(失効済み含む、監視用)。
    pub fn count(&self) -> usize {
        self.keys.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_reports_empty() {
        let g = KeyGuardian::new();
        assert_eq!(g.verify("anything"), KeyDecision::RegistryEmpty);
    }

    #[test]
    fn issue_then_verify_round_trips_with_role() {
        let g = KeyGuardian::new();
        let key = g.issue("alice", "developer", None);
        assert!(key.starts_with("adb_"));
        match g.verify(&key) {
            KeyDecision::Ok { owner, role } => {
                assert_eq!(owner, "alice");
                assert_eq!(role, "developer");
            }
            other => panic!("expected Ok, got {other:?}"),
        }
        // 一度でも発行されればレジストリは「非空」になり、未知キーは拒否される。
        assert_eq!(g.verify("wrong-key"), KeyDecision::Rejected);
    }

    #[test]
    fn revoke_owner_kills_only_that_owners_keys() {
        let g = KeyGuardian::new();
        let bob1 = g.issue("bob", "viewer", None);
        let bob2 = g.issue("bob", "viewer", None);
        let alice = g.issue("alice", "viewer", None);

        assert_eq!(g.revoke_owner("bob"), 2);
        assert_eq!(g.verify(&bob1), KeyDecision::Rejected);
        assert_eq!(g.verify(&bob2), KeyDecision::Rejected);
        assert!(matches!(g.verify(&alice), KeyDecision::Ok { .. }));
    }

    #[test]
    fn expired_keys_are_rejected_and_auto_deleted() {
        let g = KeyGuardian::new();
        let key = g.issue("carol", "viewer", Some(Duration::seconds(-1)));
        assert_eq!(g.verify(&key), KeyDecision::Rejected);
        // 自動削除: 二度目の検証でもレジストリから実際に消えている
        // (再発行しない限り同じハッシュは二度と現れない)ことを、
        // 件数の変化で確認する。
        assert_eq!(g.count(), 0);
    }

    #[test]
    fn revoke_unknown_owner_is_a_no_op() {
        let g = KeyGuardian::new();
        g.issue("dave", "viewer", None);
        assert_eq!(g.revoke_owner("nobody"), 0);
    }
}
