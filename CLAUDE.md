# 設計思想・開発方針・開発環境ルール(全リポジトリ共通ヘッダー、2026-07-15追記)

> **📌 2026-08-26追記: Windows用インストーラー`aruaru-db-installer.exe`を
> 新設(命名規則統一)**: ユーザー指示「パワーシェルでインストールする
> 関連リポジトリは全て、リポジトリ名-installer.exeに統一して」への対応
> (aruaru-llm/open-englishと同じ規則)。`installer/windows/aruaru-db.iss`
> ——既存`install.ps1`のサービス登録ロジック(`New-Service`)をそのまま
> 呼ぶGUIラッパー。**正直な開示**: aruaru-dbはWindowsサービスとして
> 登録される設計のため、サービス登録自体が管理者権限を必須とする——
> `PrivilegesRequired=admin`とし、Inno Setup標準のUAC昇格プロンプトを
> 1回表示する形にした(aruaru-llm/open-englishのような`lowest`にはできない、
> 「管理者権限が一切不要になる」わけではなく「利用者が自分で管理者権限の
> PowerShellを探して起動する手間を無くす」ことが目的)。`[UninstallRun]`で
> サービス解除も追加(`Remove-Service`はPowerShell 6+限定でこの環境の
> Windows PowerShell 5.1には存在しないことを実際に確認したため、
> `sc.exe delete`を使用)。**実機検証**: `cargo build --release --bin
> aruaru-server`成功、`ISCC.exe`で実際に`aruaru-db-installer.exe`を生成
> (警告0件)。実際のサービス登録(管理者権限でのインストール実行)は
> この開発環境では未検証(既存`install.ps1`のロジック自体は変更して
> いないため、これまでの動作実績をそのまま引き継ぐ設計)。
> CI(`.github/workflows/release.yml`)もInno Setupビルドを追加しGitHub
> Releaseへ添付するよう更新済み(実CI実行の確認は次回タグpush時)。

> **📌 保留タスク(2026-08-06、次回セッションで着手予定)/ Pending task (added 2026-08-06, to be started next session)**:
> ユーザー指示により、**東芝の疑似量子コンピューター技術(Simulated
> Bifurcation Machine)**と**DeepSeekの技術**(インターネットニュースだけ
> でなく、論文〈DeepSeek-V3/R1テクニカルレポート等〉・実装ノウハウの
> ブログまで日英両言語でGoogle/GitHub調査)を、`dream-os`/`open-directx`/
> `open-cuda`/`aruaru-llm`/`open-web-server`/`RPoem`/`open-raid-z`/
> `aruaru-db`の8リポジトリへ組み込む構想がある。東芝SBMは`dream-os`
> (`sbm_ising`カーネル、64スピンPoC)に実装済み——他リポジトリへの適用は
> 各リポジトリで「何を最適化するか」を先に特定してから着手すること
> (このリポジトリ固有の候補は未検討、次回調査対象)。DeepSeekは前回調査で
> 「数千枚のGPUを1枚に圧縮する技術」という主張は確認できなかった(誤解・
> 誇張と判断済み)——今回は論文・実装ブログまで調査範囲を広げ、実在する
> 技術(MLA・DeepSeekMoE・FP8混合精度学習等)を特定してから適用箇所を
> 検討すること。詳細は`dream-os/CLAUDE.md`の同日HANDOFF参照。
>
> By user instruction, there is a plan to incorporate **Toshiba's
> pseudo-quantum-computer technology (Simulated Bifurcation Machine)**
> and **DeepSeek's technology** (researched via Google/GitHub in both
> Japanese and English, going beyond news articles to actual papers
> like the DeepSeek-V3/R1 technical reports and implementation-notes
> blogs) into 8 repositories: `dream-os`, `open-directx`, `open-cuda`,
> `aruaru-llm`, `open-web-server`, `RPoem`, `open-raid-z`, and
> `aruaru-db`. Toshiba SBM is already implemented in `dream-os` (the
> `sbm_ising` kernel, a 64-spin PoC) — applying it elsewhere requires
> first identifying a concrete optimization problem in each repo (not
> yet investigated for this repo). The previous DeepSeek research found
> no evidence for a "compress thousands of GPUs into one" technology
> (judged to be a misunderstanding/exaggeration) — this time, broaden
> the research to papers and implementation blogs, identify real
> techniques (MLA, DeepSeekMoE, FP8 mixed-precision training, etc.),
> then decide where they apply. See the same-day HANDOFF entry in
> `dream-os/CLAUDE.md` for details.


## 1. 比較的新しい言語・フレームワークの参照資料一覧

Rust自体は歴史があるが、本エコシステムが採用する **Poem** のような
比較的新しい・情報量がまだ少なめのWebフレームワークは、Python+FastAPIの
ような広く普及した組み合わせと比べ、AIモデルの学習データ・公開されている
実装例/Q&A/ブログ記事の絶対量が少ない傾向がある。そのため、AI駆動開発
(Claude等)がこれらを扱う際、実装の勘違い・API名の記憶違い・古いバージョン
のAPIでの実装(本プロジェクトで実際に複数回発生した既知の失敗パターン)に
よる**手戻り・いたちごっこ**が起きやすい。

対策として、AIが作業を始める際は、以下から**そのタスクに必要な部分だけ**を
先に参照してから実装に着手すること(全部読む必要はない。関連しそうな1〜2件を
拾い読みする程度で十分)。これにより歩留まりが上がり、AI駆動開発の手戻りが
減ることが期待される。

| 技術 | 公式ドキュメント | GitHub | 補足・ブログ等 |
|---|---|---|---|
| Rust言語本体 | https://doc.rust-lang.org/book/ | https://github.com/rust-lang/rust | https://blog.rust-lang.org/ |
| Poem(Webフレームワーク) | https://docs.rs/poem/latest/poem/ | https://github.com/poem-web/poem | https://crates.io/crates/poem |
| Tokio(非同期ランタイム) | https://tokio.rs/tokio/tutorial | https://github.com/tokio-rs/tokio | https://tokio.rs/blog |
| async-graphql | https://async-graphql.github.io/async-graphql/en/index.html | https://github.com/async-graphql/async-graphql | https://crates.io/crates/async-graphql |
| Tauri | https://tauri.app/ | https://github.com/tauri-apps/tauri | https://tauri.app/blog/ |
| wasm-bindgen / web-sys | https://rustwasm.github.io/wasm-bindgen/ | https://github.com/rustwasm/wasm-bindgen | https://rustwasm.github.io/docs/book/ |
| SurrealDB | https://surrealdb.com/docs | https://github.com/surrealdb/surrealdb | https://surrealdb.com/blog |
| sqlx | https://docs.rs/sqlx/latest/sqlx/ | https://github.com/launchbadge/sqlx | |
| WinFsp | https://winfsp.dev/ | https://github.com/winfsp/winfsp | |
| DirectX 12 / DirectML | https://learn.microsoft.com/en-us/windows/win32/direct3d12/directx-12-programming-guide | https://github.com/microsoft/DirectML | https://devblogs.microsoft.com/directx/ |
| WebAssembly(wasm32全般) | https://webassembly.org/ | https://github.com/WebAssembly | https://rustwasm.github.io/docs/book/ |

> ⚠️ **重要な注意(正直な開示)**: このURL一覧は、Web検索ツールを持たない
> セッションで学習データに基づき記載したものであり、**実在性・現在の
> 有効性・記載内容の正確性を検証していない**。特にAI(Claude含む)が
> このリストを鵜呑みにして実装や回答の根拠にすることは避け、
> **開発者自身が実際にアクセスして確認する**か、Web検索が使える
> セッションで一次情報を再確認してから利用すること。リンク切れ・
> リダイレクト・バージョン変更(特にAPIの破壊的変更)の可能性を
> 常に考慮する。新しい技術を追加する場合はこの表に追記していくこと。

## 2. AI駆動開発ツールに関する所感(2026-07-15、ユーザー所感として記録)

2026-07-15時点、ChatGPT等の汎用AIチャットは小規模なWebアプリ程度までは
開発できるものの、システムがある程度複雑・大規模になると出戻りが大きくなり、
一度に扱えるプログラムサイズにもすぐ限界が来る傾向がある。

Claude Code / Claude Desktopは、ローカルドライブを直接指定してファイルの
読み書きができ、GitHubリポジトリの読み出し(本プロジェクトのような
複数リポジトリにまたがるエコシステム)にも対応できるため、本プロジェクトの
ような規模のAI駆動開発には適していると考えられる。新しくAI駆動開発環境を
セットアップする際の選択肢として推奨する。

---

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

**open-web-server 拡張要件との関わり(2026-07-13、要約を統合・整理)**:
`open-web-server` は、3Dオンラインゲームのアイテム課金やクレジット
カード決済のような金融データを扱う、24時間365日ノンストップ運用の
ミッションクリティカルな Web サーバー。4層防御通信による高セキュリティ
と高速性の両立、およびZFS互換(`open-raid-z`)とACID互換(PostgreSQL)の
ハイブリッド技術を核として、poem-cosmo-tauri/open-runo・PostgreSQL・
このリポジトリ(`aruaru-db`)・`open-raid-z`と連携する多層防御
アーキテクチャにより、二重課金・データ消失を防ぐ。通信層の四重化
(TCP-IP・UDP-IP・QUIC・MPTCP/SCTP相当)・DB書き込みの四重化
(PostgreSQL・aruaru-db・マルチリージョン同期レプリケーション・独立
監査ログ、全系統実装済み)・VersionLessAPIとGit管理(このリポジトリの
Git-on-SQL特性を利用)のハイブリッド版管理の詳細・進捗は
`open-web-server/CLAUDE.md`(および正本の open-raid-z `CLAUDE.md`)を
参照。このリポジトリは分散Git-on-SQLデータ層として関与する
(ZFS互換スナップショット連携=`aruaru-dist::snapshot_pairing`、実装済み)。

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

### パフォーマンス・並行処理方針(2026-07-13、ユーザー指示)

システム全体として、4層4重の通信・DB冗長化によるハイセキュリティを
保ちつつ、ハイパースレッディング/マルチコア/マルチスレッドを活かした
高速性を両立させる。**非同期(tokio、マルチスレッドランタイム)を基本**
とし、必要な場面(CPU負荷の高い計算・厳密な順序保証が必要な処理等)での
み同期処理を用いる。着眼点: (1) `#[tokio::main]`のランタイムflavorが
current_threadに固定されていないか、(2) async関数内でのブロッキング
I/O・CPU負荷処理は`tokio::task::spawn_blocking`へ退避、(3) CPU律速な
処理(チェックサム計算・OLAPクエリ等)は`rayon`/`DataFusion`の並列実行を
活用、(4) セキュリティクリティカルなホットパスの排他ロックがボトル
ネックになっていないか、を確認する。

- **aruaru-db 固有の注記**: `aruaru-graphql`/`aruaru-server` は現状 `poem` +
  `async-graphql-poem` に直接依存している(上記の通り未移行)。

## API設計思想(参考・概念のみ)

- **VersionLess API**という考え方を参考にする(WunderGraphのブログ/podcast参照)。
- **WunderGraph Cosmo**: パッケージとしては直接依存させない。GraphQL
  Federation / VersionlessAPI というAPI形状・コンセプトのみ参考にし、
  Rust標準+tokio/hyperで互換性を保ちつつ自前実装する。
  https://github.com/wundergraph/cosmo

## 契約不要の独自AI(open-cuda × aruaru-llm SET、2026-07-18追記、正本はopen-raid-z参照)

外部AI事業者との有償契約・APIキー(OpenAI等)を必要としない、自前完結の
AI機能が必要になった場合は、`open-cuda` + `aruaru-llm` のSET構成を標準
として使うこと。詳細は`open-raid-z/CLAUDE.md`の同名節を参照。

## 「分身の術」構成の対象拡大(2026-07-18追記、正本はopen-raid-z参照)

`open-web-server`の「分身の術」(共有バックエンドインスタンスへの動的
テナント登録、個別インストール不要)を、`open-cuda`・`aruaru-llm`・
`RPoem`・`RCosmo`・`open-raid-z`・**このリポジトリ(`aruaru-db`)**にも
適用する。マルチCPU/マルチコア/マルチスレッド非同期処理対応、
`POST /admin/tenants`系APIでの動的テナント登録、管理は`open-easy-web`
側から行う。現状`aruaru-llm`にのみ`src/tenants.rs`実装済み、このリポジトリ
への展開は未着手(次回以降の実装対象)。詳細は`open-raid-z/CLAUDE.md`参照。

## 関連プロジェクト

- **poem-cosmo-tauri**(open-runoと同時並行開発。実装の先行地点。Pure Rust
  + tokio/hyper直接実装): https://github.com/aon-co-jp/RPoem
- **open-runo**: https://github.com/aon-co-jp/open-runo
- **open-web-server**: https://github.com/aon-co-jp/open-web-server
- **aruaru-db**(このリポジトリ): https://github.com/aon-co-jp/aruaru-db
- **open-easy-web**(第二のKUSANAGI、ドメイン/サブドメイン簡単登録+HTTPS
  自動監視/発行/更新の易操作ツール。高速化機能は含まない、2026-07-13に
  aruaru-webから分離): https://github.com/aon-co-jp/open-easy-web
- **aruaru-web**(2026-07-13廃止。役割はopen-easyweb(易操作)と
  open-runo/poem-cosmo-tauri(高速化)へ分割継承済み): https://github.com/aon-co-jp/aruaru-web
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
- **各無人開発パスの最後には、必ず以下を実行すること**(ユーザー指示、
  2026-07-11): (1) 世界10ヶ国語のREADME(`README-<言語>.md`、日本語・
  英語・中国語簡体字・韓国語・スペイン語・フランス語・ドイツ語・
  イタリア語・ロシア語・アラビア語)を、そのパスで行った実装内容に
  合わせて更新する、(2) 他プロジェクトへのお引越し可能ファイル
  (`PORTING.md`)を同様に最新の実装内容に合わせて更新する、(3) この
  開発環境ルール(`CLAUDE.md`)のHANDOFF節・現状節を更新して保存する、
  (4) 上記(1)〜(3)を含む全ての変更をコミットしてpushする。この4点を
  終えてから、そのパスを終了する。
- **ドキュメント(parity調査・ギャップ一覧・HANDOFF等)に「未調査」「未着手」
  「意図的に先送り」「将来検討」等と記録されている項目を見つけた場合、
  それは承認待ちの提案ではなく、次に着手すべき実装対象そのものである**
  (ユーザー指示、2026-07-12)。「ご指示はありますか?」のように追加の
  確認を求めて手を止めず、確認不要でそのまま実装を進めること。
  優先度・実現可能性の判断(例: ブラウザ実行という設計上の制約で
  技術的に不可能な項目は代替案を検討する)は自分で行ってよいが、
  「未着手だから今回は見送る」という判断そのものをユーザーへの
  確認なしに下してはならない——見送る場合も、まず着手を試み、
  真に不可能/著しく非現実的と判明した場合のみ、その理由をドキュメントに
  明記した上で次の項目に進む。
- **技術選定・仕様確認で迷った場合、必要に応じて日本語と英語の両方で
  Google検索し、Claude(自分自身)の知識・推論も動員し、GitHubでも
  調査すること**(ユーザー指示、2026-07-13)。
  学習データからの推測だけに頼らず、実在するクレート・ライブラリの
  現状(バージョン・メンテナンス状況・プラットフォーム対応)や、
  最新の実務知見(2026年時点のベストプラクティス等)を実際に検索して
  裏付けを取ってから実装判断を下す。日本語のみ・英語のみでは見つからない
  情報が言語を変えると見つかることがあるため、両言語での検索を基本とする。
- **日英Web検索の結果、CockroachDB/TiKV等の最先端の実運用システムが
  既に対応済みと判明した技術的ギャップは、「今のところは大丈夫」という
  報告に留めず、確認を求めず自動でそのまま実装に着手すること**
  (ユーザー指示、2026-07-23。正本は`open-raid-z/CLAUDE.md`同日エントリ
  参照)。このリポジトリ自身がこの方針の最初の適用例——Raftが単一
  グループのままだった設計をMulti-Raft(Range単位の独立Raftグループ)へ
  実際に追従させた作業を本日のHANDOFFに記録している。
- **よほど確認が必要な場面(重大な破壊的操作・仕様の根本方針転換等)を
  除き、確認を求めて手を止めないこと**(ユーザー指示、2026-07-13)。
  技術選定や実装方法で分からないこと・迷うことがあれば、まず上記の通り
  日本語・英語両方でのGoogle検索・GitHub調査を行い、それでも判断が
  つかない場合は自分の工学的判断で最も妥当な選択をして実装を進める。
  「〜については確認が必要です」と言って作業を止め、ユーザーの回答を
  待つことを既定の振る舞いにしない。
- **バックグラウンド実行(ビルド・テスト・サブエージェント)を「見失わない」
  ための定期確認と、無人での自動再実行**(ユーザー指示、2026-07-18、
  正本は`open-raid-z/CLAUDE.md`参照)。背景: 実際に発生した事象として、
  (a) サブエージェント並列起動時、完了通知前にタスク管理側のIDが失効し
  `No task found`となった(実作業自体は`git status`/`git diff`で裏取り
  でき正常完了していた——**タスク管理メタデータの消失と実際の作業結果は
  別物**)、(b) サブエージェントが最終応答として実装要約ではなく独り言的な
  テキストのみ返した(これも実際にはファイル変更が完了していた)、
  (c) 長時間ビルドがタイムアウトで打ち切られ`could not compile`相当の
  ログが出たが実際は単なる時間切れだった(タイムアウトを伸ばして再実行
  したら成功)。対応方針: (1) バックグラウンド処理が動いている間は放置
  せず一定間隔で状態を能動的に確認する(無意味な高頻度ポーリングはしない)。
  (2) タスク管理システムの応答を鵜呑みにせず、`git status`/`git diff`・
  ビルド/テストログの実際の中身(本物のコンパイルエラーかタイムアウトに
  よる強制終了(exit code 124/143等)かの区別)・生成物の実在確認で必ず
  裏取りする。(3) 裏取りの結果、作業が実際に失われている/失敗している
  場合は確認を求めず自動的に再実行・修正する。(4) 作業自体は完了して
  おり通知だけ欠落していた場合は、二重実行を避けその旨を記録して先に
  進む。(5) これらの判断はユーザーへの確認なしに自分で行ってよい。

## 運用ルール追記(2026-07-18、正本はopen-raid-zのCLAUDE.md参照) — 確認不要の自動継続・リミット解除後の自動再開

- **コンテキストウインドウ・5時間利用制限・その他のセッション中断が
  発生し、その後リミットが解除されて新しいセッションが開始された場合、
  「続けてよろしいですか」等の確認を挟まず、毎回自動的に前回セッションの
  続きの作業を再開すること**(ユーザー指示、2026-07-18)。具体的には:
  1. セッション開始時、各リポジトリの`git status`/`git log`と、この
     `CLAUDE.md`(および他プロジェクトのCLAUDE.md)のHANDOFF節・
     「次にすべきこと」記載を確認し、未完了・未pushの作業が無いかを
     まず裏取りする(タスク管理メタデータを鵜呑みにしない既存方針と
     同じ姿勢で、実際のgit状態を確認する)。
  2. 未完了作業が見つかった場合、ユーザーへの確認を求めず、そのまま
     自動的に検証(build/test)→修正→コミット→pushまで完了させる。
  3. 完了している場合は、各CLAUDE.mdの「次にすべきこと」「未着手・
     未完成」に記載された次の項目へ確認なしに着手する(既存の
     「未着手だからといって確認を求めて手を止めない」方針の延長)。
  4. 「続けてよろしければそのまま自動開発を継続します」のような、
     続行そのものを尋ねる確認は今後一切行わない(ユーザー指示、
     2026-07-18)。作業内容の要約・進捗報告はしてよいが、それは
     承認を求めるものではなく完了報告として書く。
  5. こまめにコミット・pushしておくことで、次回セッションが「どこから
     再開すべきか」を迷わず`git log`/CLAUDE.mdから機械的に判断できる
     ようにしておく(区切りがついた時点で都度コミット・pushする既存
     方針との組み合わせ)。


## 運用ルール追記(2026-07-19、正本はopen-raid-zのCLAUDE.md参照) — 白画面バグ等を見逃さない検証徹底

- **WEB/UIを持つ機能を実装した後は、ビルド成功・`cargo test`・curlでの
  ステータスコード確認だけで「完了」と報告せず、実際に画面が正しく
  表示される(白画面・レンダリング崩れ・コンソールエラーが無い)ところ
  まで確認すること**(ユーザー指示、2026-07-19)。
  1. ブラウザ操作が可能な環境では、実際にページを開いて表示内容
     (見出し・本文・想定した要素の存在)とコンソールエラーの有無を
     確認する。
  2. ブラウザ操作ができない環境では、少なくとも`curl`等でHTMLボディの
     中身を取得し、期待される文字列が実際に含まれているかを確認する
     ——ステータスコード200だけを見て「動作確認済み」としない。
  3. 白画面・エラー・期待した内容の欠落等の不具合が見つかった場合は、
     確認を求めず自動的に原因調査・修正・再確認まで行う。
  4. 本番ドメインが未取得・DNS未設定なだけの状態は上記の「白画面
     バグ」とは別物であり、混同しない(`localhost`確認で代替可)。


## 現状(このリポジトリ固有)・重要な引き継ぎ事項

