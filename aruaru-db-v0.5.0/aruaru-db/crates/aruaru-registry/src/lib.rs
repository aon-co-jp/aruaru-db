//! aruaru-registry: 対応DBレジストリ + 毎日クロール + 取り込みアダプタ
//!
//! - `types`     : DB分類/ワイヤ/ステータス(5段階)/移行経路 の型
//! - `seed`      : 2026.06 時点の DB-Engines 上位＋著名DB 150+ 件の初期データ
//! - `registry`  : レジストリ本体 (検索・集計・クロール反映)
//! - `crawler`   : DB-Engines を主、フォールバック付きのランキング取得
//! - `scheduler` : 毎日 24h ごとの自動クロール
//! - `adapter`   : capability(ワイヤ)単位の取り込みアダプタ。PgWire は実接続。

pub mod adapter;
pub mod crawler;
pub mod registry;
pub mod scheduler;
pub mod seed;
pub mod types;

pub use registry::{CrawlReport, Registry, RegistrySummary};
pub use types::{
    BackupSupport, Category, DbEntry, MigrateVia, Status, Wire,
};
