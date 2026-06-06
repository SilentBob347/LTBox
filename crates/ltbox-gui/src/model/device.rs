//! Device + connection identity model: the device-class classifier
//! and the live connection state, split out of `main.rs`.

use crate::theme::Palette;

/// Classifies the device model into a known SKU so wizard gates ask
/// "what device class are we on?" once instead of comparing the raw
/// `device_model` string at each call site.
///
/// `Generic` covers every supported Lenovo tablet that doesn't need a
/// special branch — Y700 2nd / 3rd / 4th gen, the Yoga / Xiaoxin
/// rebrands, etc. They share the standard `xbl_s_devprg_ns.melf`
/// loader and full ROW + OtherRegion flash flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceClass {
    /// TB320FC — Lenovo Yoga Pad Pro AI / Yoga Tab Plus AI. Root flow
    /// limited to KernelSU GKI + APatch family.
    TB320FC,
    /// TB322FC — Lenovo Xiaoxin Pad Pro GT (PRC-only SKU). Flash
    /// wizard hides ROW + OtherRegion + non-CN country picks.
    TB322FC,
    /// TB323FU — Lenovo Legion Tab Y700 5th Gen (Kaanapali). Requires
    /// the multi-image `qsahara_device_programmer.xml` Sahara manifest
    /// rather than a single `.melf` loader.
    TB323FU,
    /// Any other supported model. No special-case gates apply.
    Generic,
}

impl DeviceClass {
    pub(crate) fn from_model(model: &str) -> Self {
        if model.eq_ignore_ascii_case("TB320FC") {
            Self::TB320FC
        } else if model.eq_ignore_ascii_case("TB322FC") {
            Self::TB322FC
        } else if model.eq_ignore_ascii_case("TB323FU") {
            Self::TB323FU
        } else {
            Self::Generic
        }
    }
}

/// Lenovo tablets that expose two USB-C ports. Only the port on the long
/// edge carries the USB data lines EDL/ADB need; the short-edge port is
/// charge-only on these SKUs, so LTBox advises the user to use the long-edge
/// one. (TB321FU is included here even though it is not a `DeviceClass`
/// special case — the advisory is about physical ports, not flash flow.)
pub(crate) const DUAL_USBC_MODELS: [&str; 4] = ["TB320FC", "TB321FU", "TB322FC", "TB323FU"];

/// Whether `model` is one of the [`DUAL_USBC_MODELS`] (case-insensitive).
pub(crate) fn is_dual_usbc_model(model: &str) -> bool {
    DUAL_USBC_MODELS
        .iter()
        .any(|m| model.eq_ignore_ascii_case(m))
}

/// Every supported Lenovo tablet enforces AVB rollback protection EXCEPT the
/// PRC-only TB322FC. Used to decide whether a missing fastboot
/// `stored_rollback_index` means "no ARB, skip" (TB322FC) or "ARB present but
/// fastboot can't report it, read it over EDL" (everything else). An unknown
/// model is treated as protected — safer to read + honour the index than to
/// skip and risk a rollback-rejected downgrade.
pub(crate) fn is_rollback_protected_model(model: &str) -> bool {
    !model.eq_ignore_ascii_case("TB322FC")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ConnectionStatus {
    #[default]
    None,
    Adb,
    /// ADB inside a TWRP recovery build (`ro.product.device` starts
    /// with `twrp_`). Same transition rules as `Adb`; different label.
    AdbRecovery,
    /// ADB sees the device but USB-debug auth is unaccepted
    /// (`unauthorized` / `authorizing`). Shell probes fail; dashboard
    /// shows an authorize-debug prompt.
    AdbUnauthorized,
    /// An external `adb.exe` server (or anything else listening on
    /// `127.0.0.1:5037`) is holding the Android USB interface
    /// exclusively, so LTBox's libusb claim returns `LIBUSB_ERROR_BUSY`
    /// even though the device is physically authorized. Distinct from
    /// `AdbUnauthorized` so the dashboard can offer "kill server"
    /// instead of asking the user to re-tap "Allow USB debugging".
    AdbServerBlocking,
    Fastboot,
    Edl,
}
impl ConnectionStatus {
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            Self::None => "conn_disconnected",
            Self::Adb => "conn_adb",
            Self::AdbRecovery => "conn_adb_recovery",
            Self::AdbUnauthorized => "conn_adb_unauthorized",
            Self::AdbServerBlocking => "conn_adb_server_blocking",
            Self::Fastboot => "conn_fastboot",
            Self::Edl => "conn_edl",
        }
    }
    pub(crate) fn color(&self, pal: &Palette) -> iced::Color {
        match self {
            Self::None => pal.on_surface_variant,
            Self::Adb | Self::AdbRecovery => pal.success,
            Self::AdbUnauthorized | Self::AdbServerBlocking => pal.warning,
            Self::Fastboot => pal.warning,
            Self::Edl => pal.tertiary,
        }
    }
    /// True when exec paths should skip the ADB probe. AdbUnauthorized
    /// + AdbServerBlocking count as "no usable ADB" — shell would fail.
    pub(crate) fn skip_adb(self) -> bool {
        matches!(
            self,
            Self::Fastboot | Self::Edl | Self::AdbUnauthorized | Self::AdbServerBlocking
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdlEntryAction {
    AlreadyEdl,
    AdbReboot,
    FastbootRebootThenAdb,
    ManualWait,
}
