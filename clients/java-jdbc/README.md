# Java + plain JDBC (sync) — R2DBC 非同期例も併記

aruaru-db を**標準の PostgreSQL JDBC ドライバ**でそのまま使う。純 Java
Type-4 なので Windows / macOS / Linux / UNIX / **IBM z/OS** / Android で
同じ jar が動く。独自ドライバなし。

- **同期**: `DriverManager.getConnection("jdbc:postgresql://host:5433/db?ssl=true")`
- **非同期**: R2DBC(`io.r2dbc:r2dbc-postgresql`) — `Example.java` 末尾コメント
- **ハイブリッド**: 同期 JDBC + Java 21 仮想スレッド(Loom)で高並行

## 実行

```bash
# ドライバ jar を取得(Maven Central: org.postgresql:postgresql:42.7.4)
java -cp postgresql-42.7.4.jar Example.java
```

## フレームワーク設定行

| FW | 設定 |
|---|---|
| Spring Boot (同期) | `spring.datasource.url=jdbc:postgresql://localhost:5433/app` |
| Spring WebFlux (非同期) | `spring.r2dbc.url=r2dbc:postgresql://app:secret@localhost:5433/app` |
| Quarkus | `quarkus.datasource.jdbc.url=jdbc:postgresql://localhost:5433/app` |

## 検証状況

**未検証**（この環境に JDBC jar 未導入）。ポート 5433 の標準 JDBC 接続
のみ。拡張プロトコル(`PreparedStatement`)は aruaru-wire 側で 2026-07-14
に対応済み。
