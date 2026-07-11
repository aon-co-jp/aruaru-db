# 技術スタック・開発ルール(aruaru-db)

このリポジトリ、および関連プロジェクト(`open-runo`/`open-web-server`/
`poem-cosmo-tauri`/`aruaru-web`/`open-raid-z`)で開発・保守を行う際は、以下を基本方針とする。
作業ドライブは `F:\open-runo`(E:ドライブは2026-07-10に消失、以後Fが実体)。
この節は [`open-raid-z`](https://github.com/aon-co-jp/open-raid-z) の
`CLAUDE.md` を正本とし、各プロジェクトへコピーして同期する。

## 方針転換(2026-07-10、open-raid-z 正本より転記・最終確定)

ユーザー指示により以下へ転換・確定。**Tauri・Poem・WunderGraph Cosmo(有料版
含む)を外部パッケージ/ライブラリとして直接依存させることはしない**。ただし
各ツールが提供する**機能・API形状・体験には互換性を保ち**、Rust標準ライブラリ
+ tokio/hyper で自前実装して置き換える(依存だけを断ち、機能面の互換性は
維持する)。**`poem-cosmo-tauri` と `open-runo` は2リポジトリを同時並行で
開発する**。実装(例: Poem→tokio/hyper移行)は poem-cosmo-tauri 側で先行させ、
動作確認できたファイルを open-runo へミラーする運用とする。

> **aruaru-db 固有の注記(2026-07-11)**: 本リポジトリの `aruaru-graphql` /
> `aruaru-wire` / `aruaru-server` クレートは現時点で **この方針転換に未移行**
> —— `poem` / `async-graphql-poem` / `pgwire` への直接依存が残っている
> (`Cargo.toml` の `workspace.dependencies` 参照)。2026-07-10 の方針転換は
> 本リポジトリにもいずれ適用されるべきだが、Poem/pgwire を剥がす作業は
> GraphQL 層・pgwire サーバ層の総入れ替えとなり影響範囲が広いため、今回の
> 巡回では着手していない(下記「現状・重要な引き継ぎ事項」参照)。次回以降、
> 専用のマイグレーションパスとして計画すること。なお `pgwire` への依存は
> PostgreSQL ワイヤプロトコル互換を提供するためのものであり、Poem/Cosmo の
> 置き換え対象(HTTPフレームワーク層)とは別軸の依存である点に注意。

**poem-cosmo-tauri と open-runo の違い(2026-07-11、ユーザー確認済み、
open-raid-z正本より転記)**: 両者は共通コア(Cosmo有料版機能のOSS Rust
再実装)を持つが**全く違うリポジトリのプロジェクト**であり統合対象では
ない。poem-cosmo-tauri はさらに範囲が広く、Poem/Tauriの**全機能を
AI駆動開発で一から自作・再現する**という上乗せ目標を持つ(open-runoには
ない)。詳細は open-raid-z の `CLAUDE.md` を参照。

**open-web-server 拡張要件との関わり(2026-07-11、ユーザー指示)**:
`open-web-server` リポジトリが、poem-cosmo-tauri/open-runo・PostgreSQL・
このリポジトリ(`aruaru-db`)・`open-raid-z`と組み合わせて、3Dオンライン
ゲームの課金アイテム・金融/証券データをネットワーク上で紛失しないための
通信層の四重化(TCP-IP・UDP-IP・QUIC/MPQUIC・MPTCP/SCTP、2026-07-11に
三層三重から改訂)・DB書き込みの四重化(PostgreSQL・aruaru-db・マルチ
リージョン同期レプリケーション・独立監査ログ)・VersionLessAPIとGit管理
(このリポジトリのGit-on-SQL特性を利用)のハイブリッド版管理を実装する
目標アーキテクチャの詳細・進捗は `open-web-server/CLAUDE.md`(および正本の
open-raid-z
`CLAUDE.md`)を参照。このリポジトリは分散Git-on-SQLデータ層として関与する。

## フロントエンド

- Tauriパッケージには直接依存しない。ただしTauriのデスクトップUI体験・
  `invoke()`的な呼び出しインターフェースとは互換性を保つ。
- **HTML5/CSS3・TypeScript・Bootstrap・Node.jsのスタックは廃止**。
  Rustをメイン言語としてフロントエンドとバックエンドを統合し、
  **WebAssembly (WASM)** に置き換える(コンパイル対象はRust →
  `wasm32-unknown-unknown`)。https://webassembly.org/ | https://rustwasm.github.io/
- **aruaru-db 固有の注記**: `admin/` 配下の管理GUIは現状 Tauri + TypeScript
  のまま(`README.md` にも "Tauri Admin GUI" と記載)。WASM移行は未着手。

## バックエンド・コア

- **Rust**(メイン言語、標準ライブラリ中心): https://www.rust-lang.org/ja/
- **tokio** + **hyper**(Webフレームワークなしで直接HTTPサーバを自前実装):
  https://tokio.rs/ | https://docs.rs/hyper/latest/hyper/
- Poemパッケージには依存しないが、Poemのルーティング/ハンドラAPI形状とは
  互換性のあるインターフェースを維持しながらtokio/hyper直接実装へ移行する。
- **openraft**(Raft分散合意)・**DataFusion**(OLAPクエリ)・**pgwire**
  (PostgreSQL互換プロトコル) は引き続き本リポジトリの中核依存。
- **aruaru-db 固有の注記**: `aruaru-graphql`/`aruaru-server` は現状 `poem` +
  `async-graphql-poem` に直接依存している(上記の通り未移行)。

## API設計思想(参考・概念のみ)

- **VersionLess API**という考え方を参考にする(WunderGraphのブログ/podcast参照)。
- **WunderGraph Cosmo**: パッケージとしては直接依存させない。GraphQL
  Federation / VersionlessAPI というAPI形状・コンセプトのみ参考にし、
  Rust標準+tokio/hyperで互換性を保ちつつ自前実装する。
  https://github.com/wundergraph/cosmo

## 関連プロジェクト

- **poem-cosmo-tauri**(open-runoと同時並行開発。実装の先行地点。Pure Rust
  + tokio/hyper直接実装): https://github.com/aon-co-jp/poem-cosmo-tauri
- **open-runo**: https://github.com/aon-co-jp/open-runo
- **open-web-server**: https://github.com/aon-co-jp/open-web-server
- **aruaru-db**(このリポジトリ): https://github.com/aon-co-jp/aruaru-db
- **aruaru-web**: https://github.com/aon-co-jp/aruaru-web
- **open-raid-z**(開発ルールの正本): https://github.com/aon-co-jp/open-raid-z
- **rs-to-readme**: https://github.com/aon-co-jp/rs-to-readme

## 運用ルール

- **開発中はこの`CLAUDE.md`を、コード変更のコミット/pushと必ず一緒に push する**。
- 実装で迷った場合は、学習データからの推測より公式ドキュメントを優先して参照する。
- 作業ドライブが変わった場合は、この節と関連プロジェクトの引き継ぎ資料を更新する。
- **無人自動開発(確認不要・自動デバッグ)のタイミングでは、20〜30分おきの
  スケジュール実行待ちにせず、1パス内でできる限り連続して作業を進める**
  こと。小さく検証可能な単位(1クレート/1関数ごとに `cargo test` →
  commit)を保ちながらも、次の増分に進む前にバックグラウンド待機で
  止まらない。

## 現状(このリポジトリ固有)・重要な引き継ぎ事項

- **2026-07-10 に重大な問題を発見・修正**: `main`ブランチの`Cargo.toml`が
  ワークスペースメンバーとして `crates/aruaru-query` / `aruaru-wire` /
  `aruaru-registry` / `aruaru-server`(サーバー本体バイナリ)を参照していたが、
  実際のディレクトリが存在せず `cargo check --workspace` が起動すらしない
  状態だった。調査の結果、`origin/backup-before-github-merge-20260705`という
  **mainと共通の祖先を持たない別ブランチ**に、これら4クレートを含む完全な
  実装が残っていることが判明(おそらくGitHubマージ時に履歴が分断され、
  一部crateが失われた)。このブランチから該当クレート一式、および依存関係が
  古くなっていた `aruaru-core`/`aruaru-dist`/`aruaru-graphql`/`aruaru-migrate`/
  `aruaru-backup` も含め、9クレート全てをbackup版に統一した。
  統合後、`cargo check --workspace` は全クレートで成功(警告のみ)、
  `cargo test --workspace` は63件全て成功。todo!()/unimplemented!()/TODO/FIXME
  マーカーが6件残存(次回巡回で内容確認・対応予定)と記録されていた。
  `origin/backup-before-github-merge-20260705` ブランチは統合後は用済みだが、
  削除は行っていない(履歴保全のため)。

- **2026-07-11 巡回で完了した作業**:
  - 前回パスが未コミットのまま残していた作業(aruaru-dist の raft writer、
    aruaru-graphql の admin_resolvers 拡張、aruaru-migrate の
    schema_convert、そして aruaru-query/aruaru-registry/aruaru-server/
    aruaru-wire の4クレート新規実装、約32ファイル・5500行超)を検証。
    `cargo check --workspace` / `cargo test --workspace --no-run` は
    この時点で **既に成功しており、破損した状態ではなかった** ことを確認。
  - `crates/aruaru-query/README.md`(0バイトの空ファイル)に、
    `engine.rs`/`olap.rs`/`parser.rs` の実装内容(OLTPサブセットエンジン・
    HTAPルーティング・DataFusion OLAP経路)を反映した実文書を作成。
  - todo!()/unimplemented!()/TODO/FIXME を全リポジトリ grep し、6件超
    (todo!() 2件 + TODOコメント多数)を発見。**全て実装で解消**:
    - `aruaru-backup`: `BackupEngine` に `Arc<aruaru_query::QueryEngine>` を
      持たせ、`snapshot_tables()`/`ingest_table()` 経由で実データにアクセス
      できるよう配線。`run_full`/`snapshot`/`list_backups`/`restore` を
      Parquet (arrow/parquet crate) ベースで実装(todo!()パニック2件を解消)。
      SHA-256チェックサム検証・MANIFEST.json永続化込み。単体テスト4件追加。
      **注記**: `snapshot()` は現状「コミットタグ付きの全データ Parquet
      ダンプ」であり、ドキュメントが元々意図していた Prolly Tree の
      reference counting による真の差分のみ CoW 保存(O(変更量))は
      未実装(将来の最適化として残す・パニックはしない)。S3/SFTP宛先は
      未接続のため明示的にエラーを返す(Localのみ実装済み)。
    - `aruaru-migrate`: `from_csv.rs`/`from_postgres.rs` のTODOスタブを
      実装に置き換え、`from_mysql.rs`/`from_parquet.rs` を新規追加
      (Snowflakeエクスポートも Parquet経路を共有)。読み出しは
      `aruaru-registry` の `PgWireAdapter`/`MySqlAdapter` を再利用、
      書き込みは新設の `crate::target::TargetClient`
      (tokio-postgres経由でaruaru-DBへ`CREATE TABLE IF NOT EXISTS`+`INSERT`)
      で行う。SQL組み立ては `sql_build.rs` に切り出し、クォート処理を
      単体テストで検証(ネットワーク接続なしで検証可能な部分は全てテスト化)。
      `main.rs` のCLIも `run_migration()` を実際に呼び出すよう修正。
    - `aruaru-graphql`: `admin_resolvers.rs` の `backups()` クエリ(空配列
      固定のTODO)を実装。`AdminCtx` に `backup: Arc<BackupEngine>` を追加し、
      `create_backup`/`restore_backup` ミューテーションも実バックエンド呼び
      出しに変更(`aruaru-server` の `main.rs` で `<data>/backups` を宛先に
      `BackupEngine` を構築し配線)。
    - 上記により todo!()/unimplemented!()/TODO/FIXME は **0件** になった
      (grep で再確認済み)。
  - `cargo check --workspace` / `cargo test --workspace` は全て成功。
    テスト数は 55件 → **76件**(aruaru-backup 4件・aruaru-migrate 9件を
    新規追加、既存の破損なし)。
  - `.gitignore` を新規作成し `target/` を除外。`Cargo.lock` は
    `origin/backup-before-github-merge-20260705` ブランチでは追跡されて
    いた実績(バイナリを持つワークスペースの慣行)に合わせ、今回追跡対象に
    追加した。
  - `README.md` のクレート構成表に `aruaru-registry`/`aruaru-backup` の
    行が抜けていたため追加(`aruaru-migrate`の説明にMySQLも追記)。
  - このCLAUDE.mdの技術スタック節を、open-raid-z 側で2026-07-10に確定した
    「Tauri/Poem/Cosmo非依存・Rust+tokio/hyper+WASM」方針の文言に同期。
    ただし本リポジトリのコード自体(poem/async-graphql-poem/Tauri管理GUI)は
    **この方針にまだ移行していない** ため、その旨を明記した(上記の
    「aruaru-db 固有の注記」参照)。

- **次回以降の巡回で確認・対応すべきこと**:
  1. **Poem/Tauri 依存の剥離**: open-raid-z の2026-07-10方針転換に本リポジトリ
     を追随させる場合、`aruaru-graphql`(poem/async-graphql-poem)・
     `aruaru-server`(poem HTTPサーバ)・`admin/`(Tauri+TypeScript管理GUI)の
     置き換えが必要。影響範囲が広いため専用のマイグレーションパスとして
     計画すること(pgwireへの依存はPostgreSQLワイヤ互換のためのものであり
     別軸— 剥離対象ではない)。
  2. `aruaru-backup` の真のCoWスナップショット(Prolly Tree reference
     counting による差分のみ保存)は未実装。現状は毎回全データをParquetへ
     フルダンプする簡易実装。大規模データでの性能が問題になれば対応する。
  3. `aruaru-backup` のS3/SFTP宛先は未接続(`local_dest()`がエラーを返す)。
     実際のオブジェクトストレージ/SFTPクライアント接続は未実装。
  4. `origin/backup-before-github-merge-20260705` ブランチは引き続き
     用済みだが削除しないこと(履歴保全のため)。
