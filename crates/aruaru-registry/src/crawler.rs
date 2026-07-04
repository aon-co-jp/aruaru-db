//! ランキングクローラ
//!
//! DB-Engines を第一ソースに、失敗時は別ソースへフォールバックして
//! 各DBのランキング順位・人気度スコアを取得し、レジストリへ反映する。
//!
//! ## フォールバック順
//! 1. DB-Engines Ranking (https://db-engines.com/en/ranking)
//! 2. (将来) GitHub Stars / Stack Overflow 調査などの代替ソース
//!
//! HTML 構造は変わりうるため、抽出は寛容に行い、取れた分だけ反映する。

use std::collections::HashMap;

use regex::Regex;

/// クロール結果: 正規化したDB名 → (rank, score)
pub type RankMap = HashMap<String, (u32, f64)>;

/// 名前を突き合わせ用に正規化 (英小文字・英数のみ)
pub fn normalize(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// すべてのソースを順に試し、最初に成功した結果を返す。
pub async fn crawl_all() -> anyhow::Result<RankMap> {
    match crawl_db_engines().await {
        Ok(m) if !m.is_empty() => {
            tracing::info!(count = m.len(), "crawled DB-Engines ranking");
            return Ok(m);
        }
        Ok(_) => tracing::warn!("DB-Engines returned empty; trying fallback"),
        Err(e) => tracing::warn!(error = %e, "DB-Engines crawl failed; trying fallback"),
    }

    // フォールバック (現状はプレースホルダ。将来 GitHub 等を追加)
    match crawl_fallback().await {
        Ok(m) => Ok(m),
        Err(e) => {
            tracing::error!(error = %e, "all crawl sources failed");
            Err(e)
        }
    }
}

fn http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("aruaru-db-registry-crawler/0.2")
        .timeout(std::time::Duration::from_secs(20))
        .build()
}

/// DB-Engines ランキングページをクロール
async fn crawl_db_engines() -> anyhow::Result<RankMap> {
    let client = http_client()?;
    let html = client
        .get("https://db-engines.com/en/ranking")
        .send()
        .await?
        .text()
        .await?;

    Ok(parse_db_engines(&html))
}

/// DB-Engines の HTML から (rank, name, score) を抽出。
/// ランキング表の各行はおおむね順位・製品名・スコアを含む。
/// HTML 変更に強くするため、行ごとに緩く正規表現で拾う。
pub fn parse_db_engines(html: &str) -> RankMap {
    let mut out = RankMap::new();

    // テーブル行を粗く分割
    let row_re = Regex::new(r"(?is)<tr[^>]*>(.*?)</tr>").unwrap();
    // 製品名: ランキングページは <td class="...">...<a ...>NAME</a> を含む
    let name_re = Regex::new(r#"(?is)class="text-content"[^>]*>\s*<a[^>]*>([^<]+)</a>"#).unwrap();
    let name_re_alt = Regex::new(r#"(?is)<a href="/en/system/[^"]*">([^<]+)</a>"#).unwrap();
    // スコア: 数値.数値 を含むセル
    let score_re = Regex::new(r#"(?is)<td class="[^"]*"[^>]*>\s*([0-9]+\.[0-9]+)\s*</td>"#).unwrap();

    let mut rank = 0u32;
    for row in row_re.captures_iter(html) {
        let inner = &row[1];
        let name = name_re
            .captures(inner)
            .or_else(|| name_re_alt.captures(inner))
            .map(|c| c[1].trim().to_string());
        let Some(name) = name else { continue };
        if name.is_empty() {
            continue;
        }
        rank += 1;
        let score = score_re
            .captures(inner)
            .and_then(|c| c[1].parse::<f64>().ok())
            .unwrap_or(0.0);
        out.insert(normalize(&name), (rank, score));
    }

    out
}

/// フォールバックソース: GitHub Stars (OSS DB の人気度)
///
/// DB-Engines が取得できない場合に、主要 OSS DB の GitHub リポジトリの
/// スター数を JSON API から取得し、スター降順で順位づけする。
/// 無認証の GitHub API はレート制限が緩くない (60 req/h) ため、
/// 著名 OSS DB の少数セットに限定する (1 日 1 回の実行を想定)。
async fn crawl_fallback() -> anyhow::Result<RankMap> {
    let client = http_client()?;
    let mut scored: Vec<(String, f64)> = Vec::new();

    for (norm_name, repo) in github_repo_map() {
        let url = format!("https://api.github.com/repos/{repo}");
        match client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(stars) = json.get("stargazers_count").and_then(|v| v.as_u64()) {
                        scored.push((norm_name.to_string(), stars as f64));
                    }
                }
            }
            Err(e) => tracing::debug!(repo, error = %e, "github stars fetch failed"),
        }
    }

    // スター降順で rank を付与。score はスター数(千単位)。
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = RankMap::new();
    for (i, (name, stars)) in scored.into_iter().enumerate() {
        out.insert(name, ((i + 1) as u32, (stars / 1000.0 * 10.0).round() / 10.0));
    }
    tracing::info!(count = out.len(), "github stars fallback crawl done");
    Ok(out)
}

