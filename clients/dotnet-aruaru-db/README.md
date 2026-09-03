# `Aruaru.Db` — 公式 .NET コネクタ(薄いラッパー)

**独自の PostgreSQL ドライバではない。** 業界標準の
[`Npgsql`](https://www.npgsql.org/) をそのまま使い、その上に aruaru-db の
Git-on-SQL 機能を .NET の慣用 API(async/await)で足すだけの薄い層。
Npgsql は同期 API も持つのでハイブリッドにも使える。

正本: [`../../docs/CLIENTS.md`](../../docs/CLIENTS.md)

## インストール

```xml
<PackageReference Include="Aruaru.Db" Version="0.1.0" />
```

(まだ NuGet 未公開。`src/Aruaru.Db.csproj` をプロジェクト参照するか
`dotnet pack` してローカル NuGet フィードへ置く。)

## API

| メンバー | 実体 |
|---|---|
| `AruaruDb.Connect(connectionString)` / `AruaruDb.FromDataSource(dataSource)` | `NpgsqlDataSource` をそのまま |
| `.ExecuteAsync(sql)` | `NpgsqlCommand.ExecuteNonQueryAsync` の薄い透過 |
| `.CommitAsync(message) -> Task<string>` | `SELECT aruaru_commit($1)` → commit_id(**結果は `ExecuteScalarAsync`、列名ではなく戻り値そのもの**) |
| `.QueryAsOfAsync(baseSelect, commitId, parameters)` | `baseSelect` の末尾へ ` AS OF COMMIT '<id>'` を安全に付与。`IsSafeCommitId`(英数字+`-`/`_`、≤128)で検証、非安全なら `InvalidCommitIdException`(SQL インジェクション防止、ネットワークに触れる前に拒否) |
| `.DataSource` | 内部の `NpgsqlDataSource`(透過的に何でも) |

## 例

```csharp
await using var db = AruaruDb.Connect("Host=localhost;Port=5433;Username=app;Password=secret;Database=app");
await db.ExecuteAsync("INSERT INTO items(id, qty) VALUES ('sword', 1)");
var first = await db.CommitAsync("first import");

await db.ExecuteAsync("UPDATE items SET qty = 5 WHERE id = 'sword'");
await db.CommitAsync("restock");

// VersionlessAPI: 過去のコミット時点を読む(最新は 5、これは 1)
await using var rows = await db.QueryAsOfAsync("SELECT qty FROM items WHERE id = 'sword'", first);
```

ASP.NET Core / Minimal API は起動時に `AruaruDb.Connect(...)` した
インスタンス(または DI 済みの `NpgsqlDataSource` を `FromDataSource`)を
シングルトンとして登録するだけ。

## ビルド・テスト

```bash
dotnet build src/Aruaru.Db.csproj
dotnet test tests/Aruaru.Db.Tests.csproj    # ネットワーク不要ぶん
ARUARU_DB_TEST_CONNSTRING="Host=127.0.0.1;Port=5433;Username=app;Password=secret;Database=app" \
  dotnet test tests/Aruaru.Db.Tests.csproj --filter LiveCommitAndAsOfRoundTrip
```

## 検証状況

- **`dotnet build src/Aruaru.Db.csproj` = 2026-09-03 実施・成功**
  (0 警告・0 エラー)。
- **`dotnet test tests/Aruaru.Db.Tests.csproj` = 2026-09-03 実施・
  10/10 green**(この開発機には .NET 8 ランタイムが無く `net10.0` の
  共有ランタイムのみだったため、`tests/Aruaru.Db.Tests.csproj` に
  `<RollForward>LatestMajor</RollForward>` を追加して解消)。
- **実サーバ往復(`ARUARU_DB_TEST_CONNSTRING`)= 2026-09-03 実施・
  passed(10/10)**。ただし途中で**実バグを1件発見・修正**:
  `NpgsqlDataSource.Create(connectionString)` の既定動作(接続確立時に
  `pg_type` 等へブートストラップ問い合わせして型情報を読み込む)が、
  aruaru-db が `pg_catalog` を完全実装していないため
  `"SELECT: missing FROM"` で失敗していた。
  `NpgsqlDataSourceBuilder` + `ServerCompatibilityMode.NoTypeLoading`
  でこのブートストラップ自体を無効化して解消(`Connect()` 内、
  他言語のドライバでも同種の型システム自動探索を無効化する設定が
  必要になり得る——Python `asyncpg` は 2026-09-03 時点で同種の問題に
  遭遇し、こちらは未解決のまま〈下記〉)。修正後は`connect` →
  `commit()` → `queryAsOf()` の一連が実サーバに対して green。
