//! Hardcoded per-partition UFS LUN map for the supported Lenovo
//! Qualcomm tablets.
//!
//! LTBox supports a small fixed device list — TB320FC, TB321FU,
//! TB322FC, TB520FU, TB710FU. For these devices, the partitions LTBox
//! dumps or flashes individually (root / unroot, country-code patch,
//! anti-rollback patch) always live on the same physical UFS LUN
//! across every shipping firmware revision. The mapping is a hardware
//! property of the SoC's storage layout, not a firmware property, so
//! it is safe to bake in here.
//!
//! Why this exists: the v3 GUI used to scan + parse the firmware
//! folder's `rawprogram*.xml` (and decrypt `rawprogram*.x` first if
//! the user shipped only the encrypted variant) on every per-partition
//! op to recover `(LUN, start_sector, num_sectors)`. That scan is
//! correct for the full firmware-flash plan (which iterates every
//! `<program>` node) but wasteful for a single-partition dump where
//! only the LUN matters: qdl-rs / Firehose can resolve start + length
//! from the device GPT once we know which LUN to read. Routing the
//! known partitions through this map lets the GUI skip the catalog +
//! decrypt round-trip entirely for ARB / country-code / unroot / root
//! flows.
//!
//! Anything outside the map returns `None` so callers fall back to
//! the catalog path — adds new partitions are additive and never
//! silently regress.

/// UFS LUN for the partitions LTBox operates on individually.
///
/// Returns the SoC's `physical_partition_number` (a `u8` to match
/// `qdl::firehose_*` parameter types). Slot suffixes are handled —
/// `vbmeta`, `vbmeta_a`, `vbmeta_b` all resolve to the same LUN.
/// Lookup is case-insensitive on the base label.
///
/// Returns `None` for any label outside the supported set; the
/// caller should fall through to the rawprogram catalog.
pub fn lun_for_partition(label: &str) -> Option<u8> {
    let base = strip_slot_suffix(label).to_ascii_lowercase();
    match base.as_str() {
        // LUN 0 — large user-data + scratch + system vbmeta chain.
        // Confirmed against TB322FC `rawprogram_unsparse0.xml`
        // (`physical_partition_number="0"`).
        "persist" | "frp" | "userdata" | "metadata" | "vbmeta_system" => Some(0),
        // LUN 4 — boot chain + region/identity blobs. Confirmed
        // against TB322FC `rawprogram4.xml`
        // (`physical_partition_number="4"`).
        "boot" | "init_boot" | "vbmeta" | "vendor_boot" | "devinfo" | "dtbo" => Some(4),
        _ => None,
    }
}

/// Strip the trailing `_a` / `_b` A/B slot suffix if present. Pure
/// helper, exposed because callers occasionally need the same
/// canonicalisation when grouping partitions.
pub fn strip_slot_suffix(label: &str) -> &str {
    if label.len() < 2 {
        return label;
    }
    let tail = &label[label.len() - 2..];
    if tail.eq_ignore_ascii_case("_a") || tail.eq_ignore_ascii_case("_b") {
        &label[..label.len() - 2]
    } else {
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lun_0_partitions_resolve() {
        for name in ["persist", "frp", "userdata", "metadata", "vbmeta_system"] {
            assert_eq!(lun_for_partition(name), Some(0), "{name}");
        }
    }

    #[test]
    fn lun_4_partitions_resolve() {
        for name in [
            "boot",
            "init_boot",
            "vbmeta",
            "vendor_boot",
            "devinfo",
            "dtbo",
        ] {
            assert_eq!(lun_for_partition(name), Some(4), "{name}");
        }
    }

    #[test]
    fn slot_suffix_strips() {
        assert_eq!(lun_for_partition("vbmeta_a"), Some(4));
        assert_eq!(lun_for_partition("vbmeta_b"), Some(4));
        assert_eq!(lun_for_partition("vbmeta_system_a"), Some(0));
        assert_eq!(lun_for_partition("boot_a"), Some(4));
        assert_eq!(lun_for_partition("init_boot_b"), Some(4));
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(lun_for_partition("VBMETA_A"), Some(4));
        assert_eq!(lun_for_partition("Persist"), Some(0));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(lun_for_partition("super"), None);
        assert_eq!(lun_for_partition("misc"), None);
        assert_eq!(lun_for_partition(""), None);
        assert_eq!(lun_for_partition("xbl"), None);
    }

    #[test]
    fn strip_slot_suffix_leaves_unsuffixed_alone() {
        assert_eq!(strip_slot_suffix("vbmeta"), "vbmeta");
        assert_eq!(strip_slot_suffix("a"), "a"); // too short to match
        assert_eq!(strip_slot_suffix(""), "");
    }
}
