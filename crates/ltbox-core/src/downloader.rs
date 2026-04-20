//! HTTP download helpers for root-pipeline asset fetches.
//!
//! Blocking `ureq` wrapper that streams a URL to disk and appends progress
//! lines to a caller-owned log. Pairs with [`crate::github::GitHubClient`]
//! for release-asset URL resolution.

use std::io::{Read, Write};
use std::path::Path;

use crate::error::{LtboxError, Result};

const USER_AGENT: &str = "LTBox-rs/3.0";
const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// Shared ureq agent (user-agent + timeout) for all outbound HTTP in this crate.
pub(crate) fn build_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS)))
        .build()
        .new_agent()
}

/// Download `url` to `out_path` in 64 KiB chunks. Progress is throttled to
/// one log line per 10%. Creates missing parent dirs; overwrites existing file.
pub fn download_to_file(url: &str, out_path: &Path, log: &mut Vec<String>) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut resp = build_agent()
        .get(url)
        .call()
        .map_err(|e| LtboxError::Download(format!("GET {url}: {e}")))?;

    let display_name = out_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    log.push(format!("[dl] {display_name} ← {url}"));

    // None on chunked responses.
    let total: Option<u64> = resp
        .headers()
        .get(ureq::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(out_path)?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_pct_bucket: i32 = -1;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| LtboxError::Download(format!("read: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;

        if let Some(total) = total
            && total > 0
        {
            let pct = (downloaded * 100 / total) as i32;
            let bucket = pct / 10;
            if bucket > last_pct_bucket {
                last_pct_bucket = bucket;
                log.push(format!(
                    "[dl] {display_name} {pct}% ({:.1}/{:.1} MB)",
                    downloaded as f64 / 1_000_000.0,
                    total as f64 / 1_000_000.0,
                ));
            }
        }
    }

    log.push(format!(
        "[dl] {display_name} done ({:.1} MB)",
        downloaded as f64 / 1_000_000.0,
    ));
    Ok(())
}
