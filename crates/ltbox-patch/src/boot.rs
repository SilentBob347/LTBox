//! Boot image patching — wraps magiskboot for root operations.

use fs_err as fs;
use std::path::Path;

use ltbox_core::{LtboxError, Result};

/// Unpack a boot image into components.
pub fn unpack(image: &Path, work_dir: &Path) -> Result<i32> {
    let name = image
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("boot.img");
    let dst = work_dir.join(name);
    if image != dst {
        fs::copy(image, &dst).map_err(|e| LtboxError::BootImage(e.to_string()))?;
    }
    run_magiskboot(work_dir, &["unpack", name])
}

/// Repack boot image from components.
pub fn repack(orig_image: &str, work_dir: &Path) -> Result<()> {
    run_magiskboot(work_dir, &["repack", orig_image])?;
    Ok(())
}

/// CPIO operations on ramdisk.
pub fn cpio(work_dir: &Path, cpio_file: &str, commands: &[&str]) -> Result<i32> {
    let mut args = vec!["cpio", cpio_file];
    args.extend_from_slice(commands);
    run_magiskboot(work_dir, &args)
}

/// SHA1 hash of a file (computed in Rust, no magiskboot needed).
pub fn sha1(file_path: &Path) -> Result<String> {
    let data = fs::read(file_path).map_err(|e| LtboxError::BootImage(e.to_string()))?;
    Ok(sha1_hash(&data))
}

/// Compress a file.
pub fn compress(work_dir: &Path, format: &str, input: &str, output: &str) -> Result<()> {
    run_magiskboot(work_dir, &[&format!("compress={format}"), input, output])?;
    Ok(())
}

/// Cleanup temporary files.
pub fn cleanup(work_dir: &Path) -> Result<()> {
    run_magiskboot(work_dir, &["cleanup"])?;
    Ok(())
}

/// Get kernel version from a kernel binary.
pub fn get_kernel_version(kernel_path: &Path) -> Result<Option<String>> {
    let data = fs::read(kernel_path).map_err(|e| LtboxError::BootImage(e.to_string()))?;
    let needle = b"Linux version ";
    if let Some(pos) = data.windows(needle.len()).position(|w| w == needle) {
        let ver: String = data[pos + needle.len()..]
            .iter()
            .take_while(|&&b| b.is_ascii_digit() || b == b'.')
            .map(|&b| b as char)
            .collect();
        if !ver.is_empty() {
            return Ok(Some(ver));
        }
    }
    Ok(None)
}

/// Process-wide CWD guard: `boot_main` resolves filenames relative to CWD,
/// so we must `chdir` into `work_dir`. PollDevice fires concurrently via
/// `spawn_blocking` — a static mutex serializes the chdir/run/restore sequence.
static MAGISKBOOT_CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_magiskboot(work_dir: &Path, args: &[&str]) -> Result<i32> {
    // Recover from poisoning: inner catch_unwind turns magiskboot panics
    // into errors, so the mutex stays safe to reuse.
    let _guard = MAGISKBOOT_CWD_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(work_dir).map_err(|e| LtboxError::BootImage(e.to_string()))?;

    let mut full_args = vec!["magiskboot".to_string()];
    full_args.extend(args.iter().map(|s| s.to_string()));
    let cmds = magiskboot::base::CmdArgs::from_env_args(full_args);

    // catch_unwind surfaces magiskboot-rs panics as Err so the GUI stays alive.
    let args_repr = args.join(" ");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        magiskboot::cli::boot_main(cmds).unwrap_or(1)
    }));

    if let Some(dir) = original_dir {
        let _ = std::env::set_current_dir(dir);
    }

    match result {
        Ok(code) => Ok(code),
        Err(_) => Err(LtboxError::BootImage(format!(
            "magiskboot panicked while running: {args_repr}"
        ))),
    }
}

fn sha1_hash(data: &[u8]) -> String {
    use digest::Digest;
    let mut h = sha1::Sha1::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
