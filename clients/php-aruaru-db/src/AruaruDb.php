<?php
declare(strict_types=1);

namespace Aruaru\Db;

/**
 * aruaru-db 公式 PHP コネクタ(薄いラッパー) / official PHP connector.
 *
 * 独自の PostgreSQL ドライバではない。標準の PDO(pgsql)をそのまま使い、
 * その上に aruaru-db の Git-on-SQL 機能を PHP の慣用 API で足すだけ。
 * Laravel / Symfony / 素の PHP、どれでも同じ。
 *
 * 正本: ../../docs/CLIENTS.md
 */
final class AruaruDb
{
    public function __construct(private \PDO $pdo)
    {
        $this->pdo->setAttribute(\PDO::ATTR_ERRMODE, \PDO::ERRMODE_EXCEPTION);
    }

    /** DSN 例: "pgsql:host=localhost;port=5433;dbname=app;sslmode=require" */
    public static function connect(string $dsn, string $user, string $password): self
    {
        return new self(new \PDO($dsn, $user, $password));
    }

    /** Laravel の DB ファサードなど、既存の PDO を包む。 */
    public static function fromPdo(\PDO $pdo): self
    {
        return new self($pdo);
    }

    public function pdo(): \PDO
    {
        return $this->pdo;
    }

    /** commit_id が `AS OF COMMIT '<id>'` のリテラルとして安全か。 */
    public static function isSafeCommitId(string $id): bool
    {
        return $id !== '' && \strlen($id) <= 128 && \preg_match('/\A[A-Za-z0-9_-]+\z/', $id) === 1;
    }

    /** 透過: PDO::exec 相当。 */
    public function execute(string $sql, array $params = []): void
    {
        $this->pdo->prepare($sql)->execute($params);
    }

    /** 透過: 全行。 */
    public function fetchAll(string $sql, array $params = []): array
    {
        $st = $this->pdo->prepare($sql);
        $st->execute($params);
        return $st->fetchAll(\PDO::FETCH_ASSOC);
    }

    /** 透過: 単一値。 */
    public function fetchValue(string $sql, array $params = []): mixed
    {
        $st = $this->pdo->prepare($sql);
        $st->execute($params);
        return $st->fetchColumn();
    }

    /** Git-on-SQL: 全テーブルをスナップショットし commit_id を返す。 */
    public function commit(string $message): string
    {
        $cid = $this->fetchValue('SELECT aruaru_commit(?)', [$message]);
        if ($cid === false || $cid === null) {
            throw new \RuntimeException('aruaru_commit() returned no commit id');
        }
        return (string) $cid;
    }

    /**
     * VersionlessAPI: $baseSelect を過去のコミット時点で読む。
     * $baseSelect は `AS OF COMMIT` を含まない通常の SELECT。
     * commit_id は isSafeCommitId で検証し、非安全なら InvalidArgumentException
     * (ネットワーク前に弾く、SQL インジェクション防止)。
     */
    public function queryAsOf(string $baseSelect, string $commitId, array $params = []): array
    {
        return $this->fetchAll($this->asOfSql($baseSelect, $commitId), $params);
    }

    public function queryAsOfValue(string $baseSelect, string $commitId, array $params = []): mixed
    {
        return $this->fetchValue($this->asOfSql($baseSelect, $commitId), $params);
    }

    private function asOfSql(string $baseSelect, string $commitId): string
    {
        if (!self::isSafeCommitId($commitId)) {
            throw new \InvalidArgumentException(
                "commit id '{$commitId}' is not a safe literal (expected hex / [A-Za-z0-9_-], <=128 chars)"
            );
        }
        return \rtrim($baseSelect) . " AS OF COMMIT '{$commitId}'";
    }
}
