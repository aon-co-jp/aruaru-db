using Aruaru.Db;
using Npgsql;
using Xunit;

namespace Aruaru.Db.Tests;

public class AruaruDbTests
{
    [Theory]
    [InlineData("a1b2c3d4e5f6", true)]
    [InlineData("9f8e7d6c-1234-4abc-9def-000011112222", true)]
    [InlineData("commit_42-X", true)]
    [InlineData("", false)]
    [InlineData("abc'; DROP TABLE items; --", false)]
    [InlineData("abc def", false)]
    public void IsSafeCommitId_AcceptsHashesAndUuids_RejectsSql(string id, bool expected)
    {
        Assert.Equal(expected, AruaruDb.IsSafeCommitId(id));
    }

    [Fact]
    public void IsSafeCommitId_RejectsOverlongIds()
    {
        Assert.False(AruaruDb.IsSafeCommitId(new string('x', 200)));
    }

    [Fact]
    public void IsSafeCommitId_RejectsNull()
    {
        Assert.False(AruaruDb.IsSafeCommitId(null));
    }

    [Fact]
    public async Task QueryAsOfAsync_RejectsUnsafeCommitId_BeforeTouchingTheNetwork()
    {
        // A NpgsqlDataSource is constructed but never opened/connected --
        // if QueryAsOfAsync reached the network it would fail with a
        // connection error, not the validation exception below.
        await using var ds = NpgsqlDataSource.Create("Host=127.0.0.1;Port=1;Username=x;Password=x;Database=x;Timeout=1");
        await using var db = AruaruDb.FromDataSource(ds);

        await Assert.ThrowsAsync<InvalidCommitIdException>(
            () => db.QueryAsOfAsync("SELECT qty FROM items", "' OR 1=1 --"));
    }

    /// <summary>
    /// 実サーバ相手の往復。環境変数 ARUARU_DB_TEST_CONNSTRING があるときだけ走る。
    /// 例: Host=127.0.0.1;Port=5433;Username=app;Password=secret;Database=app
    /// </summary>
    [Fact]
    public async Task LiveCommitAndAsOfRoundTrip()
    {
        var connString = Environment.GetEnvironmentVariable("ARUARU_DB_TEST_CONNSTRING");
        if (string.IsNullOrEmpty(connString))
        {
            return; // skip: no live server configured
        }

        await using var db = AruaruDb.Connect(connString);
        await db.ExecuteAsync("CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, qty INT)");
        await db.ExecuteAsync(
            "INSERT INTO items(id, qty) VALUES ('sword', 1) ON CONFLICT (id) DO UPDATE SET qty = EXCLUDED.qty");

        var first = await db.CommitAsync("first import");
        await db.ExecuteAsync("UPDATE items SET qty = 5 WHERE id = 'sword'");
        await db.CommitAsync("restock");

        await using (var cmd = db.DataSource.CreateCommand("SELECT qty FROM items WHERE id = 'sword'"))
        await using (var reader = await cmd.ExecuteReaderAsync())
        {
            Assert.True(await reader.ReadAsync());
            Assert.Equal(5, int.Parse(reader.GetString(0)));
        }

        await using var oldReader = await db.QueryAsOfAsync("SELECT qty FROM items WHERE id = 'sword'", first);
        Assert.True(await oldReader.ReadAsync());
        Assert.Equal(1, int.Parse(oldReader.GetString(0)));
    }
}
