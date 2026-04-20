//! Anti-rollback — rollback-index detection, aggregation, patching.
//!
//! Device index aggregates from fastboot `stored_rollback_index:N` as
//! `max(v > 1)`, `None` when all slots are stock. Tri-state [`RollbackMode`]:
//! `ON` always patches, `AUTO` patches only when behind, `OFF` skips.
//! Chained images go through `avb::resign_image` when signed, else
//! `avb::add_hash_footer`.

use fs_err as fs;
use std::collections::HashMap;
use std::path::Path;

use ltbox_core::{LtboxError, Result};
use tracing::info;

use crate::avb::{self, AvbImageInfo};

/// Rollback-patch mode: `On` always patches, `Auto` only when image
/// index < device index, `Off` skips entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackMode {
    On,
    Auto,
    Off,
}

/// Aggregate fastboot `stored_rollback_index:N` → single device index.
/// Returns `max(v where v > 1)`, or `None` when all slots are stock 0/1.
pub fn compute_device_rollback_index(stored: &HashMap<u32, u64>) -> Option<u64> {
    stored.values().copied().filter(|v| *v > 1).max()
}

/// Decide whether to patch given mode, image index, and device index.
/// `device_index = None` → device has no non-stock value committed; skip
/// under any mode. Callers must gate on fastboot reachability before
/// calling — unreachable under `ON` should abort at the wizard level.
pub fn needs_patch(mode: RollbackMode, image_index: u64, device_index: Option<u64>) -> bool {
    match mode {
        RollbackMode::Off => false,
        RollbackMode::On => device_index.is_some(),
        RollbackMode::Auto => match device_index {
            Some(d) => image_index < d,
            None => false,
        },
    }
}

/// Result of rollback-index analysis against a device.
pub struct RollbackAnalysis {
    pub device_index: u64,
    pub image_index: u64,
    pub needs_patch: bool,
    pub image_info: AvbImageInfo,
}

/// Legacy: pure equality check against `device_rollback_index`.
/// New callers should use [`analyze_rollback_with_mode`].
pub fn analyze_rollback(image_path: &Path, device_rollback_index: u64) -> Result<RollbackAnalysis> {
    let image_info = avb::extract_image_avb_info(image_path)?;
    let image_index = image_info.rollback_index;
    let needs_patch = image_index != device_rollback_index;
    info!(
        "Rollback analysis (legacy): device={device_rollback_index}, image={image_index}, needs_patch={needs_patch}"
    );
    Ok(RollbackAnalysis {
        device_index: device_rollback_index,
        image_index,
        needs_patch,
        image_info,
    })
}

/// Rollback analysis with mode. `device_index = None` → no non-stock
/// value committed; never triggers a patch.
pub fn analyze_rollback_with_mode(
    image_path: &Path,
    device_index: Option<u64>,
    mode: RollbackMode,
) -> Result<RollbackAnalysis> {
    let image_info = avb::extract_image_avb_info(image_path)?;
    let image_index = image_info.rollback_index;
    let needs_patch = needs_patch(mode, image_index, device_index);
    let reported_device = device_index.unwrap_or(0);
    info!(
        "Rollback analysis: mode={mode:?}, device={device_index:?}, image={image_index}, needs_patch={needs_patch}"
    );
    Ok(RollbackAnalysis {
        device_index: reported_device,
        image_index,
        needs_patch,
        image_info,
    })
}

