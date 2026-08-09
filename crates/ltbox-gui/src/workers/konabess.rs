//! KonaBess stage-D inspection worker: resolve slot, dump stock images, run
//! the existing exploit gate, classify DTBs, then pause for UI selection.

use crate::{
    ConnectionStatus, KonaBessPrepared, LiveLabels, PhaseReporter, open_edl_session,
    prepare_tb323fu_efisp, provision_tb323fu_efisp, transition_to_edl,
};
use ltbox_core::{live, tr_args};
use ltbox_patch::konabess::{ClassifiedDtb, KonaBessExport};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct KonaBessInspectionResult {
    pub(crate) prepared: KonaBessPrepared,
    pub(crate) candidates: Vec<ClassifiedDtb>,
    pub(crate) log: Vec<String>,
}

struct InspectionPaths {
    work_dir: PathBuf,
    backup_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExploitGateKind {
    SignedVbmeta,
    Tb323fuEfisp,
}

const fn exploit_gate_kind(is_tb323fu: bool) -> ExploitGateKind {
    if is_tb323fu {
        ExploitGateKind::Tb323fuEfisp
    } else {
        ExploitGateKind::SignedVbmeta
    }
}

trait KonaBessInspectionBackend {
    fn resolve_active_slot(&mut self, log: &mut Vec<String>) -> Result<String, String>;
    fn enter_edl(&mut self, log: &mut Vec<String>) -> Result<(), String>;
    fn dump_partition(
        &mut self,
        partition: &str,
        destination: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    fn run_exploit_gate(
        &mut self,
        slot_suffix: &str,
        vendor_boot: &Path,
        vbmeta: &Path,
        work_dir: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    fn classify(
        &mut self,
        vendor_boot: &Path,
        export: &KonaBessExport,
    ) -> Result<Vec<ClassifiedDtb>, String>;
    fn prepare_for_selection(&mut self, log: &mut Vec<String>) -> Result<(), String>;
    fn recover_after_error(&mut self, log: &mut Vec<String>);
}

struct DeviceBackend<'a> {
    conn: ConnectionStatus,
    loader: &'a Path,
    is_tb323fu: bool,
    ll: &'a LiveLabels,
    session: Option<ltbox_device::edl::EdlSession>,
    writes_started: bool,
}

impl DeviceBackend<'_> {
    fn session(&mut self) -> Result<&mut ltbox_device::edl::EdlSession, String> {
        self.session.as_mut().ok_or_else(|| {
            tr_args!(
                "err_edl_session_open_failed",
                error = ltbox_core::i18n::tr("err_task_failed")
            )
        })
    }
}

impl KonaBessInspectionBackend for DeviceBackend<'_> {
    fn resolve_active_slot(&mut self, log: &mut Vec<String>) -> Result<String, String> {
        ltbox_device::controller::poll_active_slot(std::time::Duration::from_secs(30), log)
            .map_err(|error| error.to_string())
    }

    fn enter_edl(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        transition_to_edl(self.conn, self.ll, log)?;
        self.session = Some(open_edl_session(self.loader, false, log)?);
        Ok(())
    }

    fn dump_partition(
        &mut self,
        partition: &str,
        destination: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        let lun = ltbox_core::partition_lun::lun_for_partition(partition).unwrap_or(4);
        self.session()?
            .dump_partition(partition, destination, 0, lun, log)
            .map_err(|error| {
                tr_args!(
                    "err_root_dump_partition_failed",
                    partition = partition,
                    error = error
                )
            })
    }

    fn run_exploit_gate(
        &mut self,
        slot_suffix: &str,
        vendor_boot: &Path,
        vbmeta: &Path,
        work_dir: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        match exploit_gate_kind(self.is_tb323fu) {
            ExploitGateKind::Tb323fuEfisp => {
                let efi_dir = work_dir.join("efisp_gbl");
                let staged = prepare_tb323fu_efisp(
                    self.session()?,
                    slot_suffix,
                    Some(vendor_boot),
                    work_dir,
                    &efi_dir,
                    log,
                )?;
                self.writes_started = staged.is_some();
                provision_tb323fu_efisp(self.session()?, staged.as_deref(), log)?;
                Ok(())
            }
            ExploitGateKind::SignedVbmeta => {
                let info = ltbox_patch::avb::extract_image_avb_info(vbmeta)
                    .map_err(|error| error.to_string())?;
                validate_signing_key(info.public_key_sha1.as_deref())
            }
        }
    }

    fn classify(
        &mut self,
        vendor_boot: &Path,
        export: &KonaBessExport,
    ) -> Result<Vec<ClassifiedDtb>, String> {
        let image = std::fs::read(vendor_boot).map_err(|error| error.to_string())?;
        ltbox_patch::konabess::classify_vendor_boot_dtbs(&image, export)
            .map_err(|error| error.to_string())
    }

    fn prepare_for_selection(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        self.session()?
            .reset_to_edl(log)
            .map_err(|error| error.to_string())?;
        self.session = None;
        Ok(())
    }

