//! Qualcomm 9008 EDL WinUSB driver detection + auto-install on Windows.
//!
//! LTBox requires `qcserlib.inf` from Qualcomm's userspace driver bundle,
//! then runs the signed per-arch installer through Windows UAC.

use std::path::Path;
use std::process::Command;

use ltbox_core::i18n::tr;
use ltbox_core::{live, tr_args};

use super::{DriverError, DriverStatus, Result};

/// `Command::new` with no console window.
fn silent_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// INFs whose absence triggers a missing-driver banner.
const REQUIRED_INFS: &[&str] = &["qcserlib.inf"];

#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
    #[serde(default)]
    draft: bool,
}

#[derive(Debug, serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

const RELEASES_API: &str =
    "https://api.github.com/repos/qualcomm/qcom-usb-userspace-drivers/releases?per_page=10";
/// Windows release tags carry a `win` token (`release-win-v1.0.2.0`); the
/// repo also publishes Linux-only tags that ship no `.exe` installer.
const WIN_TAG_NEEDLE: &str = "win";
const USER_AGENT: &str = concat!("ltbox/", env!("CARGO_PKG_VERSION"));

/// Signed installer asset name for the host architecture. The release
/// ships one self-extracting `.exe` per arch.
fn arch_installer_asset() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "qcom_usb_userspace_drivers_arm64.exe"
    } else if cfg!(target_arch = "x86") {
        "qcom_usb_userspace_drivers_x86.exe"
    } else {
        "qcom_usb_userspace_drivers_x64.exe"
    }
}

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

/// Download the host-arch userspace-driver installer and run it elevated.
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

    let asset_name = arch_installer_asset();
    let release = releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter(|r| r.tag_name.to_ascii_lowercase().contains(WIN_TAG_NEEDLE))
        .find(|r| {
            r.assets
                .iter()
                .any(|a| a.name.eq_ignore_ascii_case(asset_name))
        })
        .ok_or(DriverError::NoAsset)?;

    let asset_url = release
        .assets
        .into_iter()
        .find(|a| a.name.eq_ignore_ascii_case(asset_name))
        .map(|a| a.browser_download_url)
        .ok_or(DriverError::NoAsset)?;

    live!(
        log,
        "[Driver] {}",
        tr_args!("live_driver_asset", name = asset_name)
    );

    let tmp_dir = std::env::temp_dir().join(format!("ltbox_qcom_drv_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let exe_path = tmp_dir.join(asset_name);

    let dl_agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(30)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(300)))
        .build()
        .new_agent();

    download_with_progress(&dl_agent, &asset_url, asset_name, &exe_path, log)?;

    live!(log, "[Driver] {}", tr("live_driver_running_installer"));
    let result = run_installer_elevated(&exe_path, log);
    cleanup(&tmp_dir);
    result?;

    live!(log, "[Driver] {}", tr("live_driver_install_finished"));
    Ok(())
}

/// Run the signed installer through UAC and map cancel vs failure.
fn run_installer_elevated(exe: &Path, log: &mut Vec<String>) -> Result<()> {
    // Escape for a PowerShell single-quoted string literal (`'` → `''`).
    // The temp path is process-id-derived so quotes are not expected, but
    // escape defensively rather than trust the environment.
    let exe_str = exe.to_string_lossy().replace('\'', "''");
    // `$p.ExitCode` can be `$null` for some self-extracting installers that
    // hand off to a detached child; `exit $null` would silently become exit
    // 0 and report a false success. Treat a null exit code as a failure
    // (exit 1) so the caller surfaces `InstallerFailed` instead of a green
    // toast over a driver that never actually installed.
    let script = format!(
        "try {{ $p = Start-Process -FilePath '{exe_str}' -Verb RunAs -Wait -PassThru \
         -ErrorAction Stop; if ($null -eq $p.ExitCode) {{ exit 1 }} else {{ exit $p.ExitCode }} }} \
         catch {{ exit 1223 }}"
    );

    let out = silent_command("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(&script)
        .output()
        .map_err(DriverError::Io)?;

    let code = out.status.code().unwrap_or(-1);
    match code {
        0 => Ok(()),
        // ERROR_CANCELLED — the user dismissed the UAC elevation prompt.
        1223 => {
            live!(log, "[Driver] {}", tr("live_driver_install_cancelled"));
            Err(DriverError::InstallCancelled)
        }
        other => {
            live!(
                log,
                "[Driver] {}",
                tr_args!("live_driver_installer_failed", exit = other)
            );
            Err(DriverError::InstallerFailed { exit_code: other })
        }
    }
}

/// Stream the installer download with driver-flow log formatting.
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

    /// Host-arch installer asset name resolves to one of the three
    /// shipped variants.
    #[test]
    fn arch_asset_is_known() {
        let name = arch_installer_asset();
        assert!(
            matches!(
                name,
                "qcom_usb_userspace_drivers_x64.exe"
                    | "qcom_usb_userspace_drivers_arm64.exe"
                    | "qcom_usb_userspace_drivers_x86.exe"
            ),
            "unexpected asset name: {name}"
        );
    }
}
