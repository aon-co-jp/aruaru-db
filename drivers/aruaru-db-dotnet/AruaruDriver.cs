// aruaru-DB .NET Client
// NuGet: AruaruDB.Dotnet 0.5.0 (aruaru-db-dotnet)
// 内部依存: Npgsql 9+
// dotnet add package Npgsql

using Npgsql;
using System.Data;

namespace AruaruDB;

public class AruaruClient : IAsyncDisposable
{
    private readonly NpgsqlDataSource _ds;

    public AruaruClient(string host = "localhost", int port = 5432,
        string db = "aruaru", string user = "root")
    {
        var connStr = $"Host={host};Port={port};Database={db};Username={user};";
        _ds = NpgsqlDataSource.Create(connStr);
    }

    // Git-on-SQL
    public async Task BranchAsync(string name)
    {
        await using var cmd = _ds.CreateCommand("SELECT aruaru_branch($1)");
        cmd.Parameters.AddWithValue(name);
        await cmd.ExecuteNonQueryAsync();
    }

    public async Task<string?> CommitAsync(string message)
    {
        await using var cmd = _ds.CreateCommand("SELECT aruaru_commit($1) as commit_id");
        cmd.Parameters.AddWithValue(message);
        return (string?) await cmd.ExecuteScalarAsync();
    }

    public async Task<List<Dictionary<string, object?>>> LogAsync(int limit = 20)
    {
        var result = new List<Dictionary<string, object?>>();
        await using var cmd = _ds.CreateCommand(
            "SELECT * FROM aruaru_log ORDER BY timestamp DESC LIMIT $1");
        cmd.Parameters.AddWithValue(limit);
        await using var reader = await cmd.ExecuteReaderAsync();
        while (await reader.ReadAsync())
        {
            var row = new Dictionary<string, object?>();
            for (int i = 0; i < reader.FieldCount; i++)
                row[reader.GetName(i)] = reader.GetValue(i);
            result.Add(row);
        }
        return result;
    }

    public async ValueTask DisposeAsync() => await _ds.DisposeAsync();
}

// ── 使用例 ──────────────────────────────────────────────────
class Program
{
    static async Task Main()
    {
        await using var db = new AruaruClient();

        await db.BranchAsync("feature/dotnet-test");

        await using var cmd = db._ds.CreateCommand(
            "CREATE TABLE IF NOT EXISTS tasks (id SERIAL PRIMARY KEY, title TEXT)");
        await cmd.ExecuteNonQueryAsync();

        var commitId = await db.CommitAsync("Add tasks table");
        Console.WriteLine($"Committed: {commitId}");

        var log = await db.LogAsync(5);
        foreach (var row in log)
            Console.WriteLine(string.Join(", ", row.Select(kv => $"{kv.Key}={kv.Value}")));
    }
}
