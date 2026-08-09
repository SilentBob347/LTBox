//! Offline KonaBess GPU-table import for Android vendor-boot v4 images.
//!
//! KonaBess replaces the complete contiguous `qcom,gpu-pwrlevels-*` sibling
//! set. This module mirrors that behavior directly in flattened-device-tree
//! bytes and deliberately does not perform AVB, EDL, or GUI work.

mod export;
mod fdt;
mod vendor_boot;

use std::path::Path;

use fs_err as fs;
use ltbox_core::{LtboxError, Result};

pub use export::{GpuGroup, GpuLevel, GpuProperty, GpuTable, KonaBessExport, parse_export};
pub use fdt::{FdtGpuInfo, parse_fdt_gpu_info, replace_fdt_gpu_table};
pub use vendor_boot::{
    ClassifiedDtb, GpuGroupShape, GpuTableShape, VendorBootDtbInfo, classify_vendor_boot_dtbs,
    extract_vendor_boot_dtbs, inspect_vendor_boot_dtbs, replace_vendor_boot_dtb,
};

/// Read and parse a KonaBess export file.
pub fn read_export(path: &Path) -> Result<KonaBessExport> {
    let text = fs::read_to_string(path).map_err(|e| {
        LtboxError::Patch(format!(
            "cannot read KonaBess export {}: {e}",
            path.display()
        ))
    })?;
    parse_export(&text)
}

/// Apply an export to one DTB and write a rebuilt vendor-boot image once all
/// validation and in-memory rebuilding has succeeded.
///
/// In particular, an incompatible chip cannot create or truncate `output`.
pub fn apply_export_to_vendor_boot_file(
    input: &Path,
    output: &Path,
    target_index: usize,
    export: &KonaBessExport,
) -> Result<()> {
    let image = fs::read(input).map_err(|e| {
        LtboxError::Patch(format!(
            "cannot read vendor_boot image {}: {e}",
            input.display()
        ))
    })?;
    let rebuilt = replace_vendor_boot_dtb(&image, target_index, export)?;
    fs::write(output, rebuilt).map_err(|e| {
        LtboxError::Patch(format!(
            "cannot write vendor_boot image {}: {e}",
            output.display()
        ))
    })
}

fn error(message: impl Into<String>) -> LtboxError {
    LtboxError::Patch(format!("KonaBess: {}", message.into()))
}