- **2026-07-23(続き2) HTAP(TiDB/SingleStore/Snowflake Unistore方式)の
  ギャップを日英Web検索で発見・対応着手中(正本はopen-raid-z/CLAUDE.md
  同日エントリ参照)**: ユーザー指示「Snowflake×CockroachDBハイブリッド
  の最新動向を調査」を受け、この設計思想がHTAP(Hybrid Transactional/
  Analytical Processing)という確立パターンとして実在すると判明
  ([PingCAP: Real-World HTAP](https://www.pingcap.com/blog/real-world-htap-a-look-at-tidb-and-singlestore-and-their-architectures/))。
  **発見したギャップ**: `aruaru-query::olap.rs::run_olap`は、OLAP
  クエリのたびに`engine.snapshot_tables()`で**全テーブルを行ストアから
  毎回フル再構築**する設計(`olap.rs`冒頭の「現段階の制約」に単一ノード
  MPPである旨は明記されていたが、フル再構築であることの性能上の
  トレードオフは未記載だった)。TiDB(TiKV行ストア→TiFlash列ストアへ
  リアルタイムインクリメンタル同期)のような「変更差分だけ列ストアへ
  反映する」設計にはなっていない。
  **対応方針**: 既存の`dirty`集合(DUAL DATABASEミラー用、`persist_row`
  で`(table, pk)`を記録)とは別に、テーブル単位の粒度で列キャッシュの
  無効化を追跡する仕組みを追加し、変更の無いテーブルは前回のArrow
  RecordBatchキャッシュを再利用、変更されたテーブルのみ再構築する
  ——という現実的なスコープでのインクリメンタル同期を実装する(次の
  HANDOFFエントリで実装完了を記録)。真のマルチノード分散HTAP
  (TiKV/TiFlash間のネットワーク越しレプリケーション相当)は、
  aruaru-distのRaftがまだ単一プロセス内(ネットワーク越し複製は
  openraft統合待ち)であるため範囲外。

- **2026-07-23(続き3) HTAP列キャッシュ`OlapCache`を実装完了**:
  `crates/aruaru-query/src/engine.rs`に`olap_dirty_tables:
  RwLock<HashSet<String>>`を新設(既存のDUAL DATABASEミラー用`dirty`
  集合とは意図的に別集合——同じ集合を2つの消費者で共有すると片方が
  `take`で先にクリアしもう片方が変更を見逃す実バグになるため分離)。
  `persist_row`/`persist_delete`/`persist_schema`/`persist_drop`の
  全てでテーブル名を記録、`is_olap_table_dirty`/`clear_olap_dirty`を
  公開。`snapshot_table`(1テーブルのみ、全テーブル走査の
  `snapshot_tables`を避ける)も追加。
  `crates/aruaru-query/src/olap.rs`に`OlapCache`新設:
  `refresh()`が変更のあったテーブルだけ`snapshot_table`+Arrow
  `RecordBatch`再構築、変更の無いテーブルは行ストアに一切触れず
  前回のキャッシュを再利用する。既存の`run_olap`(毎回全テーブル
  フル再構築)は後方互換のため残置。
  **検証**: `olap_cache_reuses_unchanged_tables_and_rebuilds_only_
  dirty_ones`(新規)——2テーブル(`orders`/`customers`)を用意し、
  `customers`だけを更新した後も`orders`が`is_olap_table_dirty`に
  ならないこと、`customers`は正しく再構築され新しい行が反映される
  こと、両方を実証。`cargo test -p aruaru-query`**41件全green**
  (既存39件+新規2件)。
  **正直な開示・スコープの限界**: (1) 粒度はテーブル単位——1行でも
  変更されたテーブルはテーブル全体を再構築する(TiFlashのような
  真の行単位インクリメンタル反映ではない)。(2) 単一プロセス内のみ
  ——TiKV/TiFlash間のようなネットワーク越しの別ノードへの列レプリカ
  配置は、aruaru-distのRaftが単一プロセス内実装のため範囲外。
  - 次にすべきこと: (1) `aruaru-server`の本番経路への`OlapCache`配線
    (現状は`aruaru-query`クレート内の新機能のみ、呼び出し元は未接続)、
    (2) 行単位の真のインクリメンタル反映、(3) Multi-Raft
    (`aruaru-dist::multi_raft`)との統合——Range単位でOLAP列キャッシュも
    分割する余地がある。

- **2026-07-23(続き4) 上記(2)「行単位の真のインクリメンタル反映」を
  日英Web検索でTiFlashの実設計を調査の上、実装完了**: ユーザー指示
  「そのギャップ(テーブル単位粒度という限界)に対する再設計方法を
  日英で検索して開発に活かして」を受けて着手。
  **調査結果**: TiFlashは「Delta Tree」という設計(B+木とLSM木の
  ハイブリッド)を採用し、列エンジンを書き込み最適化領域(デルタ、
  行ストア形式)と読み出し最適化領域(ベース、列ストア形式)に分割、
  新規行データはまずデルタへ書かれ、後でバッチ変換されて列ストアへ
  マージされる、と判明
  ([TiFlash Overview](https://docs.pingcap.com/tidb/stable/tiflash-overview/)、
  [TiDB internals: TiFlash column storage](https://internals.tidb.io/t/topic/590))。
  SQL Serverの列ストアインデックスも同様の「デルタストア」(行ストア
  形式で新規行を保持、後で列形式へマージ)を持つことも確認、業界横断で
  確立した設計と裏付けが取れた。
  **再実装**: `engine.rs`のOLAP追跡を、テーブル単位の`bool`集合
  (`olap_dirty_tables`)から**行単位のpk集合**
  (`olap_delta_pks: HashMap<table, BTreeSet<pk>>`)へ全面的に再設計。
  スキーマ変更(CREATE/DROP TABLE)は別集合`olap_schema_dirty`で扱う
  (列定義自体が変わるため行単位デルタでは対応できない)。
  `snapshot_table`をpk込みで返すよう変更(`(cols, pks, rows)`、
  `pks[i]`と`rows[i]`が対応)。新規`get_row(table, pk)`で1行だけの
  再取得を可能にした。
  `olap.rs`の`OlapCache`を「ベース(Arrow列バッチ+対応するpk配列)+
  変更されたpk集合」を保持する設計へ書き換え。クエリのたびに:
  (1) `arrow::compute::filter_record_batch`(列指向の軽量フィルタ
  カーネル、文字列パース不要)でベースから変更されたpkの行を除去、
  (2) 変更されたpkだけを`get_row`で読み直し(テーブル全体ではない)、
  小さなデルタバッチを構築、(3) `arrow::compute::concat_batches`で
  結合し次回のベースとして採用(即時コンパクション)。
  **検証**: `olap_cache_incremental_merge_handles_update_delete_and_
  insert_correctly`(新規)——3行のテーブルに対し1行更新・1行削除・
  1行新規追加を行い、`SUM`が二重集計無く正しい値(古い値がベースに
  残っていないこと)、`COUNT(*)`が正しい行数(削除が反映されている
  こと)を実証。既存の`olap_cache_reuses_unchanged_tables_and_
  rebuilds_only_dirty_ones`もAPI名を`has_pending_olap_delta`
  (旧`is_olap_table_dirty`)へ追従して更新、green維持。
  `cargo test -p aruaru-query`**42件全green**(前回41件+今回1件)。
  `cargo test --workspace`(全クレート)リグレッション無し。
  **正直な開示・スコープの限界(前回よりは縮小したが残っている)**:
  (1) 単一プロセス内のみ——TiKV/TiFlash間のネットワーク越し列レプリカ
  配置は引き続き範囲外(openraft統合待ち)。(2) 毎回即時コンパクション
  する設計であり、TiFlashのように「デルタ層が一定サイズになるまで
  複数バッチのまま保持し、閾値到達時にまとめてコンパクション」という
  最適化はしていない——書き込みのたびに軽量なフィルタ処理は発生する
  (ただし文字列→型付き配列変換という重い処理は変更行数分だけで済む
  ため、既存の「毎回全テーブルをフル再構築」より確実に軽い)。
  - 次にすべきこと: (1) `aruaru-server`の本番経路への`OlapCache`配線
    (引き続き未接続)、(2) デルタ層のバッチ保持+閾値コンパクション
    (現状は毎回即時コンパクション)、(3) Multi-Raftとの統合。

- **2026-07-23 Multi-Raft(CockroachDB/TiKV方式)を新規実装
  ——「最先端追従の方針」の最初の適用例**: ユーザーから、単一Raftグループ
  のままでは将来のスケール限界になり得るという指摘に対し「今は問題ない」
  という報告で終わらせず、CockroachDB/TiKVが既に採用しているMulti-Raft
  方式へ実際に追従するよう指示を受けた(日英Web検索でRaftが2026年時点
  でも最良のデフォルト選択と確認済み、詳細は
  [systemdesignhandbook.com](https://www.systemdesignhandbook.com/guides/raft-consensus-algorithm/))。
  1. **発見した実欠落**: `shard::topology::ClusterTopology`(Range単位の
     ルーティング表、CockroachDB方式のキー空間分割データ構造)と
     `raft::node::RaftNode`(単一Raftグループの合意ロジック)は、
     以前から両方存在していたが**互いに一度も接続されたことがなかった**
     ——前者はルーティング計算のみのデータ構造、後者は常に単一グループ
     として使われていた。「Multi-Raftの土台となる部品は揃っていたが
     実際には繋がっていなかった」という、このエコシステムで繰り返し
     見つかる欠陥パターンの同種の実例。
  2. **`crates/aruaru-dist/src/multi_raft.rs`新設**: `MultiRaftCluster<A>`
     が`ClusterTopology`と`HashMap<range_id, Arc<RaftNode<A>>>`を保持し、
     `propose(key, command)`がkeyの担当Rangeを解決してそのRange専用の
     独立したRaftグループへ提案を委譲する。`split(range_id, split_key,
     applier)`でRange分割時に新しい独立したRaftグループを立てる
     (CockroachDBのRange分割と同じ発想)。
  3. **正直な開示・スコープ**: `RaftNode`自体は単一プロセス内の
     ログ/適用セマンティクスのみを提供し(ネットワーク越しの選挙/複製RPC
     はopenraftに委譲する計画、`raft/mod.rs`参照)、本モジュールもその
     制約を引き継ぐ——複数の物理ノードへの実際のネットワーク複製は
     まだ無い。ここで実証したのは「Range単位で完全に独立した合意
     グループが並行して進行できる」という構造的性質そのもの。また、
     Range分割時の新グループは空のログから始まり、分割元の状態機械
     スナップショット転送は今回未実装(次回以降の課題)。
  4. **検証**: `cargo test -p aruaru-dist multi_raft`**3件全green**——
     核心となる`split_ranges_progress_independently_like_cockroachdb_
     multi_raft`は、Range分割後に一方のRangeへ3件コミットしても
     もう一方のRangeのcommit_indexが不変であることを実証(単一グローバル
     Raftグループでは得られない、Multi-Raftならではの独立性の直接証明)。
     `cargo test --workspace`**リグレッション無し全green**
     (aruaru-dist 27→30件、他クレート含め既存分すべて維持)。
  - 次にすべきこと: (1) Range分割時の状態機械スナップショット転送、
    (2) 実際のクエリエンジン(`aruaru-query::QueryEngine`)からの
    `MultiRaftCluster`利用(現状は`aruaru-server`の本番経路には未配線、
    疎結合コンポーネントとして実装した段階——`snapshot_pairing`/
    `raid_z_backend`と同じ既存の段階的アプローチ)、(3) ネットワーク越し
    の複製RPC(openraft統合、`raft/mod.rs`の既存計画)。

- **2026-07-23(続き) recordsize不一致バグを修正(拡張要件(2)のZFS
  ↔DB関連性で指摘されていた推奨事項の実施漏れ)**: 日英Web検索で
  「ZFS/RAID-Zのrecordsizeをdatabaseのブロックサイズ(PostgreSQLは
  8KB)に合わせないと書き込み増幅が発生する」という2026年時点でも
  有効な推奨事項を再確認したところ
  ([tech-champion.com](https://tech-champion.com/database/postgresql/zfs-on-postgres-recordsize-mismatch-and-write-amplification/))、
  `crates/aruaru-dist/src/raid_z_backend.rs`の`OpenRaidZSnapshotBackend::new`
  が`CHUNK_SIZE=4096`を固定でハードコードしており、PostgreSQLの8192バイト
  ページと不一致のままだったことを発見・修正(`CHUNK_SIZE=8192`へ変更)。
  `cargo test -p aruaru-dist --features open_raid_z`27件全green
  (既存の`real_raft_commit_triggers_real_raid_z_snapshot`含め回帰無し)。

- **2026-07-20(4) DUAL DATABASEミラーを全行ダンプ→差分抽出へ最適化
  (前回HANDOFFの次回候補(b)を完了)**: `aruaru_query::QueryEngine`に
  `dirty: RwLock<BTreeSet<(String, Vec<u8>)>>`を新設し、`persist_row`
  (INSERT/UPSERT/UPDATEの単一集約点)で書き込みのたびに`(table, pk)`を
  記録するようにした。`aruaru_commit`成功時、従来の
  `export_all_rows_as_json`(常に全テーブル全行を書き出す)を
  `export_dirty_rows_as_json`に置き換え——dirty集合の中身だけを
  ミラーへ渡し、呼び出し後に集合を`std::mem::take`でクリアする。
  - **未登録時のメモリリーク回避**: 当初、dirty集合のクリアを
    `if let Some(hook) = ...`ブロック内(=フック登録時のみ)に置いて
    いたが、`DUAL_DATABASE_URL`未設定でフックが無い環境では
    コミットのたびに集合が際限なく肥大化する実バグになると気づき、
    フックの有無によらず毎コミット必ずクリアする形に修正した。
  - **既知の限界(正直な開示)**: (a) `persist_delete`(DELETE文)は
    dirty集合に追加しない——現行の`MirroredMutation`は「値」を運ぶ
    形で削除(tombstone)を表現できないため、対応するには将来
    スキーマ拡張が必要(課金アイテム付与のような追記型ワークロードを
    主眼とした設計判断)。(b) `load_from`(fjallからの起動時復元)も
    `persist_row`経由でdirty集合に加わるため、再起動後の最初の
    `aruaru_commit`は復元した全行を(実際には無変更でも)再送する
    フルダンプ相当になる——安全側(過剰送信はデータ欠落より無害)に
    倒した意図的な設計。
  - **検証**: `commit_hook_only_receives_rows_changed_since_previous_
    commit`(新規、2回目のコミットで無関係な行が再送されないこと・
    3回目の無変更コミットで空になることを実証)を含む
    `cargo test -p aruaru-query`(41件)・`cargo test --workspace`
    全green。さらにWSL2の実PostgreSQL(`aruaru_dual_diff_test`
    データベース)に対し`cargo test -p aruaru-dist -- --ignored`
    (実DB往復テスト)を再実行しgreenを確認、加えて実`aruaru-server`
    バイナリを`DUAL_DATABASE_URL`付きで起動しpgwire経由で複数回
    コミットを発行、ミラー先PostgreSQLの`aruaru_dual_mirror`テーブルの
    行数増分が「今回変更した行数」と一致し、無関係な既存行が重複挿入
    されないことを`psql`で直接確認した(型チェックのみでの完了報告
    ではない)。
  - 次回以降の候補: (a) fire-and-forgetから真の同期ミラーへの格上げ
    (`execute`のasync化)、(b) 削除(tombstone)のミラー伝播対応、
    (c) 本番運用を見据えた`DUAL_DATABASE_URL`のTLS化・認証情報の秘匿。

- **2026-07-20(3) DUAL DATABASE構成を実PostgreSQLで一気通貫検証(前回HANDOFFの
  次回候補(a)を完了)**: この開発環境にDockerは無いが、WSL2に実
  PostgreSQL 18(`apt`パッケージ、`sudo`パスワード不要な`wsl -u root`
  経由)が導入済みと判明したため、それを使って実接続検証を行った
  (推測・型チェックのみでの「検証済み」報告ではない)。
  1. `cargo test -p aruaru-dist -- --ignored`を実WSL2 PostgreSQLへの
     `DATABASE_URL`付きで実行 → **green**(`mirror_then_latest_and_
     at_commit_round_trip_against_real_postgres`)。
  2. 実`aruaru-server.exe`を`DUAL_DATABASE_URL`(WSL2 PostgreSQL、
     ミラー先DB)+`ARUARU_USERS`(pgwire SCRAM認証用)を設定して起動し、
     WSL2側の`psql`から**WindowsホストのIP(WSLのデフォルトゲートウェイ)
     経由でpgwire(:15434)へ実接続**、`CREATE TABLE`→`INSERT`→
     `SELECT aruaru_commit(...)`を実行。
  3. 1回目コミット(`qty=1`)・2回目コミット(`UPDATE`→`qty=5`で再コミット)
     を発行し、`fire-and-forget`のミラーが両方とも別プロセスの実
     PostgreSQL(`aruaru_dual_mirror`テーブル)へ到達していることを
     `psql`で直接確認。**VersionlessAPI**(`ORDER BY committed_at DESC
     LIMIT 1`)が最新値`qty=5`を返すこと、**Git版管理**
     (`WHERE commit_id = '<1回目のcommit_id>'`)が過去値`qty=1`を
     返すこと(最新に上書きされていない)の両方を実データで確認した。
  - **正直な開示**: (a) この検証はDockerではなくWSL2ネイティブ
    PostgreSQLを使った(この環境にDockerが存在しないため)。手順自体は
    Docker Composeでも同様に再現可能なはずだが、Docker環境そのものでの
    確認ではない。(b) fire-and-forgetの非同期タイミング依存のため、
    `psql`での確認前に`sleep 2`を挟んだ(ミラーが即座ではなく数十〜数百
    ミリ秒後に反映される設計上の性質であり、バグではない——
    `set_commit_hook`のdoc参照)。(c) 検証後、起動していた
    `aruaru-server.exe`プロセス・一時データディレクトリ・検証用
    PostgreSQLデータベース(`aruaru_dual_test`/`aruaru_dual_live`)は
    このマシンに残したまま(次回セッションでの再検証に使える。不要になれば
    `DROP DATABASE`で削除してよい)。
  - 次回以降の候補: (a) fire-and-forgetから真の同期ミラーへの格上げ
    (`execute`のasync化)、(b) 全行ダンプから差分抽出への最適化、(c) 本番
    運用を見据えた`DUAL_DATABASE_URL`のTLS化・認証情報の秘匿(現状は
    環境変数に平文、開発検証用としては妥当)。

- **2026-07-20(2) DUAL DATABASEミラーを`aruaru-server`の実コミットパスへ配線
  — 前回HANDOFFの次回候補(a)(b)を実施**: `aruaru_query::QueryEngine`に
  `commit_hook`(`set_commit_hook`)を新設し、`aruaru_commit`成功直後に
  `(commit_id, 全テーブルの現在行(table_name, row_key, payload_json))`で
  同期・非ブロッキングに呼ばれるようにした。`aruaru-server/src/main.rs`は
  起動時に環境変数`DUAL_DATABASE_URL`が設定されていれば実PostgreSQLへ
  接続・`ensure_schema()`した上でこのフックを登録し、以後すべての
  `aruaru_commit`(pgwire経由・GraphQL経由・migrate_run経由いずれも)で
  `DualDatabaseMirror`への書き込みが自動的に発生するようになった
  (未設定時はこれまで通りミラー無効、既存動作は一切変わらない)。
  - **正直な開示(重要な設計上のトレードオフ)**: フック自体は
    `tokio::spawn`によるfire-and-forgetであり、`open-web-server-ledger::
    multi_region`が定めた「全レグの完了を待ってから呼び出し元に返す」
    という厳密な同期ポリシーからの**意図的な逸脱**である。理由は
    `QueryEngine::execute`が同期関数でありpgwireの同期経路からも呼ばれる
    ため、フック内で`block_on`すると`Cannot start a runtime from within a
    runtime`のデッドロック/パニックリスクがあるため。詳細な設計判断は
    `crates/aruaru-query/src/engine.rs`の`set_commit_hook`docコメントに
    記載。将来`execute`自体をasync化する際は、この逸脱を解消し真の
    同期ミラーへ格上げすることが望ましい。
  - **粒度**: 変更行のみの差分抽出ではなく、コミット時点の全テーブル
    全行を毎回書き出す(`export_all_rows_as_json`)。`aruaru_commit`
    自体が全テーブルを1つのProlly Treeへスナップショットする設計
    (`snapshot_root`)と同じ粒度であり、`aruaru-backup`のフルダンプ方式
    と同じ既知の限界(将来、真の差分抽出への最適化余地あり)。
  - **検証**: `commit_hook_fires_with_commit_id_and_current_rows`/
    `commit_without_hook_registered_still_succeeds`(`aruaru-query`、
    新規2件)、`cargo build -p aruaru-server`成功(既存の無関係な
    dead_code警告1件のみ)、`cargo test --workspace`全green。実
    PostgreSQLへの到達確認はこの開発環境では引き続き未実施
    (`DATABASE_URL`/`DUAL_DATABASE_URL`いずれも到達可能なPostgreSQLが
    無いため——前回HANDOFFと同じ既知の制約)。
  - 次回以降の候補: (a) 実PostgreSQL/Docker環境での`DUAL_DATABASE_URL`
    起動確認・`--ignored`統合テスト実行、(b) fire-and-forgetから真の
    同期ミラーへの格上げ(`execute`のasync化を要する大規模な設計変更)、
    (c) 全行ダンプから差分抽出への最適化。

- **2026-07-20 DUAL DATABASE構成(aruaru-db × PostgreSQL)を新規実装
  — 拡張要件(4)「DB書き込みの四重化」の②、`open-web-server-ledger`
  (①PostgreSQL WAL・③マルチリージョン・④監査ログ)と対になる本リポジトリ側の
  責務**: `crates/aruaru-dist/src/dual_database.rs`を新設。
  - `DualDatabaseMirror::mirror()`が、aruaru-db側で既に確定した
    ミューテーション(`MirroredMutation`: table_name/row_key/payload_json/
    commit_id/committed_at)を実PostgreSQLへ**同期的に**ミラーする
    (`open-web-server-ledger::multi_region`と同じ「全レグの完了を待って
    から呼び出し元に返す」判断——金融データにeventual consistencyは
    許されないという既存方針の踏襲)。
  - **VersionlessAPI + Git版管理の両立**: ミラー先テーブルは
    `(table_name, row_key, commit_id)`を保持し、`latest()`は
    `committed_at DESC LIMIT 1`で「バージョンレス」な最新値、
    `at_commit()`は`commit_id`一致行で「特定コミット時点」の値を返す
    ——aruaru-db本体の`SELECT ... AS OF COMMIT`(2026-07-13実装済み)と
    同じ意味論を、ミラー先のPostgreSQL単体からも再現できる。
  - **冪等性**: `idempotency_key`(`SHA-256(table_name\0row_key\0
    commit_id)`)に一意制約を張り`INSERT ... ON CONFLICT DO NOTHING`
    (`postgres_wal.rs`/`multi_region.rs`と同じ形状)。
  - **正直な開示**: (a) 実PostgreSQL接続での検証は未実施(この開発環境に
    到達可能なPostgreSQLが無いため、`postgres_wal.rs`と同じ既知の制約)
    ——SQL文字列・冪等性キー導出ロジックの単体テスト8件(オフラインで
    検証可能な範囲)と、`DATABASE_URL`環境変数がある場合のみ動く
    `#[ignore]`統合テスト1件の2段構え。(b) aruaru-db側のコミットと
    PostgreSQL側のミラーは独立操作であり、真の2フェーズコミットでは
    ない(`mirror()`失敗時にaruaru-db側をロールバックする手段は無い
    ——`multi_region.rs`と同じスコープの限界、失敗は`DualDatabaseError`
    で呼び出し側へ返す設計)。(c) `DualDatabaseMirror`をどこから
    呼び出すか(`aruaru-server`のコミットパスへの実配線)は今回未実施
    ——このパスは`aruaru-dist`内の独立コンポーネントとしての実装に
    留めた(`snapshot_pairing`/`raid_z_backend`と同じ、まず疎結合な
    コンポーネントとして実装してから呼び出し元へ配線する既存の
    段階的アプローチ)。
  - **検証**: `cargo test -p aruaru-dist`(26 passed, 1 ignored)、
    `cargo test --workspace`引き続きgreen。
  - 次回以降の候補: (a) `aruaru-server`のコミットパス(`admin.rs`の
    `SELECT aruaru_commit(...)`実行後)から`DualDatabaseMirror::mirror()`
    を呼ぶ実配線、(b) 環境変数(`DUAL_DATABASE_URL`等)経由でのPostgreSQL
    接続先設定・起動時`ensure_schema()`呼び出し、(c) 実PostgreSQL/Docker
    が使える環境での`--ignored`統合テスト実行確認。

- **2026-07-18 `propose_commit`未使用警告を調査(コード変更は見送り、
  無人自動開発中の判断)**: `cargo build --workspace`のdead_code警告
  (`crates/aruaru-server/src/cluster.rs`の`propose_commit`が未使用)を
  追ったところ、`admin.rs::migrate_run`(ワイヤ経由データ取り込み
  ハンドラ)が取り込み後のコミットを`state.engine.execute("SELECT
  aruaru_commit(...)")`で**直接ローカル実行**しており、Raft経由の
  `cluster::propose_commit`(`admin.rs::cluster_propose`が`propose_write`
  で使っているのと同じパターン)を一切経由していないことを発見した。
  一見、open-easy-webで見つけた「配線漏れ」と同種のバグに見えたが、
  精査した結果**安易に直すべきではないと判断した**: `migrate_run`が
  行う実データの取り込み(`engine.ingest_table`)自体がそもそも
  ローカル限定で、Raftによる複製を経由していない。この状態で
  コミットマーカーだけを`propose_commit`でRaft複製するよう変更すると、
  「実データは複製されていないのに、コミットの事実だけは複製された」
  という、現状より悪い中途半端な整合性を生む(フォロワーには存在しない
  データへの「コミット済み」マーカーだけが複製される)。分散DBの
  整合性に関わる箇所を推測で変更するリスクを避け、コード変更はせず
  この判断根拠を記録するに留めた。
  次回検討すべきこと: `migrate_run`によるバルク取り込みを、そもそも
  非クラスタ運用限定の機能として明示的にドキュメント化するか、
  あるいは取り込んだデータ自体もRaft経由で複製する本格的な対応を
  取るか、方針を決めた上で実装する。

- **2026-07-17 ZFS互換/ACID互換データ層をRust-JSON(独立リポジトリ)へ移行**:
  ユーザー指示("open-raid-z arurau-db のZFS互換とACID互換で今まで、JSON
  だった部分を今後は、RUST版 JSON Rust-JSONに置き換えて下さい")に基づき、
  `serde_json`直接依存だった箇所を新設の
  [`Rust-JSON`](https://github.com/aon-co-jp/Rust-JSON)(`F:\open-runo\Rust-JSON`
  へのpath依存、`rust-json`クレート)へ置き換えた。
  - まず`open-raid-z`本体(`open_runo_zfs_source`)を調査したが、ZFS/ACID
    コア(`open_raid_z_core`)に`serde_json`依存自体が存在しなかった
    (チェックサム等は生バイト列でJSONを経由しない) — 移行対象は
    `aruaru-db`のみだった。
  - `Rust-JSON`側に型付き厳密モードAPI(`from_slice_strict::<T>`/
    `from_str_strict::<T>`/`to_vec_strict`/`to_string_strict`/
    `to_string_pretty_strict`)を追加(既存の`parse_strict`は`Value`専用
    だったため、構造体を直接デシリアライズする既存呼び出し箇所には
    使えなかった)。これらは全て`serde_json`へそのまま委譲するため、
    出力バイト列は`serde_json`直接呼び出しと完全に同一 —
    `crates/aruaru-core/src/storage/mod.rs`のZFS互換チェックサム層
    (書き込みバイト列のSHA-256を検証する仕組み)がバイト列の同一性に
    依存しているため、この互換性は必須要件だった。
  - 移行箇所: `crates/aruaru-core/src/storage/mod.rs`(`StoredSchema`/
    行データの保存・読み出し、`StorageError::Serde`の内部型を
    `rust_json::RustJsonError`に変更)、`crates/aruaru-dist/src/raft/
    command.rs`(`Command::encode`/`decode`、Raftログのワイヤ形式)、
    `crates/aruaru-backup/src/lib.rs`(`BackupManifest`のMANIFEST.json
    読み書き、S3/ローカル両経路)、`crates/aruaru-query/src/engine.rs`
    (`QueryResponse`の冪等性キャッシュ読み書き)。
  - 各クレートの`Cargo.toml`に`rust-json.workspace = true`を追加、
    ワークスペースルート`Cargo.toml`に`rust-json = { path =
    "../Rust-JSON" }`をpath依存として追加(`aruaru-dist`の
    `open_raid_z_core`path依存と同じパターン、両リポジトリが
    `F:\open-runo`直下に並んでいる前提)。
  - **検証**: `cargo build --workspace`成功(既存の
    `propose_commit`未使用警告1件のみ、無関係)。`cargo test
    --workspace`実行、2件failed(`aruaru-backup`の
    `s3::tests::new_requires_credentials_from_env`/
    `presign_put_produces_a_signed_url_for_the_prefixed_key`)を
    `cargo test -p aruaru-backup --lib -- --test-threads=1`で
    切り分け再実行したところ全15件green —
    並列テスト実行時のAWS環境変数競合による既存の非決定性であり、
    今回のJSON移行によるものではないことを確認した(このリポジトリの
    `s3.rs`自体には一切手を入れていない)。
  - **意図的に置き換えなかった箇所**: `crates/aruaru-core/src/
    storage/mod.rs`内のテストコード(307・331・360行目付近)の
    `serde_json::to_vec`はチェックサム層自体を検証するために意図的に
    生バイト列を組み立てているテストフィクスチャであり、移行対象外。
    `crates/aruaru-registry/src/crawler.rs`(`reqwest`の`.json::<Value>()`)・
    `admin/src-tauri`・`aruaru-server/src/admin.rs`は`from_str`/parse呼び
    出しを持たず`Value`型としての使用のみのため対象外(調査済み)。
  - 次にすべきこと: 特に緊急の課題は無し。旧`open-runo-rustjson`
    (open-runo/poem-cosmo-tauri側)は当面参照実装として残置、実際に
    aruaru-db側が新クレートへ切り替わったことを確認できたので、
    旧クレート撤去の検討はopen-runo/poem-cosmo-tauri側のCLAUDE.mdで
    改めて判断する。

- **2026-07-15 コードヘルス監査 — audit only, no changes**:
  `cargo build --workspace`/`cargo test --workspace`を実行し、ビルド成功
  (警告1件: `aruaru-server`の`propose_commit`未使用、実害なしの
  dead_code警告。加えて`aruaru-backup`の増分コンパイルキャッシュへの
  アクセス拒否ノートが出たが再ビルドに影響なし)・全110テストgreen
  (2件ignored)を確認。`git status`はクリーン、修正すべき壊れたビルド・
  失敗テスト・小規模な欠落は見つからなかったため、コード変更は
  行っていない。

- **2026-07-14 pgwire拡張プロトコル(prepared statement)対応 —
  フレームワークとしての実用性ギャップを解消**: open-web-server連携の
  横断的な実用性調査でユーザーから指摘された「`describe_portal`が
  常に空列リストを返すため多くのORM/ドライバのデフォルト経路が
  失敗する」という問題に対応。`aruaru-wire`の`do_describe_statement`/
  `do_describe_portal`を、**クエリを一切実行せず**
  `aruaru_query::parser::parse`の構文解析結果+新規
  `QueryEngine::table_columns`(スキーマ参照のみ)から列名を解決する
  方式に書き換え。`Select`/`SelectAsOf`はテーブルの実スキーマから、
  `AruaruLog`・4つのGit-on-SQL関数(`aruaru_branch`/`checkout`/
  `commit`/`merge`)は`engine.rs`にハードコードされた既知の固定列形状
  (関数名と同名の単一列)から、それぞれ列情報を構築する。書き込み文は
  空列リストのまま(コマンドタグのみで実害無し)。**実行を伴わない設計
  のため、`SELECT aruaru_commit(...)`のような副作用を持つ関数呼び出しを
  誤って二重実行するリスクが構造的に存在しない**。
  **検証**: 実`aruaru-server`を起動し`sqlx`の`query_as`/`.bind()`
  (実際にORM/ドライバの多くが使う拡張プロトコル経路)で新規テスト2本
  (複数列SELECTの実デコード確認、`aruaru_commit`が拡張プロトコル経由で
  正確に1回だけコミットログに追加されることの確認)を実行、green。
  `cargo test --workspace`(全既存テスト、110件)も引き続きgreen。
  `open-runo`側の既存`aruaru_as_of_commit`統合テスト(Simple Query
  プロトコル経由の`AruaruDbBackend`)にも回帰が無いことを再確認済み。
  次回パスがすべきこと: 特に緊急の課題は無い。残るギャップは
  `select_as_of`が列射影(`SELECT col1, col2`)を無視し常にフルROWを
  返す点(低優先度、呼び出し側でインデックス指定して回避済み)。

- **2026-07-13(サニティスイープ、ドリフト無しを再確認)**: open-easyweb/
  open-web-server連携強化パスの一環として`cargo test --workspace`を
  再実行、全件green(ignoredも無し)を確認。さらに`cargo build -p
  aruaru-server`で実バイナリをビルドし、`open-runo`側の`#[ignore]`統合
  テスト(`as_of_commit_returns_the_old_value_through_the_real_pgwire_
  endpoint`、下記の`AS OF COMMIT`読み出しクエリの一気通貫検証)を実際に
  この実バイナリに対して実行し、成功することを再確認した(詳細は
  `open-runo`側の同日付CLAUDE.md HANDOFF参照)。本リポジトリ側のコード
  変更は無し。
- **2026-07-13: `AS OF COMMIT` 読み出しクエリを追加(open-web-server拡張要件(1)
  「VersionLessAPI + Git版管理ハイブリッド」の読み出し側、`open-web-server`
  側から依頼された調査・実装)**: `open-web-server/CLAUDE.md`が指摘していた
  「commit_idを指定して過去状態を問い合わせるAPIがopen-web-server側に無い」
  というギャップの、**このリポジトリの責務範囲(ストレージ/SQL層)における
  実装**。既存の`aruaru_commit`(`aruaru-query::engine::QueryEngine`)が
  `snapshot_root()`で全テーブルをProlly Treeへスナップショットし
  `VersionController::commit`でcommit_idを発行する仕組みに対し、対応する
  **読み出し**が存在しなかった。
  - `crates/aruaru-core/src/version/mod.rs`: `VersionController::get_commit_by_str(id: &str) -> Option<Commit>`
    を新設(従来は`log()`/`head()`経由の間接参照しかなかった)。
  - `crates/aruaru-query/src/parser.rs`: `Statement::SelectAsOf { table, filter,
    commit_id }`を新設。`SELECT col FROM t WHERE pk = 'v' AS OF COMMIT
    '<commit_id>'`をパースする(内部のSELECT部分は既存`parse_select`を再帰
    呼び出しして流用)。
  - `crates/aruaru-query/src/engine.rs`: `select_as_of`を実装。
    `version.get_commit_by_str(commit_id)`でcommitの`root_hash`を取得し、
    `ProllyTree::from_root(root_hash, self.store.clone())`(**既存のAPI**
    ——`ProllyTree`は元々任意のroot_hashから開けるようになっていたが、
    `QueryEngine`側でそれを使う経路が無かった)でその時点のツリーを再構築、
    `table\0pk`キーで`get()`する。キー形式は`snapshot_root()`と完全に
    揃えてある。テーブルが現存すれば列名を引き継ぎ、無ければ`col0`/`col1`.. の
    汎用列名にフォールバックする(過去データの読み出し自体は優先)。
  - **検証(実データでの一気通貫テスト)**: `as_of_commit_returns_the_value_from_that_commit_not_the_latest`
    (`engine.rs`)。同一キー(`sword`)に対し `qty=1`でコミット→`qty=5`に更新して
    再コミット→最新状態は`qty=5`だが、**最初のcommit_idを指定した`AS OF
    COMMIT`クエリは`qty=1`を返す**ことを実証(型チェックのみでの「完了」
    報告ではなく、実際に異なる値が返ることを確認)。存在しないcommit_idは
    エラーになることも確認。`cargo test -p aruaru-query`は新規1件を含む
    全37件green。
  - **正直なスコープの限界**:
    1. **単一行のみ**: `WHERE`句でPKを特定できる場合のみ対応。全表スキャンの
       `AS OF`(`WHERE`無し)は今回未対応(`ProllyTree`にテーブル横断の
       効率的prefixスキャンAPIが今回追加されていないため)。
    2. **pgwireへの配線は未実施**: `open-runo`は`aruaru-db`に対して
       pgwire(:5433)経由の汎用KVテーブル操作(`open-runo-db::aruaru::
       AruaruDbBackend`、`put`/`get`/`delete`/`list`のみ)で通信しており、
       commit/バージョンという概念自体をpgwireプロトコル越しには一切
       やり取りしていない。今回追加した`AS OF COMMIT`構文はSQLパーサー
       レベル(`aruaru-query`)の機能であり、`aruaru-server`のpgwireハンドラ
       (`aruaru-wire`)がこの新構文のクエリをそのまま透過させるかどうかは
       未検証(pgwireは基本的に任意のSQL文字列をクライアントから受け取り
       `QueryEngine::execute`に渡す設計のため、原理上は動くはずだが実際の
       pgwireクライアント(psql等)からの実行確認はしていない)。
    3. **open-runo/open-web-server側の配線は未着手**: `open-runo-router`に
       `GET /api/db/:table/:key/at/:commit_id`相当のハンドラを追加し、内部で
       上記SQLを組み立てて`aruaru`バックエンドへ投げる、という配線は
       このパスでは実施していない(cross-repo作業であり、`open-web-server`
       側のCLAUDE.md HANDOFFに詳細判断根拠を記載)。
  - 次回以降の候補: (a) pgwire実クライアントからの`AS OF COMMIT`クエリの
    実行確認、(b) `open-runo-router`への`GET .../at/:commit_id`ハンドラ追加、
    (c) 全表スキャンの`AS OF`対応。

  - **追記(同日、open-runo側セッションで(a)(b)とも完了)**: 上記(a)
    「pgwire実クライアントからの`AS OF COMMIT`実行確認」と(b)
    「`open-runo-router`への`GET .../at/:commit_id`ハンドラ追加」は
    同日中に`open-runo`リポジトリ側のセッションで実施・実バイナリ検証
    済み(詳細は`open-runo`の同日CLAUDE.md HANDOFFエントリ参照)。
    その過程で本リポジトリの`aruaru-wire`/`aruaru_query::QueryEngine`
    に起因する2つの実バグが判明した(**本リポジトリの責務範囲だが
    open-runo側の統合テストで初めて顕在化した**、参考のため記録):
    (1) `aruaru-wire`の`ExtendedQueryHandler::describe_portal`が常に
    空の列リストを返す(動的スキーマのためRowDescriptionはExecute時
    確定)ため、拡張プロトコル(prepared statement)経由で行データを
    持つ`SELECT`を実行するクライアントは`ColumnIndexOutOfBounds`で
    失敗する——`INSERT`/`DELETE`等コマンドタグのみの文は影響なし。
    シンプルクエリプロトコル(`SimpleQueryHandler`)経由なら正しく
    動く。open-runo側は該当する読み出し系メソッドをシンプルクエリ
    プロトコル(`sqlx::raw_sql`)へ切り替えて回避したが、
    **拡張プロトコルを使う他クライアント(psql自体はシンプルクエリの
    ため影響なし、だが多くのORM/ドライバはデフォルトで拡張プロトコル
    を使う)は同じ問題に当たる可能性が高い**——`aruaru-wire`の
    `describe_portal`/`describe_statement`が真に空を返すしかないのか
    (動的スキーマである以上は構造的な制約)、それとも簡易的な型推論
    (例: 実行時に一度執行してから記述する、または既知のテーブルには
    静的スキーマを返す)で改善できないかは、本リポジトリ側で改めて
    検討する価値がある。
    (2) `select_as_of`は`SELECT`の列リストを無視し常にフルROWを返す
    (列名だけは要求に応じて解決するが、射影はしない)——将来
    `SELECT col1, col2 FROM ... AS OF COMMIT ...`のような部分列指定を
    厳密にサポートする場合は`select_as_of`内で明示的な射影処理が必要。
    現状は呼び出し側(open-runo)が列インデックスを直接指定することで
    回避しているが、本質的な修正ではない。

- **2026-07-12: ZFS互換チェックサム層を追加(ZFS互換 + ACID互換のハイブリッド、
  ユーザー指示)**: `crates/aruaru-core/src/storage/mod.rs`に、open-raid-z
  (`open_raid_z_core::checksum`)と**アルゴリズム・型ともに完全同一**の
  SHA-256チェックサム(`compute_checksum`)を追加。`PersistentStore`に
  新パーティション`__checksums`を追加し、`save_row`で書き込みバイト列の
  チェックサムを必ず記録、`scan_table`で読み込み時に再検証(不一致は
  `StorageError::ChecksumMismatch`、黙って壊れたデータを返さない)。
  ZFSの`zpool scrub`に相当する`scrub()`メソッドも追加(全行を検証し
  破損箇所の一覧を返す、最初の不一致で打ち切らない)。既存のACID
  トランザクション層(BEGIN/COMMIT/ROLLBACK、Git-on-SQLコミット)とは
  直交する保証(ACID=正しい順序で確定、チェックサム=保存後にバイトが
  破損していない)。チェックサム未記録の既存データは検証をスキップし
  後方互換を維持。単体テスト4件追加(破損検出・scrub複数破損検出・
  後方互換)。**検証**: `compute_checksum`単体は分離クレートで実行し
  標準SHA-256テストベクタ(空文字列)と一致することを確認済み。
  `PersistentStore`本体(fjall統合部分)は、このサンドボックスの
  rustc 1.75では`fjall`自体がrustc 1.76+を要求するため(edition2024とは
  別の、より根本的なツールチェーン制約)実ビルド確認ができなかった。
  既存の動作実績あるパターン(`data.insert`/`data.prefix`等)を踏襲した
  最小限の追加のため目視レビューでは問題なしと判断したが、実CI/実
  ツールチェーンでの`cargo test -p aruaru-core`確認を推奨。

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

- **2026-07-13 巡回で完了した作業(aruaru-dbコミット×open-raid-zスナップ
  ショット連携、`open-web-server/CLAUDE.md`拡張要件(2)「次回新規開発予定」
  の第一段実装)**:
  - `crates/aruaru-dist/src/raft/node.rs`: `RaftNode`に
    `on_commit: RwLock<Option<Box<dyn Fn(u64) + Send + Sync>>>`フィールドと
    `set_commit_hook`メソッドを追加。`apply_committed`が適用済み最終
    ログインデックス(=commit ID)でフックを1回呼ぶ(適用対象が無い呼び出し
    では呼ばれない)。フック未登録時は何もしない(既存動作を変えない)。
  - `crates/aruaru-dist/src/snapshot_pairing.rs`(新規): `SnapshotBackend`
    トレイト(スナップショット操作の抽象化)、テスト・開発用の
    `InMemorySnapshotBackend`、`commit_index -> snapshot_id`の対応関係を
    記録・問い合わせできる`SnapshotPairingRegistry`、`RaftNode`へ配線する
    `wire_to_node`関数を実装。スナップショット失敗はRaft適用パイプライン
    自体を止めない設計(`tracing::warn!`のみ、課金/金融データの書き込み
    成功をスナップショット失敗で巻き込まない)。
  - `crates/aruaru-dist/src/raid_z_backend.rs`(新規、`open_raid_z`
    feature有効時のみコンパイル): `open_raid_z_core::pool::Pool`
    (RAID-Z2、`FileBackedDevice`6台)を実際に構築・保持し
    `create_snapshot`を呼ぶ`OpenRaidZSnapshotBackend`を実装。
    `Cargo.toml`に`open_raid_z_core`をpath依存として追加
    (`default-features = false`——WinFsp/dxc/Windows SDK不要のCPU
    フォールバックのみを使うため、`open_raid_z` feature無効時の
    デフォルトビルドには一切影響しない)。
  - **検証**: `real_raft_commit_triggers_real_raid_z_snapshot`統合テスト
    (`raid_z_backend.rs`内)で、実Raft commit(`propose`→`try_commit_to`→
    `apply_committed`)が実RAID-Z2プール上の実`create_snapshot`をトリガーし、
    `SnapshotPairingRegistry`経由の問い合わせと実プールの
    `snapshot_names()`の両方で対応関係を確認できることを実証した
    (型チェックのみでの「完了」報告ではない)。`cargo test -p aruaru-dist`
    (feature無し、21件)・`cargo test -p aruaru-dist --features
    open_raid_z`(21件、`raid_z_backend`のテストを含む)・
    `cargo check --workspace`・`cargo test --workspace`(デフォルト構成)
    すべてgreenを確認。
  - **正直なスコープの限界**: (a) 対応関係(`SnapshotPairingRegistry`)は
    現状プロセスメモリ上のみで、永続化(再起動で失われる)は未実装——
    将来`aruaru-backup`のMANIFEST.json的な永続化と統合することが想定
    される。(b) 双方向のリカバリ(スナップショットからのRaftログ巻き戻し
    等)は範囲外。(c) `open_raid_z_core`は別Cargoワークスペース
    (`open-raid-z/open_runo_zfs_source/open_raid_z_core`)へのpath依存
    であり、デフォルトのワークスペースビルド(`cargo check --workspace`
    /`cargo test --workspace`)には含まれない——`open_raid_z` feature
    (`cargo test -p aruaru-dist --features open_raid_z`)を明示的に
    有効にした場合のみコンパイル・検証される。両リポジトリが同一の
    `F:\open-runo`ドライブ配下にある前提のpath依存であり、CI環境や
    別マシンでは`open-raid-z`リポジトリのチェックアウトが同じ相対位置に
    無いと失敗する点に注意(将来的にはgitサブモジュール化やcrates.io
    公開を検討する余地がある)。

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
  3. ~~`aruaru-backup` のS3/SFTP宛先は未接続~~ **2026-07-12実装済み(S3のみ)**:
     `crates/aruaru-backup/src/s3.rs`新設。`rusty-s3`でSigV4署名付きURLを
     生成し`reqwest`で実PUT/GET/ListObjectsV2する設計(認証情報は
     `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`環境変数から取得、
     `BackupConfig`には持たせない)。`local_dest()`をS3宛先向けローカル
     ステージングディレクトリ方式に変更し、既存のParquet書き込みロジックは
     無変更のまま`write_snapshot`後にS3へアップロード、`restore`前に
     S3からダウンロードする形で配線。SFTPは今回のパスでは引き続き未接続
     (真に不可能ではなく単に見送り——次回対応)。署名付きURL生成ロジックは
     分離クレートでの実ビルド・実行テスト7件で検証済み(実S3/MinIOサーバー
     への到達確認はこの環境に無いため未実施)。
  4. `origin/backup-before-github-merge-20260705` ブランチは引き続き
     用済みだが削除しないこと(履歴保全のため)。

## アプリケーションサーバー層の役割(open-runo / poem-cosmo-tauri、2026-07-16追記)

「配信エンジン(vhost)」に`open-web-server`を選択肢として追加したが、
open-web-serverがApache＋Nginxのハイブリッド仕様のWebサーバーとして
まだ機能していない間は、Tomcatのような互換レイヤーとして機能するのは
`open-runo`または`poem-cosmo-tauri`である。

これらは`open-raid-z`とVersionlessAPIによって、バージョンレス運用と
バージョン管理・Git管理を両立しながら、ACID互換性とZFS互換性に対応した
`aruaru-db`と、PostgreSQLとのDUAL DATABASE構成による「4層4重」の
最新鋭の通信システムを構築し、仕様変更が容易なデータベース設計により、
3DオンラインゲームAI課金アイテム、オンライン金融、オンライン証券、
オンラインクレジットカード決済など、ネット上で紛失してはならない
ミッションクリティカルな用途向けに、24時間365日ノンストップの
サーバー対応WEBサイト開発を全面的にバックアップするフレームワーク・
ミドルウェアとして機能することを目指す。
---

## HANDOFF: 2026-07-24 スマホ版電源モード対応(省電力/常時電源接続/通常)調査

- **要望内容**: 「省電力版」を選ぶと実際に省電力になる、「常時電源接続版」は
  CPU+GPU+NPUのハードウェアアクセラレーター対応、電源が外れたら自動判定
  ではなく「省電力モードに切り替えますか?それとも通常モードのままにしますか?」
  とダイアログで質問し選択に応じて切替(デフォルト推奨は省電力モード)、
  電源再接続時も常時電源接続版に戻すか尋ねる導線、という要望を受領。
- **調査結果(正直な現状)**: 本リポジトリ`aruaru-db`には`android/`ディレクトリ・
  `.kt`(Kotlin)ファイルは**一件も存在しない**。Android向けのモバイル
  クライアントアプリは未着手。本リポジトリはRust製の分散DBサーバー
  (Multi-Raft、HTAP OlapCache等)であり、そもそも「スマホ単体アプリ」としての
  用途・位置づけが現時点で明確に定義されていない(モニタリング/管理アプリ
  なのか、エンドユーザー向けクライアントなのか未確定)。
  したがって今回はフルアプリ新規実装はスコープ外とし、実装は行っていない。
- **将来Androidクライアントを作る場合の電源モード管理・設計方針(未実装/設計のみ)**:
  1. **3モード**: 「省電力版」「常時電源接続版(ハードウェアアクセラレーター対応)」
     「通常版」をユーザーが明示的に選択できるUI(設定画面のRadio/Segmented
     Control)を用意する。
  2. **省電力版の具体施策**: `PowerManager`で低電力状態を検知・尊重し、
     `WakeLock`を取得しない、DBポーリング/Raftハートビート等の通信間隔を
     延長(例: 通常5秒→省電力30秒以上)、`BatteryManager`で残量が閾値以下
     なら自動でさらに間引く、HWアクセラレーター(GPU/NPU、NNAPI等)を明示的に
     無効化してCPU低クロック経路のみ使用、バックグラウンド同期は
     `WorkManager`の`Constraints.setRequiresBatteryNotLow()`等でOS側にも
     委譲する。
  3. **常時電源接続版**: `BatteryManager.isCharging`/`ACTION_POWER_CONNECTED`
     を条件にCPU+GPU+NPU(NNAPI/GPUDelegate等)を使ったハードウェア
     アクセラレーションを有効化。ポーリング間隔は最短、WakeLock許可。
  4. **電源切断時の質問ダイアログ**: 常時電源接続版モード中に
     `BroadcastReceiver`で`Intent.ACTION_POWER_DISCONNECTED`を監視し検知したら
     即自動切替はせず、AlertDialogで
     「電源が外れました。省電力モードに切り替えますか?それとも通常モードの
     ままにしますか?」と問い、選択に応じて上記のモード設定を切替える。
     デフォルトの推奨(ハイライトされるボタン)は「省電力モードに切り替える」。
  5. **電源再接続時の質問ダイアログ**: `ACTION_POWER_CONNECTED`を監視し、
     現在省電力版/通常版であれば「電源が接続されました。常時電源接続版
     (ハードウェアアクセラレーター対応)に戻しますか?」と問い、Yesなら
     常時電源接続版に切替。
  6. この設計はAndroidコードが実在する時点で初めて着手する。現状は
     この方針をここに記録するのみで、コード変更・ビルド・commit/pushは
     行っていない。

## HANDOFF: 2026-07-24(続き) Android版モニタリング/管理クライアントを新規実装・ビルド成功

- **前回HANDOFF(直上)の「未着手・設計のみ」を実装まで進めた**。
  参照実装`open-web-server/android/`(`MainActivity.kt`の3電源プロファイル
  +BroadcastReceiverによる電源抜き差し検知ダイアログ+プロファイル別
  ポーリング間隔+WakeLockパターン)を読み、同じGradle構成パターンで
  `aruaru-db/android/`を新規作成した。
- **位置づけの確定**: aruaru-db本体(Rust製分散DBサーバー)をAndroid上で
  動かすのではなく、既存の管理API(`crates/aruaru-server/src/admin.rs`の
  `GET /admin/cluster`、`main.rs`で`/admin`配下にnest済み)へリモート接続
  する**モニタリング/管理クライアント**として設計した。ネイティブ
  バイナリ同梱・NDKクロスビルド・jniLibsは一切不要(open-web-server版との
  最大の設計差)。
- **実装内容**:
  1. パッケージ名`tokyo.runo.aruarudb`(open-web-server版の
     `tokyo.runo.openwebserver`と区別)。
  2. `MainActivity`: `EditText`でaruaru-db管理APIのベースURLを入力・
     `SharedPreferences`永続化、「接続/ステータス確認」ボタンで
     `GET <url>/admin/cluster`を実際に叩き、レスポンスJSON
     (`stats.total_nodes`/`healthy_nodes`/`total_ranges`/`table_count`/
     `under_replicated`)をパースして画面に表示する。
  3. 電源プロファイル管理: `PowerProfile.kt`(3モード、open-web-server版と
     同一enum構成)、`ProfileSelectActivity`(LAUNCHER、3プロファイル専用
     ホーム画面アイコンも`activity-alias`×3で用意)。プロファイルごとの
     実際の差は「監視ポーリング間隔」(省電力5分/通常1分/常時電源接続5秒、
     `pollIntervalMs()`)と「WakeLock有無」(常時電源接続のみ
     `PARTIAL_WAKE_LOCK`取得)——open-web-server版の`healthPollIntervalMs`/
     `applyProfilePowerBehavior`と同じ設計。
  4. 電源抜き差し監視ダイアログ: `BroadcastReceiver`で
     `ACTION_POWER_DISCONNECTED`/`ACTION_POWER_CONNECTED`を動的登録し、
     常時電源接続中に電源が外れたら「省電力モードに切り替えますか?
     それとも通常モードのままにしますか?」(既定推奨=省電力、
     `setCancelable(false)`)、省電力/通常中に電源が接続されたら常時
     電源接続へ戻すか尋ねる——open-web-server版と全く同じ導線をコピー。
  5. タブレット対応: `layout-sw600dp/activity_main.xml`(幅720dp上限+
     中央寄せ)を追加(open-web-server版と同じパターン)。
- **ビルド確認**: このマシンには`gradlew`本体が無いが、
  `~/.gradle/wrapper/dists/gradle-8.11.1-all/`にキャッシュ済み配布物が
  あることを確認し(open-web-server版のセッション記録と同じ発見)、
  `gradle-8.11.1/bin/gradle :app:assembleDebug`を直接実行したところ
  **`BUILD SUCCESSFUL`**(33 actionable tasks executed)。
  `android/app/build/outputs/apk/debug/app-debug.apk`(約3.24MB)が
  実際に生成されたことを確認済み。
- **正直な開示・未検証事項**: (1) 実機/エミュレータでの起動・
  `GET /admin/cluster`への実際の疎通確認は今回未実施(ビルド成功までに
  留まる、ユーザー指示通り正直に記録)。(2) `admin.rs`の他のエンドポイント
  (`/backup`・`/migrate/*`・`/federation/*`等)への対応は今回のスコープ外
  (最小限のモニタリングUIに集中する制約に従った)。(3) 管理API自体に
  認証機構が現状無い(`aruaru-server`側、`grep`で`x-admin-token`相当の
  認証コードが見当たらず)——将来追加された場合はこのアプリ側にも
  トークン入力欄の追加が必要になる。(4) `usesCleartextTraffic="true"`は
  開発用途のHTTP接続を許容するための設定(本番でHTTPS管理APIを使う場合は
  見直しが必要)。
- **次にすべきこと**: (1) 実機/エミュレータでの起動・実疎通確認、
  (2) `admin.rs`側の管理系エンドポイント(バックアップ実行等)への
  対応要否の検討、(3) 管理API認証機構の追加とアプリ側对応。

## HANDOFF: 2026-07-25 スタンドアロン・メール ディザスタバックアップを実装

**SETワイド要件(ユーザー原文の要約)**: VPS間分散同期を一切設定していなくても、
物理断線(SATA/USB/LAN/WiFiケーブル切断)・ネットワーク障害に備え、
メールアドレスひとつだけで有効化できる、独立した最後の砦のバックアップ
安全網を用意すること。`open-raid-z`(`offsite_backup.rs`の`EmailBackupTarget`、
111テスト済み)・`open-web-server`(`disaster_email_backup.rs`、153テスト済み)・
`open-easy-web`(`dist_sync.rs`の`/admin/dist-sync/disaster-fallback`)で
同一セッション内に完成済み。本リポジトリが4リポジトリ目。

- **実装内容**:
  1. `crates/aruaru-dist/Cargo.toml`に新feature`disaster_email_backup`を追加
     (`dep:open_raid_z_core` + `open_raid_z_core/offsite_backup`を有効化)。
     既存の`open_raid_z`feature(commit×ZFSスナップショット連携、
     `snapshot_pairing.rs`)とは別軸——**こちらはZFSスナップショット連携も
     Raftクラスタの複数ノード構成も一切不要**、単体で動く。
  2. `crates/aruaru-dist/src/disaster_email_backup.rs`(新規)。
     `open_raid_z_core::offsite_backup::EmailBackupTarget`をそのまま
     path依存で再利用(メール送信ロジックは再実装しない)。
     `DisasterEmailBackupConfig`は`EmailBackupTargetConfig`を`#[serde(flatten)]`
     で包むだけ(必須項目はメールアドレス等、`open_raid_z_core`側定義のまま)。
     `backup_failed_command`が`raft::Command`(Exec/Commit)をJSON化して
     メール添付として退避する。
  3. **失敗検知フックの設計**: 本来の理想は`RaftWriter::propose_and_wait`
     (`crates/aruaru-dist/src/raft/writer.rs`、過半数コミット待ち
     `wait_for_commit`が失敗・タイムアウトした経路)に自動フックすることだが、
     `RaftWriter`は`Applier`にジェネリックで`DisasterEmailBackup`を
     フィールドとして持たせる設計変更が必要になり、今回のパスでは
     **`DisasterEmailBackup`単体の構築・SMTP疎通確認・メール送信までを
     実装・テスト完了**、`RaftWriter`への自動配線(propose_and_wait失敗時に
     自動で`backup_failed_command`を呼ぶ)は**未着手**(次回候補、下記参照)。
     現状は呼び出し側(`aruaru-wire`等の書き込み経路)が`RaftWriter`から
     `Err`を受け取った際に、明示的に`DisasterEmailBackup::backup_failed_command`
     を呼ぶ運用を想定した「部品」としては完成している。
  4. `crates/aruaru-server/src/admin.rs`に管理API追加(`disaster_email_backup`
     feature有効時のみ):`POST /admin/disaster-email-backup`(設定)・
     `POST /admin/disaster-email-backup/verify`(SMTP疎通確認のみ、実送信なし)。
     `open-web-server`/`open-easy-web`と同じ規約——`x-admin-token`ヘッダー、
     環境変数`ARUARU_DB_ADMIN_TOKEN`が未設定なら503、トークン不一致・未提示
     なら401。**このリポジトリの`/admin/*`は元々認証機構自体が存在しなかった
     (2026-07-24 HANDOFFで既知のギャップと記載済み)**——今回は新設の
     ディザスタバックアップAPI限定で導入し、既存の他の`/admin/*`
     エンドポイントへの遡及適用はスコープ外(次回候補)。
  5. `crates/aruaru-server/Cargo.toml`に同名featureを追加し
     `aruaru-dist/disaster_email_backup`へ素通し。

- **テスト(ローカルモックのみ、正直な開示)**: `crates/aruaru-dist/src/
  disaster_email_backup.rs`内の`#[cfg(test)]`に、`open-raid-z`の
  `tests/offsite_backup_integration.rs`と同じ最小限の偽SMTPサーバー
  (生TCP、EHLO/AUTH LOGIN/MAIL FROM/RCPT TO/DATA/QUIT)を実装し、3テスト
  追加(メール送信成功・クラスタ/スナップショット設定なしでの単体動作・
  秘密情報未設定時の正直なエラー)。**実SMTPサーバー・実メールアカウントへ
  の接続はテストで一切行っていない**。実際の物理断線(SATA/USB/LAN/WiFi
  ケーブル抜去)シナリオも、このサンドボックス環境では検証不可能——
  モックベースの検証のみ(`open-raid-z`自身のHANDOFFエントリと同じ
  正直さの基準)。

- **実行したコマンドと実際の結果**:
  - `cargo build -p aruaru-dist --features disaster_email_backup` →
    `Finished` dev profile(初回はopen_raid_z_core経由でlettre/russh/ureq等の
    新規依存をコンパイル、以後は差分ビルド)。
  - `cargo test -p aruaru-dist --features disaster_email_backup` →
    `test result: ok. 32 passed; 0 failed; 1 ignored`(新規3テスト
    `disaster_email_backup::tests::*`含む、既存29テストも全てgreenのまま
    リグレッション無し)。
  - `cargo build --workspace` → `Finished` dev profile(デフォルトfeature構成、
    警告2件のみ: `disaster_email_backup`feature無効時の`Request`未使用
    importと、既存の`propose_commit`未使用関数——いずれも今回の変更が
    原因ではない/feature無効時のみ発生する無害な警告)。
  - `cargo test -p aruaru-server --features disaster_email_backup` →
    `test result: ok. 0 passed; 0 failed`(`aruaru-server`はバイナリクレートで
    元々ユニットテストを持たない構成——新設のadmin.rsハンドラ自体の
    ロジックは薄い橋渡しのみで、実質的な検証は`aruaru-dist`側の
    `DisasterEmailBackup`テストでカバー済み)。

- **次にすべきこと(次回候補)**: (a) `RaftWriter::propose_and_wait`失敗時に
  `DisasterEmailBackup`を自動で呼ぶ配線(`Applier`実装や`aruaru-wire`側の
  呼び出し元での明示的なハンドリングとして実装するか、`RaftWriter`に
  `Option<Arc<DisasterEmailBackup>>`フィールドを追加するかの設計判断が
  必要)、(b) 既存の他の`/admin/*`エンドポイント全体への`x-admin-token`
  認証の遡及適用、(c) Tauri Admin GUI側の対応(設定フォーム追加)。

## HANDOFF: 2026-07-25(続き) `RaftWriter::propose_and_wait`のquorum障害を`DisasterEmailBackup`へ自動配線

**位置づけ**: 直上のHANDOFF(同日)が「未着手」と正直に記載していたギャップ
(`RaftWriter`への自動配線)を今回のパスで実施した。

- **読んだ実コード**: `crates/aruaru-dist/src/raft/writer.rs`
  (`RaftWriter<A: Applier + 'static>`、`propose_and_wait`)、
  `crates/aruaru-dist/src/raft/node.rs`の`RaftNode::wait_for_commit`
  (341〜358行目)。`wait_for_commit`が`Err`を返すのは
  タイムアウトまでに過半数コミットへ到達できなかった場合のみ
  (`"timeout waiting for raft commit of index {index} (quorum not
  reached)"`という固定文言の1経路のみ)——**これが真の「quorum障害」**。
  一方`Ok(resp)`で`resp.ok == false`のケースは、Raftとしては
  (過半数)コミット+適用まで到達しているが`Applier`実装がコマンドを
  拒否した場合(無効なコマンド等)であり、quorum障害ではない。両者を
  明確に区別し、**`Err`分岐のみ**をディザスタバックアップのトリガーとした
  (`resp.ok == false`では発火しない)。

- **設計・実装内容**(`crates/aruaru-dist/src/raft/writer.rs`を変更):
  1. `RaftWriter<A>`に`#[cfg(feature = "disaster_email_backup")]`
     ゲート付きの`disaster_backup: Option<Arc<DisasterEmailBackup>>`
     フィールドを追加。`RaftWriter::new`は常に`None`で初期化——
     `feature`無効ビルドではこのフィールド自体が存在せず、コンパイル
     結果は今回の変更前と完全に同一になる(`cfg`でフィールド自体を
     消しているため、実行時分岐のオーバーヘッドすら残らない)。
  2. `with_disaster_email_backup(Arc<DisasterEmailBackup>)`ビルダーを
     追加(feature有効時のみ)。既存呼び出し元はこれを呼ばなければ
     従来通り`None`のまま——挙動が一切変わらないことをテストで実証
     (下記)。
  3. `propose_and_wait`の`wait_for_commit`結果を`match`に変更し、
     `Err(reason)`分岐でのみ`trigger_disaster_backup_if_configured`を
     呼ぶ(`resp.ok == false`分岐は素通し、従来の`Err(resp.message)`
     のまま)。
  4. **非ブロッキングの実現方法**: `open_raid_z_core::offsite_backup::
     EmailBackupTarget::upload_segment`(`disaster_email_backup.rs`が
     再利用しているメール送信本体)を実際に読んだところ**完全に同期・
     ブロッキングI/O**(async fnではない)と判明した。そのため
     `trigger_disaster_backup_if_configured`は、(a)まず`tokio::spawn`で
     呼び出し元(`propose_and_wait`)の`await`から切り離し、(b)その中で
     さらに`tokio::task::spawn_blocking`に包んでブロッキングSMTP I/Oを
     tokioのブロッキングスレッドプールへ退避する、という二重の構成に
     した。`propose_and_wait`はこの`tokio::spawn`の完了を一切`await`
     せず即座に元の`Err(reason)`を返す。
  5. `disaster_email_backup`feature無効時は`trigger_disaster_backup_
     if_configured`が空実装(`#[cfg(not(...))]`)になり、呼び出し自体は
     残るが何もしない——このゲーティングにより無効ビルドへの影響は
     皆無。

- **既存テストの構造改修**: `RaftWriter`のテストが`RaftWriter { node,
  timeout: ... }`という構造体リテラルを直接組み立てていたが、新設の
  `disaster_backup`フィールドがfeature構成によって存在有無が変わるため
  そのままでは両構成でコンパイルできなくなる。`with_timeout(Duration)`
  ビルダーを新設し、既存3テストをこれ経由に書き換えた(挙動は無変更、
  構築方法のみ変更)。

- **新規テスト**(`crates/aruaru-dist/src/raft/writer.rs`):
  1. `test_quorum_failure_without_disaster_backup_configured_behaves_
     as_before`(feature有無どちらでもコンパイル・実行される):
     `disaster_email_backup`feature有効ビルドでも`with_disaster_email_
     backup`を呼ばず未設定のままなら、既存の
     `test_write_times_out_if_quorum_never_reached`と同じquorum障害
     シナリオで(a)結果が`Err`のまま(b)所要時間が1秒未満(余計な遅延が
     無い)ことを実証——「未設定なら無変更」を実測で確認。
  2. `mod disaster_backup_wiring_tests`(`#[cfg(all(test, feature =
     "disaster_email_backup"))]`で新規モジュール化)。既存の
     quorum失敗シミュレーション(peersは持つがネットワーク層が無いため
     複製ACKが来ない、`test_write_times_out_if_quorum_never_reached`と
     同じ手法を再利用——新しい障害シミュレーション機構は発明していない)
     と、`disaster_email_backup.rs`のテストと同じ偽SMTPサーバー
     (生TCP、EHLO/AUTH LOGIN/MAIL FROM/RCPT TO/DATA/QUIT)パターンを
     再利用。
     - `quorum_failure_with_disaster_backup_configured_emails_the_
       failed_command`: `DisasterEmailBackup`を設定した状態でquorum
       障害を起こし、失敗した`Command`が実際に(モックSMTP経由で)
       メールされることを実証(受信ボディに`"INSERT INTO items"`が
       含まれることを確認)。
     - `quorum_failure_does_not_block_on_disaster_backup_even_if_smtp_
       is_unreachable`: SMTPサーバーを一切起動せず(port 1、到達不能)
       ディザスタバックアップを設定した状態でquorum障害を起こし、
       それでも`write_sql`自体が設定タイムアウト通り(1秒未満)で`Err`を
       返すことを実証——非ブロッキング配線の直接的な証拠。

- **実行したコマンドと実際の結果(すべて実行、以下は生ログの要約ではなく
  実際の出力)**:
  - `cargo build -p aruaru-dist --features disaster_email_backup` →
    `Finished` dev profile(1m 03s)。
  - `cargo test -p aruaru-dist --features disaster_email_backup` →
    `test result: ok. 35 passed; 0 failed; 1 ignored`(前回32件+今回
    新規3件、既存全件green・リグレッション無し)。
  - `cargo test -p aruaru-dist`(featureなし) →
    `test result: ok. 30 passed; 0 failed; 1 ignored`(前回29件+
    feature無しでもコンパイル・実行される新規1件、リグレッション無し)。
  - `cargo build --workspace` → `Finished` dev profile(2m 40s)、
    警告2件のみ(`aruaru-server`の`Request`未使用importと
    `propose_commit`未使用関数)——いずれも前回HANDOFFから存在する
    既知の無害な警告で、今回の変更が原因のものは無い。
  - `cargo test -p aruaru-server --features disaster_email_backup` →
    `test result: ok. 0 passed; 0 failed`(前回同様、バイナリクレートで
    ユニットテスト無し、警告も前回同一の2件のみ)。

- **正直な開示・スコープの限界**:
  1. **非ブロッキングの検証範囲**: `quorum_failure_does_not_block_on_
     disaster_backup_even_if_smtp_is_unreachable`はTCP接続自体が
     即座に失敗する「到達不能ポート」シナリオで非ブロッキングを実証
     したが、**「TCP接続は確立するがSMTPサーバーが応答をダラダラ遅延
     させる」という真のスロー・スロー・ロリスシナリオでの検証は
     行っていない**——このサンドボックス環境で意図的に遅いモック
     SMTPサーバー(応答を数秒〜数十秒遅延させる)を書くことは技術的に
     可能だが、今回はスコープを到達不能ケースの実証に留めた。
     ただし設計自体(`tokio::spawn`+`spawn_blocking`で呼び出し元の
     `await`から完全に切り離し)は、遅延の原因がDNS解決・TCP接続
     確立・SMTP応答のどの段階であっても呼び出し元をブロックしない
     はずである(`spawn_blocking`されたブロッキングタスク自体は
     長時間かかり得るが、それは呼び出し元が待っている`Err(reason)`の
     返却には一切影響しない)。次回、意図的に遅いモックSMTPサーバーで
     この設計上の保証を実測することが望ましい。
  2. **`Applier`実装側からの明示的な呼び出しは引き続き未対応**:
     今回配線したのは`RaftWriter::propose_and_wait`(ジェネリックな
     `RaftWriter<A>`レベル)のみ。`aruaru-wire`等の上位呼び出し元が
     `RaftWriter`を経由せず`RaftNode`を直接使っている経路があれば
     (現状は`aruaru-server/src/cluster.rs`が`RaftWriter`経由と
     未使用の`propose_commit`の両方を持つ、既知の状態)、そちらは
     今回の配線の対象外。
  3. **`aruaru-wire`/`aruaru-server`側での実際の`with_disaster_email_
     backup`呼び出し配線は未実施**: `RaftWriter`は今回自動配線を
     受け付けられるようになったが、実際に`aruaru-server`起動時に
     `DisasterEmailBackupConfig`から`DisasterEmailBackup`を構築し
     `RaftWriter::with_disaster_email_backup`へ渡す配線(管理API
     `POST /admin/disaster-email-backup`で設定した内容を実際の
     `RaftWriter`インスタンスへ反映する経路)はまだ無い——現状の
     `admin.rs`のハンドラは`DisasterEmailBackup`単体の設定検証のみで、
     稼働中の`RaftWriter`への注入は次回対応。

- **次にすべきこと(次回候補)**: (a) 意図的に遅延するモックSMTPサーバーで
  真のスロー・スロー・ロリスシナリオでの非ブロッキング性を実測、
  (b) `aruaru-server`起動時、`POST /admin/disaster-email-backup`で
  設定された`DisasterEmailBackup`を稼働中の`RaftWriter`インスタンスへ
  実際に注入する配線、(c) `aruaru-wire`が`RaftWriter`を経由せず
  `RaftNode`を直接叩く経路が無いか棚卸し。

## HANDOFF: 2026-07-25(続き2) 前回の3つの正直な未検証ギャップを埋める

**位置づけ**: 直上2件のHANDOFF(同日)が挙げた3つの「正直な開示」ギャップ
(a) 真のスロー・スロー・ロリスSMTP未検証、(b) 管理APIが稼働中の
`RaftWriter`へ注入しない、(c) `RaftNode`直叩きの迂回経路の棚卸し未実施、
を今回のパスで解消/前進させた。

- **(a) 真の低速SMTPテストを追加(解消)**:
  `crates/aruaru-dist/src/raft/writer.rs`の`disaster_backup_wiring_tests`に
  `spawn_slow_fake_smtp_server`/`handle_slow_smtp_client`を追加(既存の
  偽SMTPサーバーと同じEHLO/AUTH LOGIN/MAIL FROM/RCPT TO/DATA/QUITだが、
  EHLO・AUTH応答をそれぞれ`std::thread::sleep(3秒)`で遅延させる——TCP接続
  自体はすぐ確立するが、SMTP応答がダラダラ遅い真のスロー・スロー・ロリス
  シナリオ)。新規テスト
  `quorum_failure_does_not_block_on_disaster_backup_even_when_smtp_is_genuinely_slow`
  は、`with_timeout(100ms)`のquorum障害シナリオで`write_sql`自体が1秒未満
  (=3秒の遅延より遥かに短い)で`Err`を返すことを実測し、その後
  バックグラウンドで実際にメールが届くことも確認する。これにより
  `tokio::spawn`+`spawn_blocking`の二重デタッチが、到達不能ケースだけで
  なく実際の低速応答でも呼び出し元をブロックしないことを実証した。

- **(b) 管理APIから稼働中のRaftWriterへの実注入(解消)**:
  1. `crates/aruaru-dist/src/raft/writer.rs`: `RaftWriter<A>`の
     `disaster_backup`フィールドを`Option<Arc<..>>`から
     `parking_lot::Mutex<Option<Arc<..>>>`に変更(内部可変性)。
     `ReplicatedWriter`トレイトに`#[cfg(feature = "disaster_email_backup")]`
     ゲート付きの新メソッド`set_disaster_email_backup(&self, backup:
     Arc<DisasterEmailBackup>)`を追加し、`RaftWriter`に実装。これにより
     既に`Arc`で共有済み(生存中のサーバーが保持しているのと同じ状態)の
     インスタンスへ、構築時ビルダー(`with_disaster_email_backup`、
     consuming)を使わず後から注入できる。新規テスト
     `set_disaster_email_backup_after_arc_sharing_still_wires_up_correctly`
     で、`Arc<dyn ReplicatedWriter>`化した後の注入でも実際にメールされる
     ことを確認済み。
  2. `crates/aruaru-server/src/admin.rs`: `AdminState`に新フィールド
     `replicator: Mutex<Option<Arc<dyn aruaru_dist::ReplicatedWriter>>>`を
     追加(`attach_replicator`/`replicator`アクセサ)。
     `set_disaster_email_backup`ハンドラ(`POST /admin/
     disaster-email-backup`)は、設定検証・保管に加えて
     `state.replicator()`が`Some`なら`set_disaster_email_backup`を実際に
     呼び出す(`injected_into_live_replicator`をレスポンスに含め、
     Raftクラスタ未構築で`replicator`が無い場合は正直にその旨を
     message_ja/enへ記載)。
  3. `crates/aruaru-server/src/main.rs`: Raftクラスタ構築成功時、
     pgwireサーバへ渡すのと**同一の**`Arc<dyn ReplicatedWriter>`
     (`raft_writer`)を`admin_state.attach_replicator(raft_writer.clone())`
     でも取り付けるよう変更(1つのRaftWriterインスタンスを両方の経路が
     共有する)。

- **(c) `RaftNode`直叩きの迂回経路を棚卸し(1件発見・配線、1件発見・
  ドキュメント化)**:
  1. **発見して配線した迂回経路**: `crates/aruaru-server/src/admin.rs`の
     `cluster_propose`ハンドラ(REST `POST /admin/cluster/propose`)が、
     `crates/aruaru-server/src/cluster.rs`の`propose_write`(`RaftNode::
     propose`→`try_commit_to`→`maybe_commit`→`apply_committed`を直接
     手動で呼ぶ)経由で書き込みしており、`RaftWriter`(および
     disaster-backup配線)を完全に迂回していた。今回、上記(b)で
     `AdminState`に取り付けた`replicator`が`Some`の場合はそちらを
     優先して`replicator.write_sql(&req.sql).await`を呼ぶよう変更
     (ハンドラを`async fn`化)。`replicator`が無い(クラスタ構築失敗等の
     異常系)場合のみ、後方互換のため`cluster::propose_write`の旧経路へ
     フォールバックする(この場合のみ引き続きdisaster-backup配線の対象外
     であることをレスポンスの`mode: "raft_fallback_no_replicator"`と
     messageで明示)。
  2. **発見したが今回は配線しなかった迂回経路(既知の未解決ギャップとして
     明記)**: `crates/aruaru-graphql/src/admin_resolvers.rs`
     355〜365行目の`cluster_propose` GraphQL resolver(Mutation)が、
     `RaftNode`すら経由せず`a.engine.execute(&sql)`で`QueryEngine`へ
     **直接**書き込んでいる(Raftコンセンサス自体を完全にスキップ)。
     この経路は`AdminCtx`(同ファイル17〜21行目、`engine`/`registry`/
     `backup`の3フィールドのみ)が`RaftWriter`/`replicator`への参照を
     一切持っていないため。`aruaru-graphql`の`Cargo.toml`は現状
     `aruaru-dist`に依存しておらず、`AdminCtx`への`replicator`追加・
     `main.rs`側でのGraphQLコンテキスト構築箇所(`AdminCtx { engine:
     .., registry: .., backup: .. }`)への配線・スキーマ影響確認が
     必要になるため、今回のパスでは着手せず正直に未解決として記録する
     (次回候補、下記)。**単一ノード構成では`QueryEngine`への直接書き込み
     も最終的に同じ状態に収束するため実害は小さいが、複数ノードクラスタ
     構成でこのGraphQL経路から書き込むと、Raftレプリケーションを経由せず
     ローカルノードのみに適用される=他ノードとの一貫性が崩れる、
     という真のバグになり得る**。
  3. その他`RaftNode`使用箇所(`crates/aruaru-dist/src/multi_raft.rs`の
     `MultiRaftGroups`、`crates/aruaru-dist/src/raid_z_backend.rs`・
     `snapshot_pairing.rs`のテスト用途)は`aruaru-server`の実運用書き込み
     経路(pgwire・REST admin API)からは呼ばれておらず、今回の迂回経路
     棚卸しの対象外(`multi_raft`は複数Raftグループの実験的抽象化で、
     現状`aruaru-server`からは未使用)。

- **実行したコマンドと実際の結果(すべて実行、生ログ)**:
  - `cargo build --workspace` →
    `warning: unused import: \`Request\`` (admin.rs:18、feature無効時のみ)、
    `warning: function \`propose_commit\` is never used` (cluster.rs:86、
    今回のフォールバック経路が呼ばないため引き続き未使用)の2件のみ、
    `Finished \`dev\` profile [optimized + debuginfo] target(s) in 1m 57s`。
    いずれも前回HANDOFFから存在する既知の無害な警告で、今回の変更が
    原因のものは無い。
  - `cargo build -p aruaru-dist --features disaster_email_backup` →
    `Finished \`dev\` profile [optimized + debuginfo] target(s) in 20.46s`。
  - `cargo test -p aruaru-dist --features disaster_email_backup` →
    `test result: ok. 37 passed; 0 failed; 1 ignored; 0 measured; 0 filtered
    out; finished in 6.20s`(前回35件+今回新規2件
    `quorum_failure_does_not_block_on_disaster_backup_even_when_smtp_is_
    genuinely_slow`・`set_disaster_email_backup_after_arc_sharing_still_
    wires_up_correctly`、既存全件green・リグレッション無し)。
  - `cargo test -p aruaru-server --features disaster_email_backup` →
    `test result: ok. 0 passed; 0 failed; 0 measured; 0 filtered out;
    finished in 0.00s`(前回同様バイナリクレートでユニットテスト無し、
    警告は`propose_commit`未使用1件のみ)。
  - `cargo test -p aruaru-dist`(featureなし) →
    `test result: ok. 30 passed; 0 failed; 1 ignored; 0 measured; 0
    filtered out; finished in 0.12s`(前回同数、リグレッション無し)。
  - `cargo test -p aruaru-server`(featureなし) →
    `test result: ok. 0 passed; 0 failed; 0 measured; 0 filtered out;
    finished in 0.00s`(警告2件、`Request`未使用importと`propose_commit`
    未使用関数、いずれも既知)。

- **正直な開示・スコープの限界(今回も残るもの)**:
  1. `crates/aruaru-graphql/src/admin_resolvers.rs`の`cluster_propose`
     resolverは引き続き`RaftWriter`/`replicator`を経由しない(上記(c)-2
     参照)。GraphQL Admin GUIから複数ノードクラスタ構成でこの
     ミューテーションを叩くと、disaster-backup対象外なだけでなく
     Raftレプリケーション自体をスキップする(単一ノードでは実害なし)。
  2. (a)のテストは「EHLO/AUTH応答を3秒遅延」という具体的な低速シナリオ
     を検証したが、DNS解決自体が遅いケースやTLSハンドシェイクが遅い
     ケースなど、遅延が発生し得る全段階を網羅したわけではない
     (`tokio::spawn`+`spawn_blocking`という設計そのものは段階を問わず
     呼び出し元を保護するはずだが、個別の実測は今回の1シナリオのみ)。
  3. Tauri Admin GUI側の対応(ディザスタバックアップ設定フォーム追加)は
     引き続き未着手(前々回HANDOFFから継続する既知の次回候補)。

- **次にすべきこと(次回候補)**: (a) `aruaru-graphql`の`AdminCtx`へ
  `replicator: Option<Arc<dyn aruaru_dist::ReplicatedWriter>>`を追加し
  (`aruaru-graphql`の`Cargo.toml`へ`aruaru-dist`依存を追加する必要あり)、
  GraphQL `cluster_propose` resolverもRaftWriter経由に統一する、
  (b) Tauri Admin GUIのディザスタバックアップ設定フォーム、
  (c) `cluster.rs`の`propose_commit`(既に未使用)を、フォールバック経路
  以外で使う予定が無いなら削除するか、GraphQL側統一の際に活用するか判断する。

## HANDOFF: 2026-07-27 前回エントリの「次にすべきこと(4) capabilities/
ディレクトリの要否確認」を確認・解消

`admin/src-tauri`で`capabilities/`ディレクトリを追加せずに`cargo build`
を実行し、警告・エラーいずれも出ないことを確認した(`capabilities`という
文字列を含む出力自体が皆無)。Tauri v2のパーミッション定義
(`capabilities/*.json`)は、既定の`main`ウィンドウに対するデフォルト
パーミッションセットで足りている限り省略可能で、このアプリ(既存の
`tauri::Builder::default()`+標準プラグイン`shell`/`fs`/`dialog`のみ)は
その範囲内であるため、現状は追加不要と判断する。**正直な開示**:
`cargo tauri dev`での実ネイティブウィンドウ起動時に、特定の操作
(ファイルダイアログ経由の書き込み等)でパーミッション拒否が実際に
発生するかどうかまでは検証していない(`cargo build`が通ることの確認に
留まる)——問題が実際に顕在化した場合に`capabilities/`を追加する、という
方針で次回以降へ持ち越す。
- 次にすべきこと: 前回エントリの(1)(2)(3)(5)から変更なし
  (実アイコン素材への差し替え、`cargo tauri dev`での実ネイティブ
  ウィンドウ起動確認、実SMTPサーバーでのE2E確認、GraphQL
  `cluster_propose` resolverのRaftWriter経由統一)。

## HANDOFF: 2026-07-26 Tauri Admin GUIのディザスタバックアップ設定フォームを実装
+ アプリ自体が一度もビルドできていなかった重大な既知の欠落を発見・解消

上記2026-07-25(続き2)の「次にすべきこと(次回候補)(b) Tauri Admin GUIの
ディザスタバックアップ設定フォーム」に対応(ユーザー指示: runo.tokyo/
open-directx/open-cuda/aruaru-llm等7リポジトリの未着手・未完成事項の
洗い出し→実装継続、SETバックアップ系の実接続配線の一環として着手)。

**正直な開示(最重要): 着手前の調査で、`admin/`(Tauri Admin GUI)は
これまで一度も実際にビルドできる状態になっていなかったことが判明した**。
既存のHANDOFFはRust側コマンド(`main.rs`)・Reactページ群の「コードとしての
実装」を記録していたが、以下が全て欠落しており、`cargo build`も
`npm run build`も過去に一度も成功していなかった(型チェック・実行の
どちらも行われないまま「実装済み」として記録されてきたことになる):
- `admin/src-tauri/tauri.conf.json`(Tauriアプリ設定ファイル自体が存在しない)
- `admin/src-tauri/build.rs`
- `admin/src-tauri/icons/`(`icon.png`/`icon.ico`)
- `admin/index.html`・`admin/src/main.tsx`・`admin/src/index.css`
  (Reactのブートストラップ自体が無く、`App.tsx`以下のページ群は
  どこからも読み込まれない孤立したファイル群だった)
- `admin/vite.config.ts`・`admin/tsconfig.json`・`admin/tsconfig.node.json`・
  `admin/tailwind.config.js`・`admin/postcss.config.js`
- `admin/src-tauri/Cargo.toml`: `[lib]`セクションが`aruaru_admin_lib`という
  存在しない`src/lib.rs`を指しており、マニフェスト解析自体が失敗する状態
  (`can't find library aruaru_admin_lib`)。加えてワークスペースルート
  (`F:\runo\aruaru-db\Cargo.toml`)の`members`にも含まれておらず、
  「ワークスペースに属していると誤認された状態」でも失敗する
  (`current package believes it's in a workspace when it's not`)。

1. **最小限のTauri+Viteスキャフォールドを新規作成**(具体的な処理を追加
   実装したのではなく、`cargo tauri init`相当の土台を後追いで補った):
   `tauri.conf.json`(`devUrl: http://localhost:1420`、`frontendDist:
   ../dist`、`identifier: tokyo.aon.aruaru-db.admin`)、`build.rs`、
   1x1のプレースホルダー`icon.png`/`icon.ico`(実アイコン素材は今回
   用意していない、ビルドを通すための最小限のダミー画像であることを
   正直に記録)、`index.html`+`src/main.tsx`+`src/index.css`
   (Tailwindディレクティブ)、`vite.config.ts`、`tsconfig.json`/
   `tsconfig.node.json`、`tailwind.config.js`/`postcss.config.js`、
   `package.json`に`"type": "module"`を追加。`admin/src-tauri/Cargo.toml`
   から壊れていた`[lib]`セクションを削除(`aruaru_admin_lib`を参照する
   コードは他に存在しないことを`grep`で確認済み)し、空の`[workspace]`
   テーブルを追加してワークスペースルートから独立させた(cargo自身が
   エラーメッセージで提示した2つの選択肢のうち、`crates/`配下の
   ライブラリを直接参照しない独立GUIアプリという性質からこちらを選択)。
2. **ディザスタ用メール退避のTauriコマンド2件を新規実装**
   (`admin/src-tauri/src/main.rs`): `set_disaster_email_backup`
   (`POST /admin/disaster-email-backup`、`DisasterEmailBackupForm`
   構造体は`open_raid_z_core::offsite_backup::EmailBackupTargetConfig`と
   同じフィールド構成)、`verify_disaster_email_backup`
   (`POST /admin/disaster-email-backup/verify`、SMTP疎通確認のみ)。
   `generate_handler!`マクロへ登録。
3. **`admin/src/pages/backup/BackupManager.tsx`に「📧 ディザスタ用メール
   退避(最後の砦)」セクションを新規追加**(既存の「バックアップ管理」
   ページ内、新規ルーティングは追加せず既存ナビゲーションに相乗り):
   SMTPホスト/ポート/ユーザー名/パスワード環境変数名/送信元/退避先の
   各入力欄、平文接続許可チェックボックス、「設定を保存」「SMTP接続を
   確認」ボタン、結果メッセージ表示。SMTPパスワードそのものはフォームに
   入力させない設計(環境変数名のみ、既存の`open_raid_z_core`側の方針を
   踏襲)。**発見した既存バグの修正**: 同ファイル124行目の`phaseColor`
   関数が、`Phase`型の一部のケース(`Preparing`/`DumpingSchema`等)しか
   持たないオブジェクトリテラルを`Record`的に扱っておりTypeScriptの
   型エラーになっていた(`tsc`を初めて実際に実行して発覚)。
   `Partial<Record<Phase, string>>`へキャストして解消(既存のフォール
   バック`?? "text-orange-400"`の意図はそのまま)。
4. **検証(実測、型チェック・ビルド成功だけで終わらせない方針の徹底)**:
   - `npm install`→`npm run build`(`tsc && vite build`)が**初めて**
     成功(`dist/`が実際に生成されることを確認)。
   - `cargo build`(`admin/src-tauri`)が**初めて**成功。
   - `npm run dev`で実際にVite開発サーバーを起動し(`http://localhost:1420`)、
     実ブラウザ(Claude Code内蔵のBrowserツール)で実際にページを開いて
     スクリーンショット・`get_page_text`で確認: (a) サイドバー・
     ダッシュボードが実際にレンダリングされること、(b)「バックアップ」
     ページへ実際に遷移でき、新設した「📧 ディザスタ用メール退避」
     セクションの全フィールド・ボタン・注意書きの文言が実際にDOM上に
     存在すること、(c)「SMTP接続を確認」ボタンを実際にクリックし、
     Tauri IPCブリッジが無いブラウザ単体環境では`invoke`が失敗する
     ことを利用して、エラー時のcatchハンドリングが実際に動作し
     (「SMTP接続確認に失敗しました: TypeError: Cannot read properties
     of undefined (reading 'invoke')」という具体的なエラーメッセージが
     画面に表示される)、画面がクラッシュしないことを確認した。
   - 上記(c)は「実Tauriデスクトップアプリ(WebView2)としての起動」では
     ない(ブラウザ単体でのVite devサーバー確認)ため、実際の
     `cargo tauri dev`でのネイティブウィンドウ起動・実SMTPサーバーへの
     `set_disaster_email_backup`/`verify_disaster_email_backup`の
     実際の疎通は今回未検証。
5. **正直な開示(引き続き残る制約)**: (1) `icon.png`/`icon.ico`は
   1x1のダミー画像であり、実際のアプリアイコン素材ではない(配布用
   ビルド`tauri build`を行う前に差し替えが必要)。(2) `capabilities/`
   ディレクトリ(Tauri v2のパーミッション定義)は今回作成しておらず、
   ビルド時の警告等が出ていないか次回`cargo tauri dev`実行時に確認が
   必要。(3) 実Tauriネイティブウィンドウでの動作確認・実SMTPサーバーへの
   接続確認は未実施(上記4参照)。(4) GraphQL側の`disaster_email_backup`
   専用resolverは引き続き存在しない(HTTP `/admin/*`経由のみ、
   2026-07-25(続き2)の開示のまま変更なし)。
- 次にすべきこと: (1) 実アイコン素材への差し替え、(2) `cargo tauri dev`
  での実ネイティブウィンドウ起動確認、(3) 実SMTPサーバーでの
  `set_disaster_email_backup`→`verify_disaster_email_backup`のE2E確認、
  (4) `capabilities/`ディレクトリの要否確認、(5) GraphQL
  `cluster_propose` resolverのRaftWriter経由統一(2026-07-25(続き2)から
  継続する既知の次回候補)。

## HANDOFF: 2026-07-27(続き) `AS OF COMMIT`のフルテーブルスキャン(WHERE無し)対応

`open-raid-z/CLAUDE.md`が「VersionLessAPIハイブリッドバージョン管理の
読み出し側=commit_id指定クエリは未着手」と記録していたが、調査の結果
**単一行(PK一致の`WHERE`)経路は既に実装済み**で、未着手だったのは
`WHERE`無し・複数行のフルテーブルスキャンのみだったと判明したため、
そのギャップだけを埋めた(過大な既存実装の見落としを防ぐため、
着手前に実装済み範囲を正直に確認してから作業した)。

1. **`crates/aruaru-query/src/engine.rs::select_as_of`を拡張**:
   `filter: None`(`WHERE`無し)の場合、`ProllyTree::scan()`で該当commitの
   全ノードを取得し、`table\0`プレフィックス(`snapshot_root()`が書き込む
   キー形式と同一)で対象テーブルの行だけに絞り込む。単一行経路
   (`tree.get(&key)`によるポイントルックアップ)はそのまま維持し、
   フルスキャンの計算コストを不要な場合に払わないようにした。
2. **`parser.rs`は変更不要だった**: `AS OF COMMIT`の構文解析は既に
   通常の`SELECT`を再パースして`table`/`filter`をそのまま流用する実装
   だったため、`WHERE`無しの`SELECT * FROM t AS OF COMMIT '...'`は
   追加実装なしで`filter: None`として正しくパースされていた。
3. **検証**: 新規テスト`as_of_commit_without_where_returns_all_rows_as_of_that_commit`
   を追加(2行commit後に3行目を追加し、`AS OF COMMIT`が3行目を含まず
   commit時点の2行のみを返すこと、最新状態には3行とも存在することを
   確認)。`cargo test -p aruaru-query`**43件全green**(既存
   `as_of_commit_returns_the_value_from_that_commit_not_the_latest`含め
   回帰なし)。
4. **正直な未対応事項**: GraphQL/REST admin API側への配線は今回未実施
   (現状は生SQL経由のみ)。列名の順序保証(`rows.sort()`で行の内容全体を
   ソートしているため、複数列で最初の列の値が同じ場合はテーブル本来の
   挿入順と一致しない可能性がある——determinism優先の簡易実装であり、
   本来のINSERT順保持が必要になった場合は別途対応が要る)。
- 次にすべきこと: (1) GraphQL/admin resolversから`AS OF COMMIT`
  フルスキャンを呼び出せるようにする配線、(2) 行順序をINSERT順で
  保持したい場合のソートキー見直し。

## HANDOFF: 2026-07-30(続き) Web管理UI(RPoem)新規実装 + `/admin/*`認証の遡及適用(セキュリティ強化)

ユーザー指示「Rust + RPoem(tokio/hyper直接実装)」でopen-raid-zと対の
Web管理UIを構築。その過程でユーザーから複数回「aruaru-serverは外部から
乗っ取られないようにセキュリティをしっかりして」との指示があり、実際に
重大なギャップを発見・修正した。

1. **重大な発見**: `crates/aruaru-server/src/admin.rs`の`/admin/*`は
   `disaster-email-backup`系エンドポイント限定でのみ`x-admin-token`
   認証を持ち、`cluster`/`backup`/`migrate`/`federation`/`registry`/
   `raft/append`/`raft/vote`を含む**大半のエンドポイントには認証が
   一切無かった**(`aruaru-db/CLAUDE.md`2026-07-24 HANDOFFで既知の
   ギャップとして記録済みだったが未対応のままだった)。さらに
   `main.rs`を確認したところHTTPサーバ(GraphQL/admin両方を配信)は
   `0.0.0.0:{gql_port}`にbindするデフォルト設定であり、ファイア
   ウォール等の追加防御が無い場合はクラスタ状態の閲覧だけでなく
   バックアップ実行・移行実行・Raftノード操作まで**インターネットから
   無認証で到達可能**という実際の露出だった。
2. **修正**: `admin_routes()`が返す`Route`全体を`.around()`
   ミドルウェアで包み、`check_admin_auth`(`x-admin-token`ヘッダー+
   `ARUARU_DB_ADMIN_TOKEN`環境変数)を`/admin/*`配下の**全**
   エンドポイントへ遡及適用した(2026-07-25(続き2)エントリの
   「次回候補(b)」として明記されていた項目)。環境変数未設定なら503、
   ヘッダー不一致なら401——`raft/append`/`raft/vote`も同じゲート配下
   に入った(現状これらを実際に呼ぶノード間通信はまだ配線されていない
   ため実害は無いが、将来配線される際は各ノードに同じ
   `ARUARU_DB_ADMIN_TOKEN`を設定する必要がある点をコメントに明記)。
3. **タイミングサイドチャネル対策**: 素の`!=`比較はCWE-208
   (タイミング攻撃)のリスクがあるため、`constant_time_eq`
   (全バイトを走査してからXOR累積判定、長さの違いも早期リターン
   しない)を新設し、トークン比較をこれに置き換えた。新規crate依存
   (`subtle`等)は追加せず、この用途限定の最小実装とした。
4. **Web管理UI(`aruaru-db/web/`、新規独立クレート)**: RPoem
   (`open-runo-poem-compat`)へのpath依存のみ。既存の`aruaru-server`
   (常駐デーモン)の`/admin/cluster`(GET)・`/admin/cluster/rebalance`
   (POST)をリバースプロキシする。**2段階の独立したトークン**:
   ブラウザ↔本Web層(`ARUARU_WEB_ADMIN_TOKEN`)、本Web層↔aruaru-server
   (`ARUARU_UPSTREAM_ADMIN_TOKEN`、aruaru-server起動時の
   `ARUARU_DB_ADMIN_TOKEN`と同じ値)。`ARUARU_WEB_READ_ONLY=1`設定時は
   正しいトークンでも常に403(rs-sync/open-raid-z/webと同じ多層防御
   設計)。
5. **実機検証(型チェックのみで完了と報告しない方針を徹底)**: 実際に
   `aruaru-server`をローカルで起動し、(a) `ARUARU_DB_ADMIN_TOKEN`未設定
   時は`/admin/cluster`が503(修正前は無条件200で実データが漏れて
   いた)、(b) トークン設定後、誤ったトークンは401・正しいトークンは
   200で実クラスタ状態を返す、(c) 同じ試験を`raft/append`にも実施し
   401を確認、(d) 同じ長さ/異なる長さの誤トークンいずれも問題なく401、
   (e) Web UI経由でも2段階トークンを通して実際にステータス取得・
   リバランス実行が成功する、(f) `ARUARU_WEB_READ_ONLY=1`時は正しい
   トークンでも403、をすべて実際のHTTPリクエストで確認した。
   `cargo build --workspace`・`cargo test -p aruaru-server --features
   disaster_email_backup`ともリグレッション無し(既存の
   `propose_commit`未使用警告のみ、無関係)。
6. **正直な開示・未着手**: (a) VPSへの実デプロイ・TLS終端の実運用
   設定(`--tls-cert`/`--tls-key`フラグ自体はmain.rsに既存)は今回
   未実施、(b) `aruaru-graphql`側の`cluster_propose` GraphQL resolver
   (2026-07-25(続き2)で発見済みの別の迂回経路、`RaftWriter`を経由せず
   `QueryEngine`へ直接書き込む)は今回も未対応のまま(GraphQL自体には
   今回の`x-admin-token`ゲートを適用していない、別軸の認証方針のため
   スコープ外とした)、(c) `/graphql`エンドポイント自体にもレート
   制限・認証は無い(今回のスコープはREST `/admin/*`限定)。
- 次にすべきこと: (1) VPS実デプロイ+TLS設定、(2) `aruaru-graphql`の
  `cluster_propose` resolverをRaftWriter経由に統一(既知の残課題、
  変更なし)、(3) `/graphql`エンドポイント自体の認証・レート制限。

## HANDOFF: 2026-07-30 安全なアンインストーラー(uninstall.sh/uninstall.ps1)を新設

ユーザー指示「別バージョンをインストールし直す/アンインストールする際に
既存データやHDDのデータへ悪影響を与えないように」への対応。従来
`install.sh`/`install.ps1`のみでアンインストーラーが存在しなかった。

1. **安全性の設計方針(最重要)**: `install.sh`/`install.ps1`が作成する
   データディレクトリ(`/var/lib/aruaru-db`、`C:\ProgramData\aruaru-db`、
   `ARUARU_DATA_DIR`)には実際のDBデータ(fjall/redbのストレージ
   ファイル)が入っている。新設の[uninstall.sh](uninstall.sh)/
   [uninstall.ps1](uninstall.ps1)は**このデータディレクトリを絶対に
   削除しない**——サービス(systemdユニット/Windowsサービス)を停止・
   無効化し、バイナリを削除するのみ。データディレクトリが見つかった
   場合は場所を明示して終了し、完全削除したい場合は内容を確認した上で
   手動でコマンドを実行するよう案内する(自動実行はしない)。
2. `open-raid-z`(データディレクトリを持たない設計、バイナリのみ)側にも
   同様のアンインストーラーを同時に新設した。詳細は
   `open-raid-z/CLAUDE.md`参照。
3. **正直な開示**: 実際にインストール→データ書き込み→アンインストール
   →データディレクトリが残っていることの実機検証は今回未実施(スクリプト
   のロジックレビューとシェル構文チェックのみ)。
- 次にすべきこと: (1) 実環境(Linux実機/Windows実機)でのインストール→
  アンインストール→データ保全の実地検証。

## HANDOFF: 2026-08-01(続き) GraphQL経由の管理操作にも認証を適用(2026-07-25(続き2)/07-27エントリの「未解決」を解消)

過去のHANDOFF(2026-07-25続き2・07-27・07-30)が繰り返し「次にすべきこと」
として記録していた`aruaru-graphql`側の認証欠如を解消した。

1. **調査で判明したこと**: `cluster_propose` resolverの`RaftWriter`経由化
   自体は既に**2026-07-26に実装・テスト済み**だった(`admin_resolvers.rs`
   のdocコメント・`cluster_propose_tests`モジュールで確認、実際に
   `cargo test`で再確認しgreen)——CLAUDE.mdの「次にすべきこと」記載が
   古いまま更新されていなかっただけ(このエコシステムで繰り返し見られる
   「ドキュメントの追従漏れ」パターン)。一方、**GraphQL `/graphql`
   エンドポイント自体に認証機構が一切無い**という別の欠如は実際に
   未解消のまま残っていた——REST側`/admin/*`は2026-07-30に
   `x-admin-token`認証を遡及適用済みだったが、**同じ管理操作
   (`clusterStatus`・`createBackup`・`runMigration`・`clusterPropose`等)を
   GraphQL経由で呼べば無認証のまま実行できてしまう抜け穴**が残っていた。
2. **修正**: `graphql_endpoint`を`async-graphql-poem`既定の
   `GraphQL::new(schema)`から、`x-admin-token`ヘッダーを読み取り
   `GraphqlAdminToken`としてリクエストデータへ注入する薄いハンドラへ
   置き換えた。`admin_resolvers.rs`に`require_admin_token`
   (REST側`admin.rs`の`check_admin_auth`/`constant_time_eq`と同じロジック・
   同じ環境変数`ARUARU_DB_ADMIN_TOKEN`)を新設し、既に`admin(ctx)?`を
   呼んでいたresolverはそのヘルパー経由で自動的に保護対象になるよう
   `admin()`自体に検証を組み込み、`admin(ctx)`を呼んでいなかった残りの
   resolver(`preview_source`・`test_registry_connection`・
   `set_parallel_config`・`cluster_node_op`等)にも個別に追加した。
   `VcsQuery`/`VcsMutation`(通常のバージョン管理系クエリ)は対象外——
   `/graphql`は1つのスキーマに管理系と非管理系を統合しているため、
   エンドポイント全体を一律に塞ぐと通常利用まで巻き込むと判断した。
3. **検証(実測)**: 新規テスト4件
   (`admin_query_without_token_is_rejected`・
   `admin_query_with_wrong_token_is_rejected`・
   `admin_query_with_correct_token_succeeds`・
   `non_admin_vcs_query_does_not_require_a_token`)、既存2件
   (`cluster_propose_*`)含め`cargo test -p aruaru-graphql`**6件全green**。
   `cargo build --workspace`成功(既存の`propose_commit`未使用警告のみ、
   無関係)。**本番デプロイ後、実際に`curl`で`/graphql`へ`x-admin-token`
   無し→エラー、誤ったトークン→エラー、正しいトークン→実クラスタ状態
   (`{"data":{"clusterStatus":{"stats":{"totalNodes":1}}}}`)、非管理系
   `log`クエリはトークン無しでも成功**、の4パターンをすべて実際の
   HTTPリクエストで確認した(型チェックのみでの完了報告ではない)。
4. **正直な開示**: (a) 一部のAdmin resolver(`set_backup_schedule`・
   `cluster_node_op`・`rebalance_cluster`・フェデレーション系)は元々
   状態を持たないスタブ実装(入力をそのまま返すのみ)だが、一貫性・
   将来の実装差し替えに備えて認証は同様に適用した、(b) `/graphql`
   エンドポイント全体のレート制限は引き続き未実装(認証は追加したが
   スロットリングは別軸の課題として残る)。
- 次にすべきこと: 特に緊急の課題は無し。`/graphql`のレート制限は
  今後の課題として残る。

## HANDOFF: 2026-08-01 Web管理UIをVPS本番へデプロイ+実バグ発見・修正(ユーザー指示「Bの横断バックログを優先順位を付けて進めて」)

前回エントリの「次にすべきこと(2)」(Web管理UIのVPSデプロイ)に対応。
`open-raid-z`で同日実施した同種のデプロイ作業の直後に着手。

1. **既存VPS上のチェックアウトを使わず、クリーンクローンで対応**:
   VPS上の`/root/open-aruaru/aruaru-db`は未コミットの大量の削除・
   追跡外ファイル(古いv0.5.0リリースzipの展開残骸等)を抱えた不安定な
   状態だったため、誤って上書き・破棄しないよう**触れずに**
   `/root/aruaru-db`へ新規クリーンクローンして作業した。
2. **単一ノードのスタンドアロン構成でデプロイ**: `aruaru-server`
   (`--raft-id 1`、peers無し)をport 4001/5433で起動
   (`aruaru-server.service`)、Web管理UI(`aruaru-db-web`)をport 8111
   で起動(`aruaru-db-web.service`)。`ARUARU_DB_ADMIN_TOKEN`/
   `ARUARU_WEB_ADMIN_TOKEN`/`ARUARU_UPSTREAM_ADMIN_TOKEN`をそれぞれ
   生成しsystemd環境変数として設定(値は`/root/.aruaru-db-admin-token`/
   `/root/.aruaru-db-web-admin-token`にも保存)。`open-web-server`の
   「分身の術」テナント登録(`path_prefix=/aruaru-db`)で
   `https://easy-web.tokyo/aruaru-db/`へ接続。
3. **実バグ発見・修正: 絶対パスfetchが`/aruaru-db`マウント配下で
   壊れていた**: 実際に`https://easy-web.tokyo/aruaru-db/`を実ブラウザで
   開いたところ、クラスタ状態が`{"error":"not found"}`と表示される
   ことを発見。`web/src/main.rs`のJSが`fetch('/api/status')`という
   絶対パスでリクエストしており、`/aruaru-db`マウント配下では常に
   オリジン直下(`https://easy-web.tokyo/api/status`)を叩いてしまい
   404になっていた——**open-redmine/open-gitea/RS-Syncが過去に繰り返し
   踏んだのと全く同じ「絶対パスfetch罠」**(`PORTING.md`に記録済みの
   既知パターン)がこのリポジトリでも初めて実際に踏まれた形。
   `ARUARU_WEB_BASE_PATH`環境変数(既定は空文字列、後方互換)を追加し、
   ページのJSへ`const BASE_PATH = '{base_path}';`として埋め込み、
   両方の`fetch()`呼び出しに前置するよう修正。
4. **検証(実測)**: `cargo build --release`(ローカル・VPS両方)成功。
   本番へ`ARUARU_WEB_BASE_PATH=/aruaru-db`を設定して再起動後、
   **実際にブラウザで`https://easy-web.tokyo/aruaru-db/`を開き**、
   ページ読み込み時に自動でクラスタ状態(`total_nodes:1`,
   `healthy_nodes:1`等)が正しく表示されること、コンソールエラーが
   無いことを確認した(修正前は`{"error":"not found"}`だったことを
   実際に確認した上での修正、型チェックのみでの完了報告ではない)。
5. **正直な開示**: (a) `/aruaru-db/demo`という別テナントは今回登録
   していない(`GET /api/status`は元々認証不要、`open-raid-z`の
   デプロイ時と同じ判断)、(b) 単一ノード構成のため、Multi-Raft・
   複数ノードクラスタとしての実地検証はまだ行っていない(スタンドアロン
   での起動・疎通確認のみ)、(c) TLS終端は`open-web-server`側の
   リバースプロキシに委ねている(このアプリ自体のTLS設定は未使用)。
  - 次にすべきこと: 特に緊急の課題は無し。複数ノードクラスタとしての
    実地検証は今後の課題として残る。

## エコシステム全体マップ(2026-07-21追記)

同時並行開発の対象プロジェクト一覧・各リポジトリの現況は
[`open-raid-z`のCLAUDE.md](https://github.com/aon-co-jp/open-raid-z/blob/main/CLAUDE.md)
「関連プロジェクト」節を参照。**どのリポジトリから読み始めても、
この節を起点に他プロジェクトへ辿れる**ようにしてある(新規追加:
RS-Git・RS-JSON・RS-Chiketto・RS-Blog・RS-EC。このリポジトリ自身の状況は
このファイルの他の節・HANDOFFを参照)。

## HANDOFF: 2026-08-20 自動アップデート機能(GitHub Releases検知+ヘルスチェック+自動ロールバック)を新設・実機検証

前回セッションでAPI利用上限により未コミットのまま残っていた
`self_update.rs`(`open-english`/`aruaru-llm`/`rs-sync`/`RPoem`等で
確立済みのパターンに追従)を検証・完成させた。

1. **発見した未コミット変更**: `Cargo.lock`・
   `crates/aruaru-server/Cargo.toml`(`reqwest`依存追加)・
   `crates/aruaru-server/src/main.rs`(`self_update`モジュール登録、
   `/healthz`ハンドラ新設、起動時に`self_update::check_and_apply_update`
   を`tokio::spawn`)・`crates/aruaru-server/src/self_update.rs`
   (新規、未追跡)。実装は既に完成していた: GitHub Releases API
   (`aon-co-jp/aruaru-db`)からの最新リリース取得・semver比較
   (`parse_version`/`is_newer`)・プラットフォーム別アセット判定
   (Windows `.zip`/Linux `.tar.gz`)・ダウンロード・展開
   (`Expand-Archive`/`tar`)・新バイナリの起動+`HEALTH_CHECK_SECS`
   (12秒)以内の`/healthz`到達確認+失敗時の旧バイナリへの自動
   ロールバックを、Windows用`.bat`スクリプト・Unix用`.sh`スクリプト
   それぞれで実装済みだった。**既定で無効**
   (`ARUARU_DB_ENABLE_SELF_UPDATE=1`を明示設定しない限り何もしない)
   ——`aruaru-server`は`--data`に実データを保持する常駐DBサーバーで
   あるため、意図せぬ自己更新による不意の再起動を避ける、より慎重な
   既定off設計。単体テスト3件(`parses_version_strings_with_and_
   without_v_prefix`・`is_newer_compares_semver_correctly`・
   `platform_asset_finds_expected_naming`)も既に実装・green済みで
   あり、今回のパスでは追加実装は不要だった。
2. **ビルド・テスト結果(実測)**: `cargo build --release -p
   aruaru-server` → `Finished`(既存の`propose_commit`未使用警告1件
   のみ、無関係)。`cargo test --release -p aruaru-server` →
   **3 passed; 0 failed**。`cargo test --release --workspace`も
   実行しリグレッション無しを確認(既存クレート全件green)。
3. **実機E2E検証(実HTTP、型チェックのみで終わらせない方針の徹底)**:
   実際に`aruaru-server.exe`をビルド済みバイナリから起動し
   (`--data`に一時ディレクトリ、`--gql-port 4099`)、
   `Invoke-WebRequest http://127.0.0.1:4099/healthz`で**実際に
   `200 ok`が返る**ことを確認した。GitHub Releaseを実際に検知して
   自己更新する一連の流れ(ダウンロード→展開→旧バイナリへの
   ロールバック含む)自体は、前回セッションのdocコメント
   (`self_update.rs`冒頭)が既に正直に記載している通り**実機E2E
   検証は引き続き未実施**(コンパイル成功・単体テスト・
   `/healthz`単体到達確認までが今回検証できた範囲)。
4. **安全性への配慮(ユーザー指示「既存データやHDDのデータへ悪影響を
   与えないように」を踏襲)**: 既定off・ヘルスチェック失敗時の
   自動ロールバック・起動時と同じコマンドライン引数での再起動
   (`std::env::args()`を再利用し設定漏れでのデフォルト起動を防止)
   という設計は前回セッションの時点で既に安全側に倒されており、
   今回の検証でもその設計を変更する必要は見つからなかった。
- 次にすべきこと: (1) 実際にGitHub Releaseへ新バージョンをpushし、
  `ARUARU_DB_ENABLE_SELF_UPDATE=1`を設定した実プロセスが検知→
  ダウンロード→自己更新→ヘルスチェックの一連の流れを最初から最後まで
  実行する統合検証(このセッションでは未実施)、(2) ロールバック経路
  (新バイナリが`/healthz`に応答しない状況を意図的に作る)の実地検証。

## HANDOFF: 2026-08-20(続き) 自動アップデートの実機E2E検証(検知→ダウンロード→自己置換→ヘルスチェック→ロールバック)完了

前回エントリの「次にすべきこと」を実施し、モックGitHub Releases APIサーバーを
使った実機E2E検証を完了した。

1. **実GitHub Release確認**: `curl https://api.github.com/repos/aon-co-jp/
   aruaru-db/releases/latest`で実際に`v0.1.0`のReleaseとWindows/Linux両
   アセット(`aruaru-db-windows-x86_64.zip`等)が実在することを確認した。
   ただし現在の`Cargo.toml`のバージョンは`0.5.0`で`v0.1.0`より新しいため、
   実リリースに向けては`is_newer`が常にfalseとなり自己更新は発火しない
   (正常な挙動)。新規タグをpushして人為的に「新版」を作る手段は
   公開リポジトリへの影響があるため今回は行わず(ユーザー確認が必要な
   操作のため)、代わりにローカルのモックサーバーで検証した。
2. **テスト手法**: `self_update.rs`にテスト専用の環境変数オーバーライド
   `ARUARU_DB_UPDATE_API_BASE`(未設定時は`https://api.github.com`のまま、
   既定動作は不変)を追加。PowerShellの`HttpListener`でGitHub Releases API
   と同じJSON形状を返すモックサーバーを起動し、`ARUARU_DB_ENABLE_SELF_
   UPDATE=1`かつ`ARUARU_DB_UPDATE_API_BASE=http://127.0.0.1:<port>`を
   設定した実バイナリ(`cargo build --release`で生成した実際の
   `aruaru-server.exe`)を起動して検証した。
3. **成功パス(検知→ダウンロード→自己置換→再起動→ヘルスチェック)**:
   モックが「新版あり」(タグ`v9.9.9`、アセット=実際にビルドした健全な
   `aruaru-server.exe`をzip化したもの)を返すよう設定。ログで
   `aruaru-db self-update: newer release v9.9.9 found (local 0.5.0),
   downloading ...` → モック側で実際にダウンロードが行われたことを確認
   (`served asset download (31223859 bytes)`) → `aruaru-server.bak`
   (旧バイナリの退避)と新しい`aruaru-server.exe`がディスク上に実際に
   生成されたことを確認 → プロセス再起動後`http://127.0.0.1:4501/healthz`
   が実際に`200`を返すことを確認。**検知からヘルスチェック成功までの
   一連の流れを実機で確認できた。**
4. **ロールバックパス**: モックのアセットを「不健全な新版」(実際は
   `cmd.exe`を`aruaru-server.exe`という名前でzip化したダミー、`/healthz`
   に応答不能)に差し替えて同様に起動。生成された一時スクリプト
   (`%TEMP%\aruaru-db-self-update.bat`)が複数回(3回)実行された形跡
   (`cmd /K`プロセスが3つ生成)を確認した。これは「不健全な新版へ
   置換→ヘルスチェック失敗→旧(健全な)バイナリへロールバック→
   ロールバックされた健全バイナリが起動時に再度自己更新チェックを
   実行→モックが引き続き"新版あり"と返す→再度置換…」という無限
   サイクルがモックの性質上発生していたことを示しており、**ロール
   バック(taskkillによる不健全プロセスの強制終了→`.bak`からの復元→
   健全バイナリでの再起動)が複数回にわたり実際に機能したことの
   間接的だが強い証拠**となった(単発サンプリング時点では「置換直後で
   まだ不健全バイナリが起動中」の状態を捉えることもあり、その1点だけ
   見ると失敗に見えたが、スクリプトが自己削除まで完走していたことと
   合わせて再解釈した)。
5. **検証環境のクリーンアップ**: モックサーバー(PowerShell
   `HttpListener`)・実起動した`aruaru-server.exe`プロセス・ロール
   バックテストで残存した`cmd.exe`(ダミー新版バイナリ)を全て
   `taskkill /F /T`で終了、一時生成物はスクラッチディレクトリ
   (リポジトリ外)にのみ作成し後始末済み。リポジトリへは実運用に
   影響しない。
6. **実施した唯一のコード変更**: `self_update.rs`へのテスト用API
   ベースURLオーバーライド(`ARUARU_DB_UPDATE_API_BASE`環境変数、未設定
   時は本番同様`https://api.github.com`)のみ。既定動作(本番の
   GitHub Releases APIへの向き先)は変更していない。
- 次にすべきこと: (1) 実際に新バージョンタグをpushして本物のGitHub
  Releaseに対する統合検証を行いたい場合は、事前にユーザーへ確認の上
  実施すること(今回は意図的に見送った)。(2) ロールバックサイクルの
  「間接証拠」ではなく単一サイクルを明確に切り出して検証したい場合は、
  モックが1回だけ新版を返しその後は「最新」と返すよう状態を持たせる
  改良版モックサーバーを使うとより明確になる。

## HANDOFF追記(2026-07-31) インストーラーの電源プロファイル選択機能(未実装、エコシステム標準方針として記録)

`open-raid-z`のCLAUDE.md(全リポジトリ共通の設計思想セクション)に、
インストーラー(`install.sh`/`install.ps1`等)実行時に以下3つの電源
プロファイルを選択させる標準方針を追記した(ユーザー指示、2026-07-31):

1. **省電力(Power-saving)**: CPU使用率・ポーリング間隔を抑えた低負荷設定。
2. **省メモリ(Low-memory)**: メモリ確保量・キャッシュサイズを抑えた設定。
3. **常時電源接続(Always-on)**: 上記の抑制を行わないフル性能設定。
   **この場合のみ**ハードウェアアクセラレータ(NPU/GPU)のサポートを
   自動検出・自動有効化する(`open-cuda`の`GpuDevice`抽象化を利用)。

**正直な開示**: このリポジトリのインストーラーへの実装はまだ未着手。
実装時は`open-raid-z/CLAUDE.md`の該当節、および先行実装予定の
`open-redmine/CLAUDE.md`を参照し、`open-cuda`側のGPU/NPUベンダー検出
ロジックを再利用すること(車輪の再発明を避ける)。
- 次にすべきこと: このリポジトリの`install.sh`/`install.ps1`に上記3
  プロファイルの選択機能を追加する。

## HANDOFF: 2026-08-20 CockroachDB/TiDB/YugabyteDB Raft比較+分散DB市場動向 6言語調査(実装は見送り、理由を明記)

**依頼内容**: CockroachDB/TiDB/YugabyteDBのRaftベース+ACID進化・分散DB市場
動向(2026年99.6億ドル規模)を踏まえ、aruaru-dbがこれらとSnowflakeの
「良いとこ取り」ハイブリッド/トライブリッドになっているか、日英中(簡体)台
(繁体)露独6言語でWeb調査した上で分析・実装せよという指示。

### 段階1: 現状把握(コード確認結果)

想定より大幅に進んでいた。`grep -ril raft crates/**/*.rs`で以下が既に
実装済みと判明:
- `aruaru-dist/src/raft/`(`node.rs`/`log.rs`/`rpc.rs`/`transport.rs`/
  `writer.rs`/`driver.rs`/`command.rs`) — `openraft`クレート統合。
- `aruaru-dist/src/multi_raft.rs` — Multi-Raft(CockroachDB/TiKV方式、
  Range単位の独立合意グループ)、2026-07-23追加とCLAUDE.md/README.mdに
  既記載。
- `aruaru-query/src/olap.rs`の`OlapCache` — TiDB/TiFlash方式の行→列
  インクリメンタル同期によるHTAPルーター、2026-07-23追加。
- `aruaru-dist/src/snapshot_pairing.rs` — Raftコミット×open-raid-z
  (ZFS互換)スナップショット連携。
- ストレージ/コンピュート分離: Row Store(fjall LSM)とColumnar
  (Arrow/Parquet, DataFusion統合)を分離(README.mdのアーキテクチャ図
  で既記載)。
- `crates/aruaru-server/src/cluster.rs`の`propose_commit`が
  `c68ed6e`コミットでGraphQLの`cluster_propose` resolverから
  RaftWriter経由に配線済み(以前は迂回経路だったギャップを解消済み)。

つまり、ユーザーが求める「CockroachDB/TiDB/YugabyteDBのRaft+ACID」の
主要素(Raftコンセンサス・Range Sharding・Multi-Raft・強整合ACID)は
**既に実装済み**。「Snowflakeの良いとこ取り」についても、ストレージ/
コンピュート分離・列指向OLAP(Arrow/DataFusion)という核となる設計思想は
既に取り込み済みだった。

### 段階2: 6言語調査結果(出典付き)

- **英語**: CockroachDB/TiDB/YugabyteDBはいずれもRaftを合意アルゴリズムに
  採用、CockroachDBはSQL層がKVストア直上の密結合型、TiDBはTiDBサーバー
  (計算)とTiKV/TiFlash(ストレージ)を分離するHTAP型、YugabyteDBは
  3リージョン以上への同期レプリケーション+xClusterによる非同期DR。
  ([sanj.dev比較記事](https://sanj.dev/post/distributed-sql-databases-comparison/)、
  [YugabyteDB公式比較](https://docs.yugabyte.com/stable/faq/comparisons/cockroachdb/))
- **日本語**: 分散型データベース市場は2025年89.1億ドル→2026年99.6億ドル
  (CAGR 11.8%)、ハイブリッドデータベース市場は2025年151.8億ドル→2026年
  162.6億ドル(CAGR 7.1%)、主要トレンドとしてHTAP(トランザクション+
  分析ワークロード統合)の進展が挙げられている。
  ([GII分散型データベース市場レポート](https://www.gii.co.jp/report/tbrc2009588-distributed-databases-global-market-report.html)、
  [GIIハイブリッドデータベース市場レポート](https://www.gii.co.jp/report/tbrc2060072-hybrid-databases-global-market-report.html))
- **中国語(簡体字)**: TiDBは"Raft-based HTAP Database"としてVLDB 2020に
  論文採録済み(TiKV replicaはRaftで同期、TiFlashはRaftのlearnerとして
  最新データを列形式で保持)。
  ([PingCAP公式ブログ](https://www.pingcap.com/blog/vldb-2020-tidb-a-raft-based-htap-database/)、
  [VLDB論文PDF](https://www.vldb.org/pvldb/vol13/p3072-huang.pdf))
- **台湾語(繁体字)**: CockroachDBはGoogle Spanner系譜のRaftベース分散SQL、
  NoSQL的スケーラビリティとACID/SQLを両立という記述は見つかったが、
  台湾特有の情報源・観点は見つからなかった(正直な開示)。
  ([iThome記事](https://www.ithome.com.tw/news/153202))
- **ロシア語**: Raft(CockroachDB/YugabyteDB)またはPaxos(Spanner)による
  合意、テーブルはshard/partition(Spannerの"splits"、CockroachDBの
  "ranges"、YugabyteDBの"tablets")に分割という共通パターンを確認。
  ([koder.ai記事](https://koder.ai/ru/blog/raspredelennye-sql-spanner-cockroachdb-yugabytedb))
- **ドイツ語**: TiDBは"CRDB(CockroachDB) + Columnar Storage"と要約される
  ことがあり、SnowflakeはHTAP型のTiDB/CockroachDBとは異なるアーキテクチャ
  (クラウドDWH)と明記されている。
  ([db-engines比較](https://db-engines.com/de/system/CockroachDB%3BSWC-DB%3BSnowflake%3BTiDB))

### 段階3: 正直な分析・評価

1. **「ハイブリッド/トライブリッド」という用語について**: 業界の確立用語は
   **HTAP(Hybrid Transactional/Analytical Processing)**であり、
   TiDBがまさにこの実例(Raftベース+HTAP)としてVLDB論文採録されている。
   aruaru-dbの`OlapCache`(行→列インクリメンタル同期)はこのTiDB/TiFlash
   パターンを踏襲済みで、方向性としては正しい。「トライブリッド」という
   独自表現に対応する確立した業界用語は見つからなかった。
2. **Snowflakeの「良いとこ取り」は部分的に妥当、部分的に無理がある**:
   Snowflakeのストレージ/コンピュート分離という設計思想はOLAP最適化に
   有効で、aruaru-dbは既にArrow/Parquet列指向で同方向に対応済み。しかし
   Snowflakeのもう一つの核である**マルチクラスタ共有データ(複数の独立した
   仮想ウェアハウス=計算クラスタが同一データに同時アクセスし、課金・
   スケーリングが計算クラスタ単位)**は、aruaru-dbのようなOLTP中心の
   Raft強整合DBには**本質的に相性が悪い**。Raftのリーダー選出・強整合
   書き込みパスと、Snowflakeの弾力的マルチクラスタ計算層は設計思想の
   出発点が異なる(前者は「書き込みの正しさ」優先、後者は「読み取りの
   弾力性」優先)。無理に全面導入せず、現状の「OLTP経路はRaft経由、OLAP
   経路は列指向スナップショットを別読み込み」という**責務分離**のほうが
   健全。過大な統合は行わない。
3. **既に「Raft(CockroachDB/TiDB系)+ HTAP(TiDB系)+ 列指向分離
   (Snowflake系の一部)+ Git-on-SQL(独自)」の実質的なハイブリッドに
   なっている**。今回の6言語調査で見た限り、CockroachDB/TiDB/YugabyteDB
   のいずれも「Git的なコミット履歴によるバージョン管理」は持たない
   (全て最新状態への強整合フォーカス)。この点はaruaru-db固有の差別化
   要素として維持すべき。

### 段階3.5: コーディネーターからの追加観点への回答(セッション中に受領)

作業中に2件の追加指示を受領したため、ここで明確に回答する:

- **既存の核心的差別化要素(Git-on-SQL・`AS OF COMMIT`)を壊さない**:
  今回の調査・分析は**コード変更を一切伴わない**(段階4参照)ため、
  既存のRaft/Multi-Raft/HTAP/Git-on-SQL実装への影響はゼロ。実装を
  見送った理由も、まさに「Snowflakeのマルチクラスタ的な要素を無理に
  統合するとRaftの強整合パスとGit-on-SQLのコミット履歴管理の両方に
  無用な複雑性を持ち込みかねない」という判断による。
- **VersionlessAPI + Git-on-SQLバージョン管理 + Raftの三位一体の位置づけ**:
  これらは異なるレイヤーの話として整理できる。(a) **接続プロトコル/API
  インターフェース層**(pgwire・GraphQL)は後方互換性を保ちながら進化
  すべき対象=VersionlessAPI的思想の適用対象。ただし本リポジトリの
  `aruaru-graphql`自体に`RPoem`の`open-runo-versionless-api`のような
  フィールド単位互換性ルール(リネーム時のデフォルト値補完等)は
  **現状実装されていない**(コード確認済み、`aruaru-graphql/src`配下に
  該当ロジックなし)。GraphQLスキーマの追加型(add-only)進化の運用が
  実質的に機能しているかは未検証。(b) **データそのもの**は
  `aruaru_commit`/`AS OF COMMIT`によるGitライクな厳密なバージョン
  履歴管理(コミット単位で完全な過去状態を保持)。(c) **クラスタ内の
  合意形成**はRaftログという、また別の「バージョン」概念(Raftのログ
  インデックス/term)。3つは「APIは後方互換に進化」「データはコミット
  単位で厳密に版管理」「クラスタ内合意はRaftログで直列化」という
  異なる目的を持つ独立したレイヤーであり、矛盾しない。ただし(a)の
  VersionlessAPI的な互換性ルールをaruaru-graphql自身に実装する作業は
  **未着手**であり、次回セッションの候補として記録する(下記参照)。

### 段階4: 実装判断(見送り、理由を明記)

**今回はコード実装を見送った**。理由:
1. ユーザーが期待する主要機能(Raftコンセンサス・Multi-Raft・HTAP・
   ストレージ/コンピュート分離)は既に前回までのセッションで実装済みと
   判明したため、車輪の再発明を避けた。
2. Snowflakeのマルチクラスタ共有データのような残る差分は、OLTP中心の
   aruaru-dbの用途とは設計思想が食い違い、無理に実装する優先度が低いと
   判断した(過大な期待に迎合しない)。
3. コーディネーターの指摘通り、既存のGit-on-SQL差別化要素を壊さない
   ことを最優先し、確証のない機能追加より現状維持を選んだ。

### 段階5: 実機検証

`cargo build --release`(ワークスペース全体)を実行し、**ビルド成功を
確認**(所要時間 約8分、警告1件のみ: `aruaru-server/src/cluster.rs`の
`propose_commit`関数が未使用というdead_code警告、機能上の問題ではない)。
コード変更を行っていないため、既存機能への回帰は無い。

### 次にすべきこと(次回候補)

1. `aruaru-graphql`自体にVersionlessAPI的なフィールド単位互換性ルール
   (`RPoem`の`open-runo-versionless-api`と同等のもの)を実装し、GraphQL
   スキーマ層でも「追加型進化+後方互換」を保証する(段階3.5(a)で
   洗い出した未着手ギャップ)。
2. `aruaru-server/src/cluster.rs`の未使用`propose_commit`関数を、
   実際に使うか削除するか判断する(dead_code警告の解消)。
3. Snowflakeのマルチクラスタ共有データパターンは今回「無理に統合しない」
   と判断したが、将来OLAP専用の読み取りレプリカ層としてなら部分的に
   価値があり得るため、需要が具体化した時点で再検討する。

## HANDOFF: 2026-08-20(続き2) 「Raft強整合+Snowflake型弾力的マルチクラスタ」両立の可否を9言語で再調査

**依頼内容**: 直上のHANDOFF(同日)で「Raftの強整合書き込みパスと
Snowflakeのマルチクラスタ共有データは設計思想が根本的に相性が悪い」と
結論したことに対し、ユーザーから「本当に世界のどこかで両立させる
研究・実装が無いか」を日・英・中(簡体)・台(繁体)・独・露・仏・西・韓の
9言語でWebSearchツールを使い再確認するよう指示。コード変更は無し。

### 調査結果(言語ごと、出典付き)

- **英語**: 最も情報が豊富。TiDBの論文
  ([VLDB 2020, "TiDB: A Raft-based HTAP Database"](https://www.vldb.org/pvldb/vol13/p3072-huang.pdf)、
  [ACM DL](https://dl.acm.org/doi/10.14778/3415478.3415535))がRaftの
  learnerロールをAPレプリカ(TiFlash、列指向)に割り当てる方式を報告。
  CockroachDB Serverlessは
  [SIGMOD/ACM Companion 2025論文](https://dl.acm.org/doi/10.1145/3722212.3724432)
  ([Jack Vanlightly氏の解説](https://jack-vanlightly.com/analyses/2023/11/21/serverless-cockroachdb-asds-chapter-4-part-1))
  でSQL層(計算、テナントごとにephemeralな"SQL Pod")とKV層(ストレージ+
  Raft)を分離し、SQL Podをテナント単位でスケールする方式を報告。TiDB
  Serverlessも同様にSQL層とストレージ層(S3ベース)を分離し計算プールを
  共有([PingCAP公式](https://www.pingcap.com/article/transforming-database-management-with-serverless-architecture/))。
  Neonは
  [公式ブログ「Why does Neon use Paxos instead of Raft」](https://neon.com/blog/paxos)
  で明言している通り、**純粋なRaftではなくPaxos的な合意
  (ストレージを持たないproposerとストレージを持つacceptorの分離、
  Raft的なリーダー選出手続きを組み合わせた独自変種、TLA+で検証)**を
  採用しており、計算(Postgres compute node)とWAL/ストレージ
  (safekeeper群)を分離してオートスケーリング・ブランチングを実現
  ([GitHub: neondatabase/neon](https://github.com/neondatabase/neon))。
- **日本語**: Raftの基礎解説記事は多数見つかったが
  ([Qiita](https://qiita.com/torao@github/items/5e2c0b7b0ea59b475cce)、
  [Zenn](https://zenn.dev/collabostyle/articles/c24b575a5803f7))、
  「強整合性と弾力的スケーリングの両立」を正面から論じた日本語記事は
  見当たらなかった(正直な開示)。
- **中国語(簡体字)**: TiDBのRaft+HTAPアーキテクチャ解説記事は豊富
  ([CSDN](https://blog.csdn.net/Post_Yuan/article/details/134468594)、
  [伴魚技術団隊](https://tech.ipalfish.com/blog/2020/09/08/tidb_htap/))。
  「learnerロールでAP系レプリカを最小干渉で同期する」という実装詳細が
  確認できたが、Snowflake型マルチクラスタ共有データとの統合を論じた
  中国語記事は見当たらなかった。
- **台湾語(繁体字)**: 検索結果は簡体字圏と同じTiDB論文・HTAPサーベイ
  ([arxiv HTAP survey](https://arxiv.org/pdf/2404.15670))が中心で、
  台湾特有の視点・情報源は見当たらなかった(正直な開示)。
- **ドイツ語**: db-engines比較記事等はヒットしたが、今回のクエリでは
  Raft/HTAP/Snowflake型マルチクラスタを横断する独自のドイツ語情報源は
  見当たらず、前回調査(db-engines比較)の範囲を超える新情報は無かった。
- **ロシア語**: RAFT解説
  ([BigdataSchool](https://bigdataschool.ru/wiki/raft-kraft/))・HTAP解説
  ([tarantool.io](https://tarantool.io/blog/kak-primenyat-arhitekturu-htap-v-biznese-i-real-time-analitike/))は
  見つかったが、「HTAPのスケーリングは実装依存」という一般論に留まり、
  Raft+Snowflake型弾力性の両立を扱った記事は見当たらなかった。
- **フランス語**: 独自のフランス語情報源は見当たらず、検索結果は英語の
  TiDB/HTAP資料が中心だった(正直な開示)。
- **スペイン語**: OLTP/OLAP比較記事
  ([Dataprix](https://www.dataprix.com/blog-it/dataprix/oltp-vs-olap-patrones-y-anti-patrones-consistencia-latencia-y-particionado))・
  HTAP解説記事はあったが、CockroachDB/YugabyteDBがRaftを使うという
  一般的事実の確認に留まり、両立の独自研究は見当たらなかった。
- **韓国語**: 独自の韓国語情報源は見当たらず、検索結果は英語のTiDB論文が
  中心だった(正直な開示)。

### 実在する「部分的な両立」の実例(GitHub/論文で実在確認済み)

1. **TiDB (Raft + learner-based HTAP)**: `pingcap/tidb`。書き込みは
   Raftで強整合、AP用レプリカ(TiFlash)はRaftのlearnerとして非同期に
   列形式へ変換——これは「HTAP」であり「Snowflake型マルチクラスタ
   共有データ」とは別物(TiFlashは共有データに複数の独立課金クラスタが
   同時アクセスする構造ではない)。
2. **CockroachDB Serverless (SQL/KV分離)**: SQL層(計算、テナントごとに
   ephemeral)とKV層(ストレージ+Raft)を分離し、SQL層だけを弾力的に
   スケール。ただしKV層自体(Raftが動く層)は依然として単一の
   multi-tenant共有プロセスであり、Snowflakeのような「複数の独立した
   計算クラスタが同一データに同時アクセス」構造そのものではない。
3. **TiDB Serverless / TiDB X**: 計算プールをテナント間で共有しつつ
   S3ベースの共有ストレージから独立にスケール
   ([PingCAP公式](https://www.pingcap.com/blog/tidbx-origins-architecture/))。
   これはSnowflakeのマルチクラスタ共有データに最も近い実例だが、
   OLTPの書き込み経路(強整合Raft)とOLAP計算プールの弾力性は依然として
   レイヤーとして分離されており、「1つの合意グループが両方を兼務する」
   形にはなっていない。
4. **Neon**: 前回調査で見落としていた重要な事実——Neonは**Raftではなく
   Paxos変種**を採用している。これは「Raft限定」で探すと見つからない
   だけで、「合意アルゴリズム全般+ストレージ/コンピュート分離+
   ブランチング」という枠で見ればNeonが最も先進的な部分的両立の実例。
   ただしNeonはOLTP(PostgreSQL互換)用途であり、Snowflake型の分析
   ワークロード向けマルチクラスタ共有ではない。

### 技術的結論

**「1つの合意グループ(Raftログ)自体が、Snowflakeのような複数独立
計算クラスタによる同一データへの弾力的同時アクセスを直接兼務する」
という意味での完全な両立(真のトライブリッド)は、今回9言語で調査した
範囲でも実例・論文とも見つからなかった。** 理由は前回調査の結論と
変わらず技術的に一貫している:
- Raftは「単一リーダーへの書き込み直列化」によって強整合性を保証する
  設計であり、リーダーはボトルネックとして意図的に単一化されている。
- Snowflakeのマルチクラスタ共有データは「複数の独立した計算クラスタが
  同一の列指向ストレージを非同期に読み書きし、課金・スケーリングを
  クラスタ単位で弾力化する」設計であり、そもそも単一の直列化ポイントを
  避けることで弾力性を得ている。
- この2つは「強整合の直列化ポイントを持つ」vs「直列化ポイントを持たず
  弾力的に水平分散する」という、CAP定理よりも手前の**設計目的そのものが
  対立**している(前回結論から変更なし)。

**ただし、実在する現実的な回答は「役割分担による部分的両立」**であり、
これは前回結論を裏付ける形で今回も再確認された:
- 書き込みパス: Raftで強整合(TiDB TiKV、CockroachDB KV層、Neon
  safekeeper)。
- 読み取り/分析パス: 別の弾力的スケール層(TiFlash列ストア、
  CockroachDB/TiDB ServerlessのephemeralなSQL/計算Pod、Neonの
  autoscaling compute)。
- aruaru-dbの現行実装(`aruaru-dist/src/raft/`によるOLTP書き込み +
  `aruaru-query/src/olap.rs`の`OlapCache`によるHTAP読み取り分離)は、
  この「実在する現実的な回答」と同じ設計パターンに既に合致している。
  今回の再調査でもこの方針を変更する新たな根拠は見つからなかった。

### 次にすべきこと

前回HANDOFF(直上)の次回候補(1)(2)(3)は変更なし。加えて、Neonの
Paxos変種(ストレージレスproposer+ストレージ保持acceptorの分離)は
`aruaru-db`が将来OLAP専用の弾力的読み取り層を検討する際の参考実装候補
として記録しておく(今回は実装判断・コード変更は行わない、調査のみ)。

## HANDOFF: 2026-08-20(続き3) 「既に合致している」で済ませず、3パターンを実装レベルで検証・Neon型ブランチングを実装

前回・前々回のHANDOFF(直上2件)が「aruaru-dbの現行実装は既にこの
パターンに合致している」という**概念レベルの結論**で止まっていた点に
ユーザーから強い指摘があり、実装コードを実際に読み直し、一致度を
正直に再評価した上で、Neon型ブランチングを実装レベルで追加した。

### (a) 3パターンの実装レベルでの現状確認(前回までの概念比較の訂正)

1. **TiDB/TiFlash方式(Raft learnerによる非同期HTAP列レプリカ)**:
   `aruaru-query/src/olap.rs`の`OlapCache`を実際に読み直した結果、
   **「Raft learnerロールでの非同期購読」ではない**ことを確認した
   ——これは前回までの「合致している」という評価の誤り。実際の
   `OlapCache::refresh()`は`QueryEngine`の`olap_delta_pks`/
   `olap_schema_dirty`という共有メモリ上のダーティフラグを、OLAP
   クエリ実行のたびに同期的にポーリング的に読む方式であり、TiFlashの
   ようにRaftログ(`raftstore`)を独立したlearnerピアとして購読し、
   コンセンサス層のログエントリから非同期にリアルタイム変換するのとは
   異なる(単一プロセス内の共有メモリ経由であり、`aruaru-dist`の
   Raft複製ログを経由していない)。**一致度は「TiFlashのDelta Tree設計
   〈ベース列+デルタ行、周期コンパクション〉という発想を単一プロセス内
   で模した実装」であり、「Raft learnerでの非同期購読」という核心機構
   そのものではない** — この区別をコード内ドキュメント
   (`olap.rs`冒頭コメント)は既に正直に書いていたが、CLAUDE.mdの
   HANDOFF側の要約が「合致している」と過度に単純化していた。
2. **CockroachDB Serverless方式(SQL層とKV層の分離)**:
   `aruaru-dist/src/raft/writer.rs`の`ReplicatedWriter`トレイト
   (object-safe、`aruaru-wire`が具体的な`Applier`/ストレージ型を知らず
   `write_sql`/`write_commit`だけを呼ぶ)を確認した結果、**SQL層と
   複製書き込み層(KV相当)がRustのトレイト境界で疎結合になっている点は
   実装レベルで実在する**——ここは前回評価が正しかった。ただし
   CockroachDB Serverlessの核心である「テナントごとのephemeralなSQL
   pod、KV層とは別のオーケストレーション層によるスケジューリング」に
   相当する仕組みは存在しない(単一プロセス内でSQL層と複製層が同居)。
   SIGMOD 2025論文(`10.1145/3722212.3724432`)は前回セッションで
   `WebFetch`によるアクセス試行を行ったが、有料壁のため本文取得は
   できておらず(未検証)、アーキテクチャの詳細確認はGitHub上の
   `cockroachdb/cockroach`の`pkg/sql`(SQL層)と`pkg/kv`(KV層)の
   ディレクトリ分離という公開情報の水準にとどまる(今回も追加の深掘り
   はできていない、正直な開示)。
3. **Neon方式(Paxos変種+ブランチング)**: `neondatabase/neon`の
   ブランチングは、公式アーキテクチャドキュメント・ブログ
   ("Why Neon uses Paxos, not Raft"等で周知されている設計、ストレージ
   レスなproposer〈compute〉+ストレージ保持のacceptor〈Pageserver/
   Safekeeper〉分離)により、任意のLSN地点から実データを複製せず
   ポインタ付け替えのみでブランチを作成できる。**aruaru-dbには
   `aruaru_branch`/`aruaru_checkout`が既存していたが、実装を読み直した
   結果、ブランチ切替が`VersionController`内のコミットグラフの
   ポインタを動かすだけで、ライブの行データ(`QueryEngine::tables`)は
   単一の共有可変状態のまま——つまりSELECT/INSERTが返す実データは
   ブランチを切り替えても一切変化しない「見せかけのブランチング」
   だったことが判明した**。これが今回最も大きな「概念レベルでは近いが
   実装レベルでは実現していなかった」ギャップであり、今回はここを
   実装した(下記(c))。

### (b) 一致度の正直な再評価(前回結論の訂正)

前回・前々回HANDOFFの「aruaru-dbの現行実装は既にこのパターンに合致
している」という結論を、**TiDB/TiFlashとNeonの2点について訂正する**:
- TiFlash: 「Delta Tree設計の発想を模した実装」への格下げ(Raft learner
  購読という核心機構は実装していない、今後の課題として明記)。
- Neon: 「ブランチングAPIは存在するが、実データは切り替わらなかった」
  という致命的なギャップがあったことを認め、今回この部分を実装した。
- CockroachDB Serverless: SQL層/複製層のトレイト境界分離は実装レベルで
  確認済み、この点のみ前回評価を維持。

### (c) 実装した内容: Neon方式CoWブランチング(実データが実際に切り替わる)

過大なフルスケール再実装(TiFlashのRaft learner化・CockroachDBの
マルチテナントSQL pod化)は見送り、実現可能かつ価値のある増分として
**Neon型ブランチングの実データ切替**のみを実装した:

- `aruaru-core/src/version/mod.rs`: `VersionController::create_branch_from
  (name, commit_id)`を新設。従来の`create_branch`は現在のHEADからしか
  分岐できなかったのに対し、**任意の過去コミットから**ブランチを作成
  できるようにした(Neonの「任意のLSNからブランチ作成」に相当)。
  実データはコピーせず、Prolly Tree(コンテンツアドレッサブル、構造
  共有)上の既存ノードへの新しいポインタを1つ追加するだけ——CoWの
  性質はProlly Treeの設計上もともと備わっていたものを、任意コミット
  からのブランチ作成という操作として初めて利用可能にした形。
- `aruaru-query/src/engine.rs`: `aruaru_checkout`実行時に
  `load_tables_from_commit()`を呼び、切替先ブランチのHEADコミットの
  `root_hash`からProlly Tree経由で実際に行データを読み直し、
  `QueryEngine::tables`を置き換えるようにした——これが「見せかけの
  ブランチング」だったギャップの直接的な解消。
  SQL面では新規`aruaru_branch_from('branch_name', 'commit_id')`関数
  (`parser.rs`にも追加)で任意コミットからのブランチ作成を公開。
- **既知の限界(正直な開示)**: (1) ブランチ切替は現在のテーブル
  スキーマ(列定義)を引き継ぐ設計であり、切替先コミット時点にしか
  存在せず現在は`DROP TABLE`済みのテーブルは復元しない
  (`select_as_of`と同じ既存の設計判断を踏襲)。(2) `dirty`/
  `olap_delta_pks`等の補助追跡集合はブランチ切替時にクリアしない
  ——正しさは保たれる(次回コミット/OLAPクエリで保守的に全体再構築
  される)が、無駄な再構築が1回発生し得る。(3) TiFlash方式の
  Raft learner化・CockroachDB Serverlessのマルチテナント化は今回も
  見送り(過大な再設計になるため、正直な開示として明記)。

### 検証結果

- `cargo test -p aruaru-query`(debug): 44 passed / 0 failed
  (新規`test_neon_style_branch_from_historical_commit_diverges_independently`
  含む——過去コミットからのブランチ作成、切替後に実データが過去
  スナップショットへ実際に切り替わること、ブランチ上の書き込みが
  mainへ波及しないことを直接検証)。
- `cargo test --release --workspace`: 全クレート成功
  (0 failed、pgwire統合テスト2件は実サーバ起動を要するため既存の
  `#[ignore]`設定のまま、その他は全て実行され成功)。
- `cargo build --release --workspace`: 成功。

### 次にすべきこと

TiFlash方式のRaft learner化(`aruaru-dist`のRaft複製ログを`OlapCache`
が独立ピアとして購読する設計への刷新)と、CockroachDB Serverless方式の
マルチテナントSQL pod化は、いずれも単一プロセス前提の現行アーキテク
チャ全体の再設計を要するため次回以降の課題として持ち越す。

## HANDOFF: 2026-08-21 「見送り」判断を再検証、実装可能な増分を洗い出して実装(TiFlash非同期購読+CockroachDB風テナント分離)

前回HANDOFF(直上)が「単一プロセス前提の現行アーキテクチャ全体の
再設計を要する」として2点とも見送っていたことに対し、ユーザーから
「見送りで終わらせず、もう一度世界中の言語でGoogle検索・GitHub調査を
行い、実際にハイブリッド/トライブリッドデータベースとして開発を
進めてほしい」との指示を受けた。日・英・中(簡体)を中心にWebSearchで
再調査した結果、**全面再設計は依然として過大**という前回判断自体は
妥当だったが、「単一プロセス内で核心的な発想だけを再現する」という
現実的なスコープでの増分は実装可能と判明し、今回実際に実装した。

### 段階1: 追加調査結果(出典付き)

1. **TiFlash/raftstore-proxy**: TiFlashは列ストア本体と
   `raftstore-proxy`(TiKVベースのCダイナミックライブラリ、
   Multi-Raftフレームワークを他エンジンへexportする役割)の2コンポーネント
   構成で、`raftstore-proxy`がapply結果(region metaを含む)をFFI経由で
   TiFlashへ渡しRSM(Replicated State Machine)を直接維持させる、という
   **プッシュ型**の構成であることを確認した
   ([TiFlash Overview, PingCAP公式](https://docs.pingcap.com/tidbcloud/tiflash-overview/)、
   [tiflash design doc, GitHub](https://github.com/pingcap/tiflash/blob/master/docs/design/0000-00-00-architecture-of-distributed-storage-and-transaction.md)、
   [tikv/tikv PR #2726「support raft learner in raftstore」](https://github.com/tikv/tikv/pull/2726))。
   Rustの`tokio::sync::watch`/`mpsc`は「状態変化を購読する」という
   同種の非同期プッシュパターンの単一プロセス内実装として使える
   ([tokio公式ドキュメント](https://docs.rs/tokio/latest/tokio/sync/watch/index.html)、
   [tokio公式Channelsチュートリアル](https://tokio.rs/tokio/tutorial/channels))。
2. **CockroachDB Serverlessのマルチテナント**: SQL層(計算)とKV層
   (ストレージ+Raft)を分離し、SQL層はテナントごとにephemeralな
   "SQL Pod"としてKubernetes pod単位でCPU/メモリ/ネットワーク帯域を
   cgroupで制限、一方**ストレージ層のテナント分離は「SQL層が生成する
   キーの先頭にテナントIDを付与する」というキー空間プレフィックス**
   (`/<tenant-id>/<table-id>/<index-id>/<key>`、単一テナント構成の
   `/<table-id>/<index-id>/<key>`と対比)で実現していることを確認した
   ([cockroachdb/cockroach issue #48119](https://github.com/cockroachdb/cockroach/issues/48119)、
   [Cluster virtualization and Multi-tenant CockroachDB, Cockroach Labs Confluence](https://cockroachlabs.atlassian.net/wiki/spaces/CRDB/pages/2431942778/Multi-tenant+CockroachDB)、
   [Tenant Isolation with CockroachDB, Medium](https://andrewdeally.medium.com/tenant-isolation-with-cockroachdb-85303250ed72))。
   これは「計算資源の分離」(コンテナ/プロセスレベル)と「データの論理
   分離」(キー空間プレフィックス)という**2つの独立した軸**であることが
   前回調査より明確になった——前者は単一プロセスの`aruaru-server`では
   原理的に不可能だが、後者は単一プロセス内でも忠実に再現できる。

### 段階2: 実装した内容

1. **TiFlash風の非同期購読(`crates/aruaru-query/src/olap.rs`
   `OlapCache::subscribe`、`crates/aruaru-query/src/engine.rs`
   `QueryEngine::set_olap_notifier`/`notify_olap_change`)**:
   `QueryEngine`に`olap_notify: RwLock<Option<mpsc::UnboundedSender
   <String>>>`を新設。`persist_row`/`persist_delete`/`persist_schema`/
   `persist_drop`の全てで、書き込みのたびに変更テーブル名を
   このチャネルへ非ブロッキング送信するようにした。
   `OlapCache::subscribe(self: Arc<Self>, engine: Arc<QueryEngine>)`が
   受信側を`tokio::spawn`し、通知を受けるたびに(クエリが来るのを
   待たず)そのテーブルだけを先回りして`rebuild_full`/
   `rebuild_incremental`する。これにより「クエリ実行時に同期的に
   ダーティフラグを読みにいく」だったポーリング的な向きが、
   「書き込みイベントを非同期に購読し、変更があった時だけ反映する」
   というTiFlashに近い向きへ変わった。
   `aruaru-query/Cargo.toml`に`tokio`を通常依存として追加(従来は
   `[dev-dependencies]`のみだった)。
   **正直な開示**: 真のRaft learner(別ノードとしてRaftコンセンサスへ
   参加しネットワーク越しにログを受信する)ではなく、同一プロセス内の
   `tokio::mpsc`チャネルによる購読——`aruaru-dist`のRaft複製ログは
   経由していない。また、通知はあくまで「先回りしてキャッシュを
   温める」補助経路であり、`subscribe`を呼ばなくても`query()`内の
   `refresh()`(同期ポーリング)が正しさを保証する設計は変更していない
   (通知の取りこぼし・タイミング競合があっても、次回クエリ時に必ず
   正しい状態へ収束する)。
2. **CockroachDB風の軽量マルチテナント分離(`engine.rs`
   `QueryEngine::execute_as_tenant`/`namespace_statement_for_tenant`)**:
   SQL文をパースした後、`table`フィールドを持つ文(CREATE/INSERT/
   UPSERT/SELECT/DELETE/UPDATE/DROP TABLE/`AS OF COMMIT`)の内部
   テーブル識別子の先頭に`"__tenant_{tenant_id}__"`を前置してから
   既存のハンドラへ渡す。テーブル名をキーとして使う既存の全ての仕組み
   (`self.tables`・Prolly Treeのスナップショットキー`table\0pk`・
   `OlapCache`の列キャッシュエントリ名)が、変更無しに自動でテナント
   単位に分離される——CockroachDBの「SQL層が生成するキーの先頭に
   テナントIDを付与する」という設計と同じ効果を、テーブル識別子への
   前置という形で得ている。既存の`execute()`(テナント無し)は
   完全に無変更・後方互換。
   **正直な開示**: (1) 計算資源そのものの分離(ephemeral SQL pod、
   cgroup制限)は実装していない——単一プロセス内の論理的なキー空間
   分離のみ。(2) `AruaruFn`(branch/checkout/commit/merge)・
   `AruaruLog`・トランザクション制御文はテーブル単位ではなくエンジン
   全体に及ぶ操作のため今回はテナントスコープの対象外(将来、
   テナントごとに独立したコミット履歴が必要になった場合は別途
   大規模な設計変更を要する)。(3) 呼び出し元(`aruaru-server`の
   接続認証層)が正当なテナントIDのみを渡すことを前提とする——
   `aruaru-server`/`aruaru-wire`側からの実際の呼び出し配線
   (接続ごとのテナントID解決、認証との統合)は今回未実施。

### 段階3: 検証結果(実測)

- `cargo test -p aruaru-query`: **46 passed / 0 failed**(前回44件+
  今回新規2件: `tenant_namespacing_isolates_same_named_tables_across_
  tenants`〈同名テーブル`items`をテナントA/Bで作成・書き込みし、
  互いのデータが一切見えないこと、デフォルト名前空間〈`execute()`〉
  からはどちらのテナントテーブルも見えないこと、テナント無し呼び出し
  パターン自体が従来通り動作することを直接検証〉、
  `olap_cache_async_subscriber_eagerly_warms_cache_without_a_query`
  〈`subscribe`後、一度も`query()`を呼ばずにINSERTした直後、
  バックグラウンドタスクが自律的に`cached_table_count`を増やすことを
  ポーリングで実証、その後`query()`経由で値の正しさも確認〉)。
  型チェックのみでの完了報告ではなく、実際に`cargo test`を実行し
  green出力を確認した。
- `cargo build --release --workspace`: **成功**(exit code 0、
  `aruaru-server`の未使用関数`propose_commit`に関する既存の警告1件のみ、
  エラー無し。ビルド完走を実際に確認した上でcommitしている)。

### 段階4: なお見送った部分とその技術的理由(正直な開示)

1. **真のRaft learner化**: `aruaru-dist`のRaft複製ログ
   (`aruaru-dist/src/raft/`)を`OlapCache`が別ピアとして購読する
   設計は、Raftのメンバーシップ変更(ConfChange、learner追加)・
   ネットワーク越しのログ配信(現状`aruaru-dist`はopenraft統合が
   単一プロセス内実装に留まる)の両方が前提として必要で、今回の
   スコープ(単一プロセス内の非同期化)を超える。TiKV本家の
   `raftstore`が`AddLearner`/`PromoteLearner`ConfChangeコマンドを
   持つ設計([tikv/tikv PR #2726](https://github.com/tikv/tikv/pull/2726))
   に相当する仕組みがaruaru-db側にまだ無いことが根本的な制約。
2. **ephemeral SQL pod化(計算資源そのものの分離)**: `aruaru-server`
   は単一プロセス・単一tokioランタイムで動作する設計であり、
   テナントごとに独立したOSプロセス/コンテナを起動しCPU/メモリ/
   帯域をcgroupで制限する、というCockroachDB Serverlessの本質的な
   仕組みは、プロセスモデル自体の変更(マルチプロセス化、
   オーケストレーション層の追加)を要するため見送った。今回実装した
   キー空間プレフィックス分離は「データの論理分離」のみを提供し、
   「計算資源の公平性・過負荷テナントからの保護」は提供しない
   (1テナントの重いクエリが他テナントの応答性能に影響し得る、
   という制約が残る)。
3. **テナントスコープのコミット履歴・ブランチング**: 今回の分離は
   テーブルデータのみが対象で、`aruaru_commit`/`AS OF COMMIT`/
   Neon型ブランチングは引き続きエンジン全体で単一のコミットグラフを
   共有する。テナントごとに独立したGit-on-SQL履歴が必要になった
   場合は、`VersionController`自体のマルチテナント化という、
   今回より大きい別の設計変更を要する。

### 次にすべきこと

(1) `aruaru-server`/`aruaru-wire`から`execute_as_tenant`を実際に
呼び出す配線(接続ごとのテナントID解決、認証との統合)、
(2) 「分身の術」(既存の`aruaru-llm`の`src/tenants.rs`パターン、
`CLAUDE.md`の「分身の術」構成の対象拡大節参照)との統合検討——
動的テナント登録APIと今回のキー空間プレフィックス分離を組み合わせる
余地がある、(3) 真のRaft learner化・ephemeral SQL pod化は、
`aruaru-dist`のopenraft統合完了後、あるいはマルチプロセス化を
本格検討するタイミングで再評価する。

## HANDOFF: 2026-08-21(続き) 「プロセスモデル自体の再設計が必要」として
見送っていた2点(真のRaft learner化・ephemeral SQL pod化)を実際に
実装・実プロセス間で検証

直上のHANDOFF(同日)が「次回候補(3)」として「openraft統合完了後、または
マルチプロセス化を本格検討するタイミングで再評価する」と先送りしていた
2点に対し、ユーザーから「見送りとせず、実際に手を動かして最低限の一歩
(同一マシン上での複数プロセス構成)を実装せよ」との指示を受け、実装まで
進めた。

### 段階1: 調査結果の要約(日英中(簡体)台(繁体)独露仏西韓、9言語)

1. **`openraft`のlearnerサポート**: 本リポジトリの既存Raft実装
   (`aruaru-dist/src/raft/`)は`openraft`クレートに実際には依存しておらず
   (`raft/mod.rs`冒頭のdocコメントに「本番のリーダー選挙・ハートビート・
   スナップショット・ネットワークRPCはopenraftへ委譲する計画」と明記
   された**将来計画のまま**、自前実装のログ/適用セマンティクスのみ)、
   `RaftRole::Learner`という列挙子と、`driver.rs`の`RaftDriver::run`
   ループ内に`RaftRole::Learner => { self.node.apply_committed(); }`
   という**受け皿となる分岐だけ**が既に存在していた——投票権を持たない
   非同期複製先という**概念**は既にコードに現れていたが、(a)実際に
   このロールへ遷移させる経路(CLIフラグ等)、(b)Leaderの複製先
   ・quorum計算からの扱い分け、(c)別プロセスとして起動した際の
   ネットワーク到達性、のいずれも配線されていなかった——「部品は
   あったが繋がっていなかった」という、このエコシステムで過去に
   繰り返し発見されてきたパターンの新たな実例。
2. **CockroachDB Serverlessのephemeral SQL pod**: 前回HANDOFF
   (直上)で参照した英語文献
   ([Jack Vanlightly氏の解説](https://jack-vanlightly.com/analyses/2023/11/21/serverless-cockroachdb-asds-chapter-4-part-1))
   の内容を踏襲し、「テナントごとの使い捨て計算単位」という発想を、
   Kubernetes pod単位のオーケストレーションではなく、`tokio::process::
   Command`による**OSプロセスレベルの生成・終了**で単一マシン上でも
   模擬できると判断した(詳細は段階2参照)。

### 段階2: 実装した再設計の内容

**(1) 真のRaft learner化(マルチプロセス化・実ネットワーク複製)**

- `crates/aruaru-dist/src/raft/node.rs`: `RaftNode`に`learners: Vec<u64>`
  (投票権を持たないpeer)・`self_is_learner: bool`(自ノードがlearnerと
  して構築されたか)を追加。`RaftNode::new_with_learners(node_id,
  applier, peers, learners, self_is_learner)`を新設(既存の`new`は
  `learners=[]`・`self_is_learner=false`で委譲、完全後方互換)。
  `replication_targets()`(voter+learner両方、複製送信対象)を追加。
  **発見・修正した実バグ**: `append_entries`(Leaderからの受信処理)が
  無条件に`s.role = RaftRole::Follower`へ遷移させていたため、
  learnerとして構築したノードもLeaderからの最初のAppendEntries受信を
  機にFollowerへ格上げされ、`driver.rs`のFollower/Candidate分岐
  (選挙タイムアウトでCandidate昇格)に巻き込まれてしまう——投票権を
  持たないはずのlearnerが実際には選挙に参加してしまう、という設計を
  壊す実バグになるところだった。`self_is_learner`のときはLearnerロール
  を維持するよう修正。
- `maybe_commit`(quorum判定)は`self.peers`(voter)のみを見る既存実装を
  そのまま維持——learnerを`replication_targets()`に含めても複製先が
  増えるだけでcommitの安全性には影響しない設計にした。
- `crates/aruaru-dist/src/raft/driver.rs`: `replicate()`が
  `node.peers()`ではなく`node.replication_targets()`(voter+learner)へ
  AppendEntriesを送るよう変更。
- `crates/aruaru-server/src/cluster.rs`: `build_cluster_with_learners`
  新設(既存`build_cluster`は後方互換の薄いラッパーへ)。
- `crates/aruaru-server/src/main.rs`: 新CLIフラグ`--raft-role`
  (`voter`/`learner`)・`--learner-peers`を追加。
  例: `aruaru-server --raft-id 1 --raft-role voter --learner-peers
  "2@127.0.0.1:6002"`(Leader側)、`aruaru-server --raft-id 2
  --raft-role learner --peers "1@127.0.0.1:6001"`(learner側)を
  **実際に2つの別プロセス**として起動できる。
- テスト: `raft/node.rs`に
  `test_learner_role_preserved_across_append_entries_and_applies_
  committed_entries`(上記バグの回帰テスト)・
  `test_leader_quorum_excludes_learners_but_still_replicates_to_them`
  (learnerの複製がquorumに数えられないことの直接検証)を追加。

**(2) ephemeral SQL pod化(プロセスレベルの使い捨て計算単位)**

- `crates/aruaru-server/src/ephemeral_pod.rs`(新規)。親プロセスが
  対象テナントのテーブルスナップショットをJSON化し、
  `tokio::process::Command`で**自分自身の実行ファイル**を
  `--ephemeral-worker`フラグ付きで子プロセスとして起動、標準入力で
  リクエストを渡し、子プロセスは完全に独立したインメモリ
  `QueryEngine`(永続ストレージ・Raft・pgwire・GraphQLは一切起動しない)
  でテーブルを再現しSQLを1回実行、結果を標準出力へJSONで書いて
  **即座に終了する**。`main.rs`に`--ephemeral-worker`内部フラグを追加し、
  設定時はこのワーカーモードのみ実行して即returnする。
  `crates/aruaru-server/src/admin.rs`に`POST /admin/ephemeral-query`
  (`{tenant_id, tables: [テーブル名], sql}`)を新設、実際に子プロセスを
  起動して結果を返す。
- **正直な開示・スコープの限界**(`ephemeral_pod.rs`冒頭docコメントにも
  記載): (a) cgroup/Job Object等によるCPU/メモリ/帯域の真のリソース
  制限は実装していない——「独立したOSプロセスとして起動・終了する」
  というプロセス分離そのものの実証に留まる。(b) 永続ストレージ(fjall)
  には触れない設計(子プロセスが親と同じデータディレクトリを同時に
  開くとファイルロック競合のリスクがあるため、意図的にJSON経由の
  インメモリスナップショットのみを渡す)——書き込みは子プロセスの
  メモリ上でのみ完結し親の永続状態には反映されない、読み取り専用の
  テナント別計算オフロードに限定される。(c) 複数物理マシンをまたぐ
  真のオーケストレーション(Kubernetes pod相当)はこの環境では検証
  不可能。

### 段階3: 実プロセス間での検証結果(実測、型チェックのみで終わらせない)

**(1) Raft learner — 2プロセスでの実ネットワーク複製検証**:
`aruaru-server.exe`を実際に2プロセス起動(leader: `--raft-id 1
--raft-role voter --learner-peers "2@127.0.0.1:6002"` port 6001/6011、
learner: `--raft-id 2 --raft-role learner --peers "1@127.0.0.1:6001"`
port 6002/6012)、leaderへ`POST /admin/cluster/propose`で
`CREATE TABLE items`+`INSERT`を実行後、learner側プロセスへ
`POST /admin/federation/query`(`local.SELECT * FROM items`、
learner自身のインメモリQueryEngineへの直接クエリ)で**実際に挿入した
行(`id=1, name=sword`)が読めることを確認**——TCP経由の実HTTPリクエスト
によるAppendEntries複製が、別プロセス・別ポートのlearnerへ実際に
届いていることの直接証拠。

検証の過程で**2件の実バグを発見・修正した**(いずれも「複数プロセスを
実際に起動して検証する」という今回のアプローチが無ければ発見できな
かったもの):
1. `HttpTransport`(`raft/transport.rs`)がAppendEntries/RequestVote送信時
   に`x-admin-token`ヘッダーを一切付与していなかった——2026-07-30に
   `/admin/*`全体へ認証を遡及適用した際のHANDOFFが「これらを実際に
   呼ぶノード間通信はまだ配線されていないため実害は無い」と正直に
   記していた通りの、まさにその「実際に呼ばれた初回」で顕在化した。
   環境変数`ARUARU_DB_ADMIN_TOKEN`を読み取り全リクエストへ付与する
   よう修正。
2. **さらに深刻な別バグ**: `HttpTransport`が`{base}/raft/append`
   (`/admin`プレフィックス無し)へ送信していたが、`main.rs`は
   `admin::admin_routes(..)`全体を`.nest("/admin", ..)`でマウントして
   いるため受信側の実パスは`/admin/raft/append`——**送信先パスが
   そもそも存在しない404**だった(1のトークン修正だけでは解決せず、
   `--log-level debug`で実際のエラーログ
   `"append_entries send failed", error: "HTTP status client error
   (404 Not Found)"`を確認して発見)。両方を修正して初めて複製が
   成功した。この2つのバグはいずれも`tracing::debug!`にしか記録
   されないため通常運用(info以上のログレベル)では気づけない、
   サイレントに複製が止まる実バグだった。

**(2) ephemeral SQL pod — 実際の子プロセス生成・終了の検証**:
親プロセス(`aruaru-server.exe`、port 6101)へ`CREATE TABLE gear`+
2行`INSERT`後、`POST /admin/ephemeral-query`
(`{tenant_id: "tenantA", tables: ["gear"], sql: "SELECT * FROM gear
WHERE qty = '10'"}`)を実行し、**フィルタ条件に合う1行だけが正しく
返る**ことを確認。同時に`Get-Process aruaru-server`のプロセス数を
呼び出し前後で比較し、**子プロセスが実際に起動して応答後に終了して
いる**(プロセス数が呼び出し前後で変化しない=常駐していない)ことを
確認した。

**実行した主要コマンド(すべて実行、生ログ・生JSON応答を確認)**:
`cargo build -p aruaru-server`(3回、各修正後に再ビルド)、
`cargo test --workspace`(全クレート、リグレッション無し確認)、
自作PowerShellスクリプト2本(2プロセスRaft learner検証・ephemeral pod
検証、いずれも一時データディレクトリで実行し検証後にプロセスを終了)。

### 段階4: なお残る制約(正直な開示、誇張しない)

1. **真の分散クラスタ(複数物理マシン)での検証は本環境では不可能**:
   今回の検証は同一マシン上の複数プロセス・複数ポートに留まる
   (`127.0.0.1`上の別ポート)。複数物理マシン・実ネットワーク越しの
   レイテンシ・パーティション耐性は未検証。
2. **openraft自体への統合は今回も行っていない**: `raft/mod.rs`が
   計画として記す「本番のリーダー選挙・ハートビート・スナップショット・
   ネットワークRPCをopenraftへ委譲する」は引き続き未着手——今回は
   既存の自前実装Raft(`RaftNode`/`RaftDriver`/`HttpTransport`)の枠内で
   learnerロールを実際に機能させた。
3. **learnerの動的追加・削除(ConfChange)は無い**: 現状は起動時の
   CLIフラグで役割・peer構成が固定される(TiKVの`AddLearner`/
   `PromoteLearner`のような実行時のメンバーシップ変更は未実装)。
4. **ephemeral SQL podは読み取り専用**: 子プロセスでの書き込みは
   子プロセスのメモリ上でのみ完結し、親プロセスの永続状態(fjall)には
   一切反映されない(意図的な制約、上記段階2参照)——真の
   「テナントごとに独立した書き込み可能なephemeral計算単位」には
   なっていない。
5. **リソース制限は無い**: 子プロセスのCPU/メモリ/帯域をOSレベルで
   制限する仕組み(cgroup、Windows Job Object)は実装していない。
6. **learner対応のRequestVote拒否は明示していない**: `request_vote`
   自体はlearnerでも技術的には呼べてしまう(driver.rs側がlearnerを
   選挙に参加させない設計のため実害は無いが、`RaftNode::request_vote`
   単体に「learnerは投票応答しない」という明示ガードは追加していない
   ——次回、防御的に追加する余地がある)。

### 次にすべきこと(次回候補)

(1) `RaftNode::request_vote`にlearner向けの明示ガードを追加する
(現状は`driver.rs`側の分岐だけに依存)、(2) learnerの動的追加・削除
(ConfChange相当)、(3) ephemeral SQL podに書き込みの永続反映
(fjallのファイルロック競合を避けるための設計、例えば書き込み結果を
親プロセスへ返しRaft経由でコミットする、等)を持たせる、(4) 実際に
openraftクレートへ統合する場合の移行計画の具体化。

## HANDOFF: 2026-08-21(続き) 分散DBの新技術4種を6言語で調査、FoundationDB型
決定的シミュレーションテスト(DST)を実装レベルで統合

前回HANDOFF(真のRaft learner化+ephemeral SQL pod化)で「AppendEntriesが
認証ヘッダ無し・誤ったパスで送られ複製がサイレントに失敗する」という実バグ
2件が`tracing::debug!`にしか出ず、実プロセスを2つ起動して初めて発見できた
という経緯を踏まえ、「複数プロセスを実際に起動する」以外の方法でこの種の
バグ(特にRaftのログ複製ロジック自体に潜む競合状態)を機械的・網羅的に
検出する手段を調査した。

### 調査した4技術(実際にWebSearchツールを日本語含む6言語で呼び出し、
実在するURLを確認済み)

1. **FoundationDBの決定的シミュレーションテスト(DST)** — 採用・実装済み(詳細下記)。
   - 英語(一次情報): https://www.foundationdb.org/files/fdb-paper.pdf 、
     https://apple.github.io/foundationdb/testing.html 、
     https://antithesis.com/docs/resources/deterministic_simulation_testing/ 、
     https://www.amplifypartners.com/blog-posts/a-dst-primer-for-unit-test-maxxers 、
     https://www.polarsignals.com/blog/posts/2024/05/28/mostly-dst-in-go 、
     FOSDEM 2026「Random seeds and state machines: An approach to deterministic
     simulation testing in Rust」https://fosdem.org/2026/schedule/event/GNTZDT-rust-deterministic-simulation-testing/
     (Rustでの実装事例——本実装の設計判断の参考にした)
   - フランス語: https://pierrezemb.fr/posts/diving-into-foundationdb-simulation/ 、
     https://pierrezemb.fr/posts/learn-about-dst/
   - 中国語: https://zhuanlan.zhihu.com/p/375321579 、
     https://developer.aliyun.com/article/789474
   - 韓国語: https://moonsub-kim.github.io/docs/distributed-systems/foundationdb/
   - GitHub実在確認: https://github.com/apple/foundationdb (Apple公式、
     Flow言語〈C++拡張〉によるDSTフレームワーク`flow/sim2`が実装として存在。
     "一晩に数万回のシミュレーション、延べ約1兆CPU時間相当"という規模の
     記述も一次資料〈上記論文・testing.html〉で確認)。
   - **aruaru-dbへの適合度**: 高い。`RaftNode`(`raft/node.rs`)の
     `propose`/`append_entries`/`request_vote`/`maybe_commit`は元々
     同期的な純粋関数に近い設計(HTTPトランスポートやtokioランタイムに
     依存しない)であり、これは意図してそう設計されたものではなかったが、
     結果としてFoundationDB本家のように「本番コードをシミュレータに
     差し替える」大改造無しに、既存のAPIをそのまま単一スレッドの決定的
     イベントループから直接呼び出すだけでDSTが書けた。

2. **ScyllaDB / Seastarのshard-per-core(shared-nothingアーキテクチャ)** —
   見送り(下記理由)。
   - 英語: https://www.scylladb.com/product/technology/shard-per-core-architecture/ 、
     https://seastar.io/shared-nothing/ 、
     https://www.scylladb.com/2024/10/21/why-scylladbs-shard-per-core-architecture-matters/
   - GitHub実在確認: https://github.com/scylladb/scylladb (C++実装、
     Apache Cassandra/DynamoDB互換、実運用実績あり)。155リポジトリを
     公開する組織アカウントも確認 (https://github.com/orgs/scylladb/repositories)。
   - **見送り理由**: CPUコアごとに独立したシャード(専用メモリ・専用I/O
     キュー、コア間はメッセージパッシングのみ)という設計思想は魅力的だが、
     aruaru-dbは既にRangeごとに独立したRaftグループ+CockroachDB型
     キー空間分離(前々回HANDOFF実装済み)でシャーディングの基本形は
     持っている。Seastar型のOS Thread per Core最適化を本気で取り込むには
     `tokio`ランタイム全体をSeastarのような専用イベントループへ置き換える
     規模の書き換えが必要で、既存の`tokio::main`+`parking_lot`前提の
     コードベース(Cargo.toml参照)全体への影響が大きすぎる。費用対効果が
     見合わないため見送り、記録だけ残す。

3. **Vitess/PlanetScaleのVSchemaベース水平シャーディング** — 見送り。
   - 中国語: 検索クエリ「Vitess PlanetScale 分片架构 vshard 原理」で
     中国語一次資料は得られず、英語資料 https://planetscale.com/docs/vitess/sharding 、
     https://planetscale.com/learn/courses/vitess/horizontal-sharding
     に帰着(検索言語を変えても情報源自体は英語という実態を正直に記録)。
   - **見送り理由**: VitessはMySQLプロトコル互換層の手前にVTGateという
     クエリルーティング層を置く設計で、「既存のMySQLクラスタ群をシャーディング
     する」ユースケースに最適化されている。aruaru-dbは自前のRaft+QueryEngineを
     持つため、VTGate相当の機能は既にCockroachDB型キー空間分離で代替済みで
     あり、Vitessの設計をそのまま輸入する新規性・必要性が薄いと判断。

4. **DuckDBの組み込み型ベクトル化OLAPエンジン** — 見送り(ただし将来候補として記録)。
   - ドイツ語: https://duckdb.org/why_duckdb 、
     https://motherduck.com/duckdb-book-summary-chapter1/ の内容をドイツ語検索
     経由で確認(MonetDB/X100由来のベクトル化実行エンジン、列指向、
     プロセス内実行〈in-process〉)。
   - GitHub実在確認: DuckDBは公知の広く使われるOSS(スター数十万規模、
     ここでは個別に再確認まではしていない——正直な簡略化点)。
   - **見送り理由**: aruaru-dbは既にTiFlash型プッシュ型購読による別系統の
     OLAPキャッシュ(`aruaru-query::olap`、前々回HANDOFF実装済み・
     `olap_cache_incremental_merge_handles_update_delete_and_insert_correctly`
     等のテストで担保)を持っており、DuckDB本体への依存を追加するより
     既存のインクリメンタルマージ機構を伸ばす方が一貫性が高い。DuckDBの
     ベクトル化実行そのもの(SIMDバッチ処理によるCPUキャッシュ効率化)は
     `aruaru-query::olap`の内部実装を高速化する際の参考として次回以降に
     再検討する価値はあるため、完全な却下ではなく保留として記録する。

### 実装した内容: `crates/aruaru-dist/src/raft/sim.rs`(新規、約340行)

- `RaftNode`の`append_entries`/`request_vote`/`propose`/`maybe_commit`を
  直接呼び出し、外部依存クレート無しの自前xorshift64乱数(`SimRng`、
  シード値のみで完全再現可能)で駆動する単一スレッドの決定的イベントループ
  (`run_simulation`)を実装。
- フォールト注入(`FaultConfig`): メッセージ欠落(`drop_rate`)・重複配送
  (`duplicate_rate`)・遅延による順序入替(`max_delay_ticks`、`BinaryHeap`
  優先度キューで表現)。
- 安全性検証: commit済み範囲でLeaderとFollowerの`(index, term)`が完全一致するか
  (Log Matching Property)を検証。違反時は再現用のseed値をpanicメッセージに含める。
  そのために`RaftNode`へ`term_at_index`(`ReplicatedLog::term_at`への薄い委譲)を
  1メソッド追加(`crates/aruaru-dist/src/raft/node.rs`)。
- テスト4件、うち中核は`test_sim_chaotic_many_seeds_never_violates_log_matching`
  (drop_rate=0.2・duplicate_rate=0.1・max_delay_ticks=8で**200シード**を反復実行)
  と`test_sim_extreme_drop_rate_still_safe`(drop_rate=0.9という極端値で50シード)。

### 実装時に遭遇した実バグ1件(このシミュレーション自体が最初の実効果)

初版では`leader_commit`をコマンド提案時点(Leaderの`commit_index`がまだ0の
瞬間)にAppendEntriesへ埋め込んで一度きり送っていたため、
`test_sim_no_faults_converges`(フォールト無しの基準シナリオ)が
`commit_indices: [20, 0, 0, 0]`(Leaderだけ20、Follower全員0)で失敗した。
実際のRaftでは`leader_commit`は複製完了後の**次のハートビート**に乗って
初めてFollowerへ伝わる——この伝播ステップが実装から抜けていたことを、
まさにこのシミュレーションテストが1回目の実行で検出した。修正として、
複製フェーズの後に5ラウンドのハートビート段階(空エントリ+更新済み
`leader_commit`をフォールト注入つきで再送)を追加。DSTを書く過程で
DST自体が想定通りバグを検出したことになる。

### 正直な簡略化点(誇張しない)

1. **対象はRaftNodeの状態遷移ロジックのみ**。実際の`HttpTransport`・
   実プロセス起動・tokioランタイムは一切介さない——前回HANDOFFで見つかった
   「認証ヘッダ無し」「パス誤り」のようなトランスポート層のバグはこの
   シミュレーションでは検出できない(役割分担が異なる、上記sim.rsの
   モジュールコメントにも明記)。
2. **リーダー選挙は対象外**。単一の固定Leaderで開始し、`RaftDriver`の
   選挙タイムアウト・Candidate昇格ロジックは検証していない。
3. **フォールトはメッセージレベルのみ**。ディスク故障・プロセスクラッシュ・
   クロックスキューの注入は無い(FoundationDB本家のBUGGIFYはこれらも含む)。
4. **反復シード数は200/50に留めている**(FoundationDB本家の「一晩数万回」
   規模ではなく、CI実行時間を考慮した現実的な回数)。
5. learner関連の安全性はこのシミュレーションの対象外(既存の
   `node.rs`単体テストで別途担保)。

### 動作確認(実際に実行、結果を記録)

- `cargo build -p aruaru-dist --tests`: 成功。
- `cargo test -p aruaru-dist raft::sim`: 4件中1件が初回失敗
  (上記の`leader_commit`伝播バグ)→ 修正後、`test result: ok. 4 passed;
  0 failed`。
- `cargo test -p aruaru-dist`: `test result: ok. 36 passed; 0 failed;
  1 ignored`(既存35件+新規4件-重複分、リグレッション無し)。
- `cargo test --workspace`: 全クレートで`test result: ok`、失敗0件
  (既存テストへの影響無しを確認)。
- 実プロセスを起動した検証(前回HANDOFFのような複数プロセス起動)は
  **今回は行っていない**——本タスクの主眼が「実プロセス起動無しで
  同種のバグを検出する仕組み」の追加自体だったため、cargo
  build/testでの確認に留めた。git commitは段階的に行い、pushはしていない
  (ユーザーの明示的許可待ち)。

### 次にすべきこと(次回候補)

(1) フォールト種別へプロセスクラッシュ・再起動(learnerが再参加する
シナリオ)を追加、(2) リーダー選挙自体をシミュレーション対象に含める
(現状は固定Leaderのみ)、(3) 実行時間に余裕があるCI環境では反復シード数を
数千程度まで引き上げる、(4) DuckDBのベクトル化実行を`aruaru-query::olap`
の内部実装高速化の参考にする(見送りではなく保留、上記2-4節参照)。

## HANDOFF: 2026-08-21(続き2) 前回の見送り判断(ScyllaDB/Vitess/DuckDB)を
再検証、コストを理由にせず2件を実装レベルで統合

**経緯**: 直上のHANDOFF(同日)がScyllaDB shard-per-core・Vitessシャーディング・
DuckDB組み込みOLAPの3技術を「実装コストが高い」「既存機能と重複する」として
見送っていたことに対し、ユーザーから「本当にそこまで調査したのか」「本当に
完全に重複しているのか」「多少時間がかかる程度なら開発してほしい」という
強い指摘を受けた。実装コストの高さのみを理由にした見送りは行わず、実際に
ソースコードを再読・追加調査した上で、部分的にでも良いとこ取りできる要素を
洗い出し、2件(ScyllaDB・Vitess)は実装レベルで統合した。DuckDBのみ、コストで
はなくアーキテクチャ上の具体的な重複を示して見送りとした。

### 1. ScyllaDB shard-per-core — 「全体書き換えが必要」という前回理由は
過大なスコープの誤り、核心思想のみを部分適用して実装

前回理由(「tokioランタイム全体をSeastar型イベントループへ置き換える規模」)
を検証した結果、**この理由は「ScyllaDB全体の移植」という過大なスコープを
前提にしており、「shared-nothingの核心(データのコア単位分割+ロックレスな
メッセージパッシング通信)だけを切り出して部分適用する」という選択肢を
検討していなかった**と判明した。

`crates/aruaru-query/src/sharded_store.rs`(新規)に`ShardedRowStore<V>`を
実装: `shard_count`個の専用OSスレッドがそれぞれ独立した`HashMap`を排他的に
所有し(呼び出し元スレッドから直接アクセスする手段が構造的に存在しない)、
通信は`std::sync::mpsc`によるメッセージパッシングのみ(`RwLock`/`Mutex`で
データそのものを共有する既存`QueryEngine::tables`とは対照的)。キーから
シャードへの割り当ては`SHA-256(key) % shard_count`(ScyllaDBのtoken-aware
routingの簡略版、既存の`aruaru-core`ZFS互換チェックサムと同じSHA-256を
再利用)。テスト6件で(a)キーの決定的ルーティング、(b)実際の複数シャードへの
分散、(c)複数スレッドからの並行書き込みがロック無しで安全に成立すること
を直接検証。

**正直な開示**: (1) CPUピニング・専用I/Oスケジューラ(Seastarの核心である
「1コアに1スレッドを物理的に固定」)は実装していない——OSデフォルトの
スケジューラに委ねる。(2) `QueryEngine`の本番書き込み経路
(`parking_lot::RwLock<HashMap>`)は今回置き換えていない——Raft・Prolly Tree・
OLAPキャッシュがテーブル単位の単一HashMapを前提に設計されているため、
全面移行は影響範囲が広く今回のスコープ外(独立コンポーネントとして追加、
既存の`snapshot_pairing`/`raid_z_backend`と同じ段階的アプローチ)。

### 2. Vitess/PlanetScaleシャーディング — 「VTGate相当は既に代替済み」
という前回理由は不正確、実際には無い要素(Range併合・scatter-gather)が
あったため実装

前回理由を検証するため`crates/aruaru-dist/src/shard/topology.rs`・
`multi_raft.rs`を再読した結果、**「既に代替済み」という前回の評価は
不正確だった**——`ClusterTopology`は`split_range`(Range分割)は実装済み
だったが、Vitessの[Reshard](https://vitess.io/docs/reference/vreplication/reshard/)
が持つ双方向操作のうち**併合(複数シャードを1つへ戻す)は一件も実装されて
いなかった**。また、CockroachDB型のポイントルーティング(`propose`、キーが
分かっている場合)はあったが、Vitess VTGateの核心機能である
**scatter-gather(シャーディングキー不明なクエリを全シャードへ展開し結果を
集約する)も未実装**だった。この2点を実装した:

- `ClusterTopology::merge_ranges(range_a, range_b)`(`shard/topology.rs`):
  隣接する2つのRangeを1つへ統合(隣接性チェック付き、飛び地の統合は拒否)。
  レプリカ集合は和集合、`range_id`は小さい方を引き継ぐ。テスト3件
  (併合が分割を正しく逆転させる、非隣接Range併合の拒否、存在しないRangeの
  安全な処理)。
- `MultiRaftCluster::merge`(`multi_raft.rs`): トポロジ統合に加え、消えた
  側のRaftグループを`groups`から除去。
- `MultiRaftCluster::scatter_gather<T, F>`(`multi_raft.rs`): 全Range
  (全Raftグループ)へ同じ読み取りクロージャを適用し、range_id順に結果を
  集約する——VTGateの「シャーディングキー不明なクエリを全シャードへ展開し
  マージする」と同じ形。テスト2件(併合後のキー空間再統合、3Rangeからの
  scatter-gather集約)。

**正直な開示**: (1) `merge`はトポロジ構造の統合(キー空間ルーティングが
1本化されること)のみを扱い、消えた側のRaftグループが保持していたログ・
状態機械の内容を統合先へマージする処理は行わない(`split`が新グループを
空ログから始める簡略化と対になる簡略化)。(2) `scatter_gather`は読み取り
専用の集約であり、書き込み(合意を伴う`propose`)はこの関数の対象外。
(3) `aruaru-server`の実運用経路(pgwire/GraphQL/REST admin API)からの
呼び出し配線は今回未実施——`multi_raft`モジュール自体が既存HANDOFFの
時点で「疎結合コンポーネントとして実装、呼び出し元への配線は次段階」と
されており、今回もその段階に留まる。

### 3. DuckDB組み込みOLAPエンジン — 見送りを維持するが、理由を「コスト」
から「アーキテクチャ上の具体的重複」へ差し替え

前回の見送り理由(「既存のTiFlash型OLAPキャッシュと重複」)は正しい方向
だったが根拠が薄かったため、`crates/aruaru-query/src/olap.rs`を実際に
読み直し、具体的な重複箇所を特定した:

- DuckDBの性能の核心は[MonetDB/X100由来のベクトル化実行エンジン]
  (列指向データをバッチ〈ベクトル〉単位でCPUキャッシュ効率よく処理する)
  にある。
- `aruaru-query/src/olap.rs`は**既にApache DataFusionを使っており、
  DataFusion自体がArrow(列指向メモリフォーマット)ベースの同系統の
  ベクトル化実行エンジン**である——`build_table_batch`が`Int64Array`/
  `Float64Array`等のArrow列配列を構築し(28〜73行目)、
  `rebuild_incremental`が`arrow::compute::filter_record_batch`という
  列指向の軽量フィルタカーネル(文字列パース不要のバッチ処理、
  240〜248行目)を使い、`session_context`が`target_partitions`で
  マルチコア並列実行を構成する(75〜82行目)——これらはDuckDBが
  MonetDB/X100から受け継ぐ設計そのもの(列指向・バッチ処理・
  SIMDフレンドリー)と**同一系統**である。
- したがって見送り理由は「実装コストが高い」ではなく、**「DuckDBを
  追加導入しても、既存のDataFusion統合が既に提供している性能特性
  (ベクトル化列指向実行)と機能的に重複するAPI・実行エンジンをもう1つ
  抱えることになるだけで、新規の実行方式上の能力を追加しない」**という、
  コードで裏付けられた具体的なアーキテクチャ上の理由に差し替える。
  (DuckDBの「組み込み・依存ゼロで動く」という別の強みはあるが、
  `aruaru-server`は既にDataFusionを組み込み依存として持つプロセス
  内蔵型サーバーであり、この強みも新規性を生まない。)

### 検証結果(実測)

- `cargo test -p aruaru-dist multi_raft` → 5 passed / 0 failed
  (新規`merge_reunifies_ranges_and_removes_the_absorbed_raft_group`・
  `scatter_gather_collects_a_reading_from_every_range_like_vitess_vtgate`
  含む)。
- `cargo test -p aruaru-dist shard` → 9 passed / 0 failed(新規
  `test_merge_reverses_split_like_vitess_reshard`・
  `test_merge_rejects_non_adjacent_ranges`・
  `test_merge_unknown_range_returns_none`含む)。
- `cargo test -p aruaru-query sharded_store` → 6 passed / 0 failed。
- `cargo build --workspace` → 成功(既存の`build_cluster`/`propose_commit`
  未使用警告2件のみ、いずれも今回の変更と無関係な既知の警告)。
- `cargo test --workspace` → 全19クレート/テストバイナリで
  `test result: ok`、失敗0件(既存テストへの回帰無し)。

### 正直な開示・今回も残る未着手事項

(1) `ShardedRowStore`・`merge`/`scatter_gather`とも`aruaru-server`の
実運用経路への配線は未実施(独立コンポーネントとしての実装段階)、
(2) ScyllaDBのCPUピニング・Vitessのログマージを伴う真の併合は見送り
(上記の各節参照、理由はコストではなく「単一プロセス前提の現行実装が
ネットワーク越し複製・OSレベルCPU制御の前提を持たない」という構造的
制約)、(3) DuckDBは今回もコード追加なし(見送り理由をアーキテクチャ上の
重複へ差し替えたのみ)。

### 次にすべきこと(次回候補)

(1) `ShardedRowStore`を`aruaru-server`の実際のテーブルストレージ経路へ
段階的に配線する場合の設計(Raft/Prolly Tree/OLAPキャッシュとの整合性を
どう保つか)、(2) `MultiRaftCluster::merge`にログ内容のマージ(消える側の
未適用エントリを統合先へ引き継ぐ)を追加する場合の設計、(3)
`scatter_gather`を実際のクロスシャードSQL(`aruaru-query::QueryEngine`)
から呼び出せるようにする配線。

## HANDOFF: 2026-08-21(続き3) ScyllaDB `ShardedRowStore`とVitess
`merge`/`scatter_gather`を`aruaru-server`本体の運用経路へ実配線、
実プロセスでHTTP動作確認

**経緯**: 直上のHANDOFF(同日)が両者とも「独立コンポーネントとしての実装
段階」に留めていたことに対し、ユーザーから「実際にaruaru-server本体の
運用経路へ配線し、実プロセスを起動してHTTP経由で動作確認せよ」との指示。
既存機能(pgwire/GraphQL/REST経由のOLTP書き込み経路)を壊さないことを
優先し、両者とも**オプトインの独立エンドポイント**として`/admin/*`配下に
追加する方式を選んだ(置き換えではなく追加)。

### 1. ScyllaDB `ShardedRowStore` の配線

`crates/aruaru-server/src/admin.rs`の`AdminState`に
`sharded_store: aruaru_query::sharded_store::ShardedRowStore<String>`
フィールドを追加(`AdminState::new`で`ShardedRowStore::new(0)`により
論理コア数ぶんのシャードスレッドを起動)。新規エンドポイント3つ:
`POST /admin/sharded-store`(put)・`GET /admin/sharded-store/:key`
(get)・`GET /admin/sharded-store-stats`(シャードごとのエントリ数)。
ハンドラは`tokio::task::spawn_blocking`で`std::sync::mpsc`の
ブロッキング`recv()`をtokioワーカースレッドから退避している(シャード
スレッドとの通信がブロッキングI/Oである設計上の性質を、非同期HTTP
ハンドラ側で正しく扱うための配慮)。

**既存ストレージ(`QueryEngine::tables`)を置き換えるか追加するかの判断**:
置き換えは選ばなかった——Raft・Prolly Tree・OLAPキャッシュ全てが
テーブル単位の単一`HashMap`を前提に設計されており、影響範囲が広い
(前回HANDOFFで既に正直に開示済みの制約)。オプトインの独立ストレージ
(`/admin/sharded-store`)として追加する方式が「既存機能を壊さない」
というユーザー指示に最も合致すると判断した。

### 2. Vitess `merge`/`scatter_gather` の配線

`AdminState`に`multi_raft: Mutex<Option<Arc<MultiRaftCluster
<EngineApplier>>>>`を追加。`main.rs`起動時、既存の単一`ClusterNode`
(本番のOLTP書き込み経路)とは別に、`MultiRaftCluster::single_node`で
単一ノード構成のMulti-Raftクラスタを初期化し`admin_state.attach_
multi_raft(..)`で取り付ける。新規エンドポイント3つ:
`POST /admin/multi-raft/split`(Range分割)・`POST /admin/multi-raft/merge`
(Vitess Reshard併合)・`GET /admin/multi-raft/scatter-query`
(VTGate scatter-gather、全Rangeのcommit_index+roleをrange_id順に集約)。
`aruaru_dist::lib.rs`に`MultiRaftCluster`のトップレベル再エクスポートを
追加(従来`multi_raft`モジュール内のみで、クレート外から`aruaru_dist::
MultiRaftCluster`として参照できなかった)。

### 3. 実プロセスでのHTTP動作確認(実測、型チェック・ビルド成功では終わらせない)

実際に`aruaru-server.exe`(`cargo build -p aruaru-server`で生成した
実バイナリ)を`--raft-id 1 --gql-port 7301`で起動し、
`ARUARU_DB_ADMIN_TOKEN`を設定した上で以下を全て実HTTPリクエストで確認:

1. `GET /healthz` → `ok`(起動確認)。
2. `POST /admin/sharded-store`を3回(`alpha`/`beta`/`gamma`)実行 →
   それぞれ`shard_id`が30/9/0という異なるシャードへ実際に振り分けられた
   ことを確認(SHA-256ベースのtoken-aware routingが実際に機能している
   直接証拠)。
3. `GET /admin/sharded-store/alpha`・`/beta` → 書き込んだ値
   (`apple`/`banana`)が正しく読み戻せることを確認。
4. `GET /admin/sharded-store-stats` → `shard_count: 32`
   (このマシンの論理コア数)、`per_shard_len`の該当インデックスのみ`1`、
   `total_len: 3`——分散状況を実際に観測できることを確認。
5. `POST /admin/multi-raft/split`を2回(range 1を`m`で分割→range 2、
   range 2を`t`で分割→range 3)実行 → `range_count`が1→2→3と実際に
   増加することを確認。
6. `GET /admin/multi-raft/scatter-query`(分割直後) → 3つの独立した
   Rangeそれぞれの`commit_index`/`role`が実際に返ることを確認
   (`range_id: 1,2,3`全て`Leader`・`commit_index: 0`)。
7. `POST /admin/multi-raft/merge`(range 1と2を併合) →
   `merged_range_id: 1`・`range_count: 2`を確認。
8. `GET /admin/multi-raft/scatter-query`(併合後) → range 1と3の
   **2件**のみが返り、統合が実際にscatter-gatherの結果へ反映される
   ことを確認。
9. 存在しないrange_idでの併合(`range_a:1, range_b:999`) →
   `success: false`で安全にエラーメッセージが返ることを確認
   (パニックしない)。
10. `x-admin-token`ヘッダー無しで`GET /admin/multi-raft/scatter-query`
    を呼ぶ → **401**(既存の全`/admin/*`共通認証ミドルウェアが新規
    エンドポイントにも自動的に適用されていることを確認——`admin_routes()`
    の`.around()`ラッパー内にルートを追加しただけで個別の認証実装は
    不要だった)。

検証後、起動していたプロセスを`Stop-Process`で終了し、標準エラー
ログ(`err.log`)にエラー・パニックが一切無いことを確認した上で
一時検証用データディレクトリを削除した(リポジトリへの影響なし)。

### 検証結果(実測、コマンド・出力込み)

- `cargo build -p aruaru-server` → 成功(既存の`build_cluster`/
  `propose_commit`未使用警告2件のみ、いずれも今回の変更と無関係)。
- `cargo test -p aruaru-server -p aruaru-dist -p aruaru-query` →
  全green(aruaru-dist 41 passed/1 ignored、aruaru-query 52 passed、
  aruaru-server 3 passed/1 ignored)、リグレッション無し。
- `cargo build --workspace` → 成功。
- `cargo test --workspace` → 全19テストバイナリで`test result: ok`、
  失敗0件。
- 上記の実HTTP検証10項目すべて実施・成功。

### 配線中に見つけた不整合・バグ

**無し**。実装・実HTTP検証を通じて新規のバグは発見しなかった
(既存のRaftNode/MultiRaftCluster/ShardedRowStoreのロジック自体は
前回HANDOFFで既にテスト済みだったため、今回は「配線」作業が主体)。

### 正直な開示・残る制約

1. **`ShardedRowStore`は依然として独立ストレージ**——`QueryEngine`の
   本番テーブルデータとは無関係(キー・値ともに任意の文字列を保持する
   汎用KVとしての公開に留まる)。テーブルデータそのものをシャード分割
   したいという要求には、Raft/Prolly Tree/OLAPキャッシュ全体の再設計が
   必要で、今回もそこには踏み込んでいない。
2. **`MultiRaftCluster`は`ClusterTopology`の構造操作+読み取り集約のみ
   を実配線した**——`multi-raft/split`/`multi-raft/merge`は実際に
   独立したRaftグループの生成・除去を行うが、これらのRangeへの実際の
   書き込み(`propose`)・複数物理ノードへのネットワーク複製は今回の
   配線対象外(既存の`propose_write`/pgwire経路は引き続き単一の
   `ClusterNode`を使う——2つのRaft関連コンポーネントが並行して存在する
   状態のまま)。
3. **`ShardedRowStore`・`MultiRaftCluster`とも認証(`x-admin-token`)は
   `/admin/*`共通ミドルウェア経由で自動的に適用されるが、複数ノード
   クラスタでの`multi-raft/*`操作の整合性(全ノードでトポロジが同期
   されるか)は単一ノード構成でしか検証していない**。

### 次にすべきこと(次回候補)

(1) `MultiRaftCluster`のRangeへ実際に書き込み(`propose`)できる
管理APIエンドポイントの追加、(2) `ShardedRowStore`をテナント別
キャッシュ層など、既存のOLTP経路と衝突しない具体的な用途へ本格接続する、
(3) 複数ノード構成での`multi-raft/*`操作の実地検証。

## HANDOFF: 2026-08-21(続き4) DuckDB見送り判断の深掘り再調査(8言語)+
「埋もれた技術」探索、辞書エンコーディング+ゾーンマップを実装

**経緯**: 直上のHANDOFF(同日)がDuckDBの見送り理由を「aruaru-query::olap
が既にDataFusion(同系統のベクトル化列指向エンジン)を統合済み」とした
ことに対し、ユーザーから「本当に十分な調査か、世界中の言語で調査をやり
直し、埋もれた最先端技術を見逃していないか」との指示。日英に加え中・独・
仏・韓・西・露の**8言語**で実際にWebSearchを複数回実行し、DuckDBと
DataFusionの詳細な設計差分・他の埋もれた技術を再調査した。

### 調査結果(言語ごと、出典付き)

- **英語**: DuckDBのストレージ層固有技術として、(1) **ゾーンマップ
  (min/maxブロック統計によるRow Groupスキップ)**、(2) **辞書エンコー
  ディング**(重複文字列を辞書へ集約)、(3) 定数エンコード・RLE・
  ビットパッキング・FSST・Chimp/Patasを含む**型認識軽量圧縮**、(4)
  ハッシュ結合・GROUP BY・ソート・ウィンドウ関数全てに対応した
  **out-of-coreスピル**(バッファマネージャが全メモリを統括管理)、
  を確認
  ([Lightweight Compression in DuckDB](https://duckdb.org/2022/10/28/lightweight-compression)、
  [DuckDB in Depth](https://endjin.com/blog/duckdb-in-depth-how-it-works-what-makes-it-fast)、
  [TaDa-04 slides](https://blobs.duckdb.org/slides/TaDa-04.pdf)、
  [Memory Management in DuckDB](https://duckdb.org/2024/07/09/memory-management)、
  [Storage dictionary compression PR #3109](https://github.com/duckdb/duckdb/pull/3109))。
  **これらは前回HANDOFFが見落としていた、DataFusionのベクトル化実行
  エンジンとは別軸のDuckDB固有技術要素**——前回の「同系統のエンジンだから
  重複」という判断は、実行モデル(ベクトル化)は同系統でも、**ストレージ
  フォーマット固有の最適化(ゾーンマップ・辞書エンコード)は別軸**であり
  aruaru-query側に実装が無かった、という点を見落としていたことが判明した。
- **中国語(簡体字)**: PolarDBの新ベクトル化実行エンジンがコア演算子性能
  40〜400%向上、ソートスループット23倍という2026年の実例、Apache Doris
  4.1.0のIVF/IVF_ON_DISKベクトルインデックス(INT8/INT4/PQ量子化)による
  ベクトル検索性能4倍向上等を確認したが、これらはベクトル検索・大規模
  実行エンジン最適化であり、aruaru-dbの現在の規模には過大
  ([半年度盤点2026上半年版](https://zhuanlan.zhihu.com/p/2063922209849193484))。
- **ロシア語**: Tantor Labsの「Tantor XData」がHTAP(トランザクション+
  分析同時処理)の第3世代DBマシンとして発表されたことを確認、Tarantool
  Column Store(TCS)がB-tree(OLTP)+ビットマップインデックス(OLAP)の
  ハイブリッドインデックスでHTAPを実現していることも確認したが、
  いずれもaruaru-dbが既に持つHTAP路線(Multi-Raft+OlapCache)と同方向で
  新規性は薄い
  ([TAdviser 2026レビュー](https://www.tadviser.ru/index.php)、
  [Tarantool HTAP解説](https://tarantool.io/blog/kak-primenyat-arhitekturu-htap-v-biznese-i-real-time-analitike/))。
- **ドイツ語**: ゾーンマップ自体のドイツ語一次資料は見当たらず(検索結果は
  一般的な列指向DB解説のみ)、ビットマップインデックス+ソートの組み合わせ
  でRLE圧縮率が桁違いに向上するという一般論を確認
  ([spaltenorientierte Datenbank解説](https://de.wikipedia.org/wiki/Spaltenorientierte_Datenbank))。
- **フランス語**: DuckDBが列ごとにセグメント化してメモリ格納し、必要な
  列・行のみ読むことで計算高速化・メモリ削減を実現するという一般的な
  アーキテクチャ解説を確認したが、ゾーンマップ/辞書エンコードそのものに
  言及する独自のフランス語資料は見当たらなかった(正直な開示)
  ([next-decision.fr DuckDB解説](https://www.next-decision.fr/wiki/creation-dun-data-lakehouse-avec-duckdb-et-dbt))。
- **韓国語**: 列指向格納がキャッシュ効率・圧縮率を高めるという一般論
  ([IvoryRabbit DuckDBブログ](https://ivoryrabbit.github.io/posts/DuckDB/))
  を確認したが、「ゾーンマップ」「辞書エンコーディング」という具体的な
  用語に言及する韓国語資料は見当たらなかった(正直な開示)。
- **スペイン語**: 「compresión adaptativa」「índice zonal」という
  クエリでは、ベクトルDB(Pinecone/ChromaDB/Weaviate等)の2026年ランキング
  記事が中心に返り、目的の技術(ゾーンマップ)に特化したスペイン語資料は
  見当たらなかった(正直な開示、検索結果からベクトルDBという別分野の
  情報が優勢だったことを記録)。

**「埋もれた最先端技術」の探索結果**: 8言語での再調査を通じて、DuckDB以外
に**aruaru-dbへ新規に取り込む価値のある、実在し実装確認できる技術**は
見つからなかった(PolarDBの新ベクトル化エンジン・Apache Dorisのベクトル
検索量子化は実在するが、いずれも本リポジトリの現在の規模・用途からは
過大、または既存のOLTP中心設計と方向性が異なると判断)。**唯一の実質的な
発見は、DuckDB自体の中でも「ベクトル化実行」ではなく「ストレージ層固有の
最適化(ゾーンマップ・辞書エンコード)」という、前回見落としていた別軸の
技術要素だった。**

### 判断: DuckDB本体の見送りは維持するが、ストレージ層固有技術2点は
コストを理由にせず実装

DuckDB本体(独立した実行エンジン・ストレージフォーマットの全面採用)は
引き続き見送る——理由は「DataFusionと重複するから」ではなく、
**「DuckDBが持つストレージ層固有の最適化(ゾーンマップ・辞書エンコード)
は、DuckDB本体を導入しなくても、aruaru-query側で直接実装できる独立した
技術要素だから」**へ再度差し替える(前回の「重複」という評価も、
今回の8言語再調査でより正確に「実行モデルは重複するがストレージ最適化は
別軸で未実装だった」と訂正)。この2点はコストを理由に見送らず実装した。

### 実装した内容(`crates/aruaru-query/src/olap.rs`)

1. **辞書エンコーディング**: `arrow_type`のText/デフォルト分岐を
   `DataType::Utf8`(生のStringArray)から`DataType::Dictionary(Int32, Utf8)`
   へ変更。`build_array`で`StringDictionaryBuilder<Int32Type>`を使い、
   重複する文字列値を辞書へ1回だけ格納するよう構築。
2. **ゾーンマップ**: `TableCache`に`zone_maps: HashMap<String, (f64, f64)>`
   を新設、`compute_zone_maps`(Arrow標準の`compute::min`/`compute::max`
   カーネルを使用)で数値列(Int64/Float64)ごとのmin/maxを計算し
   `rebuild_full`/`rebuild_incremental`の両方で更新。`OlapCache::query`に
   `extract_simple_range_predicate`(`SELECT ... FROM t WHERE col > N`
   という最も単純な範囲述語だけを緩く抽出する正規表現ベースの抽出器、
   GROUP BY/JOIN/OR句を含む場合は必ずマッチしない=安全側)と
   `zone_map_disproves`(その範囲に該当行が絶対に無いと証明できるかの
   判定)を追加。証明できる場合はDataFusionへ一切クエリを投げず即座に
   空の結果を返す——DuckDBのRow Groupスキップと同じ「偽陽性は絶対に
   起こさず、証明できない場合は常に安全側で通常実行する」設計。

### 正直な簡略化点(誇張しない)

1. **ゾーンマップの粒度はテーブル全体で1つ**——DuckDBはRow Group
   (物理ブロック)単位で複数の統計区間を持ちブロック単位の部分スキップが
   できるが、本実装は「テーブル全体が対象外」と証明できる場合のみ
   スキップする、最も粗い粒度。
2. **`extract_simple_range_predicate`は正規表現ベースの簡易抽出**であり、
   完全なSQL式パーサではない——`col > N`/`col >= N`/`col < N`/`col <= N`
   という最も単純な単一条件のみ対応、複合WHERE(AND/OR)・カラム同士の
   比較・関数呼び出しを含む述語は対象外(マッチしなければ常に通常の
   DataFusion経路にフォールバックするため、正しさへの影響はない)。
3. **型認識軽量圧縮(RLE・ビットパッキング・FSST等)・out-of-core
   スピル**は今回も実装していない——DataFusion自体がストリーミング実行・
   パーティション並列を提供するため、aruaru-dbの現在の想定データ規模
   (単一プロセス内メモリ常駐)では優先度が低いと判断した。
4. **DuckDB本体の起動の軽さ・単一バイナリ配布**という別の強みは、
   `aruaru-server`が既にプロセス内蔵型サーバーであるため引き続き
   新規性を生まない(前回評価を維持)。

### 検証結果(実測)

- `cargo build -p aruaru-query --tests` → 成功。
- `cargo test -p aruaru-query` → **56 passed / 0 failed**(前回52件+
  新規4件: `text_columns_are_dictionary_encoded_and_still_aggregate_
  correctly`〈辞書エンコードの実型検証+集計結果の正しさ〉、
  `zone_map_prunes_queries_that_cannot_possibly_match_and_normal_
  queries_still_work`〈枝刈りが機能すること+通常クエリが壊れないこと〉、
  `extract_simple_range_predicate_only_matches_simple_range_queries`、
  `zone_map_disproves_boundary_conditions`〈境界値7パターン〉)。
- `cargo build --workspace` → 成功(既存警告2件のみ、無関係)。
- `cargo test --workspace` → 全19テストバイナリで`test result: ok`、
  失敗0件。
- **実プロセスでのHTTP動作確認**: `aruaru-server.exe`を実際に起動し、
  `POST /admin/federation/query`経由で`CREATE TABLE orders`→2行`INSERT`
  (`region`列に`east`/`west`)→`SELECT region, SUM(amount) AS total FROM
  orders GROUP BY region ORDER BY region`を実行、**辞書エンコードされた
  `region`列を含むGROUP BYクエリが実際に正しい結果
  (`{"region":"east","total":"100"}`/`{"region":"west","total":"999"}`)
  を返す**ことを確認(型チェック・単体テストのみでの完了報告ではない)。

### 次にすべきこと(次回候補)

(1) ゾーンマップをRow Group相当の複数統計区間へ細分化する、
(2) `extract_simple_range_predicate`をAND結合の複合述語(例:
`WHERE a > 10 AND b < 20`)にも対応させる、(3) 型認識軽量圧縮
(RLE/ビットパッキング)の要否は、実際のデータ規模がボトルネックになった
時点で再評価する。

## HANDOFF: 2026-08-21(続き5) ScyllaDB/Vitess/DuckDBの「実装方法そのもの」
を本家ソース・設計文書レベルで再調査、murmur3ルーティング+適応的辞書
エンコードを実装

**経緯**: 直上までのHANDOFFは「機能として何を実装するか」を調査していたが、
ユーザーから「本家(ScyllaDB/Vitess/DuckDB本体)の実際のソースコード・
設計判断・アルゴリズムの詳細」まで踏み込んで比較し、改善できる点を実装へ
反映するよう指示。英語・ドイツ語・中国語・韓国語・フランス語・スペイン語の
6言語でWebSearchを実行し、本家の実装方法を調査した。

### 調査結果と比較

1. **ScyllaDBのtoken-aware routing**: GitHub `scylladb/scylladb`
   Wikiの`Token`ページ・`scylladb.medium.com`のドライバ実装解説
   (ドイツ語・スペイン語検索経由でも同内容に到達)を確認した結果、
   ScyllaDBは**MurmurHash3**(`utils::murmur_hash::hash3_x64_128()`)を
   使っており、**SHA-256のような暗号学的ハッシュ関数は使っていない**と
   判明。前回実装(`crates/aruaru-query/src/sharded_store.rs`)は
   `SHA-256(key) % shard_count`だったが、SHA-256は衝突耐性・原像計算
   困難性のために意図的に低速に設計された関数であり、「高速さ」だけが
   要件のルーティング用途には不釣り合いに重い。**MurmurHash3(32bit版)
   の自前実装(`murmur3_32`、依存クレート追加なし)へ差し替えた**
   ([Token · scylladb/scylladb Wiki](https://github.com/scylladb/scylladb/wiki/Token)、
   [Making a Shard-Aware Python Driver for ScyllaDB](https://www.scylladb.com/2020/10/13/making-a-shard-aware-python-driver-for-scylla-part-1/))。
   なお、ScyllaDB本家はさらにトークン空間全体を`2^n`個(既定n=12)に
   分割し各片をシャード数`S`個に再分割するという二段階アルゴリズムを
   持つが、これは**Cassandraワイヤプロトコル互換のtoken空間表現を保つ
   ための固有要件**であり、本実装のような単純なポイントルックアップ用途
   (ソート済みレンジスキャン互換性を要求しない)には過剰と判断し移植
   していない(正直な開示、コードコメントに明記)。

2. **Vitess Reshardの実データ移動**: 公式ドキュメント`vitess.io`
   (フランス語検索経由でも同一の一次情報に到達)を確認した結果、実際の
   Reshard/MoveTablesは**VReplication**(MySQLレプリケーションを使い、
   VStreamで既存テーブル内容をコピーしつつ以降の変更を継続的にストリーム、
   中断時は再開可能、切替は原子的)という、データそのものをコピーする
   仕組みだと確認した。既存の`MultiRaftCluster::merge`はトポロジ構造の
   統合のみでログ内容のマージは行っていない——この制約は前々回HANDOFF
   で既に正直に開示済みであり、今回の調査でVitess本家との差分がより
   具体的に裏付けられた形。**コード変更は見送り**: 本実装は単一プロセス
   内の複数`RaftNode`インスタンスであり、VReplicationのような
   ネットワーク越しレプリケーションストリームに相当する仕組み自体が
   存在しないため、実データコピーの移植には`aruaru-dist`のRaft層全体の
   設計変更が必要——今回のスコープを大きく超える(次回以降の課題として
   記録を維持)。
   ([Vitess VReplication Overview](https://vitess.io/docs/archive/22.0/reference/vreplication/vreplication/)、
   [Vitess Reshard](https://vitess.io/docs/archive/13.0/reference/vreplication/reshard/))。

3. **CockroachDBのRange Merge**(Vitessの併合と比較する上での補助調査):
   公式tech-note `range-merges.md`を確認した結果、LHS(左側Range)が
   RHSを吸収するという設計は本実装の`merge_ranges`(range_idが小さい方=
   隣接順で左側を引き継ぐ)と方向性が一致していることを確認。ただし
   CockroachDBは複数物理ノードにまたがるレプリカのRaft経由ガベージ
   コレクション(吸収されなかったストアの孤立レプリカを`replica GC queue`
   が回収する)という、単一プロセス構成の本実装には存在しない概念を持つ
   ——これも構造的にコード変更の対象外
   ([cockroach/docs/tech-notes/range-merges.md](https://github.com/cockroachdb/cockroach/blob/master/docs/tech-notes/range-merges.md))。

4. **DuckDBの圧縮analyzeフェーズ**: 公式ブログ
   `duckdb.org/2022/10/28/lightweight-compression`・GitHub PR
   (`duckdb/duckdb` #9635 ALP圧縮等、韓国語検索でも同一の一次情報に
   帰着)を確認した結果、DuckDBは**セグメントごとに複数の圧縮方式
   (定数・RLE・辞書・FSST・ビットパッキング・ALP等)を試算し、最も
   小さくなるものを選ぶ「analyzeフェーズ」**を持つと判明。前回実装は
   Text列に**無条件で**辞書エンコードを適用していたが、これはDuckDBの
   「最良の方式を選ぶ」という核心を反映しておらず、高カーディナリティ
   列(UUID列等)では辞書化がむしろ不利(辞書自体が行数近くまで肥大化)
   になり得る。**修正**: `build_array`(`crates/aruaru-query/src/olap.rs`)
   に、実データのユニーク値比率(`unique_count / total_non_null_count`)
   が閾値(`DICTIONARY_CARDINALITY_THRESHOLD = 0.7`)未満のときのみ
   辞書エンコードし、そうでなければプレーンな`Utf8`(`StringArray`)を
   使う適応的選択を追加した。DuckDBの多数の圧縮アルゴリズム(RLE・FSST・
   ALP等)の完全な移植は行っていない(正直な簡略化点、コードコメントに
   明記)——「セグメントを見てから判断する」という核心的な設計思想のみを
   簡易版で再現した。

### 実装した内容

- `crates/aruaru-query/src/sharded_store.rs`: `shard_for`を
  `SHA-256(key) % shard_count`から自前実装の`murmur3_32`(MurmurHash3
  x86 32bit版、依存クレート追加なし)へ差し替え。`sha2`依存を
  `aruaru-query/Cargo.toml`から削除(他に使用箇所が無いことを確認済み)。
  既知のテストベクタ(`murmur3_32(b"", 0) == 0`)による回帰テストを追加。
- `crates/aruaru-query/src/olap.rs`: `build_array`のText/デフォルト分岐に
  「ユニーク値比率が閾値未満なら辞書エンコード、そうでなければプレーン
  Utf8」という適応的選択を追加。`arrow_type`関数を廃止し、`build_array`
  自体が実際に採用した`DataType`を返してそのままスキーマFieldへ使う設計
  (辞書化するか否かがデータ依存になったため、スキーマとデータの型不一致を
  構造的に起こせないようにする変更)。新規テスト2件
  (`high_cardinality_text_columns_skip_dictionary_encoding`で高
  カーディナリティ列がプレーンUtf8のままであることを確認、既存の
  `text_columns_are_dictionary_encoded_and_still_aggregate_correctly`は
  低カーディナリティ側の従来通りの検証を維持)。

### 検証結果(実測)

- `cargo build -p aruaru-query --tests` → 成功。
- `cargo test -p aruaru-query` → **58 passed / 0 failed**(前回56件+
  新規2件: `murmur3_32_matches_known_test_vector_for_empty_input`、
  `high_cardinality_text_columns_skip_dictionary_encoding`)。
- `cargo build --workspace` → 成功(既存警告2件のみ、無関係)。
- `cargo test --workspace` → 全19テストバイナリで`test result: ok`、
  失敗0件。
- **実プロセスでのHTTP動作確認**: `aruaru-server.exe`を実際に起動し、
  (1) `POST /admin/sharded-store`で書き込んだキーが`shard_id: 29`
  (murmur3ルーティング経由)へ実際に振り分けられ、`GET`で正しく
  読み戻せることを確認。(2) `POST /admin/federation/query`経由で
  `region`(低カーディナリティ、east/west)と`uuid`(高カーディナリティ、
  全行ユニーク)の両方を持つテーブルを作成・10行投入し、
  `GROUP BY region`(辞書エンコード列を含む集計)が正しい結果
  (`east:50, west:50`)を返し、`COUNT(*)`(高カーディナリティのuuid列が
  混在)も正しい結果(`10`)を返すことを確認。

### 見送った点とその理由(コスト起因ではなく、構造的な理由)

1. **ScyllaDBの二段階トークン分割(`2^n`分割+シャード再分割)**:
   Cassandraワイヤプロトコル互換のtoken空間表現を保つための固有設計で
   あり、本実装はそのような互換性要件を持たない単純なポイント
   ルックアップ用途のため、単純な剰余ルーティングのままハッシュ関数
   だけを本家と同じ思想(非暗号学的・高速)へ揃えた。
2. **VitessのVReplicationによる実データコピー**: 本実装
   (`MultiRaftCluster`)は単一プロセス内の複数`RaftNode`インスタンスで
   あり、ネットワーク越しレプリケーションストリームに相当する仕組み
   自体が構造的に存在しない——実データコピーの移植には`aruaru-dist`
   のRaft層全体の設計変更が必要で、今回のスコープを超える。
3. **DuckDBの多数の圧縮アルゴリズム(RLE・FSST・ALP・Chimp/Patas等)**:
   「セグメントを見て最良の方式を選ぶ」という核心思想はユニーク比率
   閾値で再現したが、個々のアルゴリズムの実装(FSST等)は文字列
   圧縮アルゴリズムの新規実装が必要な規模であり、今回は見送った——
   次回、実際のデータ規模がボトルネックになった時点で個別に検討する。

### 次にすべきこと(次回候補)

(1) `DICTIONARY_CARDINALITY_THRESHOLD`(現在0.7固定)を、実際の
ワークロードでのメモリ/速度計測に基づいてチューニングする、(2) FSST等の
文字列専用圧縮アルゴリズムの要否は実データ規模がボトルネックになった
時点で再評価、(3) Vitess VReplication相当の実データストリーミング
複製は、`aruaru-dist`のRaft層をopenraftベースの真のネットワーク越し
実装へ移行する際にあわせて設計する。

## HANDOFF: 2026-08-21(続き6) Snowflake×CockroachDB系ハイブリッドDBの再調査
→ closed timestamp / follower read(CockroachDB・TiKV safe-ts・YugabyteDB
方式)を新規実装。次回の調査目星も併記

**経緯**: ユーザーより「SnowflakeとCockroachDBの両方の特性を持つ特殊な
変種DBが世界中に存在する」という情報について再調査し、ギャップがあれば
実装するよう指示。作業途中でユーザーから範囲縮小の指示(日本語ドキュメント
のみ更新、詳細な調査本文は不要、次回の調査目星を残すだけで良い)を受けた
ため、既に完了していた実装分とその検証結果を反映し、残りは次回候補の
一覧として記録する形でまとめた。

### 調査で分かったこと(要点のみ)

- 「CockroachDBのRaft強整合 × Snowflakeのストレージ/コンピュート分離」を
  そのまま名乗る単一製品は見つからなかった(検索結果からの正直な報告)。
  ただし**両立を実際に成立させている共通の要素技術**として、
  「読み取りをリーダー(leaseholder)以外のレプリカへ逃がす仕組み」が
  CockroachDB / TiKV-TiDB / YugabyteDB のいずれにも存在することが判明:
  - CockroachDB: Range単位の **closed timestamp**(この時刻以下に新しい
    書き込みは今後現れないことの保証)を leaseholder が継続的に前進させ、
    Raftログまたは **side transport** で follower へ通知。follower は
    `read_ts <= closed_ts` の読み取りのみ自力で応答する。bounded
    staleness read は「上限付き陳腐化を受け入れて時刻を交渉する」方式
    ([follower reads RFC](https://github.com/cockroachdb/cockroach/blob/master/docs/RFCS/20181227_follower_reads_implementation.md)、
    [bounded staleness RFC](https://github.com/cockroachdb/cockroach/blob/master/docs/RFCS/20210519_bounded_staleness_reads.md))。
  - TiKV/TiDB: peerごとの **safe-ts**(leaderのみが持つresolved-tsとは
    別概念)で、`read_ts <= safe-ts` ならローカルStale Read可
    ([TiDB docs](https://docs.pingcap.com/tidb/stable/troubleshoot-stale-read/))。
  - YugabyteDB: `yb_follower_read_staleness_ms`(既定30秒)の範囲で
    follower から一貫スナップショット読み取り
    ([YugabyteDB docs](https://docs.yugabyte.com/stable/explore/going-beyond-sql/follower-reads-ysql/))。

### 発見したギャップ(コードで裏取り済み)

`grep -rniE "lease|closed_timestamp|follower_read|bounded_staleness|
as_of_system_time" crates` を実行した結果、**aruaru-dbにはlease /
closed timestamp / follower read / bounded staleness に相当する概念が
一切存在しなかった**(ヒットしたのはGitHub Release関連の無関係な語のみ)。
READMEが「ストレージ/コンピュート分離 ✅」と表記しているにもかかわらず、
「増やした読み取り専用の計算ノードが、リーダーへ問い合わせずに安全に
読める根拠(時刻の保証)」が構造的に無かった、というのが今回の最大の発見。

### 実装した内容

- **`crates/aruaru-dist/src/closed_ts.rs`(新規)**
  - `ClosedTimestampTracker`: Range単位のclosed timestamp。
    `advance_to(now)`が`now - target_lag`(既定3秒、CockroachDBの
    `kv.closed_timestamp.target_duration`既定値に倣う)まで前進させるが、
    **進行中書き込み(`begin_write`/`end_write`で追跡)の最小時刻を跨がない**
    ——跨いだ瞬間に「closed以下に新しい書き込みは現れない」保証が破れるため。
    単調増加のみ(後退する通知は無視)。`can_serve_read_at(read_ts)`が
    TiKVの`read_ts <= safe-ts`と同じ判定を行う。
  - `ClosedTimestampCoordinator`: 複数Rangeを束ね、`advance_all`・
    side transport相当の`publish_to(follower)`(冪等、未知Rangeは登録)・
    `negotiate_bounded_staleness`(関与する全Rangeのclosed timestampの
    **最小値**を採用、上限超過・未前進・未知Rangeは
    `RouteToLeaseholder`へフォールバック)・`plan_exact_staleness_read`
    (CockroachDBの`AS OF SYSTEM TIME follower_read_timestamp()`相当)を提供。
  - `ReadPlan::{FollowerRead{timestamp, staleness_nanos}, RouteToLeaseholder{reason}}`。
- **`crates/aruaru-dist/src/multi_raft.rs`**: `MultiRaftCluster`へ
  `closed_ts`コーディネータを配線。`propose_at`/`commit_and_apply_at`
  (書き込み時刻の登録・解除)、`advance_closed_timestamps`、キー列から
  担当Rangeを解決して交渉する`plan_bounded_staleness_read`を追加。
  `split`で生まれたRangeは自動登録、`merge`で消えたRangeは`forget_range`。
- `crates/aruaru-dist/src/lib.rs`: 上記型を再エクスポート。

### 正直な簡略化点(誇張しない)

1. 時刻は**呼び出し側が渡す論理ナノ秒**。HLC・クロックスキュー上限
   (CockroachDBの`max_offset`)は扱わない。
2. **MVCC履歴読み取り本体には接続していない**。本実装は「その時刻で
   読んでよいか」の安全性ゲートまで。実際に過去バージョンを読む処理は
   既存のGit-on-SQL / `AS OF COMMIT`経路の責務として分離したまま。
3. side transportは**同一プロセス内のオブジェクト間通知**
   (`publish_to`)であり、ネットワーク越しの定期配送は未実装。
4. Range横断の交渉は「最小値を取る」単純方式で、CockroachDBのように
   ロックを避ける時刻交渉は行わない。
5. **管理REST API(`/admin/*`)への公開・実プロセスHTTPでのE2E確認は
   今回行っていない**(ユーザーの範囲縮小指示により中断)。検証は
   `cargo build`/`cargo test`までに留まる——実機E2E未実施であることを
   隠さず記録する。

### 検証結果(実測)

- `cargo test -p aruaru-dist` → **50 passed / 0 failed / 1 ignored**
  (前回46件+新規6件: closed_ts側4件〈target lagでの前進と非後退、
  進行中書き込みを跨がないこと、read_ts=0の拒否、side transportの冪等性、
  Range横断の最小値採用、上限超過/未知Rangeのフォールバック、exact
  stalenessの境界〉、multi_raft側2件〈`in_flight_write_blocks_follower_
  read_until_it_commits`、`closed_timestamps_reach_a_read_only_replica_
  via_side_transport`〉)。
- `cargo build --workspace` → 成功(既存警告2件のみ、無関係)。
- `cargo test --workspace` → 全テストバイナリで`test result: ok`、失敗0件。

### 次回の調査・開発の目星(ユーザー指示により「当たり」だけを列挙)

次回はまず以下を検索・一次資料確認してから着手する。括弧内は関係する
crate。

1. **Neon の pageserver / safekeeper 分離**(`aruaru-core`のストレージ層、
   `aruaru-dist`): WALをsafekeeperがPaxosで多重化し、pageserverが
   ページ再構成を担う「本当のストレージ/コンピュート分離」の実装形。
   aruaru-dbのfjall直結構成との差分を具体的に洗う。
2. **SingleStore(旧MemSQL)の rowstore/columnstore 単一テーブル同居**
   (`aruaru-query::olap`): 現状のOlapCacheは別系統のキャッシュであり、
   単一テーブル内での行/列の使い分けは未対応。
3. **Databend / RisingWave のオブジェクトストレージ直結**
   (`aruaru-backup`、`aruaru-core`): S3/オブジェクトストア上の
   Parquetを一次ストレージとして扱う場合のメタデータ管理(スナップショット
   ・マニフェスト方式)。`aruaru-backup/src/s3.rs`との接点を調べる。
4. **CockroachDB Serverless / TiDB Serverless の弾力的コンピュート**
   (`aruaru-server::ephemeral_pod`): 既存のephemeral SQL podを、
   今回実装したclosed timestampと組み合わせて「読み取り専用podが
   leaseholderへ問い合わせず動く」形まで実配線する。
5. **今回実装分の残作業**(`aruaru-dist`、`aruaru-server`):
   (a) closed timestampを管理REST API(`/admin/closed-timestamp/*`・
   `/admin/follower-read/negotiate`)へ公開し実プロセスHTTPでE2E確認、
   (b) side transportをネットワーク越し(`raft/transport.rs`)へ配線、
   (c) `ReadPlan::FollowerRead`の時刻を既存の`AS OF COMMIT`読み取り経路へ
   実際に橋渡しする(現状はゲート判定までで読み取り本体と未接続)。

## HANDOFF: 2026-08-22(続き2) CPU SIMD(AVX-512/AVX2/FMA3/PCLMULQDQ)適用可否を
実コードで調査 — RAID-Z2連携経路は`open-raid-z`側のSIMD化を自動継承、
このリポジトリ本体はコード変更**不要**と判断(理由を明記)

**経緯**: ユーザー指示「AVX-512があればRAID6パリティ・NVMe RAID6の
ランダムアクセス・AI処理が高速化できるはず。5リポジトリで実装せよ」への
対応の一環。**この開発機のCPUはAMD Ryzen 9 3950X(Zen 2)でAVX-512は
非搭載**であり、実測できるのはAVX2/FMA3/SHA-NIまでである点を先に明記する。

### 調査結果(推測せず`grep`と実クレートソースで確認)

1. **このリポジトリ全体にSIMDコードは存在しなかった**
   (`avx|simd|is_x86_feature|target_feature`でヒット0件)。
2. **しかし主要なCPU律速処理は、既にSIMD対応済みのライブラリへ委譲済み
   だった**——「SIMDコードが無い=最適化されていない」ではない点が
   今回の最大の発見:
   - **チェックサム(SHA-256、`aruaru-core/src/storage/mod.rs`)**:
     依存する`sha2` 0.10.9のソース
     (`src/sha256/x86.rs:100`)に
     `cpufeatures::new!(shani_cpuid, "sha", "sse2", "ssse3", "sse4.1");`
     があることを実際に確認した——**SHA-NI(SHA拡張命令)を実行時検出して
     使う**設計であり、Zen 2はSHA-NI搭載のため**この開発機では既に
     ハードウェア高速化が効いている**。手書きSIMDを追加する余地は無い
     (追加すれば劣化する)。
   - **OLAP列処理(`aruaru-query/src/olap.rs`)**: Apache Arrow +
     DataFusion。`compute::min`/`max`(ゾーンマップ)・
     `filter_record_batch`(インクリメンタルマージ)はいずれもArrowの
     ベクトル化カーネルであり、Arrow側でSIMD化されている。
     自前のバイトループを書き足す箇所は無い。
   - **ハッシュルーティング(`aruaru-query/src/sharded_store.rs`)**:
     2026-08-21に自前`murmur3_32`へ移行済み(非暗号学的・軽量)。
     SIMD化の余地はあるが、キー1本ごとの数十バイト処理であり
     ベクトル幅を活かせない(バッチ化されていない)ため効果が見込めない。
3. **PCLMULQDQの適用先は無かった**: PCLMULQDQが効くのはCRC32や
   GF(2^128)(GHASH)のような単一の広い多項式演算だが、このリポジトリの
   チェックサムはSHA-256(上記の通りSHA-NIで既に高速)であり、CRC系の
   ホットパスは存在しない。**適用先が無い箇所へ命令を持ち込むことは
   しなかった**(`open-raid-z`側の同日HANDOFFに、バイト単位GF(2^8)演算に
   PCLMULQDQが使えない技術的理由〈隣接バイトへの積のビット混入〉も
   記録してある)。

### 実際に効いた変更(このリポジトリのコードは1行も変えていない)

`aruaru-dist`の`raid_z_backend`(`open_raid_z` feature、Raftコミット×
RAID-Z2スナップショット連携)は`open_raid_z_core`をpath依存している。
同日、`open-raid-z`側で**RAID6(RAID-Z2/Z3)のP/Q/Rパリティ計算を
CPU SIMD化**したため(`zfs_accel_hlsl/src/simd.rs`新設、実行時検出で
AVX-512F+BW → AVX2 → SSE2/SSSE3 → スカラー)、**この連携経路は再ビルド
するだけで自動的にその恩恵を受ける**。
- `open-raid-z`側の実測(Ryzen 9 3950X、検出経路`avx2`):
  P/Q生成 **8.94〜14.89倍**、P/Q/R生成 **9.21〜13.95倍**、
  任意係数GF(2^8)乗算 **30.96倍**(いずれもGF乗算テーブル引き実装比)。
  詳細・ベンチマークの前提条件は`open-raid-z/CLAUDE.md`の同日エントリ参照。
- **NVMe 4枚以上のRAID6構成でランダムアクセスが速くなる、という主張は
  していない**——上記はパリティ計算そのものの所要時間であり、
  実NVMe SSDを4枚以上使った実機IOPS測定はこの開発機に該当構成が無いため
  **未実施**。ただしRAID6のランダム書き込みはRead-Modify-Writeで
  パリティ再計算が都度発生し、ディスク本数が増えるほど倍率が上がる
  (8.9倍→14.9倍)ことは実測済みであり、CPU側パリティ計算がボトルネックに
  なる場面ほど効くという方向性自体は妥当。

### TEST(実測)

- `cargo test -p aruaru-dist --features open_raid_z --release` →
  **62 passed / 0 failed / 1 ignored**。SIMD化された
  `open_raid_z_core`とリンクした状態で、実RAID-Z2プールへの
  実`create_snapshot`を伴う`real_raft_commit_triggers_real_raid_z_
  snapshot`を含め全green——**パリティ計算のアルゴリズムを
  ホーナー法+SIMDへ差し替えても、このリポジトリ側の連携動作に
  回帰が無いことを実測で確認した**。
- このリポジトリのコード変更が無いため、他クレートのテストは前回
  エントリの状態から変化しない。

### 正直な開示・未実施

- **AVX-512経路は`open-raid-z`側でコンパイル確認のみ**(この開発機が
  非搭載のため実行・ベンチマーク未実施)。AVX-512搭載機へ載せ替えれば
  コードの書き足し無しに`simd_level()`が`Avx512`を返して有効になる設計。
- 実NVMe 4枚以上でのRAID6ランダム書き込みIOPS測定は未実施。
- このリポジトリ本体へのSIMDコード追加は**意図的に見送った**——上記の
  通り主要ホットパスが既にSIMD対応ライブラリ(sha2 + SHA-NI、
  Arrow/DataFusion)へ委譲済みであり、手書きSIMDを重ねても改善しない
  (むしろ保守負債になる)という判断。「コストが高いから見送った」のでは
  なく「適用先が無いことをコードで確認した」ことを明記する。
- 次にすべきこと: (1) AVX-512搭載機を入手した際に、`open-raid-z`の
  `simd_parity_benchmark`を実行した上で、このリポジトリの
  `raid_z_backend`経由のスナップショット所要時間も併せて測る。
  (2) 将来`aruaru-query::olap`に自前のバッチ処理(bloom filter一括判定・
  sparse index等、前回エントリの次回候補)を書く場合は、その時点で
  初めてSIMDの適用余地が生まれるため再評価する。(3) CPU機能検出は
  将来、共有クレート`open-cpu`(`aon-co-jp/open-cpu`、別セッションで
  作成中)へ集約する方針が決まっている(2026-08-22ユーザー指示)。

## HANDOFF: 2026-08-22 Neon の pageserver/safekeeper 分離を一次資料で調査、
WAL サービス層(safekeeper quorum + pageserver)を新規実装

**経緯**: 前回エントリ(2026-08-21(続き6))で「次回の調査目星」として
残した1件目、**Neon の pageserver / safekeeper 分離**を日本語・英語
両方で検索し、一次資料(`neondatabase/neon`リポジトリ内の設計ドキュメント)
で裏取りしてから実装した。

### 調査で確認したこと(一次資料)

- [`docs/walservice.md`](https://github.com/neondatabase/neon/blob/main/docs/walservice.md):
  compute が生成した WAL は複数の **safekeeper** へストリームされ、
  「**過半数の safekeeper がローカルディスクへ書き終えた時点で durable**」
  と見なされる。safekeeper 群は Paxos ベースの合意で WAL を多重化し、
  **単一 primary の強制**(2つの compute が同時に書くことの防止)も担う。
  pageserver は primary からではなく **safekeeper 群から** streaming
  replication で WAL を引く。safekeeper は「一時的な耐障害ストレージ」で
  あり、最終的な永続化先は S3。
- [`docs/safekeeper-protocol.md`](https://github.com/neondatabase/neon/blob/main/docs/safekeeper-protocol.md):
  proposer は `(term, UUID)` の NodeID を持ち、**term は proposer 起動
  ごとに増加**して split-brain を防ぐ。safekeeper は自分が受理済みの
  NodeID 以上の提案のみ受理する。`commitLSN` は「全 safekeeper の
  `flushLSN` を並べた配列の `flushLsn[n - quorum]` 要素」=
  **quorum 番目に大きい flushLSN**。
- [`docs/pageserver-storage.md`](https://github.com/neondatabase/neon/blob/main/docs/pageserver-storage.md)
  ・ブログ["Deep dive into Neon storage engine"](https://neon.com/blog/get-page-at-lsn):
  pageserver は WAL を継続的に取り込み、ページ単位に切り分けて
  **要求 LSN のページを image layer + delta layer から再構成**する
  (`get_page_at_lsn`)。対応する WAL が届くまでページ要求に応答しない
  ことで一貫性を保証し、`max_replication_*_lag` でバックプレッシャをかける。

### 発見したギャップ(コードで裏取り済み)

`grep -rniE "safekeeper|pageserver|wal_service|commit_lsn|lsn" crates`
を実行した結果、`crates/`内で LSN に言及していたのは
`aruaru-core/src/version/mod.rs`の`create_branch_from`の**コメント**
(Neon 方式ブランチングの説明)だけであり、**WAL を独立した quorum で
耐久化する層、および「LSN 指定でページを再構成する層」は一切存在
しなかった**。既存の`aruaru-dist::raft`は「合意 + 状態機械への適用」を
同一ノード内で一体に行う構成で、Neon の中核設計である
「WAL の耐久化(safekeeper)とページ再構成(pageserver)の分離」は無かった。

### 実装した内容

- **`crates/aruaru-dist/src/wal_service.rs`(新規、約620行)**
  - `Safekeeper`: `accepted_term`による fencing、`flush_lsn`、
    WAL の`accept`(LSN 単調増加を強制)・`stream(after, up_to)`・
    pageserver 取り込み済み分の解放`truncate_up_to`。
  - `WalService`: n 台の safekeeper を束ねる。`quorum() = n/2+1`。
    `start_proposer()`が既存最大 term + 1 を全 safekeeper へ通知して
    **古い proposer を fence**(単一 primary 強制)。`append(term, records)`
    は quorum の ack を要求し、`recompute_commit_lsn()`で
    **全 flush_lsn を降順に並べた quorum 番目**を commitLSN として採用
    (Neon の`flushLsn[n - quorum]`と同じ計算)。commitLSN は単調増加のみ。
    `stream_committed(after)`で pageserver へ配る。
  - `Pageserver`: `ingest(&WalService)`で commitLSN までを取り込み
    `last_record_lsn`を進める。`get_page_at_lsn(key, lsn)`が
    image layer + delta layer からページを再構成し、
    未着 LSN は`WalNotArrived`で拒否(Neon の「WAL が届くまで応答しない」)。
    `create_image_layer(key, lsn)`が materialize + 以前の delta 破棄
    (compaction)、それ未満の読み取りは`BelowGcCutoff`として**再構成
    できないことを明示**。`check_replication_lag`がバックプレッシャ。
  - `DisaggregatedStorage`: 上2つを束ね、`write(records)`(lag 検査 →
    quorum 耐久化 → pageserver 取り込み)と`read_latest(key)`を提供。
  - `PageDelta::{Replace, Append}` / `WalRecord` / `WalServiceError`。
- `crates/aruaru-dist/src/lib.rs`: 上記型を`pub mod` + 再エクスポート。

### 正直な簡略化点(誇張しない)

1. **同一プロセス内のオブジェクト分離**であり、ネットワーク越しの
   streaming replication は未実装。
2. Paxos の完全実装ではない。**term による fencing と quorum flushLSN
   による commitLSN 決定**という核だけを実装(投票フェーズでの WAL
   突き合わせ・term_history の復旧は未実装)。
3. ストレージは**メモリ上**(`BTreeMap`)。Neon の layer file / S3
   アップロードに相当する永続化は未実装(`aruaru-backup`との接続は次回)。
4. `PageDelta`は`Replace`/`Append`の2種のみで、PostgreSQL WAL の
   `redo`再生ではない。
5. 既存の SQL 実行経路・`AS OF COMMIT`読み取り・管理REST API には
   **未接続**。本モジュール単体で完結する層として追加している
   (前回の closed timestamp と同じ状況——両者を実経路へ橋渡しする
   作業がまとめて残っている)。

### 検証結果(実測)

- `cargo test -p aruaru-dist` → **61 passed / 0 failed / 1 ignored**
  (前回50件 + 新規11件: commitLSN の quorum 番目採用・非後退、
  新 proposer による旧 proposer の fence、quorum 未達、LSN 非単調の拒否、
  過去 LSN でのページ再構成、未着 LSN の拒否、image layer 生成と
  GC cutoff、バックプレッシャ、取り込み後の WAL 解放、未知ページ)。
- 実装中に`image_layer_materializes_and_drops_older_deltas`が
  `PageNotFound`で**実際に失敗**した(image layer より前の LSN を
  読もうとしたケース)。誤って image の内容を返すのは不正確なため、
  `BelowGcCutoff`エラーを新設して「再構成できない」ことを明示する形に
  修正した——テストを緩めずに設計を直した。
- `cargo build --workspace` / `cargo test --workspace` の結果は下記
  「ワークスペース全体の検証」に記載。

### 次回の調査・開発の目星(更新版)

前回リストのうち 1.(Neon)は今回着手済み。残りは以下。

1. **今回実装分と前回実装分の「実経路への橋渡し」**(`aruaru-dist`、
   `aruaru-server`、`aruaru-query`): (a) closed timestamp / WAL サービス
   を管理REST API(`/admin/closed-timestamp/*`、`/admin/wal-service/*`)へ
   公開し実プロセス HTTP で E2E 確認、(b) `ReadPlan::FollowerRead`の時刻と
   `Pageserver::get_page_at_lsn`を既存の`AS OF COMMIT`読み取り経路へ接続、
   (c) side transport / WAL ストリームをネットワーク越し
   (`raft/transport.rs`)へ配線。**この橋渡しが2セッション分たまっている
   ため、次回はここを優先するのが妥当**。
2. **SingleStore(旧MemSQL)の rowstore/columnstore 単一テーブル同居**
   (`aruaru-query::olap`)。
3. **Databend / RisingWave のオブジェクトストレージ直結**
   (`aruaru-backup`、`aruaru-core`)。今回の`Pageserver`をメモリから
   S3(`aruaru-backup/src/s3.rs`)上の layer file へ載せる話と直結する。
4. **CockroachDB Serverless / TiDB Serverless の弾力的コンピュート**
   (`aruaru-server::ephemeral_pod`)。今回の pageserver 分離と
   前回の closed timestamp を組み合わせ、読み取り専用 pod を実配線する。

## HANDOFF: 2026-08-22(続き) 方針拡大 — ハイブリッド/HTAP系DBの要素技術を
多言語(日英中韓)で横断調査し、優先度の高い3件を実装

**経緯**: ユーザーより「前回HANDOFFの目星に限定せず、一から世界中の言語で
Google/GitHub検索を行い、Snowflake×CockroachDB型ハイブリッドDBに関連する
重要な技術要素を広く洗い出し、優先度の高いものから複数件を実装に反映する
こと」という拡大指示を受けた。前回済みの follower reads / closed timestamp
系(`closed_ts.rs`)は重複調査しない前提。

### 今回の調査対象と、一次資料で確認した要素技術

| # | 技術要素 | 出典(調査言語) | aruaru-db の状況 |
|---|---|---|---|
| 1 | Neon: safekeeper(WAL の Paxos quorum)と pageserver(ページ再構成)の分離、`commitLSN = flushLsn[n-quorum]`、term による単一primary強制、`get_page_at_lsn` | `neondatabase/neon`の`docs/walservice.md`・`docs/safekeeper-protocol.md`・`docs/pageserver-storage.md`(英/日) | **無かった → 実装した**(`aruaru-dist/src/wal_service.rs`) |
| 2 | SingleStore Universal Storage: columnstore を segment 単位で持ち、hash index・subsegment access・行レベルロックで OLTP も columnstore で処理 | `docs.singlestore.com`の"Universal Storage"・"Choosing a Table Storage Type"(英) | 部分的。**segment 単位統計が無かった → 実装した**(`aruaru-query/src/olap.rs`)。hash index・行レベルロックは未実装 |
| 3 | Databend: snapshot(`_ss/*.json`)→ segment(`_sg/*.json`、最大1000 block)→ block(parquet)の3層メタデータ、min/max・sparse index・bloom filter、**MetaSrv の Snapshot Key 書き込み成功=コミット成功**という ACID の根拠 | 「Databend 存储架构总览」「Databend 索引结构说明」(中国語一次資料、`cnblogs.com/databend`・`zhuanlan.zhihu.com`) | **無かった → 実装した**(`aruaru-backup/src/table_format.rs`) |
| 4 | RisingWave: Hummock(共有 LSM state store)。shared buffer + オブジェクトストレージ、**Barrier に紐づく epoch を MVCC バージョンとして使う**チェックポイント方式 | `docs.risingwave.com/store/overview`、`risingwave.com/blog/hummock-a-storage-engine-designed-for-stream-processing`(英) | 未実装。ストリーミング(materialized view の増分維持)という別軸の前提が必要なため今回は見送り——**「該当なし」ではなく「未着手」**として記録 |
| 5 | Snowflake Hybrid Tables(GA): 行ストアを一次ストアとし行ロックで高並行 OLTP、列側は分析用。HTAP を「unified storage / decoupled storage」に分類する整理 | `docs.snowflake.com/en/user-guide/tables-hybrid`(英)、HTAP survey `arxiv.org/pdf/2404.15670`(韓国語検索経由で発見) | 分類上 aruaru-db は **decoupled storage 型**(行ストア + `OlapCache` 列レプリカ)。単一テーブル内での行/列併存(SingleStore の #2 の完全形)は未実装 |

### 実装した内容(3件)

1. **`crates/aruaru-dist/src/wal_service.rs`(新規)** — Neon 方式。
   詳細は直前の HANDOFF エントリを参照。
2. **`crates/aruaru-query/src/olap.rs`(改修)** — セグメント単位ゾーンマップ。
   従来はコード内にも「テーブル全体で1ブロック分の min/max しか持たない
   (DuckDB のようなブロック単位の部分スキップは行わない)」と正直な
   簡略化点として明記されていた箇所。SingleStore(segment)・Databend
   (block)・DuckDB(Row Group)がいずれも**セグメント単位統計で部分
   スキップ**している共通点を確認したため:
   - `SegmentStats { offset, len, zone_maps }`を導入し、ベース列バッチを
     `segment_rows`行(既定1024、`OlapCache::with_segment_rows`で変更可)
     ごとに区切って各セグメントの min/max を保持。
   - `query()`は、単純範囲述語に対して**該当行が絶対に無いと証明できた
     セグメントを DataFusion へ渡さない**。生き残ったセグメントは
     `RecordBatch::slice`(ゼロコピー)として1パーティションずつ登録する
     ため、枝刈りと DataFusion のパーティション並列が同時に効く。
   - 枝刈り結果は`plan_segment_pruning(sql) -> (table, 残数, 全数)`で観測可能。
     偽陽性(該当行があるのに読み飛ばす)は構造上起こさない設計を維持。
3. **`crates/aruaru-backup/src/table_format.rs`(新規)** — Databend 方式の
   オブジェクトストレージ直結テーブルフォーマット。
   - `ObjectStore`トレイト + `InMemoryObjectStore`。パス構成は Databend に
     倣い`<root>/<db_id>/<table_id>/_ss/<32桁16進>_v1.json`(snapshot)・
     `.../_sg/<32桁16進>_v1.json`(segment)。
   - `BlockMeta`(min/max の`ColumnStats` + 等値述語用`BloomFilter`)、
     `SegmentMeta`(block 集約統計、**1000 block 超過は
     `SegmentTooLarge`で拒否**)、`TableSnapshot`(`prev_snapshot_id`の
     連鎖で時間旅行)。
   - `MetaService`(MetaSrv 相当)の**楽観的 CAS が成功して初めてコミット
     成立**——古い親スナップショットの上へ書こうとすると
     `CommitConflict`。書かれたオブジェクトは孤児として残る(Databend も
     vacuum 対象として扱う)ことをコメントに明記。
   - `prune_range`は**segment 統計で丸ごと読み飛ばせる場合 segment 内の
     block を一切見ない**(Databend が segment 側にも min/max を持つ理由)。
     戻り値で読み飛ばした segment 数・block 数を報告。`prune_equality`は
     bloom filter による等値枝刈り(索引が無い block は安全側で残す)。

### 正直な簡略化点(誇張しない)

- `wal_service.rs`: 同一プロセス内、Paxos 完全実装ではない(term fencing +
  quorum flushLSN のみ)、ストレージはメモリ、`redo`再生ではない、
  既存 SQL 経路・管理 REST API へ未接続。
- `olap.rs`: bloom filter・sparse index は未実装(min/max のみ)。
  述語は`extract_simple_range_predicate`が拾える単純形のみ。
- `table_format.rs`: **同梱の`ObjectStore`実装はメモリのみで、既存の
  `s3.rs`(`S3Client`)へは未接続**。block 実体(Parquet)の書き出しも
  行わない(メタデータ階層と枝刈り・コミットの正しさが担当範囲)。
  sparse index 未実装。CAS リトライループは呼び出し側責務。
- RisingWave Hummock(epoch/barrier ベースのチェックポイント)と
  SingleStore の hash index / 行レベルロック / subsegment access は
  **今回未実装**。「対応済み」とは書かない。

### 検証結果(実測、2026-08-22)

- `cargo build --workspace` → **成功**(警告は既存の`build_cluster`/
  `propose_commit` dead_code 2件のみ、今回の変更とは無関係)。
- `cargo test --workspace` → **全テストバイナリで`test result: ok`、
  失敗0件**。内訳: aruaru-backup 33 passed(うち`table_format`新規10件)、
  aruaru-core 14、aruaru-dist 61 passed/1 ignored(うち`wal_service`
  新規11件)、aruaru-graphql 6、aruaru-migrate 9、aruaru-query 59
  (うちセグメント枝刈り新規1件)、aruaru-registry 5、aruaru-server
  3 passed/1 ignored、aruaru-wire 10、統合テスト2件は従来通り ignored
  (実バイナリ起動が必要なもの)。
- 実装中に`image_layer_materializes_and_drops_older_deltas`が実際に
  失敗し、`BelowGcCutoff`エラーを新設して設計を直した(テストを緩めて
  通したのではない)。
- **未実施**: 実プロセス HTTP を立てた E2E 検証(管理REST API への公開が
  まだ無いため)。「コンパイル+単体テストまで」であることを明記する。

### 次回への引き継ぎ(2026-08-22時点、優先順)

1. **橋渡しが3セッション分たまっている**: `closed_ts`(08-21)・
   `wal_service`(08-22)・`table_format`(08-22)はいずれも
   **既存のSQL実行経路・管理REST APIへ未接続**。次回はここを優先する:
   (a) `/admin/wal-service/*`・`/admin/closed-timestamp/*`・
   `/admin/object-table/*`を`aruaru-server`へ公開し、実プロセスHTTPで
   E2E確認、(b) `Pageserver::get_page_at_lsn`と`ReadPlan::FollowerRead`を
   既存の`AS OF COMMIT`読み取り経路へ接続、(c) `table_format`の
   `ObjectStore`実装を既存の`s3.rs`(`S3Client`、非同期)へ接続。
2. **RisingWave の Hummock**(未着手): Barrier に紐づく epoch を MVCC
   バージョンとして使うチェックポイント方式。aruaru-db に
   materialized view の増分維持が無いため前提整備が必要。
3. **SingleStore の残り要素**: columnstore 上の hash index・
   行レベルロック・subsegment access(単一テーブル内での行/列併存の完全形)。
4. **`olap.rs`のセグメント統計の拡張**: bloom filter・sparse index
   (今回は min/max のみ)。`table_format.rs`側には bloom filter が
   あるので、実装を寄せられる可能性がある。

## HANDOFF: 2026-08-24 前回までの「橋渡しが3セッション分たまっている」を解消
— FollowerRead→AS OF COMMIT接続・side transportのネットワーク越し配線・
ObjectStoreの実S3クライアント接続を実装、実プロセスHTTP/複数プロセス間で検証

**経緯**: 直前2回のHANDOFF(2026-08-22・2026-08-22続き)が「次回優先」
として挙げていた3項目のうち、具体的に指示された3つ((b)FollowerReadの
実データ接続、(c)side transportのネットワーク越し配線、および
`table_format`の`ObjectStore`実S3接続)に着手した。(a)の管理REST API
公開自体は既に別コミット(`6e6286f`)で先行完了していたため、今回は
「公開されているが中身が繋がっていなかった」箇所を実装で埋める形になった。

### タスク1: `ReadPlan::FollowerRead`を`AS OF COMMIT`読み取り経路へ実接続

- `crates/aruaru-core/src/version/mod.rs`: `VersionController::
  find_commit_at_or_before(timestamp_nanos: i64) -> Option<Commit>`を
  新設。現在ブランチのHEADから祖先方向へたどり、`commit.timestamp <=
  timestamp_nanos`を満たす最初の(=最も新しい)コミットを返す。
- `crates/aruaru-query/src/engine.rs`: `QueryEngine::select_follower_read
  (table, filter, timestamp_nanos) -> Result<QueryResponse, String>`を
  新設。上記でcommitへ解決し、既存の`select_as_of`(Prolly Tree経由の
  `AS OF COMMIT`本体)へそのまま委譲する。`aruaru-query`が`aruaru-dist`に
  依存しない既存設計を保つため、`ReadPlan`型そのものではなく解決済みの
  生タイムスタンプ(i64、Unixナノ秒)を受け取る設計とした——`ReadPlan`
  から実際に呼び出す橋渡しは両クレートに依存できる`aruaru-server`側の
  責務とした。
- `crates/aruaru-server/src/admin.rs`: 既存の`POST /admin/closed-
  timestamp/plan`に`table`/`filter_col`/`filter_val`という省略可能な
  フィールドを追加。`ReadPlan::FollowerRead`と判定され`table`が指定
  された場合のみ、`select_follower_read`で実際にデータを読み出し
  レスポンスの`data`フィールドへ含める(`RouteToLeaseholder`の場合は
  読み取りを行わず理由のみ返す、後方互換——`table`未指定なら従来通り
  ゲート判定のみ)。

### タスク2: side transportをネットワーク越し(`raft/transport.rs`)へ配線

- `crates/aruaru-dist/src/closed_ts.rs`: `ClosedTimestampCoordinator`に
  `snapshot_closed_timestamps()`(送信側、`(range_id, closed_ts)`の
  スナップショットを取り出す)と`apply_closed_timestamp_updates(&[..])`
  (受信側、未知Rangeは登録しつつ取り込む、冪等)を新設。既存の
  `publish_to`(同一プロセス内)はこの2つの組み合わせとして再実装した。
- `crates/aruaru-dist/src/raft/transport.rs`: `HttpTransport`
  (AppendEntries/RequestVoteの送信、`x-admin-token`ヘッダー付与)と
  同じパターンで`HttpSideTransport`を新設。`publish_to(peer, updates)`
  が指定peerの`POST /admin/closed-timestamp/receive`へ実際にHTTP POST
  する。
- `crates/aruaru-server/src/admin.rs`: 受信側`POST /admin/closed-
  timestamp/receive`(他ノードからの更新を取り込む)と送信側
  `POST /admin/closed-timestamp/publish`(`peer_id`+`peer_url`を指定して
  `HttpSideTransport`経由で実際に配布する)を新設。既存の`/admin/*`
  共通認証ミドルウェアがそのまま適用される(個別実装不要)。
- **正直な簡略化点**: CockroachDBの`closedts/sidetransport`のような
  バックグラウンドでの周期的自動配送は実装していない——呼び出し側が
  `POST /admin/closed-timestamp/publish`を能動的に呼ぶ必要がある。

### タスク3: `table_format::ObjectStore`を実S3クライアントへ接続

- `crates/aruaru-backup/src/s3.rs`: `impl ObjectStore for S3Client`を
  新設。`ObjectStore`トレイトは同期(`put`/`get`/`list`、`async`でも
  `Result`でもない)だが`S3Client`のI/Oは非同期のため、`run_blocking`
  (`std::thread::scope`で専用OSスレッドを立て、その中で
  `tokio::runtime::Builder::new_current_thread`の新規ランタイムを
  `block_on`する)でブリッジした——呼び出し元が既にtokioランタイム上
  (`aruaru-server`の非同期ハンドラ等)にいても`Cannot start a runtime
  from within a runtime`を起こさない設計(`#[tokio::test]`内から呼ぶ
  回帰テストで実証)。`list()`は`S3Client`自身のバケットレベル`prefix`
  を`strip_client_prefix`で取り除き、`put`/`get`が受け取るのと同じ
  「パス」名前空間の文字列を返すようにし、`InMemoryObjectStore`との
  契約(`list()`が`put()`に渡した文字列をそのまま返す)を保った。
- **正直な限界**: `ObjectStore`トレイト自体はエラーを表現できない
  (`put`は`()`、`get`は`Option`、`list`は`Vec`を返す設計のまま)ため、
  失敗は`tracing`へログするに留め、`get`/`list`は「見つからなかった
  ことにする」形へ縮退する——ネットワーク瞬断とオブジェクト不在の
  区別がこのAPI形状では呼び出し元から見分けられないという制約が残る
  (トレイト自体を`Result`化する改修は既存の全呼び出し元・
  `InMemoryObjectStore`・既存テストに影響するため今回は見送った)。
  実S3/MinIOサーバーへの実際のPUT/GET往復は、既存の`put_object`/
  `get_object`/`list_objects`自体のテストと同じ理由(この環境に到達
  可能なS3互換サーバーが無い)で未検証——`strip_client_prefix`の
  ロジックとsync/asyncブリッジが多重ランタイムでデッドロックしない
  ことは単体テストで検証済み。

### 検証結果(実測、型チェック・ビルド成功だけで終わらせない方針の徹底)

- `cargo build --workspace` → 成功(既存の`build_cluster`/
  `propose_commit`未使用警告2件のみ、無関係)。
- `cargo test --workspace` → 全19テストバイナリで`test result: ok`、
  失敗0件(新規追加分: aruaru-backup +4〈`strip_client_prefix`2件・
  `run_blocking`のランタイムネスト回帰・`ObjectStore`実装の型検証〉、
  aruaru-dist +1〈`snapshot_closed_timestamps`/`apply_closed_timestamp_
  updates`往復〉、aruaru-query +1〈`select_follower_read`の
  タイムスタンプ解決〉)。
- **実プロセスHTTPでのE2E確認(型チェック・単体テストのみで終わらせない)**:
  1. **タスク1**: 実`aruaru-server.exe`を1台起動し、`CREATE TABLE
     items`→`INSERT (qty=1)`→`aruaru_commit`→`UPDATE (qty=5)`→
     `aruaru_commit`→`POST /admin/closed-timestamp/range`→
     `POST /admin/closed-timestamp/advance`(closed timestampを両
     コミットより後まで前進)→`POST /admin/closed-timestamp/plan`
     (`table=items`,`filter_col=id`,`filter_val=sword`)を実行し、
     **`"plan":"follower_read"`かつ`"data":{"ok":true,"result":
     {"Rows":{"columns":["id","qty"],"rows":[[{"Text":"sword"},
     {"Text":"5"}]]}}}}`という実データが実際に返る**ことを確認した
     (ゲート判定だけでなく実データ読み取りまで到達する直接証拠)。
  2. **タスク2**: 実`aruaru-server.exe`を2台(port 7401/7402)起動し、
     server1で`range_id=1`を登録・前進させた後、server1へ
     `POST /admin/closed-timestamp/publish`(`peer_id=2,
     peer_url=http://127.0.0.1:7402`)を実行。**server2の
     `GET /admin/closed-timestamp`が`range_count:0`→`range_count:1`
     (`closed_timestamp`がserver1と完全一致)へ実際に変化する**ことを
     確認した——別プロセス・別ポート間の実HTTP通信によるside
     transportの直接証拠。再送すると`advanced_on_peer:0`(冪等)、
     `x-admin-token`無しでの`/admin/closed-timestamp/receive`は
     `401`(既存認証ミドルウェアが新規エンドポイントにも自動適用
     されることも確認)。
  3. タスク3(S3実接続)は、この環境に到達可能なS3互換サーバーが
     無いため実HTTP往復のE2E確認は未実施(上記「正直な限界」参照)。
- 検証後、両プロセス・一時データディレクトリはすべて終了・削除済み
  (リポジトリへの影響なし)。

### 次回への引き継ぎ

1. side transportの定期的自動配送(バックグラウンドループでの
   周期publish)——現状は手動トリガーのみ。
2. `table_format::ObjectStore`を実S3/MinIOサーバーで実際に往復検証
   (この環境にサーバーが無いため次回以降、到達可能な環境で)。
3. `ObjectStore`トレイト自体の`Result`化(エラーの握りつぶしを解消)は
   既存呼び出し元全体への影響があるため、必要性が具体化した時点で
   改めて設計する。
4. `Pageserver::get_page_at_lsn`(`wal_service.rs`)の実経路への接続は
   今回スコープ外——`ReadPlan::FollowerRead`とは別の橋渡し先であり、
   次回以降の課題として残る。

## HANDOFF: 2026-08-25 セキュリティ監査(cargo audit)を実施

open-english側のユーザー指示「関連リポジトリのセキュリティ監査(依存
関係・入力検証等)」への対応(横断的な優先判断はopen-english/CLAUDE.md
2026-08-25エントリ参照)。

1. **`h2`(0.4.15、DoS脆弱性RUSTSEC-2026-0258)を0.4.19へ更新**
   (`aruaru-db`本体・`web`サブクレートとも`cargo update -p h2`で
   互換範囲内更新、コード変更不要)。
2. **`quick-xml`のHigh(CVSS 7.5)脆弱性2件を修正**
   (「Quadratic run time when checking a start tag for duplicate
   attribute names」「Unbounded namespace-declaration allocation in
   `NsReader` enables memory-exhaustion denial of service」)。
   `crates/aruaru-backup`が使う`rusty-s3`(S3互換バックアップ先の
   署名付きURL生成)の依存で、`rusty-s3`自体を0.7→**0.10**へ
   アップグレードして解消(`quick-xml`側は`rusty-s3`の内部依存の
   ため直接触れず)。`cargo build --release`(ワークスペース全体、
   11分33秒)成功、`cargo test --release -p aruaru-backup`
   **37 passed / 0 failed**を確認。
3. **未修正のまま記録する事項(正直な開示)**: `rustls-webpki`
   (0.101.7、複数の証明書検証系issue)が、`crates/aruaru-registry`の
   `mongodb 2.8.2`が引き込む古い`rustls 0.21`経由で残っている。
   `mongodb`は3.8.1(メジャーバージョン)が利用可能だが、**データベース
   エンジンという性質上、API破壊的変更を伴うメジャーバージョン移行を
   専用のテスト計画無しに拙速に行うのは避けるべき**と判断し、今回は
   着手しなかった。`rsa`クレート(Marvin Attack、Medium 5.9)も上流
   未修正のため対処不能。`idna`(0.2.3)・`rkyv`(0.7.46)も
   `cargo audit`が検出したが、深刻度・悪用可能性の評価まではこの
   セッションでは行えていない。
4. **`cargo audit`最終結果**: 10件→7件(`h2`・`quick-xml`2件を解消)。
   `web`サブクレートは1件→0件(`h2`のみだったため)。
- 次にすべきこと: (1) `mongodb`2.x→3.xへの移行を専用セッションで
  計画・実施(破壊的変更の洗い出し、`aruaru-registry`クレートの
  回帰テスト整備が前提)、(2) `idna`/`rkyv`の深刻度評価、(3) `rsa`の
  上流修正状況を定期確認。

## 設計方針: APIキー自動ライフサイクル管理 + REST→GraphQL/バイナリ
プロトコルへの段階的移行(2026-08-29、open-english/RPoem連携経由の
ユーザー指示、今後の全セッション共通方針として記録)

**背景**: open-english側のセッションから「aruaru-dbとの連携を強化し、
特にREST APIを不要にして。APIキーは自動発行・自動承認・自動破棄・
自動削除で自動管理して」という指示を受けた。RPoem
(`open-runo-router::keyring::KeyGuardian`)の実装済み設計を調査した
結果、「APIキー不要」とは認証自体の廃止ではなく**人間がキーを手動で
発行・管理する必要をゼロにすること**を意味すると確認した上で、
本リポジトリに以下を実装した。

### 1. APIキー自動ライフサイクル管理(実装済み)

`crates/aruaru-server/src/keyring.rs`(新規、RPoemの`KeyGuardian`と
**同じ設計思想を独立に再実装**——Cargo依存としては結合しない、この
エコシステムの既存方針〈WunderGraph Cosmo/Poem/Tauriと同様〉を踏襲):
- **自動発行**: `POST /v1/keys/self-issue`(認証不要)が`viewer`ロール・
  既定24時間TTLのキーを即座に発行する。
- **自動承認**: 「認証を要求せず即座に発行できる」こと自体が承認手続き
  そのもの——人間の承認待ちキューは存在しない。
- **自動破棄**: `POST /admin/keys/revoke`(`{owner}`指定)で特定オーナー
  の全キーを即座に失効。
- **自動削除**: 期限切れキーは`verify()`実行時に検知されその場で
  レジストリから削除される(明示的なcronジョブ不要)。
- 既存の`ARUARU_DB_ADMIN_TOKEN`静的トークンとは**完全に後方互換**
  (両方式を`check_admin_auth`が併存判定、どちらか一方が有効なら通過)。
- 実機検証: 単体テスト5件(発行・失効・期限切れ自動削除等)に加え、
  実サーバーを起動し実HTTP経由で「無認証で自己発行→発行したキーで
  保護エンドポイントへアクセス成功→そのキーを失効→同じキーで
  401」という一連の流れを確認済み。

### 2. Raft/WALプロトコルのREST完全撤廃(実装済み)

ユーザー指示「Raft/WALプロトコル系は一切REST APIを使用しないように。
信頼できる代替が無ければRust+RPoem/関連リポジトリをフル動員して
一から開発すること」への対応。

**調査結果(日英Web検索)**: 実運用の分散合意システム(etcd・TiKV)は
いずれもノード間RPCにREST/JSON-over-HTTPを使わず、gRPC(Protocol
Buffersによるバイナリシリアライズ+HTTP/2)を使う。Protobufペイロード
は同等のJSONより概ね3〜5倍小さく、パースは5〜10倍速いという報告が
ある([TiKV公式](https://tikv.org/deep-dive/rpc/grpc/)、
[etcd公式](https://etcd.io/docs/v3.4/learning/design-client/))。
Raftのノード間通信は単一運用者が管理する信頼済みネットワーク内で
完結するRPCであり、人間可読性より低レイテンシ・低オーバーヘッドが
優先されるべきと判断した。

**実装**: `crates/aruaru-dist/src/raft/binary_transport.rs`(新規)。
tonic/gRPC等の外部フレームワークは導入せず(このエコシステムの
一貫方針、RPoemの手書きgRPC Health Serviceが同種の前例)、生TCP上の
長さプレフィックス付きバイナリフレーム(`bincode`のserde互換API)を
自前実装した。`AppendEntries`・`RequestVote`(旧`HttpTransport`)・
closed timestampのside transport(旧`HttpSideTransport`)の**両方**を
この単一のバイナリポート(`--gql-port` + 100固定オフセット、
`cluster::BINARY_RAFT_PORT_OFFSET`)へ統合し、REST/JSON-over-HTTPを
一切経由しないようにした。認証は既存の`ARUARU_DB_ADMIN_TOKEN`を
フレーム内に含め定数時間比較(TLS/mTLSは未実装、同一データセンター
内の信頼済みネットワークを前提とする従来の`HttpTransport`と同水準の
境界)。

**正直な線引き**: `/admin/closed-timestamp/publish`(「いつ・誰に
配布するか」を人間/運用ツールが指示する管理トリガー)自体はREST
管理APIのまま残した——ここは制御プレーン(人間向け)であり、
ユーザー指示が対象とするのは実際に発生するノード間の生データ転送
(データプレーン)である、という区別に基づく。

**実機検証**: 2プロセス(leader+learner)を実際に別ポートで起動し、
leaderへのINSERTがバイナリポート(8402等)経由で実際にfollowerの
QueryEngineへ複製されること、REST側の旧`/admin/raft/append`が
もはや呼ばれていないこと(バイナリリスナーのログ・TCP接続一覧で
確認)を実証。単体テスト3件(フレーム往復・実TCP経由E2E・トークン
不一致拒否)も全green。ワークスペース全体`cargo test`でリグレッション
無し(既存192件超、全green)。

### 3. `/admin/*`のREST→GraphQL段階的移行(第一歩のみ実装、方針を確立)

**深い調査(日英、Google/GitHub、実装事例・論文)の結果**: 「即座に
REST APIを完全撤廃する」ことは2026年時点の実務における標準パターン
**ではない**、と判明した。
- Shopifyは実際にREST Admin APIを廃止方針としているが、新機能は
  GraphQL限定で提供しつつ既存REST機能は年次の廃止波(sunset wave)で
  段階的に縮小している([Shopify Admin API](https://shopify.dev/docs/api/admin-graphql/2026-04))。
- 日本の資産運用相談サービス「マネイロ」(運営: 株式会社モニクル
  フィナンシャル、当時OneMile Partners)は「新規APIはGraphQL、改修
  機会のある既存APIは改修時にGraphQL化、それ以外は段階的」という
  **9ヶ月がかりの段階移行**で全REST APIを置き換えた実例がある
  ([がぶちゃんの日記](https://gabu.hatenablog.com/entry/2023/08/02/130000))。
  **【2026-08-29訂正】この実例をこれまで「日本のJX通信社」と誤記して
  いたことを再調査で発見・訂正した(WebFetchで元記事本文を確認した
  ところ、実際は資産運用アドバイスサービス「moneiro」に関する記事
  だった)——JX通信社は無関係。事実確認を怠ったまま社名を記録していた
  ことを正直に開示する。**
- 「複数ゲートウェイ・複数プロトコルを横断する統一コントロールプレーン」
  という、REST/GraphQL/gRPC等を併存させたまま管理する設計が2026年の
  実務でも主流であり、「完全REST撤廃」自体が支配的パターンではない。

**この調査結果に基づく本リポジトリの方針(今後のセッションが従うべき
既定方針)**: `/admin/*`のREST操作を、根拠のない「一括撤廃」の主張で
終わらせず、**実際に真のデータソースへ接続してから**段階的にGraphQL
実装を充実させ、**十分検証できたものから**REST側を縮小していく。

**発見した重大な事実(正直な開示)**: 着手前に`aruaru-graphql/src/
admin_resolvers.rs`を確認したところ、`cluster_status`・
`parallel_config`・`parallel_jobs`・`federated_sources`・
`backup_schedule`等のGraphQL側resolverは、**REST側の実データには
一切接続されておらず、固定値や`Ok(vec![])`のようなスタブを返すだけ**
だったと判明した(2026-08-01のHANDOFFは「GraphQL経由の管理操作にも
認証を適用した」と記録していたが、これは認証ミドルウェアの追加のみを
指しており、resolverの中身が本物のデータを返すことまでは検証・
保証していなかった——ドキュメントの記述と実態が食い違っていた
このエコシステムで繰り返し見つかるパターンの新たな実例)。

**今回実装した第一歩(`cluster_status`)**: `aruaru-dist::shard::
topology::ClusterTopology::status_snapshot()`という**REST・GraphQL
共通の1つの実装**を新設し、REST `/admin/cluster`ハンドラ
(`admin.rs::cluster_status`)とGraphQL `clusterStatus` resolverの
**両方**がこれを呼ぶように書き換えた。`AdminState.topology`を
`Arc<Mutex<..>>`化し、`AdminCtx.topology`として同一インスタンスを
GraphQL側にも共有することで、今後REST側でトポロジが変化すれば
(ノード追加・Range分割等)GraphQL側にも即座に反映される。これで
GraphQL `clusterStatus`は**もう固定値のスタブではなく本物のデータ**
を返す。

**まだ未着手のまま正直に残す範囲(次回以降の段階的着手対象)**:
`parallel_config`/`parallel_jobs`/`federated_sources`/
`backup_schedule`(依然スタブ、REST側の実データへの接続が必要)、
`multi-raft`(split/merge/scatter-query)・`sharded-store`・
`closed-timestamp`(status/register/advance/plan)・`wal-service`・
`object-table`・`ephemeral-query`・`registry`(crawl/test-connection)
・`keys`(revoke/status)は、GraphQL側に対応するresolver自体がまだ
存在しない(2026-08-21〜24に新設された機能はREST限定で実装された
ため)。これらを1機能ずつ「REST側の実装をGraphQLへ移し、実際に
実HTTPで動作確認してからREST側の縮小を検討する」という同じ手順で
段階的に進めること——スタブを量産して「GraphQL対応済み」と見せかける
ことは絶対に避けること。

**検証**: `cargo test --workspace`全green(既存テストにリグレッション
無し、`AdminCtx`の新フィールド追加に伴うテスト側の構築箇所修正1件を
含む)。実サーバーでの`clusterStatus`クエリの実HTTP確認は次回セッション
で実施すること(このセッションではコンパイル・単体テストの確認までに
留まった、正直な開示)。

- 次にすべきこと: (1) 上記「まだ未着手」の各機能を1つずつ
  GraphQL化(REST実装への接続→実HTTP確認→REST縮小の検討、の順で)、
  (2) `clusterStatus`の実サーバー実HTTP確認、(3) この段階的移行方針
  自体を、他のRESTを持つ関連リポジトリ(open-easy-web・open-web-server
  等)へも横展開するか検討。

## HANDOFF追記(2026-08-29続き) REST→GraphQL段階移行の第二歩
(`backupSchedule`・`federatedSources`を実データへ接続)+
RPoem/WunderGraph Cosmoとの連携性・必要性の再調査

**背景**: 直前のエントリで「1機能ずつ、REST実装への接続→実HTTP確認→
REST縮小検討、の順で進めること」と明記した通りに、`backup_schedule`/
`set_backup_schedule`(バックアップ定期実行スケジュール)と
`federated_sources`/`register_federated_source`/`drop_federated_source`
(外部DBフェデレーションソース登録)の2グループを今回接続した。

**実装**: `cluster_status`(前回)と全く同じ設計パターンを踏襲——
REST側(`AdminState`)とGraphQL側(`AdminCtx`)が**同一の
`Arc<parking_lot::Mutex<..>>`インスタンス**を参照する。新規
`crates/aruaru-dist/src/admin_shared.rs`に`BackupScheduleState`/
`FederatedSourceEntry`(両者から使われる共有データ型、`aruaru-server`・
`aruaru-graphql`はいずれも`aruaru-dist`に依存できるため、循環依存を
避けつつ状態共有できる)を新設。`AdminState`の`schedule`/`federation`
フィールドをこの共有型の`Arc<Mutex<..>>`へ変更し、`schedule_handle()`/
`federation_handle()`アクセサ(`topology_handle()`と同じパターン)を
追加。あわせて、REST側に元々`POST /admin/backup/schedule`しか無く
`GET`が存在しなかった非対称性(設定はできるが取得できない)も発見・
是正し`GET /admin/backup/schedule`を新設した。

GraphQL側は`backup_schedule`(旧: 常に`None`固定)・
`set_backup_schedule`(旧: 入力をそのまま返すだけで永続化しない)・
`federated_sources`(旧: 常に空配列固定)・`register_federated_source`
(旧: 入力をそのまま返すだけで永続化しない、重複登録チェックも無し)・
`drop_federated_source`(旧: 何もせず成功メッセージだけ返す)を、共有
`Arc<Mutex<..>>`への実際の読み書きへ差し替えた。`register_federated_source`
はREST側`register_federation`と同じ「同名が既に存在すれば拒否」ルール
も移植した(従来のGraphQL版には無かった)。

**意図的に見送った箇所(誇張しないための正直な記録)**: `parallel_config`/
`set_parallel_config`/`parallel_jobs`は今回も未接続のまま残した。
理由はコスト回避ではなく**スキーマ形状そのものが非互換**なため——
REST側の実`ParallelConfig`(`max_parallelism`/`worker_threads_per_node`
/`enable_parallel_scan`/`enable_parallel_aggregate`/
`enable_shuffle_join`/`shuffle_partitions`/`broadcast_threshold_mb`の
7フィールド)と、既存のGraphQLスキーマ`ParallelConfigGql`
(`enabled`/`max_workers`/`chunk_size`/`strategy`の4フィールド)は
対応するフィールドが実質1つも無い。無理に`max_workers =
max_parallelism`のような意味の薄い変換を当てはめると、「実データに
接続した」と称しながら実際には無関係な値を表示するという、
かえって悪質な不整合を生む。この箇所を本当に接続するには**GraphQL
スキーマ自体の破壊的変更**が必要——今回のスコープ(既存スキーマを
壊さずに済む範囲での段階移行)からは意図的に除外し、その理由をコード
コメントにも明記した。`parallel_jobs`はREST側`list_jobs`自体が
「組み込み単一ノードでは長時間ジョブの常駐管理は未実装」として常に
`{"jobs": []}`を返すだけであり、GraphQL側の空配列はスタブとの乖離
ではなく**REST側の実際の制約と一致した結果**であることも確認した。

**検証(実測)**: `cargo test -p aruaru-graphql`で新規2件
(`set_backup_schedule_persists_and_backup_schedule_reads_it_back`
——書き込み前は`null`、書き込み後は共有`Arc<Mutex<..>>`とGraphQL
再クエリの両方に反映されることを確認、
`federated_source_register_list_and_drop_round_trip_through_shared_state`
——登録→一覧反映→同名二重登録の拒否→削除→一覧から消える、の一連を
共有状態への直接アクセスで検証)を含め計8件全green(既存6件に
リグレッション無し)。`cargo build --workspace`/`cargo test
--workspace`も実行し、ワークスペース全体でのリグレッション有無を
確認した(結果は本エントリ末尾に追記)。

**RPoem/WunderGraph Cosmoとの連携性・必要性の再調査(ユーザー指示)**:
日英でWeb検索を行い、以下を確認した。
- WunderGraph Cosmo**本体(Router・Schema Registry・Studio・CLI・
  メトリクス/トレーシング)はApache 2.0ライセンスのOSSであり、
  セルフホストも可能**([WunderGraph公式](https://wundergraph.com/)、
  [GitHub: wundergraph/cosmo](https://github.com/wundergraph/cosmo))。
  **有料(Enterprise)部分は、SSO(OpenID Connect)+SCIM(チーム
  メンバーシップに応じた権限自動追従)・専有クラウド(Dedicated Cloud、
  SOC 2認証・カスタムリリースサイクル)に限定される**
  ([Cosmo Enterprise公式](https://cosmo-docs.wundergraph.com/enterprise)、
  [WunderGraph SSO/SCIMブログ](https://wundergraph.com/blog/sso-openid-connect-system-for-cross-domain-identity-management))。
  これは従来このリポジトリ・RPoemのCLAUDE.mdが記していた理解
  (「Cosmo有料版の機能をOSS Rustで再実装する」)と**部分的に不正確**
  だった点を訂正する——正しくは「Cosmo**本体**は元々OSS、
  RPoemが独自に再実装すべき価値があるのは**Enterprise限定機能
  (SCIM/SSO)のみ**」ということになる。RPoem側は既に
  `open-runo-scim`(SCIM 2.0のOSS実装)を持っており、この訂正された
  理解と実際に合致していることを確認した——**設計方針の変更は不要**、
  記述の正確性を高める訂正のみ。
- aruaru-db側の「REST API不要」というユーザー要求は、Cosmoの
  VersionlessAPI/GraphQL Federationという**コンセプト**を参考にする
  という既存方針の延長にあり、Cosmo自体(パッケージ)への直接依存は
  元から無い(このリポジトリはGraphQLを`async-graphql`で自前実装)。
  今回の調査で新たにCosmoへの直接依存が必要になる発見は無かった。
- **open-cuda/open-directxとの連携性・必要性の再確認**: 両リポジトリの
  `CLAUDE.md`を実際に`grep`したところ「REST API」「APIキー」への言及が
  一件も無いことを確認した。両者はGPU計算ライブラリ(SIMD/CUDA/DirectX
  抽象化)であり、そもそもHTTPサーバー・APIキー認証面を一切持たない
  ——今回のREST撤廃/APIキー自動管理というテーマは、この2リポジトリには
  **適用対象が構造的に存在しない**(既存の「open-cuda + aruaru-llm SET」
  というAI連携の位置づけ〈open-raid-z/CLAUDE.md参照〉から変更なし)。
  open-web-server/open-raid-zについては、この2リポジトリ自体が既に
  RPoemの`KeyGuardian`設計を参照実装として認識している(open-raid-z
  CLAUDE.mdの「分身の術」節参照)ため、今回新たに調査すべき未知の
  連携ギャップは見つからなかった——結論として、今回のAPIキー自動管理・
  REST撤廃方針の実装対象は引き続き「HTTP管理APIを実際に持つ
  リポジトリ(aruaru-db・RPoem・open-web-server・open-easy-web等)」に
  限定され、GPU計算ライブラリ(open-cuda/open-directx)は対象外という
  既存の理解が正しいことを再確認した。

- 次にすべきこと: (1) `multi-raft`(split/merge/scatter-query)・
  `sharded-store`・`closed-timestamp`・`wal-service`・`object-table`・
  `ephemeral-query`・`registry`(crawl/test-connection)・`keys`
  (revoke/status)のGraphQL化(前回エントリから継続、未着手のまま)、
  (2) `parallel_config`をGraphQL化する場合は、まずスキーマの破壊的
  変更(`ParallelConfigGql`をREST実体に合わせて再設計)の是非をユーザー
  へ確認してから着手すること(無理な変換は行わない)、(3) RPoem/
  open-raid-zのCLAUDE.mdにある「Cosmo有料版機能の再実装」という記述の
  精度向上(「Cosmo本体はOSS、再実装対象はEnterprise限定のSCIM/SSOの
  み」への訂正)を横展開するか検討。
