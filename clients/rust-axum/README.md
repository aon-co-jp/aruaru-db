# Rust + Axum + sqlx (async)

aruaru-db を**標準の sqlx(PgPool)** でそのまま使う非同期サンプル。
**Poem / RPoem** でも同じ `PgPool` をハンドラで受け取るだけ(コードの
DB 部分は変わらない)。同期版は `postgres` crate の `Client::connect`
へ置換(ワイヤ形式は同一、速度差なし)。

## Cargo.toml(抜粋)

```toml
axum = "0.7"
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres"] }
serde_json = "1"
anyhow = "1"
```

## 実行

```bash
export ARUARU_DB_DSN="postgres://app:secret@localhost:5433/app?sslmode=require"
cargo run   # :8000 で待受
```

## エンドポイント

`POST /items/:id?qty=5&message=...`(UPSERT → `aruaru_commit`)/
`GET /items/:id`(最新)/ `GET /items/:id/at/:commit`(`AS OF COMMIT`)。

## 検証状況

**2026-09-06: 実際にビルド可能なプロジェクトとして組み立て、WSL2
Ubuntu(Linux)上で実サーバへ接続する検証を行い、3件の実バグを発見・
修正した**(この例は2026-09-03の新設時点では一度もビルド・実行された
ことが無かった):

1. `pool.begin()`(明示トランザクション)が`BeginFailed`で失敗し
   後続リクエストで接続状態が異常化 → 他の全コネクタと同じ
   「明示的トランザクションでラップしない」設計へ撤去。
2. `get_latest`/`get_as_of`が`Option<i32>`で列デコードして
   `ColumnDecode`エラー(コメントは正しく「Stringで受ける」と
   書かれていたが実装が追従していなかった) → `Option<String>`→
   `i32`パースへ修正。
3. `get_as_of`が`AS OF COMMIT`をバインドパラメータとして渡そうと
   していた(aruaru-wireは非対応) → `is_safe_commit_id`検証後の
   安全な文字列連結へ修正。

修正後、実サーバ(`aruaru-server`)へ`upsert→commit→最新値取得→再
upsert`の一連を実行し正しいJSONが返ることを確認した。**未検証のまま
残る部分**: `get_as_of`(`AS OF COMMIT`往復)自体の最終確認は、この
検証中に発見した`aruaru-wire`側の別バグ(グローバルなトランザクション
状態が全接続で共有される設計欠陥、`../../CLAUDE.md`2026-09-06 HANDOFF
参照)により最後まで到達できなかった——次回、そのバグ修正後に
再確認すること。
