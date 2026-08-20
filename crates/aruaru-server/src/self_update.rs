//! 自動アップデート機能(2026-08-19新設)。
//!
//! `open-english/server/src/self_update.rs`・`aruaru-llm/src/self_update.rs`
//! と同じ設計思想(コピペではなくこのリポジトリの実際の配布形態に合わせた
//! 実装)。`aruaru-db`の`.github/workflows/release.yml`はWindows/Linux向けに
//! `aruaru-db-windows-x86_64.zip`/`aruaru-db-linux-x86_64.tar.gz`
//! (いずれも`aruaru-server`実行ファイル+`install.sh`/`install.ps1`)を
//! GitHub Releasesへ添付する構成のため、専用アンインストーラーは無い。
//!
//! ## 正直な開示(最重要、`aruaru-llm`側と同じ既知の制約)
//!
//! - **実機E2E検証は未実施**: コンパイル成功・単体テスト(バージョン比較・
//!   アセット名判定)の確認までで、実際にGitHub Releaseの新版を検知して
//!   自己更新する一連の流れは最初から最後まで実行していない。
//! - **既定で無効**: `ARUARU_DB_ENABLE_SELF_UPDATE=1`(または`true`)を
//!   明示的に設定しない限り何もしない。`aruaru-server`は`--data`に
//!   実データを保持する常駐DBサーバーであり、意図せぬ自己更新で
//!   稼働中のプロセスが不意に再起動されることを避けるため、より
//!   慎重な既定off運用とした。
//! - **ヘルスチェック**: `GET /healthz`(このコミットで`main.rs`へ新設)を
//!   使う。新バイナリ起動後`HEALTH_CHECK_SECS`秒以内に到達できなければ、
//!   退避しておいた旧バイナリへロールバックする。この一連の流れも
//!   実機検証はしていない(コードレビューベース)。
//! - **`--data`/`--pg-port`/`--gql-port`等の起動引数の扱い**: 新バイナリは
//!   このプロセスが実際に使ったのと同じコマンドライン引数
//!   (`std::env::args()`)で再起動する——設定漏れによる意図しない
//!   デフォルト設定での起動を避けるため。

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

const GITHUB_REPO: &str = "aon-co-jp/aruaru-db";

pub(crate) const HEALTH_CHECK_SECS: u64 = 12;

#[derive(Debug, Deserialize)]
pub(crate) struct ReleaseAsset {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LatestRelease {
    pub(crate) tag_name: String,
    pub(crate) assets: Vec<ReleaseAsset>,
}

pub(crate) fn parse_version(raw: &str) -> (u64, u64, u64) {
    let trimmed = raw.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.').map(|s| s.parse::<u64>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

pub(crate) fn is_newer(remote: &str, local: &str) -> bool {
    parse_version(remote) > parse_version(local)
}

fn self_update_enabled() -> bool {
    std::env::var("ARUARU_DB_ENABLE_SELF_UPDATE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

async fn fetch_latest_release() -> anyhow::Result<LatestRelease> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?;
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let res = client.get(&url).header("User-Agent", "aruaru-db-self-updater").send().await?;
    if !res.status().is_success() {
        anyhow::bail!("GitHub releases API returned HTTP {}", res.status());
    }
    Ok(res.json::<LatestRelease>().await?)
}

fn platform_asset(release: &LatestRelease) -> Option<&ReleaseAsset> {
    let is_windows = cfg!(target_os = "windows");
    release.assets.iter().find(|a| {
        let name = a.name.to_lowercase();
        if is_windows {
            name.ends_with(".zip") && name.contains("windows")
        } else {
            name.ends_with(".tar.gz") && name.contains("linux")
        }
    })
}

pub(crate) async fn download_to(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let client = reqwest::Client::builder().build()?;
    let bytes = client.get(url).header("User-Agent", "aruaru-db-self-updater").send().await?.bytes().await?;
    std::fs::write(dest, &bytes)?;
    Ok(())
}

async fn extract_binary(archive_path: &std::path::Path, extract_dir: &std::path::Path) -> anyhow::Result<PathBuf> {
    let _ = std::fs::remove_dir_all(extract_dir);
    std::fs::create_dir_all(extract_dir)?;

    if cfg!(target_os = "windows") {
        let status = tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    archive_path.display(),
                    extract_dir.display()
                ),
            ])
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("Expand-Archive failed with status {status}");
        }
        Ok(extract_dir.join("aruaru-server.exe"))
    } else {
        let status = tokio::process::Command::new("tar")
            .args(["xzf", &archive_path.to_string_lossy(), "-C"])
            .arg(extract_dir)
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("tar extraction failed with status {status}");
        }
        Ok(extract_dir.join("aruaru-server"))
    }
}

