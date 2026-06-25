# aruaru-DB Admin (Tauri 2)

Rust(Tauri) + React/TypeScript によるデスクトップ管理アプリ。
ダークテーマ・日本語 UI。`aruaru-server` の管理 API に接続して操作する。

## 起動

```bash
cd admin
npm install
npm run tauri:dev      # 開発 (Vite + Tauri)
npm run tauri:build    # 配布ビルド (Win/macOS/Linux)
```

## 機能

### データ
- **ダッシュボード** / **コミットログ** / **ブランチ** / **クエリ** — Git-on-SQL の参照・操作 (GraphQL 経由)

### 運用・分散（このバージョンで拡充）

| 機能 | 画面 | 内容 |
|------|------|------|
| 🚚 **お引越し** | `MigrationWizard` | ①取り込み: PostgreSQL/CockroachDB/Snowflake/MySQL/CSV/Parquet → aruaru-DB。接続テスト・並列ワーカー指定。②まるごと移植。取り込みは PostgreSQL/MySQL ワイヤ経由で**実際にテーブルを取り込みコミット**(永続化込み)。Mongo/CQL も接続テスト・取り込み対応 |
| 💾 **バックアップ** | `BackupManager` | フル/増分/スナップショット、保存先 ローカル/S3互換/SFTP、AES-256-GCM 暗号化、保持期間、Cron 自動スケジュール、リストア（PITR 対応） |
| ⚙️ **分散並列化** | `ParallelView` | 並列度・ワーカースレッド・シャッフル分割数・broadcast 閾値の設定、並列スキャン/集計/シャッフルジョインの有効化、分散実行プラン可視化（フラグメント×並列度×ノード）、実行中ジョブのノード別進捗監視 |
| 🌐 **分散DB統合** | `FederationView` | 外部DB（他 aruaru/PostgreSQL/CockroachDB/Snowflake/MySQL）を統合ソースとして登録、接続テスト、プッシュダウン最適化、複数DBを横断するフェデレーテッドクエリ |
| 🗃️ **対応DB** | `DatabaseRegistry` | 150+件の対応DBレジストリ。DB-Engines等から毎日クロールしてランキング・対応状況を自動更新。5段階ステータス(GA/Beta/PG互換接続可/読取専用/計画中)、PGワイヤ互換DBは実接続テスト可 |
| 🖧 **クラスタ** | `ClusterView` | ノード状態(Raft role/term)、Range 分布、リバランス、ノード追加/除去 |

## アーキテクチャ

```
React UI ──invoke──> Tauri コマンド(Rust) ──HTTP/JSON──> aruaru-server 管理API
                     (src-tauri/src/main.rs)
```

フロントは `invoke("コマンド名", { baseUrl, ... })` で Rust 側を呼ぶ。
Rust 側は `reqwest` で `aruaru-server` の管理エンドポイントに転送する。
**サーバ未接続時は各画面がサンプル表示にフォールバック**するため、UI 単体で動作確認できる。

## サーバ側 管理 API（`aruaru-server` に実装済み・v0.3）

`crates/aruaru-server/src/admin.rs` に実装。GraphQL と同じ Poem サーバ（:4000）に
`/admin` 配下でマウントされ、共有 `QueryEngine` を参照する。

実データで応答するもの: クラスタ状態（単一ノードの実コミット数・行数）、バックアップ台帳、
並列設定の保存/取得、SQL からの分散実行プラン生成、`local.*` のフェデレーテッドクエリ実行、
接続テスト（TCP 名前解決 / ファイル存在）。

エンジン未完のため受理のみ（`note` に明記）: 実バックアップ I/O、外部DBからの取り込み、
リモートプッシュダウン、マルチノード操作 → aruaru-backup / aruaru-migrate / 分散レイヤ完成時に差し替え。

| メソッド | パス | 用途 |
|---------|------|------|
| POST/GET | `/admin/backup` | バックアップ作成 / 一覧 |
| POST | `/admin/backup/restore` | リストア / PITR |
| POST | `/admin/backup/schedule` | スケジュール設定 |
| POST | `/admin/migrate/test` | 移行元接続テスト |
| POST | `/admin/migrate/preview` | スキーマプレビュー |
| POST | `/admin/migrate/run` | 取り込み実行 |
| POST | `/admin/migrate/instance` | まるごと移植 |
| GET/POST | `/admin/parallel` | 並列実行設定 取得/更新 |
| POST | `/admin/parallel/explain` | 分散実行プラン |
| GET | `/admin/parallel/jobs` | 実行中ジョブ |
| GET/POST | `/admin/federation` | 統合ソース 一覧/登録 |
| POST | `/admin/federation/test` | 接続テスト |
| POST | `/admin/federation/drop` | 削除 |
| POST | `/admin/federation/query` | 横断クエリ |
| GET | `/admin/cluster` | クラスタ状態 |
| POST | `/admin/cluster/node` | ノード追加/除去 |
| POST | `/admin/cluster/rebalance` | リバランス |

## 接続先

`src/App.tsx` の `SERVER_BASE`（管理API, 既定 `http://localhost:4000`）と
`SERVER_GQL`（GraphQL, `…/graphql`）を環境に合わせて変更する。
