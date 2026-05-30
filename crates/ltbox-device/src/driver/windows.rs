//! Qualcomm 9008 EDL USB driver detection + auto-install on Windows.
//!
//! Switched from `qcom-usb-kernel-drivers` (kernel-mode usbser COM port,
//! consumed via `serialport` / `QdlBackend::Serial`) to
//! `qcom-usb-userspace-drivers` (WinUSB stub via `qcserlib.inf`, consumed
//! via `nusb` / `QdlBackend::Usb`). The kernel-mode COM path had no read /
//! write timeout configured upstream (`qdl::serial` literal
//! `// TODO: timeouts?`) and `qdl::firehose_program_storage` panics via
//! `.expect("Error sending data")` on the first write hiccup, which surfaced
//! as a hard mid-flash process abort on stalled large-partition writes.
//! `QdlBackend::Usb` already configures explicit 10 s read / write timeouts
//! at the `nusb` endpoint level (`qdl::usb::setup_usb_device`) so the same
//! stall surfaces as a recoverable `io::Error` instead.
//!
//! Install path: download the signed, per-architecture
//! `qcom_usb_userspace_drivers_<arch>.exe` from the latest
//! `qcom-usb-userspace-drivers` GitHub release and launch it through
//! `Start-Process -Verb RunAs`. The installer carries a
//! `requireAdministrator` manifest and self-elevates via the Windows UAC
//! prompt, so **LTBox itself does not need to run as Administrator** — the
//! UAC prompt is the only elevation step. A dismissed prompt surfaces as a
//! distinct [`DriverError::InstallCancelled`] (vs a generic installer
//! failure) so the GUI can ask the user to approve and retry.
//!
//! Required-driver probe ("is EDL ready?") checks for `qcserlib.inf` —
//! that's the INF that binds `USB\VID_05C6&PID_9008` to the WinUSB stub
//! (the other INFs in the bundle cover ADB / modem / WWAN endpoints
//! LTBox does not drive directly). Presence probed via
//! `pnputil /enum-drivers`, then the DriverStore FileRepository as
//! fallback.
//!
//! Cross-platform `DriverStatus` / `DriverError` / `Result` types
//! live in `driver/mod.rs`; this file is only compiled on Windows
//! (gated by `#[cfg(windows)]` in `driver/mod.rs`) so every
//! `cfg!(windows)` runtime check from the pre-rename module folds
//! into compile-time guarantees here.

use std::path::Path;
use std::process::Command;

use ltbox_core::i18n::tr;
use ltbox_core::{live, tr_args};

use super::{DriverError, DriverStatus, Result};

/// `Command::new` + `CREATE_NO_WINDOW` so `pnputil` / `powershell` do not
/// flash a console. The elevated installer child spawned by
/// `Start-Process` shows its own UAC prompt + window regardless.
fn silent_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// INFs whose absence triggers a "missing driver" banner. Only the WinUSB
/// stub for EDL (`qcserlib.inf` — the INF that maps PID 9008 to
/// `libusb_Install → winusb.inf`) is required for LTBox's EDL path;
/// every other INF in the userspace bundle is installed alongside as a
/// quality-of-life bonus but not gating.
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

/// Download the signed userspace-driver installer for the host arch from
/// the latest `qcom-usb-userspace-drivers` release and run it elevated.
///
/// Every milestone routes through `live!` so the GUI streams progress in
/// real time — the previous `log.push` only surfaced after the whole task
/// returned, so a stalled download looked indistinguishable from a fast
/// success until the final timeout error fired.
///
/// Two ureq agents:
///   * `meta_agent` — 30 s global, used for the small JSON release listing.
///   * `dl_agent` — no global cap; per-stage `connect` / `recv-response` /
///     `recv-body` timeouts so a slow link can finish the ~700 KB installer
///     without a global-timeout guillotine cutting the body read partway
///     through.
///
/// The downloaded `.exe` self-elevates via UAC ([`run_installer_elevated`]),
/// so this whole flow runs without LTBox holding Administrator rights.
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

/// Launch the signed installer `.exe` elevated via PowerShell
/// `Start-Process -Verb RunAs`, blocking until it exits.
///
/// `-Verb RunAs` raises the Windows UAC prompt, so the installer runs with
/// Administrator rights while LTBox stays in its original (non-elevated)
/// context. `-Wait` blocks until the installer finishes; `-PassThru`
/// exposes the child's exit code. A dismissed UAC prompt makes
/// `Start-Process` throw a terminating error; the `try/catch` maps that to
/// exit `1223` (`ERROR_CANCELLED`) so the caller can distinguish a user
/// cancel from a real installer failure.
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
