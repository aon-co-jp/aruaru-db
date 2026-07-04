<?php
/**
 * aruaru-DB PHP ドライバー
 *
 * パッケージ名: aruaru-db/aruaru-db-php (Composer)
 * composer require aruaru-db/aruaru-db-php
 * 内部依存: ext-pdo_pgsql
 *
 * aruaru-DB は PostgreSQL ワイヤ互換のため PDO_PGSQL がそのまま使えます。
 * このドライバーは Git-on-SQL 操作を型付き API で包むラッパーです。
 *
 * 使用例:
 *   $db = new AruaruDB\AruaruDb();
 *   $db->branch('feature/php-test');
 *   $db->execute("CREATE TABLE tasks (id INT, title TEXT)");
 *   echo $db->commit('Add tasks via aruaru-db-php');
 */

namespace AruaruDB;

class AruaruDb
{
    private \PDO $pdo;

    /**
     * @param string $host     ホスト名 (既定: 'localhost')
     * @param int    $port     ポート番号 (既定: 5432)
     * @param string $db       データベース名 (既定: 'aruaru')
     * @param string $user     ユーザー名 (既定: 'root')
     * @param string $password パスワード
     */
    public function __construct(
        string $host     = 'localhost',
        int    $port     = 5432,
        string $db       = 'aruaru',
        string $user     = 'root',
        string $password = ''
    ) {
        $dsn = "pgsql:host={$host};port={$port};dbname={$db}";
        $this->pdo = new \PDO($dsn, $user, $password, [
            \PDO::ATTR_ERRMODE            => \PDO::ERRMODE_EXCEPTION,
            \PDO::ATTR_DEFAULT_FETCH_MODE => \PDO::FETCH_ASSOC,
        ]);
    }

    /** DSN 文字列から接続する */
    public static function fromDsn(string $dsn, string $user = 'root', string $password = ''): self
    {
        $instance = new self();
        $instance->pdo = new \PDO($dsn, $user, $password, [
            \PDO::ATTR_ERRMODE            => \PDO::ERRMODE_EXCEPTION,
            \PDO::ATTR_DEFAULT_FETCH_MODE => \PDO::FETCH_ASSOC,
        ]);
        return $instance;
    }

    // ── Git-on-SQL ──────────────────────────────────────────

    /** ブランチを作成する */
    public function branch(string $name): void
    {
        $this->pdo->prepare("SELECT aruaru_branch(?)")->execute([$name]);
    }

    /** ブランチを切り替える */
    public function checkout(string $name): void
    {
        $this->pdo->prepare("SELECT aruaru_checkout(?)")->execute([$name]);
    }

    /** 現在のブランチ名を返す */
    public function currentBranch(): string
    {
        return $this->pdo->query("SELECT aruaru_current_branch()")->fetchColumn();
    }

    /** コミットしてコミット ID を返す */
    public function commit(string $message): ?string
    {
        $st = $this->pdo->prepare("SELECT aruaru_commit(?) AS commit_id");
        $st->execute([$message]);
        return $st->fetchColumn() ?: null;
    }

    /** fast-forward マージしてコミット ID を返す */
    public function merge(string $fromBranch): ?string
    {
        $st = $this->pdo->prepare("SELECT aruaru_merge(?) AS commit_id");
        $st->execute([$fromBranch]);
        return $st->fetchColumn() ?: null;
    }

    /** コミットログを取得する */
    public function log(int $limit = 20): array
    {
        $st = $this->pdo->prepare(
            "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT ?"
        );
        $st->execute([$limit]);
        return $st->fetchAll();
    }

    // ── 汎用 SQL ─────────────────────────────────────────────

    /** SQL を実行して変更行数を返す */
    public function execute(string $sql, array $params = []): int
    {
        $st = $this->pdo->prepare($sql);
        $st->execute($params);
        return $st->rowCount();
    }

    /** SELECT を実行して行の配列を返す */
    public function query(string $sql, array $params = []): array
    {
        $st = $this->pdo->prepare($sql);
        $st->execute($params);
        return $st->fetchAll();
    }

    /** 生の PDO インスタンスを返す (高度な用途向け) */
    public function pdo(): \PDO { return $this->pdo; }
}
