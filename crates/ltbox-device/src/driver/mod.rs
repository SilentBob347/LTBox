//! Cross-platform device-driver / udev-rule status + installer.
//!
//! Used by the GUI's startup driver-check banner + the Settings
//! "Install Drivers" action. Public surface stays the same regardless
//! of host OS so the GUI never has to branch by `cfg!(windows)`; the
//! platform impl picks how to interpret `Present` / `Missing` /
//! `NotWindows` for the local OS.
//!
//! ## Platform impl mapping
//!
//! | Host    | Module                  | Strategy |
//! |---------|-------------------------|----------|
//! | Windows | `driver::windows`       | `pnputil /enum-drivers` + DriverStore probe for `qcserlib.inf` (the WinUSB stub for PID 9008); install by downloading the signed per-arch `qcom_usb_userspace_drivers_<arch>.exe` from the latest `qcom-usb-userspace-drivers` release and launching it via UAC (`Start-Process -Verb RunAs`). The installer self-elevates, so LTBox itself does not need to run as Administrator. |
//! | Linux   | `driver::linux`         | Detect the LTBox udev rules at `/etc/udev/rules.d/51-ltbox-qcom.rules` and classify them against the embedded copy (`UdevRulesMissing` / `UdevRulesStale` / `UdevRulesNoPermission` / `Present`); install by re-launching the binary through `pkexec … --install-udev`. A `/sys/bus/usb/devices` walk for `05c6:9008` + serial-node permission test is deferred until a Qualcomm target is in reach. |
//! | macOS   | `driver::unsupported`   | No-op `NotWindows` — macOS needs no driver and no udev rules (libusb claims the device directly). |
//!
//! The shared `DriverStatus` / `DriverError` / `Result` types live here so
//! the GUI never branches by `cfg`; the per-OS module decides which variants
//! it can produce.

/// Shape returned by [`check_required_drivers`]. Windows produces
/// `Present` / `Missing`; Linux produces `Present` / `UdevRules*`; macOS
/// (and other hosts) produce `NotWindows`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatus {
    /// Host has no driver-action concept (macOS and other non-Windows,
    /// non-Linux hosts).
    NotWindows,
    /// Every required driver / rule is in place.
    Present,
    /// Windows: list of `.inf` filenames not yet installed.
    Missing(Vec<&'static str>),
    /// Linux: the LTBox udev rules file is not installed.
    UdevRulesMissing,
    /// Linux: a udev rules file is installed but its content differs from the
    /// rules LTBox bundles (an older LTBox wrote it, or the user edited it).
    UdevRulesStale,
    /// Linux: the udev rules file exists but could not be read to verify it
    /// (e.g. permission denied) — surfaced as a repairable state.
    UdevRulesNoPermission,
}

/// Result of comparing the locally-installed Qualcomm driver version
/// against the latest signed release on GitHub. Only produced when a
/// driver is already present AND a strictly-newer release exists — the
/// GUI uses its presence to decide whether to show the optional
/// "update available" banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverUpdate {
    /// Dotted version parsed from the installed `qcserlib.inf` `DriverVer`.
    pub current: String,
    /// Dotted version parsed from the latest Windows release tag.
    pub latest: String,
}

#[derive(thiserror::Error, Debug)]
pub enum DriverError {
    #[error("Not running on Windows — driver install is only supported on Windows")]
    NotWindows,
    // ureq has no thiserror-friendly root error, so collapse transport + status.
    #[error("Network error: {0}")]
    Http(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// GitHub release JSON could not be parsed.
    #[error("GitHub release parse error: {0}")]
    Parse(String),
    /// The latest `qcom-usb-userspace-drivers` release shipped no signed
    /// installer `.exe` for the host architecture.
    #[error("No matching signed installer found in the latest release")]
    NoAsset,
    /// The user dismissed the elevation prompt (Windows UAC for the signed
    /// installer, or polkit for the Linux `pkexec --install-udev` call), so
    /// the privileged step never ran. Distinct from `InstallerFailed` so the
    /// GUI can say "approve the prompt and try again" instead of a generic
    /// error. Neither path needs LTBox itself to run elevated — the prompt is
    /// the only elevation step.
    #[error("Driver install was cancelled at the elevation prompt.")]
    InstallCancelled,
    /// The signed installer `.exe` exited with a non-zero status.
    #[error("Driver installer exited with code {exit_code}.")]
    InstallerFailed { exit_code: i32 },
}

impl From<ureq::Error> for DriverError {
    fn from(e: ureq::Error) -> Self {
        DriverError::Http(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, DriverError>;

/// Best-effort reachability probe for the GitHub host LTBox downloads the
/// Windows Qualcomm-driver installer from. Used to pre-disable the install /
/// update buttons (with an "internet required" tooltip) instead of letting the
/// user click into a download that can only fail. A short timeout keeps a dead
/// network from stalling startup; any transport / non-2xx result reads as
/// offline.
#[cfg(windows)]
pub fn probe_connectivity() -> bool {
    // Bespoke agent: an 8s global timeout keeps a dead network from stalling
    // startup, so this probe does not reuse the shared pooled agent (which has
    // no global cap). It does reuse the shared user-agent string.
    let agent = ureq::Agent::config_builder()
        .user_agent(ltbox_core::downloader::USER_AGENT)
        .timeout_global(Some(std::time::Duration::from_secs(8)))
        .build()
        .new_agent();
    agent.get("https://api.github.com/").call().is_ok()
}

/// The driver install / update buttons this gates only exist on Windows
/// (Linux + macOS need no Qualcomm driver), so off-Windows this reports
/// "reachable" immediately — no startup network round-trip to GitHub.
#[cfg(not(windows))]
pub fn probe_connectivity() -> bool {
    true
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{check_driver_update, check_required_drivers, download_and_install};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{check_driver_update, check_required_drivers, download_and_install};

// macOS (and any other non-Windows, non-Linux target) has no driver / udev-rule
// concept — a no-op stub keeps the public surface identical so the GUI never
// branches by `cfg`.
#[cfg(not(any(windows, target_os = "linux")))]
mod unsupported;
#[cfg(not(any(windows, target_os = "linux")))]
pub use self::unsupported::{check_driver_update, check_required_drivers, download_and_install};

/// Canonical LTBox udev rules, embedded so the Linux probe can tell an
/// up-to-date install apart from a missing, stale, or hand-edited one. Same
/// source file the GUI's `--install-udev` writer embeds.
#[cfg(any(target_os = "linux", test))]
pub(crate) const UDEV_RULES: &str = include_str!("../../../../misc/udev/51-ltbox-qcom.rules");

/// Classify installed udev rules against [`UDEV_RULES`]. `installed` is the
/// file's content (`None` = file absent). Pure, so it is unit-tested on any
/// host even though the filesystem read that feeds it is Linux-only.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn classify_udev_rules(installed: Option<&str>) -> DriverStatus {
    match installed {
        None => DriverStatus::UdevRulesMissing,
        Some(content) if content == UDEV_RULES => DriverStatus::Present,
        Some(_) => DriverStatus::UdevRulesStale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_udev_rules_states() {
        assert_eq!(classify_udev_rules(None), DriverStatus::UdevRulesMissing);
        assert_eq!(classify_udev_rules(Some(UDEV_RULES)), DriverStatus::Present);
        assert_eq!(
            classify_udev_rules(Some("# hand-edited or older rules\n")),
            DriverStatus::UdevRulesStale
        );
    }
}
