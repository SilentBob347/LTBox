//! Magisk root patching — extracts payload from the Magisk APK and
//! patches `init_boot.img` via the magiskboot library (no shell-out).
//! APK layout: `lib/arm64-v8a/libmagisk{,64}.so` → `magisk`,
//! `libmagiskinit.so` → `magiskinit`, `libinit-ld.so` → `init-ld`,
//! `assets/stub.apk` → `stub.apk`.

use fs_err as fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ltbox_core::i18n::tr;
use ltbox_core::{LtboxError, Result};

use crate::boot;

/// Files extracted from the APK and staged for the magiskboot cpio pass.
pub const PAYLOAD_FILES: &[&str] = &["magisk", "magiskinit", "init-ld", "stub.apk"];

/// Resolve Magisk's `PREINITDEVICE` partition name from `/proc/self/mountinfo`.
/// Returns the bare partition name (e.g. `"metadata"`), not a block path.
///
/// Priority: `/metadata` > `/persist` | `/mnt/vendor/persist` > `/klogdump` >
/// `/cache` > `/data`. Within a tier, ext4 beats f2fs. Read-only, non-`/`-root,
/// pseudo-fs, and device-mapper mounts are filtered out.
pub fn resolve_preinit_device(mountinfo: &str) -> Option<String> {
    const PRIO_METADATA: u32 = 5;
    const PRIO_PERSIST: u32 = 4;
    const PRIO_KLOGDUMP: u32 = 3;
    const PRIO_CACHE: u32 = 2;
    const PRIO_DATA: u32 = 1;

    fn priority_for_mount(target: &str) -> Option<u32> {
        match target {
            "/metadata" => Some(PRIO_METADATA),
            "/persist" | "/mnt/vendor/persist" => Some(PRIO_PERSIST),
            "/klogdump" => Some(PRIO_KLOGDUMP),
            "/cache" => Some(PRIO_CACHE),
            "/data" => Some(PRIO_DATA),
            _ => None,
        }
    }

    // (priority, fs_preference, partition_name); higher tuple wins under Ord (ext4=1, f2fs=0).
    let mut candidates: Vec<(u32, u32, String)> = Vec::new();

    for line in mountinfo.lines() {
        // mountinfo: `mount_id parent root mount_point options - fs_type source super_options`
        let (pre, post) = match line.split_once(" - ") {
            Some(p) => p,
            None => continue,
        };
        let pre_parts: Vec<&str> = pre.split_whitespace().collect();
        let post_parts: Vec<&str> = post.split_whitespace().collect();
        if pre_parts.len() < 6 || post_parts.len() < 3 {
            continue;
        }
        let root = pre_parts[3];
        let target = pre_parts[4];
        let mount_opts = pre_parts[5];
        let fs_type = post_parts[0];
        let source = post_parts[1];

        if root != "/" {
            continue;
        }
        if !source.starts_with('/') {
            continue;
        }
        if source.contains("/dm-") {
            continue;
        }
        if fs_type != "ext4" && fs_type != "f2fs" {
            continue;
        }
        if !mount_opts.split(',').any(|o| o == "rw") {
            continue;
        }
        // Accept only /dev/block/by-name/* or /dev/block/* sources.
        let parent_ok = {
            let trimmed = source.trim_end_matches(|c| c != '/').trim_end_matches('/');
            trimmed.ends_with("by-name") || trimmed.ends_with("block")
        };
        if !parent_ok {
            continue;
        }

        let Some(prio) = priority_for_mount(target) else {
            continue;
        };
        let fs_pref = if fs_type == "ext4" { 1 } else { 0 };
        let name = source.rsplit('/').next().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        candidates.push((prio, fs_pref, name));
    }

    candidates.sort_by(|a, b| b.cmp(a));
    candidates.into_iter().next().map(|(_, _, name)| name)
}