/// 主要 OSS DB の (正規化名, GitHub owner/repo) マップ
fn github_repo_map() -> Vec<(&'static str, &'static str)> {
    vec![
        ("postgresql", "postgres/postgres"),
        ("mysql", "mysql/mysql-server"),
        ("mariadb", "MariaDB/server"),
        ("sqlite", "sqlite/sqlite"),
        ("mongodb", "mongodb/mongo"),
        ("redis", "redis/redis"),
        ("valkey", "valkey-io/valkey"),
        ("elasticsearch", "elastic/elasticsearch"),
        ("opensearch", "opensearch-project/OpenSearch"),
        ("clickhouse", "ClickHouse/ClickHouse"),
        ("cockroachdb", "cockroachdb/cockroach"),
        ("tidb", "pingcap/tidb"),
        ("yugabytedb", "yugabyte/yugabyte-db"),
        ("duckdb", "duckdb/duckdb"),
        ("scylladb", "scylladb/scylladb"),
        ("apachecassandra", "apache/cassandra"),
        ("neo4j", "neo4j/neo4j"),
        ("surrealdb", "surrealdb/surrealdb"),
        ("influxdb", "influxdata/influxdb"),
        ("questdb", "questdb/questdb"),
        ("timescaledb", "timescale/timescaledb"),
        ("apachedoris", "apache/doris"),
        ("starrocks", "StarRocks/starrocks"),
        ("milvus", "milvus-io/milvus"),
        ("qdrant", "qdrant/qdrant"),
        ("weaviate", "weaviate/weaviate"),
        ("chroma", "chroma-core/chroma"),
        ("dragonfly", "dragonflydb/dragonfly"),
        ("dolt", "dolthub/dolt"),
        ("risingwave", "risingwavelabs/risingwave"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("PostgreSQL"), "postgresql");
        assert_eq!(normalize("Microsoft SQL Server"), "microsoftsqlserver");
        assert_eq!(normalize("Amazon Aurora (PostgreSQL)"), "amazonaurorapostgresql");
    }

    #[test]
    fn test_parse_minimal() {
        // 簡易な行を含む擬似 HTML で抽出を確認
        let html = r#"
        <table>
          <tr><td class="text-content"><a href="/en/system/Oracle">Oracle</a></td><td class="rank">1290.00</td></tr>
          <tr><td class="text-content"><a href="/en/system/MySQL">MySQL</a></td><td class="rank">1010.50</td></tr>
        </table>"#;
        let m = parse_db_engines(html);
        assert_eq!(m.get("oracle").map(|(r, _)| *r), Some(1));
        assert_eq!(m.get("mysql").map(|(r, _)| *r), Some(2));
    }
}
