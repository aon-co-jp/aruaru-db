# aruaru-db 公式 Mojo コネクタ(薄いラッパー) / Official Mojo Connector

**これは独自の PostgreSQL ドライバでも、独自ワイヤプロトコル実装でもない。**
Mojo(Modular)は本稿執筆時点(2026年)でもまだ若いシステムプログラミング
言語であり、成熟したネイティブ PostgreSQL ドライバのエコシステムを持たない。
一方 Mojo は `from python import Python` による強力な Python 相互運用
(CPython 埋め込み)を持つ ── このコネクタは、既存の公式 Python コネクタ
[`clients/python-aruaru-db/`](../python-aruaru-db/)(それ自体は標準
`asyncpg`/`psycopg` の薄いラッパー)を、この相互運用層経由でそのまま呼び
出すだけの**さらに薄い層**である。

This is **not** a custom PostgreSQL driver, nor a from-scratch wire-protocol
implementation. As of this writing (2026), Mojo is still a young systems
language without a mature native PostgreSQL driver ecosystem. What Mojo
does have is strong Python interoperability (`from python import Python`,
CPython embedding) — so this connector is an even-thinner layer that calls
the existing official Python connector
([`clients/python-aruaru-db/`](../python-aruaru-db/), itself a thin wrapper
over standard `asyncpg`/`psycopg`) through that interop boundary.

このリポジトリの他の全コネクタ(Rust の `tokio-postgres`、Go の
`jackc/pgx/v5`、Java の JDBC 等)と同じ「標準ドライバ + 薄いラッパー」
という設計哲学を踏襲している ── 唯一の違いは、Mojo の場合その「標準
ドライバ」への経路が Python 相互運用を1回経由する点だけである。詳細・
背景は [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)(正本)を参照。

