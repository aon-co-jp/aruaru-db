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

**この開発環境には `mojo` コマンド(Modular の Mojo コンパイラ/インタプリタ)
も `magic`/`modular` CLI も導入されていない** ── `which mojo` / `mojo
--version` / `which magic` / `which modular` をすべて実行し、いずれも
未検出であることを確認済み。したがって:

- `aruaru_db.mojo` / `test_aruaru_db.mojo` は **この環境で一度もコンパイル・
  実行されていない**。Mojo の言語知識(構造体・`fn`/`raises`・Python
  相互運用の `from python import Python` / `PythonObject` API)に基づき
  可能な限り正確に書いたが、構文エラー・API の記憶違い(特に Mojo は
  現在も言語仕様が変化し続けている)が残っている可能性を否定できない。
- 実サーバ往復テスト(`test_live_commit_and_as_of_round_trip`)は
  `ARUARU_DB_TEST_DSN` 環境変数と、Mojo の Python 環境への `aruaru_db`/
  `psycopg[binary]` の導入の両方を前提とするため、この環境ではなおさら
  実行できていない。
- これは他の新設コネクタ(`clients/go-aruaru-db/`、
  `clients/java-aruaru-db/`、`clients/ruby-aruaru-db/`)がそれぞれの
  README で「この環境にツールチェーンが無いため未検証」と正直に記載して
  いるのと同じ状況・同じ誠実さの基準に従っている。

**次に必要な作業(この README を読んだ人向け)**:

1. `mojo`/`magic` が使える環境で `mojo build aruaru_db.mojo`(または
   `mojo run test_aruaru_db.mojo`)を実行し、コンパイルエラーが無いことを
   確認する。
2. 上記セットアップ手順で Python 側の依存(`aruaru_db`、`psycopg[binary]`)
   を導入し、ネットワーク不要のテスト(`is_safe_commit_id` 系3件・
   `query_as_of` の事前拒否1件)が green になることを確認する。
3. 実 `aruaru-server` を起動し `ARUARU_DB_TEST_DSN` を設定した上で
   `test_live_commit_and_as_of_round_trip` を実行し、実際に commit /
   `AS OF COMMIT` の往復ができることを確認する。
