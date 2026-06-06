//! APatch / FolkPatch kernel patching via kptools-rs library.
//!
//! Replaces the v2 `kptools.exe unpack → -p → repack` subprocess chain
//! with direct library calls inside `work_dir`.

use fs_err as fs;
use std::path::{Path, PathBuf};

use kptools::bootimg;
use kptools::patch::{self, ExtraConfig, PatchArgs};
use kptools::preset::ExtraType;

use ltbox_core::{LtboxError, Result, tr_args};

const SUPERKEY_MIN: usize = 8;
const SUPERKEY_MAX: usize = 63;

/// Validate superkey: 8–63 chars, ASCII alphanumeric only.
pub(crate) fn validate_superkey(sk: &str) -> Result<()> {
    let n = sk.len();
    if !(SUPERKEY_MIN..=SUPERKEY_MAX).contains(&n) {
        return Err(LtboxError::Patch(format!(
            "superkey must be {SUPERKEY_MIN}..={SUPERKEY_MAX} chars, got {n}"
        )));
    }
    if !sk.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(LtboxError::Patch(
            "superkey must be ASCII alphanumeric only".into(),
        ));
    }
    Ok(())
}

/// Patch `work_dir/boot.img` with `work_dir/kpimg` + optional KPMs.
/// Returns `work_dir/boot_patched.img`. All intermediates land in `work_dir`.
pub fn patch_boot(
    work_dir: &Path,
    kpm_paths: &[PathBuf],
    superkey: &str,
    log: &mut Vec<String>,
) -> Result<PathBuf> {
    validate_superkey(superkey)?;

    // kptools-base defaults LOG_ENABLE off so library embedders don't get
    // `[+]`/`[?]`/`[-]` chatter on stderr unasked. We want it: stderr is
    // tapped by the GUI's stdout_tap into the live log panel, and the
    // per-step context is the only window into what kptools is doing
    // mid-patch. Idempotent — safe to call per-invocation.
    kptools::log::set_log_enable(true);

    let boot_in = work_dir.join("boot.img");
    let kpimg = work_dir.join("kpimg");
    let kernel_ori = work_dir.join("kernel.ori");
    let kernel_out = work_dir.join("kernel.out");
    let boot_out = work_dir.join("boot_patched.img");

    if !boot_in.exists() {
        return Err(LtboxError::Patch(format!(
            "apatch: work_dir missing boot.img at {}",
            boot_in.display()
        )));
    }
    if !kpimg.exists() {
        return Err(LtboxError::Patch(format!(
            "apatch: work_dir missing kpimg at {}",
            kpimg.display()
        )));
    }

    ltbox_core::live!(
        log,
        "[APatch] {}",
        tr_args!(
            "log_apatch_extract_kernel",
            input = boot_in.display(),
            output = kernel_ori.display()
        )
    );
    bootimg::extract_kernel(&boot_in, &kernel_ori)
        .map_err(|e| LtboxError::Patch(format!("kptools extract_kernel failed: {e}")))?;

    // Bail if kernel is already patched. `parse_image_patch_info` returns
    // Ok for any kernel with a Linux banner (patched or pristine); only
    // `preset_offset.is_some()` distinguishes the two. The previous
    // `is_ok()` check false-positived on every pristine stock image,
    // blocking fresh root runs with "already APatch-patched".
    {
        let kimg_bytes = fs::read(&kernel_ori)?;
        if let Ok(info) = patch::parse_image_patch_info(&kimg_bytes)
            && info.preset_offset.is_some()
        {
            return Err(LtboxError::Patch(
                "boot kernel is already APatch-patched — unroot first before re-patching".into(),
            ));
        }

        // CONFIG_KALLSYMS gate: kptools needs symbol resolution; patching without it bricks.
        // Detect via raw byte search in the IKCONFIG payload (default for Android kernels).
        // Absent IKCONFIG → inconclusive, warn but don't block.
        let has_kallsyms = kimg_bytes
            .windows(b"CONFIG_KALLSYMS=y".len())
            .any(|w| w == b"CONFIG_KALLSYMS=y");
        let has_kallsyms_all = kimg_bytes
            .windows(b"CONFIG_KALLSYMS_ALL=y".len())
            .any(|w| w == b"CONFIG_KALLSYMS_ALL=y");
        let has_ikconfig_marker = kimg_bytes
            .windows(b"CONFIG_IKCONFIG".len())
            .any(|w| w == b"CONFIG_IKCONFIG");
        if !has_kallsyms && has_ikconfig_marker {
            return Err(LtboxError::Patch(
                "kernel missing CONFIG_KALLSYMS=y — kptools cannot resolve patch points. Flashing would brick the device. Aborting.".into(),
            ));
        } else if !has_kallsyms {
            ltbox_core::live!(
                log,
                "[APatch] {}",
                ltbox_core::i18n::tr("log_apatch_kallsyms_inconclusive")
            );
        } else {
            ltbox_core::live!(
                log,
                "[APatch] {}",
                ltbox_core::i18n::tr("log_apatch_kallsyms_ok")
            );
            if !has_kallsyms_all {
                ltbox_core::live!(
                    log,
                    "[APatch] {}",
                    ltbox_core::i18n::tr("log_apatch_kallsyms_all_missing")
                );
            }
        }
    }

    let extras: Vec<ExtraConfig> = kpm_paths
        .iter()
        .map(|p| {
            ExtraConfig::from_path(p, ExtraType::Kpm).map_err(|e| {
                LtboxError::Patch(format!(
                    "kptools ExtraConfig::from_path({}) failed: {e}",
                    p.display()
                ))
            })
        })
        .collect::<Result<_>>()?;
    ltbox_core::live!(
        log,
        "[APatch] {}",
        tr_args!(
            "log_apatch_patching_kernel",
            kpm_count = extras.len(),
            superkey_len = superkey.len()
        )
    );

    patch::patch_update_img(PatchArgs {
        kimg_path: &kernel_ori,
        kpimg_path: &kpimg,
        out_path: &kernel_out,
        superkey,
        root_key: false,
        additional: Vec::new(),
        extras,
    })
    .map_err(|e| LtboxError::Patch(format!("kptools patch_update_img failed: {e}")))?;

    ltbox_core::live!(
        log,
        "[APatch] {}",
        tr_args!(
            "log_apatch_repack_bootimg",
            boot = boot_in.display(),
            kernel = kernel_out.display(),
            output = boot_out.display()
        )
    );
    bootimg::repack_bootimg(&boot_in, &kernel_out, &boot_out)
        .map_err(|e| LtboxError::Patch(format!("kptools repack_bootimg failed: {e}")))?;

    Ok(boot_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superkey_too_short() {
        assert!(validate_superkey("abc").is_err());
    }

    #[test]
    fn superkey_too_long() {
        let s = "a".repeat(64);
        assert!(validate_superkey(&s).is_err());
    }

    #[test]
    fn superkey_non_alnum() {
        assert!(validate_superkey("abc!def@").is_err());
    }

    #[test]
    fn superkey_boundary_min_ok() {
        assert!(validate_superkey("abcdefgh").is_ok());
    }

    #[test]
    fn superkey_boundary_max_ok() {
        let s = "a".repeat(63);
        assert!(validate_superkey(&s).is_ok());
    }

    #[test]
    fn patch_boot_missing_boot_img_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let mut log = Vec::new();
        let err = patch_boot(tmp.path(), &[], "abcdefgh", &mut log).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("missing boot.img"), "unexpected: {msg}");
    }

    #[test]
    fn patch_boot_missing_kpimg_errors() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("boot.img"), b"fake").unwrap();
        let mut log = Vec::new();
        let err = patch_boot(tmp.path(), &[], "abcdefgh", &mut log).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("missing kpimg"), "unexpected: {msg}");
    }

    /// Regression: previous version of `patch_boot` used
    /// `parse_image_patch_info(..).is_ok()` to decide "already patched",
    /// which is True on **every** kernel with a Linux banner — including
    /// pristine stock images. The fix narrows the check to
    /// `preset_offset.is_some()`, the actual patched-state marker.
    ///
    /// This test asserts the patch-info parser reports
    /// `preset_offset = None` on a known-pristine boot image. The image
    /// path is supplied via the `LTBOX_TEST_PRISTINE_BOOT_IMG`
    /// environment variable so no developer-local path lives in the
    /// source tree. Gated behind `#[ignore]` so CI (which doesn't ship
    /// firmware) skips it. Run locally with
    ///
    ///     LTBOX_TEST_PRISTINE_BOOT_IMG=/path/to/boot.img \
    ///         cargo test -p ltbox-patch -- --ignored tb322
    ///
    /// (or `set LTBOX_TEST_PRISTINE_BOOT_IMG=...` on Windows cmd, or
    /// `$env:LTBOX_TEST_PRISTINE_BOOT_IMG="..."` in PowerShell.)
    /// See `ObsidianVault/05_Agent_Onboarding.md` for the full env-var
    /// list.
    #[test]
    #[ignore = "needs LTBOX_TEST_PRISTINE_BOOT_IMG=/path/to/boot.img"]
    fn pristine_boot_not_flagged_as_patched() {
        let env_key = "LTBOX_TEST_PRISTINE_BOOT_IMG";
        let Some(boot_img) = std::env::var_os(env_key).map(std::path::PathBuf::from) else {
            panic!(
                "{env_key} not set — point it at a known-pristine boot.img and re-run; \
                 see ObsidianVault/05_Agent_Onboarding.md"
            );
        };
        if !boot_img.exists() {
            panic!("{env_key} = {} does not exist on disk", boot_img.display());
        }
        let tmp = tempfile::tempdir().unwrap();
        let kernel = tmp.path().join("kernel.ori");
        kptools::bootimg::extract_kernel(&boot_img, &kernel).unwrap();
        let bytes = fs_err::read(&kernel).unwrap();
        let info =
            kptools::patch::parse_image_patch_info(&bytes).expect("pristine kernel parses cleanly");
        assert!(
            info.preset_offset.is_none(),
            "pristine kernel should report preset_offset=None, got {:?}",
            info.preset_offset
        );
    }
}
