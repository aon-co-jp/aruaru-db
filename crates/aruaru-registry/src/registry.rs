//! レジストリ本体: 150+ 件の対応DBを保持し、クロール結果を反映する。

use std::sync::Arc;

use parking_lot::RwLock;

use crate::crawler::{self, RankMap};
use crate::types::*;

/// 対応DBレジストリ (スレッドセーフ)
pub struct Registry {
    entries: RwLock<Vec<DbEntry>>,
}

impl Registry {
    /// seed データで初期化
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(crate::seed::seed()),
        })
    }

    /// 全エントリ (rank 昇順、未ランクは末尾)
    pub fn all(&self) -> Vec<DbEntry> {
        let mut v = self.entries.read().clone();
        v.sort_by_key(|e| e.rank.unwrap_or(u32::MAX));
        v
    }

    /// 件数
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    /// ステータスで絞り込み
    pub fn by_status(&self, status: Status) -> Vec<DbEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.status == status)
            .cloned()
            .collect()
    }

    /// 接続可能(計画中以外)なものだけ
    pub fn connectable(&self) -> Vec<DbEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.status.is_connectable())
            .cloned()
            .collect()
    }

    /// id で取得
    pub fn get(&self, id: &str) -> Option<DbEntry> {
        self.entries.read().iter().find(|e| e.id == id).cloned()
    }

    /// 集計サマリ (ステータス別件数など)
    pub fn summary(&self) -> RegistrySummary {
        let entries = self.entries.read();
        let mut by_status = [0u32; 5];
        let mut pg_compatible = 0;
        for e in entries.iter() {
            let idx = match e.status {
                Status::Ga => 0,
                Status::Beta => 1,
                Status::PgCompatible => 2,
                Status::ReadOnly => 3,
                Status::Planned => 4,
            };
            by_status[idx] += 1;
            if e.wire == Wire::Postgres {
                pg_compatible += 1;
            }
        }
        RegistrySummary {
            total: entries.len() as u32,
            ga: by_status[0],
            beta: by_status[1],
            pg_compatible: by_status[2],
            read_only: by_status[3],
            planned: by_status[4],
            postgres_wire: pg_compatible,
        }
    }

    /// クロール結果(RankMap)を反映する。マッチした件数を返す。
    pub fn apply_ranks(&self, ranks: &RankMap, crawled_at: &str) -> usize {
        let mut entries = self.entries.write();
        let mut matched = 0;
        for e in entries.iter_mut() {
            // 名前 or id の正規化で突き合わせ
            let key_name = crawler::normalize(&e.name);
            let key_id = crawler::normalize(&e.id);
            if let Some((rank, score)) = ranks.get(&key_name).or_else(|| ranks.get(&key_id)) {
                e.rank = Some(*rank);
                e.score = Some(*score);
                e.updated_at = Some(crawled_at.to_string());
                matched += 1;
            }
        }
        matched
    }

    /// 今すぐクロールしてランキングを反映する。
    pub async fn crawl_now(&self) -> anyhow::Result<CrawlReport> {
        let ranks = crawler::crawl_all().await?;
        let now = chrono::Utc::now().to_rfc3339();
        let matched = self.apply_ranks(&ranks, &now);
        Ok(CrawlReport {
            crawled: ranks.len() as u32,
            matched: matched as u32,
            crawled_at: now,
        })
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            entries: RwLock::new(crate::seed::seed()),
        }
    }
}

/// 集計サマリ
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegistrySummary {
    pub total: u32,
    pub ga: u32,
    pub beta: u32,
    pub pg_compatible: u32,
    pub read_only: u32,
    pub planned: u32,
    pub postgres_wire: u32,
}

/// クロール結果レポート
#[derive(Debug, Clone, serde::Serialize)]
pub struct CrawlReport {
    pub crawled: u32,
    pub matched: u32,
    pub crawled_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seed_has_150_plus() {
        let reg = Registry::new();
        assert!(reg.len() >= 150, "expected >=150 entries, got {}", reg.len());
    }

    #[test]
    fn test_summary_counts() {
        let reg = Registry::new();
        let s = reg.summary();
        assert_eq!(
            s.total,
            s.ga + s.beta + s.pg_compatible + s.read_only + s.planned
        );
        // PostgreSQL ワイヤ互換が複数登録されている
        assert!(s.postgres_wire >= 10);
    }

    #[test]
    fn test_apply_ranks() {
        let reg = Registry::new();
        let mut ranks = RankMap::new();
        ranks.insert("postgresql".to_string(), (4, 620.5));
        ranks.insert("mysql".to_string(), (2, 1010.0));
        let matched = reg.apply_ranks(&ranks, "2026-06-22T00:00:00Z");
        assert!(matched >= 2);
        let pg = reg.get("postgresql").unwrap();
        assert_eq!(pg.rank, Some(4));
    }
}
