//! Linux device-access provisioning: detect and install the LTBox udev rules
//! that grant the desktop user libusb / serial access to the Qualcomm EDL
//! (`05c6:9008`), Lenovo (`17ef`), and Google ADB (`18d1`) USB IDs.
//! See `misc/udev/51-ltbox-qcom.rules`.
//!
//! There is no driver *download* on Linux: the rules ship embedded in the
//! binary and are written by the privileged `ltbox --install-udev` entry
//! point. The GUI install button re-launches the binary through `pkexec` so
//! the user authorizes the write via polkit instead of a terminal.
//!
//! Deferred until a Lenovo Qualcomm target is available on Linux: the
//! `/sys/bus/usb/devices` walk for `05c6:9008` + serial-node permission test
//! (a `DevicePresentNoPermission`-style state). Rules presence / staleness is
//! pure filesystem state and is implemented + tested here today.

use super::{DriverError, DriverStatus, DriverUpdate, Result, classify_udev_rules};

/// Where `ltbox --install-udev` writes the rules; kept in sync with the GUI's
/// `UDEV_RULES_PATH`.
const UDEV_RULES_PATH: &str = "/etc/udev/rules.d/51-ltbox-qcom.rules";

pub fn check_required_drivers() -> DriverStatus {
    match std::fs::read_to_string(UDEV_RULES_PATH) {
        Ok(content) => classify_udev_rules(Some(&content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => classify_udev_rules(None),
        // File exists but is unreadable (permission or otherwise) — surface a
        // repairable state rather than silently claiming the rules are fine.
        Err(_) => DriverStatus::UdevRulesNoPermission,
    }
}

/// No versioned Linux driver release to compare against — the rules ship as a
/// static embedded file, so staleness is detected by content in
/// [`check_required_drivers`] rather than via a version banner.
pub fn check_driver_update() -> Option<DriverUpdate> {
    None
}

/// Install (or refresh) the udev rules by re-launching this binary through
/// `pkexec` with the fixed `--install-udev` flag. Only the binary's own
/// resolved path is passed — never user input.
pub fn download_and_install(log: &mut Vec<String>) -> Result<()> {
    if check_required_drivers() == DriverStatus::Present {
        log.push("[driver] udev rules already up to date".to_string());
        return Ok(());
    }

    let exe = std::env::current_exe().map_err(|e| {
        DriverError::Io(std::io::Error::new(
            e.kind(),
            format!("cannot resolve the LTBox executable path: {e}"),
        ))
    })?;
    if !exe.is_file() {
        return Err(DriverError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("LTBox executable not found at {}", exe.display()),
        )));
    }

    // Require pkexec — never silently fall back to a terminal `sudo` from the
    // GUI, which has no controlling terminal to prompt on.
    let pkexec = which_pkexec().ok_or_else(|| {
        DriverError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "pkexec not found — install polkit, or run `sudo ltbox --install-udev` in a terminal"
                .to_string(),
        ))
    })?;

    log.push(format!("[driver] pkexec {} --install-udev", exe.display()));
    let status = std::process::Command::new(pkexec)
        .arg(&exe)
        .arg("--install-udev")
        .status()
        .map_err(|e| DriverError::Io(std::io::Error::new(e.kind(), format!("pkexec: {e}"))))?;

    match status.code() {
        Some(0) => {}
        // polkit authorization denied / dialog dismissed → pkexec exits 126/127.
        Some(126 | 127) => return Err(DriverError::InstallCancelled),
        Some(code) => return Err(DriverError::InstallerFailed { exit_code: code }),
        None => {
            return Err(DriverError::Io(std::io::Error::other(
                "pkexec terminated by a signal",
            )));
        }
    }

    // Confirm the write actually landed before reporting success.
    if check_required_drivers() != DriverStatus::Present {
        return Err(DriverError::Io(std::io::Error::other(
            "udev rules still not in place after install",
        )));
    }
    log.push("[driver] udev rules installed".to_string());
    Ok(())
}

/// Locate `pkexec` on `PATH` without pulling in a `which` dependency.
fn which_pkexec() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("pkexec"))
        .find(|p| p.is_file())
}
