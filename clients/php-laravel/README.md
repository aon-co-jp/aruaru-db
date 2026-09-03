# PHP + Laravel + PDO_pgsql (sync)

aruaru-db を Laravel の**標準 `pgsql` 接続**でそのまま使う。独自ドライバ
なし。PHP は基本同期。非同期が要るなら Swoole/AMPHP + `pgsql` 拡張。

## .env

```
DB_CONNECTION=pgsql
DB_HOST=127.0.0.1
DB_PORT=5433
DB_DATABASE=app
DB_USERNAME=app
DB_PASSWORD=secret
```

`config/database.php` の `connections.pgsql` は Laravel 標準のまま。
`php.ini` で `extension=pdo_pgsql` を有効化。

## マイグレーション（`database/migrations/xxxx_create_items.php`）

```php
Schema::create('items', function (Blueprint $t) {
    $t->string('id')->primary();
    $t->integer('qty');
});
```

## コントローラ（`app/Http/Controllers/ItemController.php`）

```php
use Illuminate\Support\Facades\DB;

public function upsertAndCommit(string $id, int $qty, string $message = 'api write')
{
    DB::table('items')->updateOrInsert(['id' => $id], ['qty' => $qty]);
    // Git-on-SQL commit（生 SQL）
    $commit = DB::selectOne('SELECT aruaru_commit(?) AS c', [$message])->c;
    return ['id' => $id, 'qty' => $qty, 'commit_id' => $commit];
}

public function getAsOf(string $id, string $commit)
{
    // VersionlessAPI: 過去コミット時点
    $qty = DB::selectOne(
        'SELECT qty FROM items WHERE id = ? AS OF COMMIT ?',
        [$id, $commit]
    )?->qty;
    return ['id' => $id, 'as_of' => $commit, 'qty' => $qty];
}
```

Eloquent の通常 CRUD(`Item::find`, `->save()` 等)もそのまま動く。
`AS OF COMMIT` だけは `DB::select` の生 SQL で。

## 検証状況

**未検証**（この開発環境に PHP 未導入）。接続はポート 5433 の Laravel
標準 `pgsql` 接続のみ。