async fn apply_update(new_binary: &std::path::Path, health_addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let backup = exe.with_extension("bak");
    // 起動時に実際に使われたのと同じ引数で再起動する(モジュールdoc参照)。
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args_joined_windows = args.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(" ");
    let args_joined_unix = args.iter().map(|a| format!("'{a}'")).collect::<Vec<_>>().join(" ");

    if cfg!(target_os = "windows") {
        let script_path = std::env::temp_dir().join("aruaru-db-self-update.bat");
        let mut script = String::from("@echo off\r\nping 127.0.0.1 -n 3 > nul\r\n");
        script += &format!("copy /Y \"{}\" \"{}\"\r\n", exe.display(), backup.display());
        script += &format!("copy /Y \"{}\" \"{}\"\r\n", new_binary.display(), exe.display());
        script += &format!("start \"\" \"{}\" {}\r\n", exe.display(), args_joined_windows);
        script += &format!("ping 127.0.0.1 -n {} > nul\r\n", HEALTH_CHECK_SECS + 2);
        script += &format!(
            "powershell -NoProfile -Command \"try {{ $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 3 'http://{health_addr}/healthz'; if ($r.StatusCode -ne 200) {{ exit 1 }} }} catch {{ exit 1 }}\"\r\n"
        );
        script += "if %errorlevel% neq 0 (\r\n";
        script += &format!("  taskkill /IM \"{}\" /F >nul 2>nul\r\n", exe.file_name().and_then(|n| n.to_str()).unwrap_or("aruaru-server.exe"));
        script += "  ping 127.0.0.1 -n 2 > nul\r\n";
        script += &format!("  copy /Y \"{}\" \"{}\"\r\n", backup.display(), exe.display());
        script += &format!("  start \"\" \"{}\" {}\r\n", exe.display(), args_joined_windows);
        script += ")\r\n";
        script += &format!("del \"{}\"\r\n", script_path.display());
        std::fs::write(&script_path, script)?;
        tokio::process::Command::new("cmd").args(["/C", "start", "", script_path.to_string_lossy().as_ref()]).spawn()?;
    } else {
        let script_path = std::env::temp_dir().join("aruaru-db-self-update.sh");
        let mut script = String::from("#!/bin/sh\nsleep 3\n");
        script += &format!("cp \"{}\" \"{}\"\n", exe.display(), backup.display());
        script += &format!("cp \"{}\" \"{}\"\n", new_binary.display(), exe.display());
        script += &format!("chmod +x \"{}\"\n", exe.display());
        script += &format!("nohup \"{}\" {} >/dev/null 2>&1 &\n", exe.display(), args_joined_unix);
        script += &format!("sleep {}\n", HEALTH_CHECK_SECS + 2);
        script += &format!("if ! curl -sf --max-time 3 'http://{health_addr}/healthz' >/dev/null 2>&1; then\n");
        script += &format!("  pkill -f \"{}\" 2>/dev/null\n", exe.display());
        script += "  sleep 1\n";
        script += &format!("  cp \"{}\" \"{}\"\n", backup.display(), exe.display());
        script += &format!("  chmod +x \"{}\"\n", exe.display());
        script += &format!("  nohup \"{}\" {} >/dev/null 2>&1 &\n", exe.display(), args_joined_unix);
        script += "fi\n";
        std::fs::write(&script_path, &script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }
        tokio::process::Command::new("sh").arg(&script_path).spawn()?;
    }

    tracing::info!("aruaru-db self-update: launched swap+healthcheck script, exiting to release file locks");
    std::process::exit(0);
}

