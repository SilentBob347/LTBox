//! Root-backup metadata shared by the root and unroot workers.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::UnrootType;

pub(super) const ROOT_BACKUP_MANIFEST_NAME: &str = "root-backup.json";
const ROOT_BACKUP_MANIFEST_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackupRootTarget {
    Boot,
    InitBoot,
}

impl BackupRootTarget {
    pub(super) const fn partition_base(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::InitBoot => "init_boot",
        }
    }

    pub(super) const fn filename(self) -> &'static str {
        match self {
            Self::Boot => "boot.img",
            Self::InitBoot => "init_boot.img",
        }
    }

    fn from_partition_base(partition: &str) -> Option<Self> {
        match partition {
            "boot" => Some(Self::Boot),
            "init_boot" => Some(Self::InitBoot),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct RootBackupManifest<'a> {
    version: u8,
    #[serde(borrow)]
    root_partition: std::borrow::Cow<'a, str>,
    /// Whether the run that wrote this backup also dumped `vbmeta.img`. Absent
    /// in manifests written before the root pipeline learned to leave a chained
    /// vbmeta alone, where its presence on disk is the only available signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vbmeta: Option<bool>,
}

/// What a backup folder holds, and therefore what Unroot must restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackupContents {
    pub(super) root_target: BackupRootTarget,
    /// `true` when `vbmeta.img` must be restored alongside the root image.
    /// A run that left vbmeta untouched records `false`, and restoring a stale
    /// vbmeta over an untouched one would be a needless write.
    pub(super) restore_vbmeta: bool,
}

pub(super) fn write_root_backup_manifest(
    backup_dir: &Path,
    root_partition: &str,
    vbmeta_backed_up: bool,
) -> Result<(), String> {
    if BackupRootTarget::from_partition_base(root_partition).is_none() {
        return Err(format!("unsupported root partition: {root_partition}"));
    }
    let manifest = RootBackupManifest {
        version: ROOT_BACKUP_MANIFEST_VERSION,
        root_partition: std::borrow::Cow::Borrowed(root_partition),
        vbmeta: Some(vbmeta_backed_up),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    std::fs::write(backup_dir.join(ROOT_BACKUP_MANIFEST_NAME), contents)
        .map_err(|error| error.to_string())
}

pub(super) fn resolve_backup_contents(
    backup_dir: &Path,
    unroot_type: UnrootType,
) -> Result<BackupContents, String> {
    let manifest_path = backup_dir.join(ROOT_BACKUP_MANIFEST_NAME);
    if manifest_path.exists() {
        let contents = std::fs::read(&manifest_path).map_err(|error| error.to_string())?;
        let manifest: RootBackupManifest<'_> =
            serde_json::from_slice(&contents).map_err(|error| error.to_string())?;
        if manifest.version != ROOT_BACKUP_MANIFEST_VERSION {
            return Err(format!(
                "unsupported manifest version: {}",
                manifest.version
            ));
        }
        let root_target = BackupRootTarget::from_partition_base(&manifest.root_partition)
            .ok_or_else(|| format!("unsupported root partition: {}", manifest.root_partition))?;
        return Ok(BackupContents {
            root_target,
            // A manifest without the field predates it; fall back to the disk,
            // which is what those runs always agreed with.
            restore_vbmeta: manifest
                .vbmeta
                .unwrap_or_else(|| backup_dir.join("vbmeta.img").exists()),
        });
    }

    // Legacy backups predate the manifest. Infer the ramdisk target from the
    // stock filename, retaining init_boot as the historical missing-file
    // diagnostic when neither candidate exists.
    let root_target = match unroot_type {
        UnrootType::MagiskLkm => {
            if backup_dir.join("boot.img").exists() {
                BackupRootTarget::Boot
            } else {
                BackupRootTarget::InitBoot
            }
        }
        UnrootType::APatchGki => BackupRootTarget::Boot,
    };
    Ok(BackupContents {
        root_target,
        restore_vbmeta: backup_dir.join("vbmeta.img").exists(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trip_preserves_boot_target() {
        let temp = tempfile::tempdir().unwrap();
        write_root_backup_manifest(temp.path(), "boot", true).unwrap();

        let contents = resolve_backup_contents(temp.path(), UnrootType::MagiskLkm).unwrap();
        let target = contents.root_target;
        assert_eq!(target, BackupRootTarget::Boot);
        assert!(contents.restore_vbmeta);
        assert_eq!(format!("{}{}", target.partition_base(), "_b"), "boot_b");
        assert_eq!(target.filename(), "boot.img");
    }

    #[test]
    fn manifest_records_a_run_that_left_vbmeta_alone() {
        let temp = tempfile::tempdir().unwrap();
        write_root_backup_manifest(temp.path(), "boot", false).unwrap();

        let contents = resolve_backup_contents(temp.path(), UnrootType::MagiskLkm).unwrap();
        assert_eq!(contents.root_target, BackupRootTarget::Boot);
        assert!(!contents.restore_vbmeta);
    }

    #[test]
    fn manifest_without_the_vbmeta_field_falls_back_to_the_folder() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join(ROOT_BACKUP_MANIFEST_NAME),
            br#"{"version":1,"root_partition":"boot"}"#,
        )
        .unwrap();
        assert!(
            !resolve_backup_contents(temp.path(), UnrootType::MagiskLkm)
                .unwrap()
                .restore_vbmeta
        );

        std::fs::write(temp.path().join("vbmeta.img"), []).unwrap();
        assert!(
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm)
                .unwrap()
                .restore_vbmeta
        );
    }

    #[test]
    fn legacy_magisk_backup_infers_init_boot_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("init_boot.img"), []).unwrap();

        assert_eq!(
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm)
                .unwrap()
                .root_target,
            BackupRootTarget::InitBoot
        );
    }

    #[test]
    fn legacy_magisk_backup_infers_boot_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), []).unwrap();

        assert_eq!(
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm)
                .unwrap()
                .root_target,
            BackupRootTarget::Boot
        );
    }

    #[test]
    fn invalid_manifest_does_not_fall_back_to_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), []).unwrap();
        std::fs::write(
            temp.path().join(ROOT_BACKUP_MANIFEST_NAME),
            br#"{"version":1,"root_partition":"userdata"}"#,
        )
        .unwrap();

        assert!(resolve_backup_contents(temp.path(), UnrootType::MagiskLkm).is_err());
    }
}