/// Patch a chained image's AVB rollback index to `target_rollback_index`.
/// Signed → `resign_image`; NONE algorithm → `add_hash_footer`.
/// `target_rollback_index` must be the device-side value, never 0.
pub fn patch_chained_image(
    image_path: &Path,
    output_path: &Path,
    target_rollback_index: u64,
    key_file: Option<&Path>,
) -> Result<()> {
    let info = avb::extract_image_avb_info(image_path)?;

    if info.rollback_index == target_rollback_index {
        info!("Rollback index already matches, copying as-is");
        fs::copy(image_path, output_path)
            .map_err(|e| LtboxError::Patch(format!("Copy failed: {e}")))?;
        return Ok(());
    }

    info!(
        "Patching chained rollback: {} → {target_rollback_index}",
        info.rollback_index
    );

    fs::copy(image_path, output_path)
        .map_err(|e| LtboxError::Patch(format!("Copy failed: {e}")))?;

    if let Some(key) = key_file
        && info.algorithm != "NONE"
    {
        let key_spec = key.display().to_string();
        avb::resign_image(
            output_path,
            &key_spec,
            &info.algorithm,
            Some(target_rollback_index),
        )?;
        return Ok(());
    }

    let key_spec_owned = key_file.map(|p| p.display().to_string());
    avb::add_hash_footer(
        output_path,
        &info,
        key_spec_owned.as_deref(),
        Some(target_rollback_index),
    )?;
    Ok(())
}

/// Patch vbmeta rollback index. `key_file` is mandatory — vbmeta chains
/// must carry a valid signature.
pub fn patch_vbmeta_rollback(
    vbmeta_path: &Path,
    output_path: &Path,
    target_rollback_index: u64,
    key_file: &Path,
) -> Result<()> {
    let info = avb::extract_image_avb_info(vbmeta_path)?;

    if info.rollback_index == target_rollback_index {
        info!("vbmeta rollback index already matches");
        fs::copy(vbmeta_path, output_path)
            .map_err(|e| LtboxError::Patch(format!("Copy failed: {e}")))?;
        return Ok(());
    }

    info!(
        "Patching vbmeta rollback: {} → {target_rollback_index}",
        info.rollback_index
    );

    fs::copy(vbmeta_path, output_path)
        .map_err(|e| LtboxError::Patch(format!("Copy failed: {e}")))?;

    let key_spec = key_file.display().to_string();
    avb::resign_image(
        output_path,
        &key_spec,
        &info.algorithm,
        Some(target_rollback_index),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_indices(values: &[(u32, u64)]) -> HashMap<u32, u64> {
        values.iter().copied().collect()
    }

    #[test]
    fn compute_ignores_stock_values() {
        let indices = make_indices(&[(0, 0), (1, 1)]);
        assert_eq!(compute_device_rollback_index(&indices), None);
    }

    #[test]
    fn compute_returns_max_meaningful() {
        let indices = make_indices(&[(0, 1), (1, 2), (2, 5), (3, 3)]);
        assert_eq!(compute_device_rollback_index(&indices), Some(5));
    }

    #[test]
    fn compute_mixed_stock_and_real() {
        let indices = make_indices(&[(0, 0), (1, 7), (2, 1)]);
        assert_eq!(compute_device_rollback_index(&indices), Some(7));
    }

    #[test]
    fn compute_empty_returns_none() {
        let indices = make_indices(&[]);
        assert_eq!(compute_device_rollback_index(&indices), None);
    }

    #[test]
    fn needs_patch_off_never() {
        assert!(!needs_patch(RollbackMode::Off, 0, Some(10)));
        assert!(!needs_patch(RollbackMode::Off, 100, Some(10)));
    }

    #[test]
    fn needs_patch_on_patches_when_device_committed() {
        assert!(needs_patch(RollbackMode::On, 0, Some(5)));
        assert!(needs_patch(RollbackMode::On, 5, Some(5)));
        assert!(needs_patch(RollbackMode::On, 100, Some(5)));
    }

    #[test]
    fn needs_patch_on_skipped_when_device_none() {
        assert!(!needs_patch(RollbackMode::On, 0, None));
    }

    #[test]
    fn needs_patch_auto_only_when_behind() {
        assert!(needs_patch(RollbackMode::Auto, 3, Some(5)));
        assert!(!needs_patch(RollbackMode::Auto, 5, Some(5)));
        assert!(!needs_patch(RollbackMode::Auto, 7, Some(5)));
        assert!(!needs_patch(RollbackMode::Auto, 0, None));
    }
}