/// Extract Magisk payload from `apk_path` into `staging_dir`, overwriting
/// existing files. Only the entries in [`PAYLOAD_FILES`] are written.
/// Accepts both `libmagisk.so` (newer) and `libmagisk64.so` (older) for `magisk`.
pub fn extract_apk_payload(apk_path: &Path, staging_dir: &Path) -> Result<()> {
    fs::create_dir_all(staging_dir)?;

    let file = fs::File::open(apk_path)
        .map_err(|e| LtboxError::Patch(format!("open APK {}: {e}", apk_path.display())))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| LtboxError::Patch(format!("APK read: {e}")))?;

    let mapping: &[(&[&str], &str)] = &[
        (
            &["lib/arm64-v8a/libmagisk.so", "lib/arm64-v8a/libmagisk64.so"],
            "magisk",
        ),
        (&["lib/arm64-v8a/libmagiskinit.so"], "magiskinit"),
        (&["lib/arm64-v8a/libinit-ld.so"], "init-ld"),
        (&["assets/stub.apk"], "stub.apk"),
    ];

    let mut found_any = Vec::with_capacity(mapping.len());
    for (candidates, dst_name) in mapping {
        let mut staged = false;
        for entry_name in candidates.iter() {
            let mut entry = match zip.by_name(entry_name) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut buf)
                .map_err(|e| LtboxError::Patch(format!("APK read {entry_name}: {e}")))?;

            let dst_path = staging_dir.join(dst_name);
            let mut out = fs::File::create(&dst_path)?;
            out.write_all(&buf)?;
            staged = true;
            break;
        }
        if !staged {
            return Err(LtboxError::Patch(format!(
                "APK missing entry for {dst_name} (checked {candidates:?})"
            )));
        }
        found_any.push(dst_name);
    }
    let _ = found_any;
    Ok(())
}