This follows the exact same "standard driver, thin wrapper" philosophy as
every other connector in this repo (Rust's `tokio-postgres`, Go's
`jackc/pgx/v5`, Java's JDBC, ...) — the only difference is that Mojo's path
to a "standard driver" happens to go through one Python interop hop. See
[`../../docs/CLIENTS.md`](../../docs/CLIENTS.md) (source of truth) for
background.

## なぜ Mojo から独自にワイヤプロトコルを実装しないのか / Why not a native wire-protocol implementation

- Mojo 自体にはまだ実運用で広く使われている PostgreSQL クライアント
  ライブラリが無い(2026年時点の学習データに基づく判断。将来これが変われば
  ネイティブドライバへの移行を再検討すべき)。
- `CLAUDE.md` のリポジトリ横断方針「闇雲な代替を避ける」原則により、
  何年もセキュリティ監査を受けてきた `asyncpg`/`psycopg` を再実装するより、
  Mojo の Python 相互運用でそれらへ委譲する方が誠実で低リスク。
- 一方で commit_id の安全性検証(`is_safe_commit_id`)だけは Python 相互運用
  へ入る前に **Mojo ネイティブ**で行う ── 他の全コネクタが「ネットワークに
  触れる前にローカルでバリデーションする」のと同じ設計(SQL インジェクション
  防止をドライバや相互運用層の正しさに一切依存させない)。

## セットアップ / Setup

Mojo が埋め込む Python 環境(`magic`/`pixi` の venv、または `mojo` が
参照する Python)に、公式 Python コネクタとその依存を入れる:

```sh
# aruaru_db パッケージ(clients/python-aruaru-db/)を Mojo の Python 環境へ
pip install -e ../python-aruaru-db
# 同期経路(AruaruDbSync、既定)が使う psycopg
pip install "psycopg[binary]"
# 非同期経路(aruaru_db.AruaruDb、raw() 経由で使う場合)が使う asyncpg
pip install asyncpg
```

`aruaru_db.mojo` を自分のプロジェクトへコピーするか、`clients/mojo-aruaru-db/`
をそのまま `import` パスへ加える。

## 使い方 / Usage

```mojo
from aruaru_db import AruaruDb, is_safe_commit_id

fn run() raises:
    var db = AruaruDb.connect(
        "host=localhost port=5433 dbname=app user=app password=secret"
    )

    db.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)")
    var first = db.commit("first import")

    db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'")
    _ = db.commit("restock")

    # VersionlessAPI: 過去のコミット時点を読む(最新は 5、これは 1)。
    # commit_id は Mojo ネイティブに is_safe_commit_id で検証してから
    # Python 層(→ psycopg → pgwire)へ渡る。
    var old = db.query_as_of_val(
        "SELECT qty FROM items WHERE id = 'sword'", first
    )
    print(old)  # "1" — aruaru-wire は結果列を常に VARCHAR(text) で返す
```

`commit()` は `SELECT aruaru_commit('message')` を実行して commit_id を
返す。`query_as_of()`/`query_as_of_val()` は、`AS OF COMMIT` を含まない
普通の `base_select` を受け取り、Mojo 側でネイティブに `is_safe_commit_id`
で `commit_id` を検証してから ` AS OF COMMIT '<id>'` を安全に付与する ──
aruaru-wire は `AS OF COMMIT` 句をバインドパラメータとして受け付けないため、
このリポジトリの全コネクタが共通して行っている文字列連結前の検証を、
ここでも同じ正規表現ルール(英数字 + `-` `_`、1〜128 文字)で行う。

非同期(asyncpg 経由の `aruaru_db.AruaruDb`)が必要な場合は `db.raw()` で
得られる `AruaruDbSync` の代わりに、Python 側を直接 import して使う:

```mojo
from python import Python

fn run_async_example() raises:
    var aruaru_db_mod = Python.import_module("aruaru_db")
    # 以降は Python 側の asyncio イベントループの管理が必要
    # (Mojo の async ランタイムとの橋渡しは本コネクタの対象外)。
```

## 接続文字列 / Connection strings

libpq 形式・`postgresql://` URL のどちらも `psycopg`/`asyncpg` がそのまま
解釈するため、[`../../docs/CLIENTS.md`](../../docs/CLIENTS.md) §2 の表を
そのまま使える。既定ポートは pgwire = `5433`。

## 検証状況(誇張しない) / Verification status (no exaggeration)

**2026-09-05追記**: 当初「この環境に`mojo`コマンド自体が無い」ため
未検証としていたが、その後 **WSL2 Ubuntu + `pixi`(Modular公式インストーラ、
`curl -fsSL https://pixi.sh/install.sh | sh`)経由で実際にMojo 1.0.0
コンパイラを導入し、実機でビルド・実行検証を行った**(ネットから
入手できるかというユーザー確認への対応)。結果は以下の通り:

1. **Mojo 1.0への言語仕様変更に伴う構文更新が必要だった(修正済み)**:
   Mojo 1.0で`fn`キーワードが廃止され`def`のみに、`let`(不変束縛)が
   廃止され`var`に統一、コンストラクタの`inout self`が`out self`へ、
   String型が「UTF-8の曖昧性」を理由に`len(s)`/`s[i]`を廃止し
   `s.byte_length()`/`s[byte=i]`等の明示アクセサへ変更されていた
   ——本ファイル・`test_aruaru_db.mojo`とも実機のコンパイルエラー
   メッセージを見ながら追従修正済み。
2. **`is_safe_commit_id`(ネットワーク不要のコア安全性検証ロジック)は
   実際にMojo 1.0でコンパイル・実行に成功した**——上記の構文修正のみを
   抜き出した最小ファイルを`mojo run`で実行し、
   `"aruaru_db.mojo: is_safe_commit_id self-check passed"`という
   正しい出力を実機で確認済み。
3. **ただし`aruaru_db.mojo`/`test_aruaru_db.mojo`本体は、
   `from python import Python`(Python相互運用)が解決できないため
   依然ビルド不可能** ── 実際に`mojo build`を実行したところ
   `error: unable to locate module 'python'`(および`testing`モジュールも
   同様)。`conda.modular.com/max`チャンネル配布のMojo 1.0.0
   (`mojo`/`mojo-compiler`/`mojo-python`の各conda パッケージ)を
   実際に展開してファイル一覧を確認したところ、Python相互運用の
   標準ライブラリモジュール自体がこの配布物に含まれていないことを
   確認した(`mojo-python`パッケージは逆方向——PythonからMojoを呼ぶ
   ための`mojo`パッケージであり、本コネクタが必要とする
   「MojoからPythonを呼ぶ」向きの相互運用ではなかった)。異なる
   channel構成(`modular-community`等を追加)でも同じ結果だった。
   **これは本コネクタの設計判断(Python相互運用でPythonコネクタへ
   委譲する)自体が誤りだったという意味ではなく、この配布チャンネル・
   このバージョン(1.0.0)にPython相互運用モジュールが同梱されて
   いない、という実機確認済みの制約**——Mojoの旧バージョン
   (プレ1.0、`from python import Python`が公式に文書化されていた
   時期)や、将来Python相互運用が復活/別パッケージとして提供された
   場合は、本ファイルの構文修正済みの土台がそのまま活きる見込み。

**2026-09-06追記: 旧バージョン(0.26.2.0)で完全に動作確認できた**——
`pixi search -c https://conda.modular.com/max mojo`でこのチャンネルには
1.0.0以外に`1.0.0b2`/`1.0.0b1`/`0.26.2.0`等の旧リリースも配布されて
いることを確認し、`pixi add "mojo==0.26.2.0"`で実際に導入したところ、
**Python相互運用が正しく機能した**(`from python import Python`は
`Implicit standard library imports are deprecated`という将来的な
非推奨警告のみでコンパイル・実行に成功、`Python.import_module("sys")`
経由で実際にホストのPythonバージョン文字列を取得できた)。

続けて`pixi add python`+`pip install "psycopg[binary]"`+
`pip install -e clients/python-aruaru-db`でPython側の依存を導入し、
**`aruaru_db.mojo`/`test_aruaru_db.mojo`本体を実際にコンパイル・
実行**:

```
$ mojo run test_aruaru_db.mojo
test_live_commit_and_as_of_round_trip: skipped (set ARUARU_DB_TEST_DSN to run)
test_aruaru_db.mojo: all checks passed
```

(ネットワーク不要テスト4件——`is_safe_commit_id`受理/拒否/過長ID拒否・
`query_as_of`の事前拒否——全てpassed)。

**さらに実 `aruaru-server` を起動し `ARUARU_DB_TEST_DSN` を設定した
上で再実行したところ、実サーバ往復まで成功した**:

```
$ ARUARU_DB_TEST_DSN="host=172.22.0.1 port=5433 user=app password=secret dbname=aruaru" mojo run test_aruaru_db.mojo
test_aruaru_db.mojo: all checks passed
```

(`skipped`メッセージが出なくなり、`test_live_commit_and_as_of_round_
trip`本体——`AruaruDb.connect`→`commit`→`AS OF COMMIT`往復——が
実際に実行され成功したことを確認。WSL2からWindowsホストのポートへは
デフォルトゲートウェイのIPアドレス〈`ip route`の`default`行〉経由で
到達できた)。

**まとめ(誇張しない)**: 「Mojoはこの開発機にインストールできない」
という当初の判断は誤りで、**WSL2経由で実際にインストール・実行
できた**。当初1.0.0チャンネルで「Python相互運用モジュールが無い」と
判明した後、**同じconda.modular.com/maxチャンネルの旧バージョン
(0.26.2.0)を導入することでPython相互運用が実際に機能し、
このコネクタは設計通りにネットワーク不要テスト・実サーバ往復の
両方で動作することを実機で確認できた**。1.0.0系での動作は依然未確認
だが、実用上は0.26.2.0のように`from python import Python`を持つ
バージョンを`pixi add "mojo==0.26.2.0"`で指定すれば問題なく使える。

**次に必要な作業(この README を読んだ人向け)**:

1. Mojo 1.0.0系で改めてPython相互運用が復活/提供されるようになった
   場合の再検証(現状は0.26.2.0系での動作確認に留まる)。
2. CI/配布物としてどちらのMojoバージョンを推奨とするか(0.26.2.0の
   ような旧バージョンへピン留めするか、Python相互運用復活を待つか)の
   方針検討。
