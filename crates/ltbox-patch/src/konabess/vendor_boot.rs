//! Android vendor-boot v4 DTB enumeration and fixed-capacity rebuilding.

use std::ops::Range;

use ltbox_core::Result;

use super::{GpuTable, KonaBessExport, error, parse_fdt_gpu_info};
use crate::konabess::fdt::replace_fdt_gpu_table_with_chip;

const VENDOR_BOOT_MAGIC: &[u8; 8] = b"VNDRBOOT";
const V4_HEADER_MIN_SIZE: usize = 2_128;
const DTB_SIZE_OFFSET: usize = 2_100;

/// One group id and its number of power levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuGroupShape {
    pub id: u32,
    pub level_count: usize,
}

/// Structural shape used for automatic target classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuTableShape {
    pub groups: Vec<GpuGroupShape>,
}

impl From<&GpuTable> for GpuTableShape {
    fn from(table: &GpuTable) -> Self {
        Self {
            groups: table
                .groups
                .iter()
                .map(|group| GpuGroupShape {
                    id: group.id,
                    level_count: group.levels.len(),
                })
                .collect(),
        }
    }
}

/// Identifying information for one concatenated FDT in vendor-boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorBootDtbInfo {
    pub index: usize,
    pub model: Option<String>,
    pub chip: Option<String>,
    pub gpu_shape: Option<GpuTableShape>,
}

/// One DTB plus the shape-only classification result for an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedDtb {
    pub info: VendorBootDtbInfo,
    pub structurally_matches: bool,
}

#[derive(Debug, Clone, Copy)]
struct VendorBootLayout {
    page_size: usize,
    dtb: RangeParts,
    ramdisk_table: RangeParts,
    bootconfig: RangeParts,
    used_end: usize,
}

#[derive(Debug, Clone, Copy)]
struct RangeParts {
    start: usize,
    end: usize,
}

impl RangeParts {
    fn range(self) -> Range<usize> {
        self.start..self.end
    }

    fn len(self) -> usize {
        self.end - self.start
    }
}

