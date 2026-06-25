# aruaru-DB × WunderGraph Cosmo

aruaru-DB は **Apollo Federation v2 互換のサブグラフ**として GraphQL を公開します。
Cosmo Router がこのサブグラフ (および他サービス) を束ねて、単一の統合スーパーグラフを提供します。

```
クライアント ──> Cosmo Router (:3002) ──┬─ aruaru-db サブグラフ (:4000/graphql)
                                         └─ (他チームのサブグラフ...)
```

## サブグラフが公開するもの
- `Query`: currentBranch / branches / log / diff / sql / _entities / _service
- `Mutation`: createBranch / checkout / merge / execSql
- エンティティ: `Commit @key(fields: "id")` (他サブグラフから拡張可能)
- Federation SDL: `GET http://localhost:4000/graphql/sdl`

## 手順 (Control Plane 非依存・ローカル合成)

```bash
# 1. aruaru-server を起動 (サブグラフを :4000/graphql に公開)
cargo run -p aruaru-server -- --gql-port 4000 --pg-port 5432 --data ./data

# 2. wgc でスキーマ合成 → Router 用 execution config を生成
#    (wgc は npm i -g wgc)
wgc router compose -i cosmo/graph.yaml -o cosmo/router-config.json

# 3. Cosmo Router を起動 (合成済み config をファイル指定)
docker compose -f cosmo/compose.cosmo.yaml up

# 4. 統合 GraphQL: http://localhost:3002/graphql
```

## メモ
- 本番では Cosmo Control Plane + Schema Registry を使い、`wgc subgraph publish` で
  CDN 経由配信する構成が推奨です (上記はローカル合成のオフライン構成)。
- aruaru-server 側は自前 async-graphql によるサブグラフ実装で、リゾルバは
  実 QueryEngine / VersionController に接続済みです。