/// 起動時のメンテナンスチェックの一部として呼ぶ想定のエントリポイント。
/// `health_addr`はこのプロセスが実際にbindしたHTTPアドレス
/// (`0.0.0.0:{gql_port}`、`main.rs`参照)——ヘルスチェック先として使う。
pub async fn check_and_apply_update(health_addr: std::net::SocketAddr) {
    if !self_update_enabled() {
        tracing::debug!("aruaru-db self-update: disabled (set ARUARU_DB_ENABLE_SELF_UPDATE=1 to enable)");
        return;
    }

    let release = match fetch_latest_release().await {
        Ok(r) => r,
        Err(e) => {
            tracing::info!("aruaru-db self-update: could not check GitHub releases ({e}) — continuing without updating");
            return;
        }
    };

    let local = env!("CARGO_PKG_VERSION");
    if !is_newer(&release.tag_name, local) {
        tracing::info!("aruaru-db self-update: up to date (local {local}, latest release {})", release.tag_name);
        return;
    }

    let Some(asset) = platform_asset(&release) else {
        tracing::info!(
            "aruaru-db self-update: newer release {} found but no matching platform asset attached — skipping",
            release.tag_name
        );
        return;
    };

    tracing::info!("aruaru-db self-update: newer release {} found (local {local}), downloading {}", release.tag_name, asset.name);

    let dest = std::env::temp_dir().join(&asset.name);
    if let Err(e) = download_to(&asset.browser_download_url, &dest).await {
        tracing::info!("aruaru-db self-update: download failed ({e}) — continuing without updating");
        return;
    }

    let extract_dir = std::env::temp_dir().join("aruaru-db-self-update-extract");
    let new_binary = match extract_binary(&dest, &extract_dir).await {
        Ok(p) if p.exists() => p,
        Ok(p) => {
            tracing::info!("aruaru-db self-update: extracted archive but expected binary not found at {}", p.display());
            return;
        }
        Err(e) => {
            tracing::info!("aruaru-db self-update: extraction failed ({e}) — continuing without updating");
            return;
        }
    };

    if let Err(e) = apply_update(&new_binary, health_addr).await {
        tracing::info!("aruaru-db self-update: failed to launch update ({e})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_strings_with_and_without_v_prefix() {
        assert_eq!(parse_version("v0.5.1"), (0, 5, 1));
        assert_eq!(parse_version("0.5.1"), (0, 5, 1));
        assert_eq!(parse_version("garbage"), (0, 0, 0));
    }

    #[test]
    fn is_newer_compares_semver_correctly() {
        assert!(is_newer("v0.6.0", "0.5.1"));
        assert!(!is_newer("v0.5.1", "0.5.1"));
        assert!(!is_newer("v0.5.0", "0.5.1"));
    }

    #[test]
    fn platform_asset_finds_expected_naming() {
        let release = LatestRelease {
            tag_name: "v0.6.0".into(),
            assets: vec![
                ReleaseAsset { name: "aruaru-db-windows-x86_64.zip".into(), browser_download_url: "https://example/win".into() },
                ReleaseAsset { name: "aruaru-db-linux-x86_64.tar.gz".into(), browser_download_url: "https://example/linux".into() },
            ],
        };
        let asset = platform_asset(&release).expect("asset should be found");
        if cfg!(target_os = "windows") {
            assert!(asset.name.contains("windows"));
        } else {
            assert!(asset.name.contains("linux"));
        }
    }
}
