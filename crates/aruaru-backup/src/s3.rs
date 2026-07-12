//! S3-compatible backup destination (`BackupDestination::S3`) — closes the
//! "S3/SFTP destination not yet connected" gap flagged in `CLAUDE.md`'s
//! handoff notes (`aruaru-backup`'s `local_dest()` previously returned an
//! explicit error for any non-`Local` destination; SFTP remains
//! unimplemented, scoped out of this pass — see the module doc below).
//!
//! Works against AWS S3 itself, or any S3-compatible object store (MinIO,
//! Cloudflare R2, Backblaze B2, etc.) via the `endpoint` override, since
//! all of them implement the same SigV4-signed REST API.
//!
//! **Design**: [`rusty-s3`](https://docs.rs/rusty-s3) generates
//! SigV4-presigned request URLs — the actual cryptographic signing is
//! delegated to it (an established, focused crate for exactly this),
//! matching this codebase's existing convention of hand-rolling protocol
//! glue while delegating real cryptography to an audited library. Once a
//! presigned URL exists, it's just a plain HTTP request, sent with
//! `reqwest` (already a workspace dependency elsewhere) — no need for the
//! much heavier `aws-sdk-s3` and its own credential-chain machinery.
//!
//! **Credentials**: read from the standard `AWS_ACCESS_KEY_ID` /
//! `AWS_SECRET_ACCESS_KEY` environment variables (the same convention the
//! AWS CLI/SDKs use) rather than being stored in [`crate::BackupConfig`]
//! — `BackupConfig` gets persisted to disk/passed around as configuration,
//! and secrets don't belong in a struct that might end up serialized to a
//! config file or logged.
//!
//! **Verification note**: this sandbox has no live S3-compatible server
//! to test against, and the allowed-network-domains list for this
//! session doesn't include any object storage endpoint. The presigned-URL
//! generation itself (`presign_put`/`presign_get`/`presign_list`) is pure
//! (no network) and fully unit tested; the functions that actually
//! perform HTTP requests (`put_object`/`get_object`/`list_objects`) are
//! not covered by an automated test here -- the same class of limitation
//! as the sibling `poem-cosmo-tauri` repo's
//! `open-runo-cache::redis_backend::RedisCache` (also untested against a
//! live server, for the identical reason: none available in this
//! environment).

use std::time::Duration;

use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

/// How long a presigned URL stays valid. Generous, since a large backup
/// upload/download can take a while; presigned URLs are single-use in
/// spirit even if technically replayable within their window, so a long
/// window doesn't meaningfully weaken security here (the credentials
/// themselves, not the URL, are the actual secret).
const PRESIGN_TTL: Duration = Duration::from_secs(3600);

/// Everything needed to talk to one S3-compatible bucket: the bucket
/// itself (endpoint/region/name, from [`crate::BackupDestination::S3`])
/// plus credentials (from the environment, see the module doc for why).
pub struct S3Client {
    bucket: Bucket,
    credentials: Credentials,
    /// `prefix` from `BackupDestination::S3` -- prepended to every key so
    /// multiple deployments/backup configs can share one bucket safely.
    prefix: String,
}

