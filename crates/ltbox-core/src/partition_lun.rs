//! Hardcoded per-partition UFS LUN map for supported Lenovo Qualcomm
//! tablets (TB320FC / TB321FU / TB322FC / TB520FU / TB710FU). LUN is
//! a hardware property; the mapping holds across every firmware rev
//! on the same model. Lets ARB / country-code / unroot / root skip
//! the rawprogram catalog scan + `.x` decrypt — qdl-rs resolves
//! start/length from the device GPT once given LUN + name.

/// UFS LUN for partitions LTBox operates on individually. Slot
/// suffixes (`_a`/`_b`) and case are normalised. `None` for unknown
/// labels — caller falls back to the rawprogram catalog.
pub fn lun_for_partition(label: &str) -> Option<u8> {
    let base = strip_slot_suffix(label).to_ascii_lowercase();
    match base.as_str() {
        "persist" | "frp" | "userdata" | "metadata" | "vbmeta_system" => Some(0),
        "boot" | "init_boot" | "vbmeta" | "vendor_boot" | "devinfo" | "dtbo" => Some(4),
        _ => None,
    }
}

/// Strip the trailing `_a` / `_b` slot suffix if present.
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
