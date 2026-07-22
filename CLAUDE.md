# 開発方針・開発環境ルール(全リポジトリ共通ヘッダー、2026-07-15追記)

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

## エコシステム全体マップ(2026-07-21追記)

同時並行開発の対象プロジェクト一覧・各リポジトリの現況は
[`open-raid-z`のCLAUDE.md](https://github.com/aon-co-jp/open-raid-z/blob/main/CLAUDE.md)
「関連プロジェクト」節を参照。**どのリポジトリから読み始めても、
この節を起点に他プロジェクトへ辿れる**ようにしてある(新規追加:
RS-Git・RS-JSON・RS-Chiketto・RS-Blog・RS-EC。このリポジトリ自身の状況は
このファイルの他の節・HANDOFFを参照)。
