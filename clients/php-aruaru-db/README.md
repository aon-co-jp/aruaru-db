# `aruaru/db` — 公式 PHP コネクタ(薄いラッパー)

**独自の PostgreSQL ドライバではない。** 標準の `PDO`(`pdo_pgsql`)を
そのまま使い、その上に Git-on-SQL を PHP の慣用 API で足すだけ。PHP は
基本同期。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## インストール

```bash
composer require aruaru/db
# php.ini: extension=pdo_pgsql
```

## API

| メソッド | 実体 |
|---|---|
| `AruaruDb::connect($dsn, $user, $pass)` / `AruaruDb::fromPdo($pdo)` | PDO をそのまま |
| `->execute($sql, $params)` / `->fetchAll(...)` / `->fetchValue(...)` | PDO の薄い透過 |
| `->commit($message): string` | `SELECT aruaru_commit(?)` → commit_id |
| `->queryAsOf($baseSelect, $commitId, $params): array` | 末尾へ ` AS OF COMMIT '<id>'` を安全付与。`isSafeCommitId`(英数字+`-``_`、≤128)で検証、非安全は `InvalidArgumentException` |
| `->queryAsOfValue(...)` | 単一値版 |
| `->pdo()` | 内部 PDO(透過) |

## 素の PHP

```php
use Aruaru\Db\AruaruDb;

$db = AruaruDb::connect('pgsql:host=localhost;port=5433;dbname=app', 'app', 'secret');
$db->execute("INSERT INTO items(id,qty) VALUES ('sword',1) ON CONFLICT (id) DO UPDATE SET qty=EXCLUDED.qty");
$first = $db->commit('first import');
$db->execute("UPDATE items SET qty = 5 WHERE id = 'sword'");
$db->commit('restock');
echo $db->queryAsOfValue("SELECT qty FROM items WHERE id = ?", $first, ['sword']); // 1
```

## Laravel

`.env` は標準の `pgsql` 接続(`DB_PORT=5433`)。Eloquent の通常 CRUD は
そのまま。Git-on-SQL だけこのラッパーで、Laravel の PDO を再利用:

```php
use Aruaru\Db\AruaruDb;
use Illuminate\Support\Facades\DB;

$db = AruaruDb::fromPdo(DB::connection()->getPdo());
$commit = $db->commit('after migration');
$old = $db->queryAsOfValue("SELECT qty FROM items WHERE id = ?", $commit, ['sword']);
```

## Symfony(Doctrine DBAL)

`$db = AruaruDb::fromPdo($connection->getNativeConnection());`(DBAL 3.x の
`getNativeConnection()` が PDO を返す)。

## 検証状況

**未検証**(この開発環境に PHP 未導入)。ロジックは PDO の標準 API +
`preg_match` によるバリデーションのみ。接続はポート 5433 の標準 `pgsql`。
