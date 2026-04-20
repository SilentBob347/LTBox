//! ADB client via `adb_client` crate (ADB server at localhost:5037).

use adb_client::ADBDeviceExt;
use adb_client::RebootType;
use adb_client::server::ADBServer;
use adb_client::server_device::ADBServerDevice;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AdbError {
    #[error("ADB error: {0}")]
    Client(String),
    #[error("Device not found")]
    DeviceNotFound,
    #[error("Command failed: {0}")]
    CommandFailed(String),
    #[error("Timeout waiting for device")]
    Timeout,
}

type Result<T> = std::result::Result<T, AdbError>;

pub struct AdbManager {
    server_addr: SocketAddrV4,
    serial: Option<String>,
    pub skip_adb: bool,
    pub connected_once: bool,
}

impl AdbManager {
    pub fn new() -> Self {
        Self {
            server_addr: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5037),
            serial: None,
            skip_adb: false,
            connected_once: false,
        }
    }

    fn server(&self) -> ADBServer {
        ADBServer::new(self.server_addr)
    }

    fn device(&self) -> Result<ADBServerDevice> {
        let serial = self.serial.clone().ok_or(AdbError::DeviceNotFound)?;
        Ok(ADBServerDevice::new(serial, Some(self.server_addr)))
    }

    /// Probe for any ADB device; updates stored serial.
    pub fn check_device(&mut self) -> Result<bool> {
        let mut server = self.server();
        match server.devices() {
            Ok(devices) => {
                if let Some(dev) = devices.first() {
                    self.serial = Some(dev.identifier.clone());
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(_) => Ok(false),
        }
    }

    /// Like `check_device` but returns the raw ADB state token
    /// (`"device"`, `"unauthorized"`, …) so callers can pattern-match
    /// without importing `adb_client::DeviceState`.
    pub fn check_device_state(&mut self) -> Result<Option<&'static str>> {
        let mut server = self.server();
        let Ok(devices) = server.devices() else {
            return Ok(None);
        };
        let Some(dev) = devices.into_iter().next() else {
            return Ok(None);
        };
        self.serial = Some(dev.identifier);
        Ok(Some(match dev.state {
            adb_client::server::DeviceState::Device => "device",
            adb_client::server::DeviceState::Unauthorized => "unauthorized",
            adb_client::server::DeviceState::Authorizing => "authorizing",
            adb_client::server::DeviceState::Offline => "offline",
            adb_client::server::DeviceState::Recovery => "recovery",
            adb_client::server::DeviceState::Bootloader => "bootloader",
            adb_client::server::DeviceState::Sideload => "sideload",
            adb_client::server::DeviceState::Rescue => "rescue",
            adb_client::server::DeviceState::Connecting => "connecting",
            adb_client::server::DeviceState::NoPerm => "noperm",
            adb_client::server::DeviceState::Detached => "detached",
            adb_client::server::DeviceState::Host => "host",
            adb_client::server::DeviceState::NoDevice => "no device",
        }))
    }

    pub fn wait_for_device(&mut self) -> Result<()> {
        if self.skip_adb {
            return Err(AdbError::DeviceNotFound);
        }
        loop {
            if self.check_device()? {
                self.connected_once = true;
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    /// Run shell command; returns trimmed stdout.
    pub fn shell(&self, cmd: &str) -> Result<String> {
        let mut dev = self.device()?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        dev.shell_command(
            &cmd.to_string(),
            Some(&mut stdout as &mut dyn std::io::Write),
            Some(&mut stderr as &mut dyn std::io::Write),
        )
        .map_err(|e| AdbError::CommandFailed(e.to_string()))?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_string())
    }

    pub fn get_model(&self) -> Result<Option<String>> {
        match self.shell("getprop ro.product.model") {
            Ok(m) if !m.is_empty() => Ok(Some(m)),
            _ => Ok(None),
        }
    }

    /// Active slot suffix (`_a` or `_b`).
    pub fn get_slot_suffix(&self) -> Result<Option<String>> {
        match self.shell("getprop ro.boot.slot_suffix") {
            Ok(s) if !s.is_empty() => Ok(Some(s)),
            _ => Ok(None),
        }
    }

    pub fn get_kernel_version(&self) -> Result<Option<String>> {
        match self.shell("cat /proc/version") {
            Ok(v) => {
                if let Some(start) = v.find("Linux version ") {
                    let rest = &v[start + 14..];
                    let ver: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    if !ver.is_empty() {
                        return Ok(Some(ver));
                    }
                }
                Ok(None)
            }
            Err(_) => Ok(None),
        }
    }

    pub fn reboot(&mut self, target: &str) -> Result<()> {
        let mut dev = self.device()?;
        let reboot_type = match target {
            "bootloader" => RebootType::Bootloader,
            "recovery" => RebootType::Recovery,
            "sideload" => RebootType::Sideload,
            _ => RebootType::System,
        };
        // RebootType has no EDL variant; fall back to shell.
        if target == "edl" {
            self.shell("reboot edl")?;
            return Ok(());
        }
        dev.reboot(reboot_type)
            .map_err(|e| AdbError::CommandFailed(e.to_string()))
    }

    pub fn install(&self, apk_path: &str) -> Result<()> {
        let mut dev = self.device()?;
        let path = Path::new(apk_path);
        dev.install(path, None)
            .map_err(|e| AdbError::CommandFailed(e.to_string()))
    }
}

impl Default for AdbManager {
    fn default() -> Self {
        Self::new()
    }
}