/// Enumerate all concatenated FDT blobs and report identity and GPU shape.
pub fn inspect_vendor_boot_dtbs(image: &[u8]) -> Result<Vec<VendorBootDtbInfo>> {
    let layout = parse_vendor_boot_layout(image)?;
    let blobs = fdt_ranges(&image[layout.dtb.range()])?;
    let mut infos = blobs
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let parsed = parse_fdt_gpu_info(
                &image[layout.dtb.start + range.start..layout.dtb.start + range.end],
            )?;
            Ok(VendorBootDtbInfo {
                index,
                model: parsed.model,
                chip: parsed.chip,
                gpu_shape: parsed.table.as_ref().map(GpuTableShape::from),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(chip) = consensus_chip(infos.iter().filter_map(|info| info.chip.as_deref()))? {
        for info in &mut infos {
            if info.chip.is_none() {
                info.chip = Some(chip.clone());
            }
        }
    }
    Ok(infos)
}

/// Classify every blob by exact ordered group ids and per-group level counts.
///
/// No structural match is a normal all-false result, not an error.
pub fn classify_vendor_boot_dtbs(
    image: &[u8],
    export: &KonaBessExport,
) -> Result<Vec<ClassifiedDtb>> {
    let export_shape = GpuTableShape::from(&export.table);
    Ok(inspect_vendor_boot_dtbs(image)?
        .into_iter()
        .map(|info| ClassifiedDtb {
            structurally_matches: info.gpu_shape.as_ref() == Some(&export_shape),
            info,
        })
        .collect())
}

/// Copy all concatenated FDT blobs out of a vendor-boot image.
pub fn extract_vendor_boot_dtbs(image: &[u8]) -> Result<Vec<Vec<u8>>> {
    let layout = parse_vendor_boot_layout(image)?;
    let section = &image[layout.dtb.range()];
    fdt_ranges(section).map(|ranges| {
        ranges
            .into_iter()
            .map(|range| section[range].to_vec())
            .collect()
    })
}

/// Replace one selected DTB and rebuild the vendor-boot v4 section layout.
///
/// The output remains the same byte length as `image`. Growth may consume only
/// zero-filled bytes after the old logical image end; this prevents a shifted
/// DTB/table/bootconfig layout from overwriting an AVB footer or other trailing
/// content. The caller remains responsible for any later AVB operation.
pub fn replace_vendor_boot_dtb(
    image: &[u8],
    target_index: usize,
    export: &KonaBessExport,
) -> Result<Vec<u8>> {
    let layout = parse_vendor_boot_layout(image)?;
    let old_dtb = &image[layout.dtb.range()];
    let ranges = fdt_ranges(old_dtb)?;
    let target = ranges.get(target_index).ok_or_else(|| {
        error(format!(
            "target DTB index {target_index} is out of range ({} blobs)",
            ranges.len()
        ))
    })?;

    // This call performs the chip check before any output buffer or file is
    // produced, then serializes the replacement only after it passes.
    let chips = ranges
        .iter()
        .map(|range| parse_fdt_gpu_info(&old_dtb[range.clone()]).map(|info| info.chip))
        .collect::<Result<Vec<_>>>()?;
    let section_chip = consensus_chip(chips.iter().filter_map(|chip| chip.as_deref()))?;
    let replacement =
        replace_fdt_gpu_table_with_chip(&old_dtb[target.clone()], export, section_chip.as_deref())?;
    let new_dtb_size = old_dtb
        .len()
        .checked_sub(target.len())
        .and_then(|size| size.checked_add(replacement.len()))
        .ok_or_else(|| error("rebuilt DTB section size overflow"))?;
    let mut new_dtb = Vec::with_capacity(new_dtb_size);
    new_dtb.extend_from_slice(&old_dtb[..target.start]);
    new_dtb.extend_from_slice(&replacement);
    new_dtb.extend_from_slice(&old_dtb[target.end..]);
    rebuild_vendor_boot(image, layout, &new_dtb)
}

fn parse_vendor_boot_layout(image: &[u8]) -> Result<VendorBootLayout> {
    if image.len() < V4_HEADER_MIN_SIZE || image.get(..8) != Some(VENDOR_BOOT_MAGIC) {
        return Err(error("invalid vendor_boot magic or truncated v4 header"));
    }
    let version = usize_le_field(image, 8, "vendor_boot header version")?;
    if version != 4 {
        return Err(error(format!(
            "vendor_boot header version {version} is unsupported; expected v4"
        )));
    }
    let page_size = usize_le_field(image, 12, "vendor_boot page size")?;
    if page_size == 0 || !page_size.is_power_of_two() {
        return Err(error(format!("invalid vendor_boot page size {page_size}")));
    }
    let ramdisk_size = usize_le_field(image, 24, "vendor ramdisk size")?;
    let header_size = usize_le_field(image, 2_096, "vendor_boot header size")?;
    let dtb_size = usize_le_field(image, DTB_SIZE_OFFSET, "vendor_boot DTB size")?;
    let table_size = usize_le_field(image, 2_112, "vendor ramdisk table size")?;
    let bootconfig_size = usize_le_field(image, 2_124, "vendor bootconfig size")?;
    if header_size < V4_HEADER_MIN_SIZE || header_size > page_size {
        return Err(error(format!(
            "invalid vendor_boot v4 header size {header_size}"
        )));
    }

    let ramdisk_start = align_up(header_size, page_size)?;
    let dtb_start = checked_add(
        ramdisk_start,
        align_up(ramdisk_size, page_size)?,
        "vendor_boot DTB offset",
    )?;
    let dtb_end = checked_add(dtb_start, dtb_size, "vendor_boot DTB end")?;
    let table_start = checked_add(
        dtb_start,
        align_up(dtb_size, page_size)?,
        "vendor ramdisk table offset",
    )?;
    let table_end = checked_add(table_start, table_size, "vendor ramdisk table end")?;
    let bootconfig_start = checked_add(
        table_start,
        align_up(table_size, page_size)?,
        "vendor bootconfig offset",
    )?;
    let bootconfig_end = checked_add(bootconfig_start, bootconfig_size, "vendor bootconfig end")?;
    let used_end = checked_add(
        bootconfig_start,
        align_up(bootconfig_size, page_size)?,
        "vendor_boot logical end",
    )?;
    if used_end > image.len() {
        return Err(error(format!(
            "vendor_boot sections end at {used_end}, beyond image size {}",
            image.len()
        )));
    }
    Ok(VendorBootLayout {
        page_size,
        dtb: RangeParts {
            start: dtb_start,
            end: dtb_end,
        },
        ramdisk_table: RangeParts {
            start: table_start,
            end: table_end,
        },
        bootconfig: RangeParts {
            start: bootconfig_start,
            end: bootconfig_end,
        },
        used_end,
    })
}

fn rebuild_vendor_boot(image: &[u8], old: VendorBootLayout, new_dtb: &[u8]) -> Result<Vec<u8>> {
    let new_table_start = checked_add(
        old.dtb.start,
        align_up(new_dtb.len(), old.page_size)?,
        "rebuilt vendor ramdisk table offset",
    )?;
    let new_bootconfig_start = checked_add(
        new_table_start,
        align_up(old.ramdisk_table.len(), old.page_size)?,
        "rebuilt vendor bootconfig offset",
    )?;
    let new_used_end = checked_add(
        new_bootconfig_start,
        align_up(old.bootconfig.len(), old.page_size)?,
        "rebuilt vendor_boot logical end",
    )?;
    if new_used_end > image.len() {
        return Err(error(format!(
            "rebuilt vendor_boot needs {new_used_end} bytes but image capacity is {}",
            image.len()
        )));
    }
    if new_used_end > old.used_end
        && image[old.used_end..new_used_end]
            .iter()
            .any(|&byte| byte != 0)
    {
        return Err(error(
            "rebuilt vendor_boot would overwrite nonzero trailing content",
        ));
    }
    let new_dtb_size = u32::try_from(new_dtb.len())
        .map_err(|_| error("rebuilt vendor_boot DTB section does not fit u32"))?;

    let table = image[old.ramdisk_table.range()].to_vec();
    let bootconfig = image[old.bootconfig.range()].to_vec();
    let mut output = image.to_vec();
    let cleared_end = old.used_end.max(new_used_end);
    output[old.dtb.start..cleared_end].fill(0);
    output[DTB_SIZE_OFFSET..DTB_SIZE_OFFSET + 4].copy_from_slice(&new_dtb_size.to_le_bytes());
    output[old.dtb.start..old.dtb.start + new_dtb.len()].copy_from_slice(new_dtb);
    output[new_table_start..new_table_start + table.len()].copy_from_slice(&table);
    output[new_bootconfig_start..new_bootconfig_start + bootconfig.len()]
        .copy_from_slice(&bootconfig);
    Ok(output)
}

fn fdt_ranges(section: &[u8]) -> Result<Vec<Range<usize>>> {
    let mut ranges = Vec::new();
    let mut position = 0usize;
    while position < section.len() {
        if section[position..].iter().all(|&byte| byte == 0) {
            break;
        }
        let magic = read_be_u32(section, position, "concatenated FDT magic")?;
        if magic != 0xd00d_feed {
            return Err(error(format!(
                "invalid concatenated FDT magic at DTB-section offset 0x{position:x}"
            )));
        }
        let size = usize::try_from(read_be_u32(section, position + 4, "concatenated FDT size")?)
            .map_err(|_| error("concatenated FDT size does not fit usize"))?;
        if size < 40 {
            return Err(error(format!(
                "invalid concatenated FDT size {size} at offset 0x{position:x}"
            )));
        }
        let end = checked_add(position, size, "concatenated FDT end")?;
        if end > section.len() {
            return Err(error("concatenated FDT extends beyond the DTB section"));
        }
        ranges.push(position..end);
        position = end;
    }
    if ranges.is_empty() {
        return Err(error("vendor_boot DTB section has no FDT blobs"));
    }
    Ok(ranges)
}

fn consensus_chip<'a>(chips: impl IntoIterator<Item = &'a str>) -> Result<Option<String>> {
    let mut consensus: Option<String> = None;
    for chip in chips {
        if let Some(existing) = &consensus {
            if existing != chip {
                return Err(error(format!(
                    "vendor_boot DTBs disagree on chip: `{existing}` and `{chip}`"
                )));
            }
        } else {
            consensus = Some(chip.to_string());
        }
    }
    Ok(consensus)
}

fn usize_le_field(bytes: &[u8], offset: usize, name: &str) -> Result<usize> {
    usize::try_from(read_le_u32(bytes, offset, name)?)
        .map_err(|_| error(format!("{name} does not fit usize")))
}

fn read_le_u32(bytes: &[u8], offset: usize, name: &str) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| error(format!("truncated {name}")))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_be_u32(bytes: &[u8], offset: usize, name: &str) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| error(format!("truncated {name}")))?;
    Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

