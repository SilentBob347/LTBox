//! Qualcomm 9008 EDL USB driver detection + auto-install on Windows.
//!
//! Needs `qcadb.inf` (ADB composite) and `qcwdfser.inf` (WDF serial) from
//! `qualcomm/qcom-usb-kernel-drivers` releases. Presence probed via
//! `pnputil /enum-drivers`, then the DriverStore FileRepository as fallback.
//! Install: download → extract → `pnputil /add-driver` per `.inf`.
//!
//! Cross-platform `DriverStatus` / `DriverError` / `Result` types
//! live in `driver/mod.rs`; this file is only compiled on Windows
//! (gated by `#[cfg(windows)]` in `driver/mod.rs`) so every
//! `cfg!(windows)` runtime check from the pre-rename module folds
//! into compile-time guarantees here.

use std::path::{Path, PathBuf};
use std::process::Command;

use ltbox_core::i18n::tr;
use ltbox_core::{live, tr_args};

use super::{DriverError, DriverStatus, Result};

/// `Command::new` + `CREATE_NO_WINDOW` so `pnputil` does not flash a console.
fn silent_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

const REQUIRED_INFS: &[&str] = &["qcadb.inf", "qcwdfser.inf"];

#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

const RELEASES_API: &str =
    "https://api.github.com/repos/qualcomm/qcom-usb-kernel-drivers/releases?per_page=10";
const WIN_TAG_NEEDLE: &str = "win";
const ASSET_PREFIX: &str = "qud-win-";
const ASSET_SUFFIX: &str = "_arm64_amd64.zip";
const USER_AGENT: &str = concat!("ltbox/", env!("CARGO_PKG_VERSION"));

/// Probe whether the Qualcomm USB drivers are installed.
pub fn check_required_drivers() -> DriverStatus {
    let missing: Vec<&'static str> = REQUIRED_INFS
        .iter()
        .copied()
        .filter(|inf| !is_driver_present(inf))
        .collect();

    if missing.is_empty() {
        DriverStatus::Present
    } else {
        DriverStatus::Missing(missing)
    }
}

fn is_driver_present(inf_name: &str) -> bool {
    driver_present_via_pnputil(inf_name) || driver_present_via_driver_store(inf_name)
}

fn driver_present_via_pnputil(inf_name: &str) -> bool {
    let output = match silent_command("pnputil").arg("/enum-drivers").output() {
        Ok(o) => o,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let target = inf_name.to_ascii_lowercase();
    stdout.lines().any(|line| {
        if let Some((_, v)) = line.split_once(':') {
            v.trim().to_ascii_lowercase() == target
        } else {
            false
        }
    })
}

fn driver_present_via_driver_store(inf_name: &str) -> bool {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let repo = Path::new(&system_root)
        .join("System32")
        .join("DriverStore")
        .join("FileRepository");
    let Ok(entries) = std::fs::read_dir(&repo) else {
        return false;
    };
    let prefix = inf_name.to_ascii_lowercase();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with(&prefix) {
            return true;
        }
    }
    false
}

/// Download the latest `qcom-usb-kernel-drivers` release and `pnputil`-install
/// each `.inf` under a `Windows10/` folder.
///
/// Every milestone routes through `live!` so the GUI streams progress in
/// real time — the previous `log.push` only surfaced after the whole task
/// returned, so a stalled download looked indistinguishable from a fast
/// success until the final timeout error fired.
///
/// Two ureq agents:
///   * `meta_agent` — 30 s global, used for the small JSON release listing.
///   * `dl_agent` — no global cap; per-stage `connect` / `recv-response` /
///     `recv-body` timeouts so a slow link can finish a multi-MB ZIP
///     without the previous 30-s "timeout: global" guillotine cutting the
///     body read partway through.
pub fn download_and_install(log: &mut Vec<String>) -> Result<()> {
    live!(log, "[Driver] {}", tr("live_driver_fetch_meta"));
    let meta_agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent();

    let releases: Vec<GithubRelease> = meta_agent
        .get(RELEASES_API)
        .call()?
        .body_mut()
        .read_json()
        .map_err(|e| DriverError::Parse(e.to_string()))?;

    let release = releases
        .into_iter()
        .find(|r| r.tag_name.to_ascii_lowercase().contains(WIN_TAG_NEEDLE))
        .ok_or(DriverError::NoAsset)?;

    let (asset_name, asset_url) = release
        .assets
        .into_iter()
        .find(|a| a.name.starts_with(ASSET_PREFIX) && a.name.ends_with(ASSET_SUFFIX))
        .map(|a| (a.name, a.browser_download_url))
        .ok_or(DriverError::NoAsset)?;

    live!(
        log,
        "[Driver] {}",
        tr_args!("live_driver_asset", name = asset_name)
    );

    let tmp_dir = std::env::temp_dir().join(format!("ltbox_qcom_drv_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let zip_path = tmp_dir.join(&asset_name);

    // No global cap — stalls/slow links should still finish a 10–20 MB
    // ZIP without the previous "timeout: global" guillotine.
    let dl_agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(30)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(300)))
        .build()
        .new_agent();

    download_with_progress(&dl_agent, &asset_url, &asset_name, &zip_path, log)?;

    live!(log, "[Driver] {}", tr("live_driver_extracting"));
    let extract_dir = tmp_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir)?;
    extract_zip(&zip_path, &extract_dir)?;

    let mut inf_files: Vec<PathBuf> = Vec::new();
    walk_collect_infs(&extract_dir, &mut inf_files);
    if inf_files.is_empty() {
        cleanup(&tmp_dir);
        return Err(DriverError::NoInf);
    }

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for inf in &inf_files {
        let name = inf
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        live!(
            log,
            "[Driver] {}",
            tr_args!("live_driver_installing_inf", name = name)
        );
        let out = silent_command("pnputil")
            .arg("/add-driver")
            .arg(inf)
            .arg("/install")
            .output();
        match out {
            Ok(o) if o.status.success() => {
                succeeded += 1;
            }
            Ok(o) => {
                failed += 1;
                // pnputil writes its diagnostics to stdout, not stderr,
                // so logging only stderr left every failure as a blank
                // "failed: " line. Decode both, prefer stdout when
                // populated, fall back to a friendly hint when both are
                // empty (typical when pnputil bails on UAC before
                // emitting any text).
                let exit = o.status.code().unwrap_or(-1);
                let stdout = decode_console(&o.stdout);
                let stderr = decode_console(&o.stderr);
                let detail = if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else {
                    tr("live_driver_pnputil_no_diag")
                };
                live!(
                    log,
                    "[Driver] {}",
                    tr_args!(
                        "live_driver_pnputil_failed",
                        name = name,
                        exit = exit,
                        detail = detail,
                    )
                );
            }
            Err(e) => {
                failed += 1;
                live!(
                    log,
                    "[Driver] {}",
                    tr_args!("live_driver_pnputil_spawn_failed", name = name, error = e,)
                );
            }
        }
    }

    cleanup(&tmp_dir);

    // All installs flopped → surface as hard failure so the GUI shows
    // the red banner instead of the green "install complete" toast.
    if succeeded == 0 && failed > 0 {
        live!(
            log,
            "[Driver] {}",
            tr_args!("live_driver_all_failed", count = failed)
        );
        return Err(DriverError::PnputilAllFailed { count: failed });
    }

    let total = succeeded + failed;
    live!(
        log,
        "[Driver] {}",
        tr_args!(
            "live_driver_install_finished",
            succeeded = succeeded,
            total = total,
        )
    );
    Ok(())
}

