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
}

pub(super) fn write_root_backup_manifest(
    backup_dir: &Path,
    root_partition: &str,
) -> Result<(), String> {
    if BackupRootTarget::from_partition_base(root_partition).is_none() {
        return Err(format!("unsupported root partition: {root_partition}"));
    }
    let manifest = RootBackupManifest {
        version: ROOT_BACKUP_MANIFEST_VERSION,
        root_partition: std::borrow::Cow::Borrowed(root_partition),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    std::fs::write(backup_dir.join(ROOT_BACKUP_MANIFEST_NAME), contents)
        .map_err(|error| error.to_string())
}

pub(super) fn resolve_backup_root_target(
    backup_dir: &Path,
    unroot_type: UnrootType,
) -> Result<BackupRootTarget, String> {
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
        return BackupRootTarget::from_partition_base(&manifest.root_partition)
            .ok_or_else(|| format!("unsupported root partition: {}", manifest.root_partition));
    }

    // Legacy backups predate the manifest. Infer the ramdisk target from the
    // stock filename, retaining init_boot as the historical missing-file
    // diagnostic when neither candidate exists.
    match unroot_type {
        UnrootType::MagiskLkm => {
            if backup_dir.join("boot.img").exists() {
                Ok(BackupRootTarget::Boot)
            } else {
                Ok(BackupRootTarget::InitBoot)
            }
        }
        UnrootType::APatchGki => Ok(BackupRootTarget::Boot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trip_preserves_boot_target() {
        let temp = tempfile::tempdir().unwrap();
        write_root_backup_manifest(temp.path(), "boot").unwrap();

        let target = resolve_backup_root_target(temp.path(), UnrootType::MagiskLkm).unwrap();
        assert_eq!(target, BackupRootTarget::Boot);
        assert_eq!(format!("{}{}", target.partition_base(), "_b"), "boot_b");
        assert_eq!(target.filename(), "boot.img");
    }

    #[test]
    fn legacy_magisk_backup_infers_init_boot_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("init_boot.img"), []).unwrap();

        assert_eq!(
            resolve_backup_root_target(temp.path(), UnrootType::MagiskLkm).unwrap(),
            BackupRootTarget::InitBoot
        );
    }

    #[test]
    fn legacy_magisk_backup_infers_boot_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), []).unwrap();

        assert_eq!(
            resolve_backup_root_target(temp.path(), UnrootType::MagiskLkm).unwrap(),
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

        assert!(resolve_backup_root_target(temp.path(), UnrootType::MagiskLkm).is_err());
    }
}