fn checked_add(left: usize, right: usize, name: &str) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| error(format!("{name} overflow")))
}

fn align_up(value: usize, alignment: usize) -> Result<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| error("vendor_boot alignment overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::konabess::fdt::test_support::{synthetic_fdt, table};

    fn synthetic_vendor_boot(blobs: &[Vec<u8>], slack_pages: usize) -> Vec<u8> {
        let page = 4096usize;
        let header_size = V4_HEADER_MIN_SIZE;
        let ramdisk = [0x11, 0x22, 0x33];
        let dtb = blobs.concat();
        let table = [0x41; 12];
        let bootconfig = *b"key=value\n";
        let ramdisk_start = page;
        let dtb_start = ramdisk_start + page;
        let table_start = dtb_start + align_up(dtb.len(), page).unwrap();
        let bootconfig_start = table_start + page;
        let used_end = bootconfig_start + page;
        let mut image = vec![0u8; used_end + slack_pages * page];
        image[..8].copy_from_slice(VENDOR_BOOT_MAGIC);
        for (offset, value) in [
            (8, 4u32),
            (12, page as u32),
            (24, ramdisk.len() as u32),
            (2_096, header_size as u32),
            (DTB_SIZE_OFFSET, dtb.len() as u32),
            (2_112, table.len() as u32),
            (2_116, 1),
            (2_120, table.len() as u32),
            (2_124, bootconfig.len() as u32),
        ] {
            image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        image[ramdisk_start..ramdisk_start + ramdisk.len()].copy_from_slice(&ramdisk);
        image[dtb_start..dtb_start + dtb.len()].copy_from_slice(&dtb);
        image[table_start..table_start + table.len()].copy_from_slice(&table);
        image[bootconfig_start..bootconfig_start + bootconfig.len()].copy_from_slice(&bootconfig);
        let marker_start = image.len() - 16;
        image[marker_start..].copy_from_slice(b"TRAILING-CONTENT");
        image
    }

    #[test]
    fn enumerates_and_classifies_shapes() {
        let image = synthetic_vendor_boot(
            &[
                synthetic_fdt("sun", "No Match", &table(&[(0, 1)], 100)),
                synthetic_fdt("sun", "Match", &table(&[(0, 2), (3, 1)], 200)),
                synthetic_fdt("sun", "Other", &table(&[(0, 2), (3, 2)], 300)),
            ],
            2,
        );
        let export = KonaBessExport {
            chip: "sun".into(),
            description: String::new(),
            table: table(&[(0, 2), (3, 1)], 900),
        };
        let classified = classify_vendor_boot_dtbs(&image, &export).unwrap();
        assert_eq!(classified.len(), 3);
        assert_eq!(
            classified
                .iter()
                .filter(|blob| blob.structurally_matches)
                .map(|blob| blob.info.index)
                .collect::<Vec<_>>(),
            [1]
        );
    }

    #[test]
    fn rebuild_moves_following_sections_and_preserves_trailing_content() {
        let image =
            synthetic_vendor_boot(&[synthetic_fdt("sun", "Target", &table(&[(0, 1)], 100))], 6);
        let marker = image[image.len() - 16..].to_vec();
        let export = KonaBessExport {
            chip: "sun".into(),
            description: String::new(),
            table: table(&[(0, 300)], 900),
        };
        let rebuilt = replace_vendor_boot_dtb(&image, 0, &export).unwrap();
        assert_eq!(rebuilt.len(), image.len());
        assert_eq!(&rebuilt[rebuilt.len() - 16..], marker);
        let blobs = extract_vendor_boot_dtbs(&rebuilt).unwrap();
        assert_eq!(
            parse_fdt_gpu_info(&blobs[0]).unwrap().table,
            Some(export.table)
        );
        let layout = parse_vendor_boot_layout(&rebuilt).unwrap();
        assert_eq!(&rebuilt[layout.ramdisk_table.range()], &[0x41; 12]);
        assert_eq!(&rebuilt[layout.bootconfig.range()], b"key=value\n");
    }

    #[test]
    fn refuses_growth_into_nonzero_trailing_content() {
        let mut image =
            synthetic_vendor_boot(&[synthetic_fdt("sun", "Target", &table(&[(0, 1)], 100))], 6);
        let layout = parse_vendor_boot_layout(&image).unwrap();
        image[layout.used_end] = 0x7f;
        let export = KonaBessExport {
            chip: "sun".into(),
            description: String::new(),
            table: table(&[(0, 300)], 900),
        };
        let error = replace_vendor_boot_dtb(&image, 0, &export).unwrap_err();
        assert!(error.to_string().contains("nonzero trailing content"));
    }

    #[test]
    fn chip_mismatch_does_not_create_output_file() {
        let image =
            synthetic_vendor_boot(&[synthetic_fdt("sun", "Target", &table(&[(0, 1)], 100))], 1);
        let export = KonaBessExport {
            chip: "pineapple".into(),
            description: String::new(),
            table: table(&[(0, 1)], 900),
        };
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("vendor_boot.img");
        let output = temp.path().join("output.img");
        std::fs::write(&input, image).unwrap();

        let error = crate::konabess::apply_export_to_vendor_boot_file(&input, &output, 0, &export)
            .unwrap_err();

        assert!(error.to_string().contains("chip mismatch"));
        assert!(!output.exists());
    }
}
