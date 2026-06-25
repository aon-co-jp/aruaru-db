<?php
// aruaru-DB PHP Client
// composer require aruaru-db/client
// 内部依存: ext-pgsql / ext-pdo_pgsql

namespace AruaruDB;

class Client
{
    private \PDO $pdo;

    public function __construct(
        string $host = 'localhost',
        int    $port = 5432,
        string $db   = 'aruaru',
        string $user = 'root',
        string $pass = ''
    ) {
        $dsn = "pgsql:host={$host};port={$port};dbname={$db}";
        $this->pdo = new \PDO($dsn, $user, $pass, [
            \PDO::ATTR_ERRMODE => \PDO::ERRMODE_EXCEPTION,
        ]);
    }

    public function branch(string $name): void
    {
        $this->pdo->prepare("SELECT aruaru_branch(?)")->execute([$name]);
    }

    public function commit(string $message): ?string
    {
        $st = $this->pdo->prepare("SELECT aruaru_commit(?) as commit_id");
        $st->execute([$message]);
        return $st->fetchColumn() ?: null;
    }

    public function log(int $limit = 20): array
    {
        $st = $this->pdo->prepare(
            "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT ?"
        );
        $st->execute([$limit]);
        return $st->fetchAll(\PDO::FETCH_ASSOC);
    }

    public function exec(string $sql, array $params = []): int
    {
        $st = $this->pdo->prepare($sql);
        $st->execute($params);
        return $st->rowCount();
    }
}

// ── 使用例 ──────────────────────────────────────────────────
// $db = new AruaruDB\Client();
// $db->branch('feature/php-test');
// $db->exec("CREATE TABLE IF NOT EXISTS sessions (id SERIAL PRIMARY KEY, token TEXT)");
// $commitId = $db->commit('Add sessions table');
// echo "Committed: $commitId\n";
// foreach ($db->log(5) as $row) print_r($row);