    fn recover_after_error(&mut self, log: &mut Vec<String>) {
        if let Some(session) = self.session.as_mut() {
            session.reset_tolerant(log);
            self.session = None;
            return;
        }
        if let Ok(mut session) = ltbox_device::edl::EdlSession::open(self.loader, false, log) {
            session.reset_tolerant(log);
        }
    }
}

fn validate_signing_key(pubkey_sha1: Option<&str>) -> Result<(), String> {
    ltbox_patch::key_map::key_spec_for_signed_pubkey(pubkey_sha1)
        .map(|_| ())
        .map_err(|key| {
            tr_args!(
                "err_avb_signing_key_unknown",
                image = "vbmeta.img",
                key = key
            )
        })
}

fn persist_backup(vendor_boot: &Path, vbmeta: &Path, backup_dir: &Path) -> Result<(), String> {
    if let Err(error) = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(backup_dir)?;
        std::fs::copy(vendor_boot, backup_dir.join("vendor_boot.img"))?;
        std::fs::copy(vbmeta, backup_dir.join("vbmeta.img"))?;
        Ok(())
    })() {
        let _ = std::fs::remove_dir_all(backup_dir);
        return Err(error.to_string());
    }
    Ok(())
}

fn execute_inspection<B: KonaBessInspectionBackend>(
    backend: &mut B,
    export: &KonaBessExport,
    paths: &InspectionPaths,
    phases: &PhaseReporter,
    log: &mut Vec<String>,
) -> Result<(KonaBessPrepared, Vec<ClassifiedDtb>), String> {
    live!(log, "[KonaBess] {}", phases.marker(1));

    // This probe is intentionally the first device operation. EDL cannot
    // report an active Android slot, and failure must not fall back to `_a`.
    let slot_suffix = backend.resolve_active_slot(log)?;
    backend.enter_edl(log)?;

    live!(log, "[KonaBess] {}", phases.marker(2));
    let vendor_boot_partition = format!("vendor_boot{slot_suffix}");
    let vbmeta_partition = format!("vbmeta{slot_suffix}");
    let vendor_boot = paths.work_dir.join("vendor_boot.img");
    let vbmeta = paths.work_dir.join("vbmeta.img");
    backend.dump_partition(&vendor_boot_partition, &vendor_boot, log)?;
    backend.dump_partition(&vbmeta_partition, &vbmeta, log)?;

    // Gate only after both source images exist. TB323FU takes the shared efisp
    // path; every other model resolves vbmeta through KEY_MAP and permits an
    // absent key as unsigned.
    backend.run_exploit_gate(&slot_suffix, &vendor_boot, &vbmeta, &paths.work_dir, log)?;

    live!(log, "[KonaBess] {}", phases.marker(3));
    let candidates = backend.classify(&vendor_boot, export)?;

    // Return Firehose to Sahara so part 2 can open a fresh session after the
    // UI selection pause. Backup creation is last, making it success-only.
    backend.prepare_for_selection(log)?;
    persist_backup(&vendor_boot, &vbmeta, &paths.backup_dir)?;

    Ok((
        KonaBessPrepared {
            work_dir: paths.work_dir.clone(),
            vendor_boot,
            vbmeta,
            backup_dir: paths.backup_dir.clone(),
            slot_suffix,
        },
        candidates,
    ))
}

