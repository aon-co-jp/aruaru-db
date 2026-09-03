# aruaru-db 公式 Java コネクタ / official Java connector

**独自の PostgreSQL ドライバではない。** 標準の PostgreSQL JDBC ドライバ
(純 Java Type-4)をそのまま使い、その上に aruaru-db の Git-on-SQL 機能
(`SELECT aruaru_commit('msg')` と `... AS OF COMMIT '<id>'`)を Java の
慣用 API で足しただけの薄い層。Windows / macOS / Linux / UNIX /
**IBM z/OS** / Android で同じ jar が動く。Spring Boot(同期)・
Quarkus・素の JDBC どれでも使える。

`../java-jdbc/` にあった「依存無しの最小サンプル(`Example.java` 単体)」
を、この `java-aruaru-db`(Maven パッケージ `tokyo.aon.aruaru:
aruaru-db-connector`、テスト付き)へ統合した。`java-jdbc/` は引き続き
「依存無しで動かす最小コピペ例」として残置(移植先が Maven/Gradle を
使えない環境向け)。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## 使い方

```java
try (AruaruDb db = AruaruDb.connect(
        "jdbc:postgresql://localhost:5433/app", "app", "secret")) {
    db.execute("INSERT INTO items(id, qty) VALUES ('sword', 1)");
    String first = db.commit("first import");

    db.execute("UPDATE items SET qty = 5 WHERE id = 'sword'");
    db.commit("restock");

    // VersionlessAPI: 過去のコミット時点を読む
    try (ResultSet rs = db.queryAsOf("SELECT qty FROM items WHERE id = 'sword'", first)) {
        rs.next();
        System.out.println(rs.getString(1)); // "1"
    }
}
```

`commit()` は結果列を列名(`aruaru_commit`、`AS alias` 不可)ではなく
先頭列の位置(JDBC の 1 始まりで `getString(1)`)で読む。`queryAsOf` の
`commitId` は `isSafeCommitId`(英数字 + `-` `_`、≤128 文字)で検証して
から文字列として `AS OF COMMIT` へ埋め込む(SQL インジェクション対策、
バインドパラメータ非対応のため)。結果列は現状すべて VARCHAR(text)。

## ビルド・実行

```bash
mvn -f clients/java-aruaru-db/pom.xml test   # ユニットテスト(ネットワーク不要分)
```

## 検証状況

**未検証**——この開発環境に Maven(`mvn`)/JDK が存在しないため、
`mvn test` 自体を実行できていない。ネットワーク不要なユニットテスト
(`isSafeCommitId`・不正 commit_id の即時拒否)を `AruaruDbTest.java` に
用意してあるので、Maven が使える環境で確認すること。実サーバ往復は
未実施(この環境に到達可能な aruaru-server もこのパスでは未起動)。
設計・API 形状は同一パターンの [`rust-aruaru-db`](../rust-aruaru-db)・
[`node-aruaru-db`](../node-aruaru-db)(いずれも 2026-09-03 実サーバ
往復検証済み)に倣っている。拡張プロトコル(`PreparedStatement`)は
aruaru-wire 側で 2026-07-14 に対応済み。

---

# English

**NOT a custom PostgreSQL driver.** Uses the standard PostgreSQL JDBC
driver (pure Java Type-4) as-is, adding a thin idiomatic layer for
aruaru-db's Git-on-SQL surface. The same jar runs unchanged on Windows /
macOS / Linux / UNIX / IBM z/OS / Android.

Consolidates the dependency-free example previously living only in
`../java-jdbc/` into this Maven package (`tokyo.aon.aruaru:
aruaru-db-connector`, with tests); `java-jdbc/` remains as the minimal
copy-paste snippet for environments without Maven/Gradle.

Source of truth: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md).

`commit()` reads the result column by **position** (index 1, not by
name — `AS alias` is not honored server-side). `queryAsOf`'s `commitId`
is validated with `isSafeCommitId` before being interpolated into the SQL
text. Result columns are all VARCHAR/text currently.

**Verification status: not verified in this session** — no Maven/JDK is
available in this development environment, so `mvn test` could not be
run here. `AruaruDbTest.java` covers the network-free unit tests; run
them wherever Maven is available. No live server round-trip was
performed either. Design mirrors [`rust-aruaru-db`](../rust-aruaru-db)
and [`node-aruaru-db`](../node-aruaru-db) (both live-verified
2026-09-03).
