# COBOL から aruaru-db へ(埋め込み SQL、独自ドライバなし)

aruaru-db は **pgwire(PostgreSQL 互換)** なので、COBOL からは標準の 3 経路の
いずれかでそのまま繋がる。専用ドライバ・専用インストーラは無い。

| 経路 | 入れるもの | 対応 OS |
|---|---|---|
| **(1) ODBC** | **psqlODBC**(`psqlodbc`)を DSN 登録 + `EXEC SQL` を ODBC 対応プリコンパイラ(Micro Focus COBOL / OpenCOBOL ESQL(ODBC 版))で処理 | Windows(ODBC データソース)/ Linux・UNIX(unixODBC + psqlODBC)/ **IBM z/OS**(USS の unixODBC + psqlODBC、または DB2 Federation 経由) |
| **(2) libpq 直呼び** | PostgreSQL の `libpq`。GnuCOBOL から `CALL "PQconnectdb"` / `CALL "PQexec"` 等 | libpq が動く全 OS(Win/macOS/Linux/UNIX。z/OS は USS ビルドの libpq) |
| **(3) OCESQL** | [open-cobol-esql](https://github.com/opensourcecobol/Open-COBOL-ESQL)(`ocesql`)。`EXEC SQL` → libpq へ変換 | GnuCOBOL が動く OS |

`ARUARU.cob` は (1)/(3) 共通の埋め込み SQL 形。ポートは **5433**。
Git-on-SQL(`SELECT aruaru_commit(:msg) INTO :h`、`... AS OF COMMIT :h`)は
**ただの SQL** なのでホスト変数にバインドするだけ。

## プリコンパイル例(GnuCOBOL + OCESQL)

```bash
ocesql ARUARU.cob ARUARU.cbl
cobc -x -locesql -lpq ARUARU.cbl -o aruaru
./aruaru
```

## Micro Focus COBOL(Windows / z/OS）

`EXEC SQL` を Micro Focus の OpenESQL プリプロセッサで処理し、ODBC
データソース(psqlODBC、port 5433)を割り当てる。COBOL 側のコードは
`ARUARU.cob` と同じ。

## 検証状況

**未検証**(この開発環境に COBOL コンパイラ / psqlODBC 未導入)。埋め込み
SQL は標準 `EXEC SQL` 構文のみで、aruaru-db 固有の作法は「ポート 5433」と
「Git-on-SQL 関数」だけ。`AS OF COMMIT` にホスト変数を使う点は
`../../docs/CLIENTS.md` §5(拡張プロトコル対応)と整合。