pub(crate) fn konabess_inspection_worker(
    conn: ConnectionStatus,
    loader: PathBuf,
    export: KonaBessExport,
    is_tb323fu: bool,
    ll: LiveLabels,
    phases: PhaseReporter,
) -> Result<KonaBessInspectionResult, String> {
    let mut log = Vec::new();
    let work_dir = ltbox_core::app_paths::work_dir_for("konabess");
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir).map_err(|error| error.to_string())?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let backup_dir =
        ltbox_core::app_paths::backup_dir_for(&format!("backup_critical_{timestamp}_konabess"));
    let paths = InspectionPaths {
        work_dir: work_dir.clone(),
        backup_dir,
    };
    let mut backend = DeviceBackend {
        conn,
        loader: &loader,
        is_tb323fu,
        ll: &ll,
        session: None,
        writes_started: false,
    };

    match execute_inspection(&mut backend, &export, &paths, &phases, &mut log) {
        Ok((prepared, candidates)) => {
            live!(
                log,
                "[KonaBess] {} {}",
                ll.backup_saved_prefix,
                prepared.backup_dir.display()
            );
            Ok(KonaBessInspectionResult {
                prepared,
                candidates,
                log,
            })
        }
        Err(error) => {
            let error = if backend.writes_started {
                tr_args!("err_root_partial_write_recovery", error = error)
            } else {
                backend.recover_after_error(&mut log);
                error
            };
            let _ = std::fs::remove_dir_all(&work_dir);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltbox_patch::konabess::{GpuTable, VendorBootDtbInfo};

    #[derive(Default)]
    struct FakeBackend {
        events: Vec<String>,
        gate_error: Option<String>,
        candidate: Option<ClassifiedDtb>,
    }

    impl KonaBessInspectionBackend for FakeBackend {
        fn resolve_active_slot(&mut self, _log: &mut Vec<String>) -> Result<String, String> {
            self.events.push("slot".into());
            Ok("_b".into())
        }

        fn enter_edl(&mut self, _log: &mut Vec<String>) -> Result<(), String> {
            self.events.push("edl".into());
            Ok(())
        }

        fn dump_partition(
            &mut self,
            partition: &str,
            destination: &Path,
            _log: &mut Vec<String>,
        ) -> Result<(), String> {
            self.events.push(format!("dump:{partition}"));
            std::fs::write(destination, partition).map_err(|error| error.to_string())
        }

        fn run_exploit_gate(
            &mut self,
            _slot_suffix: &str,
            _vendor_boot: &Path,
            _vbmeta: &Path,
            _work_dir: &Path,
            _log: &mut Vec<String>,
        ) -> Result<(), String> {
            self.events.push("gate".into());
            self.gate_error.take().map_or(Ok(()), Err)
        }

        fn classify(
            &mut self,
            vendor_boot: &Path,
            _export: &KonaBessExport,
        ) -> Result<Vec<ClassifiedDtb>, String> {
            assert_eq!(
                std::fs::read_to_string(vendor_boot).unwrap(),
                "vendor_boot_b"
            );
            self.events.push("classify".into());
            Ok(self.candidate.take().into_iter().collect())
        }

        fn prepare_for_selection(&mut self, _log: &mut Vec<String>) -> Result<(), String> {
            self.events.push("pause".into());
            Ok(())
        }

        fn recover_after_error(&mut self, _log: &mut Vec<String>) {
            self.events.push("recover".into());
        }
    }

    fn export() -> KonaBessExport {
        KonaBessExport {
            chip: "waipio".into(),
            description: "test".into(),
            table: GpuTable { groups: vec![] },
        }
    }

    fn candidate() -> ClassifiedDtb {
        ClassifiedDtb {
            info: VendorBootDtbInfo {
                index: 2,
                model: Some("test".into()),
                chip: Some("waipio".into()),
                gpu_shape: None,
            },
            structurally_matches: true,
        }
    }

    fn test_paths(root: &Path) -> InspectionPaths {
        let work_dir = root.join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        InspectionPaths {
            work_dir,
            backup_dir: root.join("backup_critical_123_konabess"),
        }
    }

    fn phases() -> PhaseReporter {
        PhaseReporter::from_labels(vec!["prepare".into(), "dump".into(), "inspect".into()])
    }

    #[test]
    fn accepts_key_map_keys_and_unsigned_but_rejects_present_unknown_keys() {
        assert!(validate_signing_key(Some("2597c218aae470a130f61162feaae70afd97f011")).is_ok());
        assert!(validate_signing_key(None).is_ok());
        assert!(validate_signing_key(Some("")).is_ok());
        assert!(validate_signing_key(Some("8fcb864f11f53ed11284615fb67685522085d3a2")).is_err());
        assert!(validate_signing_key(Some("deadbeef")).is_err());
    }

    #[test]
    fn tb323fu_empty_efisp_requires_provision_and_bypasses_avb_gate() {
        assert_eq!(exploit_gate_kind(true), ExploitGateKind::Tb323fuEfisp);
        assert_eq!(exploit_gate_kind(false), ExploitGateKind::SignedVbmeta);
        assert!(crate::efisp_is_empty(&[0; 32]));
        assert!(!crate::efisp_is_empty(&[0, 0, 1, 0]));
        assert!(validate_signing_key(Some("fixed-or-unknown")).is_err());
    }

    #[test]
    fn resolves_slot_before_edl_and_hands_dump_to_classifier() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let mut backend = FakeBackend {
            candidate: Some(candidate()),
            ..FakeBackend::default()
        };
        let (_, candidates) =
            execute_inspection(&mut backend, &export(), &paths, &phases(), &mut Vec::new())
                .unwrap();

        assert_eq!(candidates, vec![candidate()]);
        assert_eq!(
            backend.events,
            [
                "slot",
                "edl",
                "dump:vendor_boot_b",
                "dump:vbmeta_b",
                "gate",
                "classify",
                "pause"
            ]
        );
        assert!(paths.backup_dir.join("vendor_boot.img").is_file());
        assert!(paths.backup_dir.join("vbmeta.img").is_file());
    }

    #[test]
    fn gate_abort_never_creates_backup_or_classifies() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let mut backend = FakeBackend {
            gate_error: Some("blocked".into()),
            ..FakeBackend::default()
        };

        let result =
            execute_inspection(&mut backend, &export(), &paths, &phases(), &mut Vec::new());

        assert_eq!(result.unwrap_err(), "blocked");
        assert!(!paths.backup_dir.exists());
        assert!(!backend.events.iter().any(|event| event == "classify"));
    }
}