/// Patch `init_boot.img` with Magisk. `work_dir` must contain the image
/// plus the four payload files from [`extract_apk_payload`]. Writes
/// `work_dir/new-boot.img`; caller handles resign + flash.
///
/// `preinit_device` → Magisk `PREINITDEVICE` config. Empty string omits
/// the entry and lets Magisk resolve at boot (can fail on some devices).
pub fn patch_init_boot(
    work_dir: &Path,
    preinit_device: &str,
    log: &mut Vec<String>,
) -> Result<PathBuf> {
    let img_name = "init_boot.img";
    let img_path = work_dir.join(img_name);
    if !img_path.exists() {
        return Err(LtboxError::Patch(format!(
            "init_boot.img not found in {}",
            work_dir.display()
        )));
    }

    log.push(format!("[Magisk] {}", tr("log_magisk_unpack_initboot")));
    boot::unpack(&img_path, work_dir)?;

    let ramdisk = work_dir.join("ramdisk.cpio");
    if !ramdisk.exists() {
        return Err(LtboxError::Patch(
            "ramdisk.cpio missing after unpack — boot image has no ramdisk".into(),
        ));
    }

    // magiskboot cpio test: 0=stock, 1=already-patched, 2=unsupported.
    log.push(format!("[Magisk] {}", tr("log_magisk_cpio_test")));
    let status = boot::cpio(work_dir, "ramdisk.cpio", &["test"])?;
    match status {
        0 => {}
        1 => {
            return Err(LtboxError::Patch(
                "init_boot.img is already Magisk-patched — flash stock first".into(),
            ));
        }
        other => {
            return Err(LtboxError::Patch(format!(
                "Unsupported boot image layout (cpio test = {other})"
            )));
        }
    }

    // SHA-1 of stock boot for Magisk config.
    let sha1 = boot::sha1(&img_path)?;
    log.push(format!("[Magisk] {}: {sha1}", tr("log_magisk_stock_sha1")));

    // Back up stock ramdisk so Magisk can restore on unroot.
    fs::copy(&ramdisk, work_dir.join("ramdisk.cpio.orig"))?;

    log.push(format!("[Magisk] {}", tr("log_magisk_compressing_payload")));
    boot::compress(work_dir, "xz", "magisk", "magisk.xz")?;
    boot::compress(work_dir, "xz", "stub.apk", "stub.xz")?;
    boot::compress(work_dir, "xz", "init-ld", "init-ld.xz")?;

    // Config file baked into .backup/.magisk.
    let mut config = String::new();
    config.push_str("KEEPVERITY=true\n");
    config.push_str("KEEPFORCEENCRYPT=true\n");
    config.push_str("RECOVERYMODE=false\n");
    config.push_str("VENDORBOOT=false\n");
    if !preinit_device.is_empty() {
        config.push_str(&format!("PREINITDEVICE={preinit_device}\n"));
    }
    config.push_str(&format!("SHA1={sha1}\n"));
    fs::write(work_dir.join("config"), &config)?;

    // Patch ramdisk in one cpio pass.
    log.push(format!("[Magisk] {}", tr("log_magisk_cpio_patch")));
    let cpio_cmds: &[&str] = &[
        "add 0750 init magiskinit",
        "mkdir 0750 overlay.d",
        "mkdir 0750 overlay.d/sbin",
        "add 0644 overlay.d/sbin/magisk.xz magisk.xz",
        "add 0644 overlay.d/sbin/stub.xz stub.xz",
        "add 0644 overlay.d/sbin/init-ld.xz init-ld.xz",
        "patch",
        "backup ramdisk.cpio.orig",
        "mkdir 000 .backup",
        "add 000 .backup/.magisk config",
    ];
    boot::cpio(work_dir, "ramdisk.cpio", cpio_cmds)?;

    // Clean up staging — don't leave plaintext payload next to the repacked image.
    for name in [
        "ramdisk.cpio.orig",
        "config",
        "magisk.xz",
        "stub.xz",
        "init-ld.xz",
    ] {
        let _ = fs::remove_file(work_dir.join(name));
    }

    log.push(format!("[Magisk] {}", tr("log_magisk_repack_initboot")));
    boot::repack(img_name, work_dir)?;

    let new_boot = work_dir.join("new-boot.img");
    if !new_boot.exists() {
        return Err(LtboxError::Patch(
            "magiskboot repack produced no new-boot.img".into(),
        ));
    }
    log.push(format!("[Magisk] {}", tr("log_magisk_patch_complete")));
    Ok(new_boot)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal mountinfo line template; only parser-relevant fields matter.
    fn line(target: &str, opts: &str, fs: &str, source: &str) -> String {
        format!("1 1 259:1 / {target} {opts} shared:1 - {fs} {source} rw",)
    }

    #[test]
    fn picks_metadata_over_persist() {
        let info = format!(
            "{}\n{}",
            line(
                "/persist",
                "rw,seclabel",
                "ext4",
                "/dev/block/by-name/persist"
            ),
            line(
                "/metadata",
                "rw,seclabel",
                "ext4",
                "/dev/block/by-name/metadata"
            ),
        );
        assert_eq!(resolve_preinit_device(&info).as_deref(), Some("metadata"));
    }

    #[test]
    fn prefers_ext4_over_f2fs_within_same_tier() {
        let info = format!(
            "{}\n{}",
            line(
                "/persist",
                "rw,seclabel",
                "f2fs",
                "/dev/block/by-name/persist"
            ),
            line(
                "/mnt/vendor/persist",
                "rw,seclabel",
                "ext4",
                "/dev/block/by-name/vendor_persist"
            ),
        );
        assert_eq!(
            resolve_preinit_device(&info).as_deref(),
            Some("vendor_persist")
        );
    }

    #[test]
    fn drops_readonly_mounts() {
        let info = line(
            "/metadata",
            "ro,seclabel",
            "ext4",
            "/dev/block/by-name/metadata",
        );
        assert_eq!(resolve_preinit_device(&info), None);
    }

    #[test]
    fn drops_device_mapper_sources() {
        let info = line("/metadata", "rw", "ext4", "/dev/block/dm-5");
        assert_eq!(resolve_preinit_device(&info), None);
    }

    #[test]
    fn drops_tmpfs_mounts() {
        let info = "1 1 0:1 / /metadata rw shared:1 - tmpfs tmpfs rw".to_string();
        assert_eq!(resolve_preinit_device(&info), None);
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(resolve_preinit_device(""), None);
    }
}