/// Decode bytes captured from a Windows console subprocess. Tries UTF-8
/// first, then falls back to lossy UTF-8 (which keeps ASCII intact and
/// only mangles the high-byte ranges) so localized pnputil output at
/// least surfaces something instead of a blank "failed: " tail.
fn decode_console(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        s.to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Stream `url` to `out_path` via the shared
/// [`ltbox_core::downloader::stream_with_progress`] streamer, formatting
/// progress lines with the driver-flow i18n keys (`live_driver_*`) and
/// the `[Driver]` log prefix. The byte loop + 5 % bucket + 750 ms tick
/// throttle live in core; only the per-event log formatting stays here.
fn download_with_progress(
    agent: &ureq::Agent,
    url: &str,
    display_name: &str,
    out_path: &Path,
    log: &mut Vec<String>,
) -> Result<()> {
    use ltbox_core::downloader::{DownloadEvent, stream_with_progress};
    let display_name = display_name.to_string();
    stream_with_progress(agent, url, out_path, log, move |log, event| match event {
        DownloadEvent::Start => {
            live!(
                log,
                "[Driver] {}",
                tr_args!("live_driver_downloading", name = &display_name)
            );
        }
        DownloadEvent::ProgressPct {
            downloaded_mb,
            total_mb,
            pct,
            speed_mbps,
        } => {
            live!(
                log,
                "[Driver] {}",
                tr_args!(
                    "live_driver_progress_pct",
                    name = &display_name,
                    pct = format!("{pct:>3}"),
                    downloaded = format!("{downloaded_mb:.1}"),
                    total = format!("{total_mb:.1}"),
                    speed = format!("{speed_mbps:.1}"),
                )
            );
        }
        DownloadEvent::ProgressChunked {
            downloaded_mb,
            speed_mbps,
        } => {
            live!(
                log,
                "[Driver] {}",
                tr_args!(
                    "live_driver_progress_chunked",
                    name = &display_name,
                    downloaded = format!("{downloaded_mb:.1}"),
                    speed = format!("{speed_mbps:.1}"),
                )
            );
        }
        DownloadEvent::Done {
            downloaded_mb,
            elapsed_s,
        } => {
            live!(
                log,
                "[Driver] {}",
                tr_args!(
                    "live_driver_dl_done",
                    name = &display_name,
                    size = format!("{downloaded_mb:.1}"),
                    elapsed = format!("{elapsed_s:.1}"),
                )
            );
        }
    })
    .map_err(|e| DriverError::Http(format!("download: {e}")))
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }
    Ok(())
}

fn walk_collect_infs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_collect_infs(&path, out);
        } else if let Some(ext) = path.extension()
            && ext.eq_ignore_ascii_case("inf")
            && path.components().any(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("Windows10")
            })
        {
            out.push(path);
        }
    }
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Missing(...)` must never carry an empty vec — empty list
    /// would make the GUI banner say "missing nothing" which is
    /// confusing.
    #[test]
    fn missing_list_is_empty_when_all_present() {
        if let DriverStatus::Missing(list) = check_required_drivers() {
            assert!(!list.is_empty());
        }
    }
}
