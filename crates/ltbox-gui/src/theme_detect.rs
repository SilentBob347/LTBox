//! OS theme detection — `true` when the host OS is in dark mode.
//!
//! Windows: reads `HKCU\...\Personalize\AppsUseLightTheme` (DWORD).
//! Other platforms return `false` until macOS / GNOME / KDE are wired up.

#[cfg(windows)]
pub fn system_prefers_dark() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let subkey = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value_name = wide("AppsUseLightTheme");
    let mut data: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    // SAFETY: subkey + value_name are null-terminated UTF-16 valid for
    // the call; `data` is a u32 passed by address with matching size.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    if status != 0 {
        return false;
    }
    data == 0
}

#[cfg(not(windows))]
pub fn system_prefers_dark() -> bool {
    false
}