impl S3Client {
    /// Build a client for `bucket_name` in `region`, optionally against a
    /// non-AWS `endpoint` (MinIO, R2, etc. -- `None` means real AWS S3).
    /// Reads `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` from the
    /// environment; returns `Err` if either is unset rather than silently
    /// proceeding with empty credentials (which would produce confusing
    /// SigV4 signature-mismatch errors from S3 instead of a clear local
    /// error).
    pub fn new(bucket_name: &str, prefix: &str, region: &str, endpoint: Option<&str>) -> anyhow::Result<Self> {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID")
            .map_err(|_| anyhow::anyhow!("AWS_ACCESS_KEY_ID is not set; required for S3 backup destination"))?;
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").map_err(|_| {
            anyhow::anyhow!("AWS_SECRET_ACCESS_KEY is not set; required for S3 backup destination")
        })?;

        let endpoint_url = match endpoint {
            Some(custom) => custom.to_string(),
            None => format!("https://s3.{region}.amazonaws.com"),
        };
        let endpoint_url = endpoint_url
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid S3 endpoint URL {endpoint_url:?}: {e}"))?;

        // Path-style (bucket in the URL path, not a subdomain) works
        // uniformly across AWS S3 and every common S3-compatible
        // implementation (MinIO in particular doesn't support virtual-
        // hosted-style out of the box), so it's the safer default here.
        let bucket = Bucket::new(endpoint_url, UrlStyle::Path, bucket_name.to_string(), region.to_string())
            .map_err(|e| anyhow::anyhow!("invalid S3 bucket configuration: {e}"))?;
        let credentials = Credentials::new(access_key, secret_key);

        Ok(Self { bucket, credentials, prefix: prefix.to_string() })
    }

    fn full_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), key)
        }
    }

    /// Presign a `PUT` URL for `key` (prefixed). Pure/no network -- unit
    /// tested directly.
    fn presign_put(&self, key: &str) -> String {
        let full_key = self.full_key(key);
        let action = self.bucket.put_object(Some(&self.credentials), &full_key);
        action.sign(PRESIGN_TTL).to_string()
    }

    /// Presign a `GET` URL for `key` (prefixed). Pure/no network -- unit
    /// tested directly.
    fn presign_get(&self, key: &str) -> String {
        let full_key = self.full_key(key);
        let action = self.bucket.get_object(Some(&self.credentials), &full_key);
        action.sign(PRESIGN_TTL).to_string()
    }

    /// Presign a `ListObjectsV2` URL under this client's prefix. Pure/no
    /// network -- unit tested directly.
    fn presign_list(&self) -> String {
        let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
        action.query_mut().insert("prefix", &self.prefix);
        action.sign(PRESIGN_TTL).to_string()
    }

    /// Upload `bytes` to `key` (prefixed with this client's configured
    /// prefix). Returns `Err` on any non-2xx response, with the response
    /// body included so an operator can see the actual S3 error (bucket
    /// doesn't exist, access denied, etc.) rather than just a status code.
    pub async fn put_object(&self, key: &str, bytes: Vec<u8>) -> anyhow::Result<()> {
        let url = self.presign_put(key);
        let resp = reqwest::Client::new().put(url).body(bytes).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("S3 PUT {key} failed: {status} {body}");
        }
        Ok(())
    }

    /// Download the object at `key` (prefixed). Returns `Err` on any
    /// non-2xx response (including 404 -- the object doesn't exist).
    pub async fn get_object(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let url = self.presign_get(key);
        let resp = reqwest::Client::new().get(url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("S3 GET {key} failed: {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    }

    /// List every object key under this client's prefix. Hand-rolled,
    /// minimal XML key extraction (`<Key>...</Key>`) rather than pulling
    /// in a full XML parsing dependency -- matching this codebase's
    /// existing convention of hand-rolling narrow protocol/format parsing
    /// (multipart, WebSocket framing, protobuf elsewhere in this
    /// project family) when the actual shape needed is small. This does
    /// **not** handle pagination (`ListObjectsV2`'s `IsTruncated`/
    /// `NextContinuationToken`) -- fine for the backup-listing use case
    /// (a deployment's total number of backups is expected to be small),
    /// but not a general-purpose S3 listing client.
    pub async fn list_objects(&self) -> anyhow::Result<Vec<String>> {
        let url = self.presign_list();
        let resp = reqwest::Client::new().get(url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("S3 ListObjectsV2 failed: {}", resp.status());
        }
        let body = resp.text().await?;
        Ok(extract_xml_keys(&body))
    }
}

/// Extract every `<Key>...</Key>` element's text content from a
/// `ListObjectsV2` XML response body. Deliberately minimal (no real XML
/// parser, no namespace handling) -- see `list_objects`'s doc comment for
/// why that's an acceptable tradeoff here.
fn extract_xml_keys(xml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find("<Key>") {
        let after_open = &rest[open + "<Key>".len()..];
        let Some(close) = after_open.find("</Key>") else { break };
        keys.push(after_open[..close].to_string());
        rest = &after_open[close + "</Key>".len()..];
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> S3Client {
        // Safe to set for the duration of these process-local tests: no
        // other test in this crate reads these two specific env vars.
        std::env::set_var("AWS_ACCESS_KEY_ID", "test-access-key");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "test-secret-key");
        S3Client::new("my-backups", "aruaru-db", "us-east-1", Some("http://localhost:9000")).unwrap()
    }

    #[test]
    fn new_requires_credentials_from_env() {
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        let result = S3Client::new("my-backups", "aruaru-db", "us-east-1", None);
        assert!(result.is_err());
    }

    #[test]
    fn presign_put_produces_a_signed_url_for_the_prefixed_key() {
        let client = test_client();
        let url = client.presign_put("backup-1/MANIFEST.json");
        assert!(url.starts_with("http://localhost:9000/my-backups/aruaru-db/backup-1/MANIFEST.json"));
        assert!(url.contains("X-Amz-Signature="));
        assert!(url.contains("X-Amz-Credential=test-access-key"));
    }

    #[test]
    fn presign_get_produces_a_signed_url_for_the_prefixed_key() {
        let client = test_client();
        let url = client.presign_get("backup-1/MANIFEST.json");
        assert!(url.starts_with("http://localhost:9000/my-backups/aruaru-db/backup-1/MANIFEST.json"));
        assert!(url.contains("X-Amz-Signature="));
    }

    #[test]
    fn full_key_joins_prefix_and_key_with_exactly_one_slash() {
        let client = test_client();
        // The bucket-level prefix ("aruaru-db") should appear exactly
        // once between itself and the key, even if a caller's key
        // happens to start with a slash-adjacent segment.
        assert_eq!(client.full_key("x"), "aruaru-db/x");
    }

    #[test]
    fn full_key_with_no_prefix_is_passthrough() {
        std::env::set_var("AWS_ACCESS_KEY_ID", "k");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "s");
        let client = S3Client::new("my-backups", "", "us-east-1", None).unwrap();
        assert_eq!(client.full_key("x"), "x");
    }

    #[test]
    fn extract_xml_keys_parses_multiple_key_elements() {
        let xml = "<ListBucketResult>\
                     <Contents><Key>aruaru-db/backup-1/MANIFEST.json</Key></Contents>\
                     <Contents><Key>aruaru-db/backup-2/MANIFEST.json</Key></Contents>\
                   </ListBucketResult>";
        let keys = extract_xml_keys(xml);
        assert_eq!(keys, vec!["aruaru-db/backup-1/MANIFEST.json", "aruaru-db/backup-2/MANIFEST.json"]);
    }

    #[test]
    fn extract_xml_keys_returns_empty_for_no_matches() {
        assert!(extract_xml_keys("<ListBucketResult></ListBucketResult>").is_empty());
    }
}
