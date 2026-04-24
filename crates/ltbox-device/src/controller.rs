//! Unified ADB / Fastboot / EDL state-transition controller.

use crate::adb::AdbManager;
use crate::edl;
use crate::fastboot::FastbootDevice;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMode {
    Unknown,
    Adb,
    Fastboot,
    Edl,
}

#[derive(Error, Debug)]
pub enum ControllerError {
    #[error("ADB error: {0}")]
    Adb(#[from] crate::adb::AdbError),
    #[error("Fastboot error: {0}")]
    Fastboot(#[from] crate::fastboot::FastbootError),
    #[error("EDL error: {0}")]
    Edl(#[from] crate::edl::EdlError),
    #[error("No device found in any mode")]
    NoDevice,
    #[error("Operation requires {0} mode")]
    WrongMode(String),
}

type Result<T> = std::result::Result<T, ControllerError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdlTransitionRoute {
    AlreadyEdl,
    AdbReboot,
    FastbootContinueThenAdb,
    ManualWait,
}

fn plan_edl_transition(in_edl: bool, in_fastboot: bool, skip_adb: bool) -> EdlTransitionRoute {
    if in_edl {
        EdlTransitionRoute::AlreadyEdl
    } else if in_fastboot && !skip_adb {
        EdlTransitionRoute::FastbootContinueThenAdb
    } else if skip_adb {
        EdlTransitionRoute::ManualWait
    } else {
        EdlTransitionRoute::AdbReboot
    }
}

pub struct DeviceController {
    pub adb: AdbManager,
    pub skip_adb: bool,
    mode: DeviceMode,
}

impl DeviceController {
    pub fn new() -> Self {
        Self {
            adb: AdbManager::new(),
            skip_adb: false,
            mode: DeviceMode::Unknown,
        }
    }

    /// Detect mode by probing each protocol.
    pub fn detect_mode(&mut self) -> DeviceMode {
        if edl::check_device() {
            self.mode = DeviceMode::Edl;
        } else if FastbootDevice::check_device() {
            self.mode = DeviceMode::Fastboot;
        } else if !self.skip_adb {
            if let Ok(true) = self.adb.check_device() {
                self.mode = DeviceMode::Adb;
            } else {
                self.mode = DeviceMode::Unknown;
            }
        } else {
            self.mode = DeviceMode::Unknown;
        }
        self.mode
    }

    pub fn current_mode(&self) -> DeviceMode {
        self.mode
    }

    pub fn ensure_fastboot(&mut self) -> Result<()> {
        if FastbootDevice::check_device() {
            self.mode = DeviceMode::Fastboot;
            return Ok(());
        }
        // skip_adb means we can't issue an ADB reboot — so waiting on a
        // Fastboot device that nothing is going to produce would hang the
        // GUI for the whole fastboot wait timeout. Surface immediately so
        // the caller can prompt the user for a manual transition.
        if self.skip_adb {
            return Err(ControllerError::NoDevice);
        }
        info!("Rebooting to bootloader via ADB...");
        self.adb.wait_for_device()?;
        self.adb.reboot("bootloader")?;
        info!("Waiting for Fastboot...");
        let _ = FastbootDevice::wait_for_device()?;
        self.mode = DeviceMode::Fastboot;
        Ok(())
    }

    pub fn ensure_edl(&mut self) -> Result<()> {
        match plan_edl_transition(
            edl::check_device(),
            FastbootDevice::check_device(),
            self.skip_adb,
        ) {
            EdlTransitionRoute::AlreadyEdl => {
                self.mode = DeviceMode::Edl;
                return Ok(());
            }
            EdlTransitionRoute::FastbootContinueThenAdb => {
                info!("Device in Fastboot, resuming boot for ADB EDL transition...");
                let mut dev = FastbootDevice::open()?;
                let _ = dev.continue_boot();
                info!("Waiting for ADB...");
                self.adb.wait_for_device()?;
                info!("Rebooting to EDL via ADB...");
                self.adb.reboot("edl")?;
            }
            EdlTransitionRoute::AdbReboot => {
                info!("Rebooting to EDL via ADB...");
                self.adb.wait_for_device()?;
                self.adb.reboot("edl")?;
            }
            EdlTransitionRoute::ManualWait => {
                let _ = edl::wait_for_device()?;
                self.mode = DeviceMode::Edl;
                return Ok(());
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(2));
        let _ = edl::wait_for_device()?;
        self.mode = DeviceMode::Edl;
        Ok(())
    }

    /// Active slot suffix; ensures Fastboot first.
    pub fn detect_active_slot(&mut self) -> Result<Option<String>> {
        self.ensure_fastboot()?;
        let mut dev = FastbootDevice::open()?;
        Ok(dev.get_slot_suffix()?)
    }
}

impl Default for DeviceController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edl_route_from_fastboot_prefers_adb_when_available() {
        assert_eq!(
            plan_edl_transition(false, true, false),
            EdlTransitionRoute::FastbootContinueThenAdb
        );
    }

    #[test]
    fn edl_route_from_fastboot_waits_manual_when_adb_skipped() {
        assert_eq!(
            plan_edl_transition(false, true, true),
            EdlTransitionRoute::ManualWait
        );
    }
}
