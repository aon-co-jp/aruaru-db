// aruaru-db 公式 .NET コネクタ(薄いラッパー) / official .NET connector.
//
// これは独自の PostgreSQL ドライバではない。業界標準の Npgsql をそのまま
// 使い、その上に aruaru-db の Git-on-SQL 機能(SELECT aruaru_commit('msg')
// と ... AS OF COMMIT '<id>')を .NET の慣用 API(async/await)で足しただけ
// の薄い層。Npgsql は同期 API も持つのでハイブリッドにも使える。
//
// This is NOT a custom PostgreSQL driver. It wraps the standard Npgsql
// driver and only adds idiomatic async helpers for aruaru-db's Git-on-SQL
// surface (aruaru_commit and AS OF COMMIT).
//
// 正本 / source of truth: ../../docs/CLIENTS.md

using System.Text.RegularExpressions;
using Npgsql;

namespace Aruaru.Db;

/// <summary>
/// commit_id が <c>AS OF COMMIT '&lt;id&gt;'</c> のリテラルとして安全でない
/// (SQL インジェクション防止のガード)場合に投げられる。
/// </summary>
public sealed class InvalidCommitIdException : ArgumentException
{
    public InvalidCommitIdException(string id)
        : base($"commit id \"{id}\" is not a safe literal (expected hex / [A-Za-z0-9_-], <=128 chars)")
    {
    }
}

/// <summary>
/// <c>aruaru_commit()</c> が commit_id を返さなかった場合。
/// </summary>
public sealed class NoCommitIdException : InvalidOperationException
{
    public NoCommitIdException() : base("aruaru_commit() returned no commit id")
    {
    }
}

/// <summary>
/// aruaru-db 公式 .NET コネクタ。<see cref="NpgsqlDataSource"/> の薄いラッパー。
/// </summary>
public sealed class AruaruDb : IAsyncDisposable
{
    private static readonly Regex SafeCommitIdPattern = new("^[A-Za-z0-9_-]{1,128}$", RegexOptions.Compiled);

    private readonly NpgsqlDataSource _dataSource;
    private readonly bool _ownsDataSource;

    private AruaruDb(NpgsqlDataSource dataSource, bool ownsDataSource)
    {
        _dataSource = dataSource;
        _ownsDataSource = ownsDataSource;
    }

    /// <summary>
    /// libpq / keyword-value いずれの接続文字列でも可
    /// (例 <c>"Host=localhost;Port=5433;Username=app;Password=secret;Database=app"</c>)。
    /// </summary>
    public static AruaruDb Connect(string connectionString)
    {
        var dataSource = NpgsqlDataSource.Create(connectionString);
        return new AruaruDb(dataSource, ownsDataSource: true);
    }

    /// <summary>既存の <see cref="NpgsqlDataSource"/>(DI 経由等)を包む。呼び出し元が所有権を持つ。</summary>
    public static AruaruDb FromDataSource(NpgsqlDataSource dataSource) => new(dataSource, ownsDataSource: false);

    /// <summary>内部の <see cref="NpgsqlDataSource"/>。透過的に何でもできる。</summary>
    public NpgsqlDataSource DataSource => _dataSource;

    /// <summary>
    /// commit_id が <c>AS OF COMMIT '&lt;id&gt;'</c> のリテラルとして安全か。
    /// aruaru-db の commit_id は英数字 + <c>-</c> <c>_</c>(ハッシュ/UUID 由来)。
    /// </summary>
    public static bool IsSafeCommitId(string? id) => id is not null && SafeCommitIdPattern.IsMatch(id);

    /// <summary>DDL/DML をそのまま実行する薄い透過。</summary>
    public async Task ExecuteAsync(string sql, CancellationToken ct = default)
    {
        await using var cmd = _dataSource.CreateCommand(sql);
        await cmd.ExecuteNonQueryAsync(ct);
    }

    /// <summary>
    /// Git-on-SQL: 現在の全テーブル状態をスナップショットし commit_id を
    /// 返す(<c>SELECT aruaru_commit($1)</c>)。
    ///
    /// <para><b>重要</b>: aruaru-db はこの関数の結果列に <c>AS alias</c>
    /// を効かせない——結果列名は文字通り <c>aruaru_commit</c> になる。
    /// よって列名ではなく <b>位置(0番目)</b> で読む。</para>
    /// </summary>
    public async Task<string> CommitAsync(string message, CancellationToken ct = default)
    {
        await using var cmd = _dataSource.CreateCommand("SELECT aruaru_commit($1)");
        cmd.Parameters.AddWithValue(message);
        var result = await cmd.ExecuteScalarAsync(ct);
        if (result is not string commitId || commitId.Length == 0)
        {
            throw new NoCommitIdException();
        }
        return commitId;
    }

    /// <summary>
    /// VersionlessAPI: <paramref name="baseSelect"/> の結果を過去のコミット
    /// 時点で読む。<paramref name="baseSelect"/> は <c>AS OF COMMIT</c> を
    /// 含まない通常の SELECT。<paramref name="commitId"/> は
    /// <see cref="IsSafeCommitId"/> で検証し(非安全ならネットワークに
    /// 触れる前に <see cref="InvalidCommitIdException"/>)、末尾へ
    /// ` AS OF COMMIT '&lt;id&gt;'` を安全に付与する。
    ///
    /// <para>結果列は現状すべて VARCHAR(text)として返る
    /// (<c>docs/CLIENTS.md</c> §5.1)——型付きの <c>GetInt32</c> 等ではなく
    /// <c>GetString</c> で受けて parse すること。</para>
    /// </summary>
    public async Task<NpgsqlDataReader> QueryAsOfAsync(
        string baseSelect, string commitId, object?[]? parameters = null, CancellationToken ct = default)
    {
        if (!IsSafeCommitId(commitId))
        {
            throw new InvalidCommitIdException(commitId);
        }
        var sql = baseSelect.TrimEnd() + " AS OF COMMIT '" + commitId + "'";
        var cmd = _dataSource.CreateCommand(sql);
        if (parameters is not null)
        {
            foreach (var p in parameters)
            {
                cmd.Parameters.AddWithValue(p ?? DBNull.Value);
            }
        }
        return await cmd.ExecuteReaderAsync(ct);
    }

    public async ValueTask DisposeAsync()
    {
        if (_ownsDataSource)
        {
            await _dataSource.DisposeAsync();
        }
    }
}
