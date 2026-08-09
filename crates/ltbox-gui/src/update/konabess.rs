//! KonaBess wizard handlers. Stage D part 1 ends after stock-image inspection
//! and target selection; patch/rebuild/flash continuation remains unwired.

use crate::*;
use iced::Task;
use ltbox_core::tr_args;

impl App {
    pub(crate) fn update_konabess(&mut self, msg: KonaBessMsg) -> Task<Message> {
        match msg {
            KonaBessMsg::KonaBessSelectLoader => self.pick_loader_with_default(|path| {
                Message::KonaBess(KonaBessMsg::KonaBessLoaderChosen(path))
            }),
            KonaBessMsg::KonaBessLoaderChosen(path) => {
                if let Some(path) = path {
                    match self.resolve_loader_input(&path) {
                        Ok(loader) if self.loader_fits_model(std::path::Path::new(&loader)) => {
                            self.konabess.loader_path = Some(loader);
                            self.konabess.loader_error = None;
                        }
                        Ok(_) => {
                            self.konabess.loader_path = None;
                            self.konabess.loader_error =
                                Some(self.t("loader_model_mismatch_tooltip").to_string());
                        }
                        Err(message) => {
                            self.konabess.loader_path = None;
                            self.konabess.loader_error = Some(message);
                        }
                    }
                }
                Task::none()
            }
            KonaBessMsg::KonaBessSelectExport => pickers::pick_file_for(
                pickers::FilePickSpec::single("picker_target_konabess_export"),
                &self.recent_paths,
                |path| Message::KonaBess(KonaBessMsg::KonaBessExportChosen(path)),
            ),
            KonaBessMsg::KonaBessExportChosen(path) => {
                if let Some(path) = path {
                    if std::path::Path::new(&path).is_file() {
                        self.remember_recent(pickers::PickerKind::File, &path);
                    }
                    match ltbox_patch::konabess::read_export(std::path::Path::new(&path)) {
                        Ok(export) => {
                            self.konabess.export_path = Some(path);
                            self.konabess.export = Some(export);
                            self.konabess.export_error = None;
                        }
                        Err(error) => {
                            self.konabess.export_path = None;
                            self.konabess.export = None;
                            self.konabess.export_error = Some(tr_args!(
                                "konabess_export_invalid",
                                error = error.to_string()
                            ));
                        }
                    }
                }
                Task::none()
            }
            KonaBessMsg::KonaBessNext => {
                match self.konabess.step {
                    0 => {
                        let selected = self.konabess.loader_path.clone();
                        match self.validate_loader_path(&selected) {
                            Ok(loader) if self.loader_fits_model(std::path::Path::new(&loader)) => {
                                self.konabess.loader_error = None;
                                self.konabess.next();
                            }
                            Ok(_) => {
                                self.error_msg = None;
                                self.konabess.loader_error =
                                    Some(self.t("loader_model_mismatch_tooltip").to_string());
                            }
                            Err(()) => {
                                self.konabess.loader_error = self.error_msg.take();
                            }
                        }
                    }
                    1 if self.konabess.can_next() => self.konabess.next(),
                    2 if self.konabess.can_next() => {
                        if self.busy {
                            return Task::none();
                        }
                        let Some(loader) = self.konabess.loader_path.clone() else {
                            return Task::none();
                        };
                        if self.validate_loader_path(&Some(loader.clone())).is_err() {
                            return Task::none();
                        }
                        let Some(export) = self.konabess.export.clone() else {
                            return Task::none();
                        };

                        self.konabess.cleanup_prepared();
                        self.konabess.next();
                        let phases =
                            self.begin_phased_op(View::Advanced, OperationPhaseKind::KonaBess);
                        let conn = self.connection;
                        let is_tb323fu = self.is_tb323fu();
                        let ll = self.live_labels();
                        let loader = std::path::PathBuf::from(loader);
                        return Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    ltbox_core::runtime::run_heavy(move || {
                                        konabess_inspection_worker(
                                            conn, loader, export, is_tb323fu, ll, phases,
                                        )
                                    })
                                    .and_then(|result| result)
                                })
                                .await
                                .unwrap_or_else(|_| Err(ltbox_core::i18n::tr("err_task_failed")))
                            },
                            |result| match result {
                                Ok(result) => {
                                    Message::KonaBess(KonaBessMsg::KonaBessInspectionReady(result))
                                }
                                Err(error) => {
                                    Message::KonaBess(KonaBessMsg::KonaBessInspectionFailed(error))
                                }
                            },
                        );
                    }
                    2 | 3 => {}
                    _ => {}
                }
                Task::none()
            }
            KonaBessMsg::KonaBessBack => {
                if self.konabess.step == 0 {
                    self.konabess.reset();
                    self.advanced_wizard_open = AdvancedWizardOpen::None;
                } else {
                    if self.konabess.prepared.is_some() {
                        self.konabess.cleanup_prepared();
                    }
                    self.konabess.back();
                }
                Task::none()
            }
            KonaBessMsg::KonaBessInspectionReady(result) => {
                self.flush_exec_done_log(result.log);
                self.end_op();
                self.current_op_step = 2;
                self.konabess.prepared = Some(result.prepared);
                self.konabess.apply_inspection_result(result.candidates);
                Task::none()
            }
            KonaBessMsg::KonaBessInspectionFailed(error) => {
                self.konabess.cleanup_prepared();
                self.konabess.step = 2;
                self.update(Message::OperationError(error))
            }
            KonaBessMsg::KonaBessTargetSelected(index) => {
                self.konabess.select_target(index);
                Task::none()
            }
            KonaBessMsg::KonaBessTargetConfirm => {
                self.konabess.confirm_target();
                Task::none()
            }
            KonaBessMsg::KonaBessTargetDismiss => {
                self.konabess.dismiss_target_popup();
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltbox_patch::konabess::{ClassifiedDtb, GpuTable, KonaBessExport, VendorBootDtbInfo};

    #[test]
    fn inspection_result_reaches_existing_target_popup_seam() {
        let root = tempfile::tempdir().unwrap();
        let work_dir = root.path().join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        let candidate = ClassifiedDtb {
            info: VendorBootDtbInfo {
                index: 4,
                model: Some("test".into()),
                chip: Some("waipio".into()),
                gpu_shape: None,
            },
            structurally_matches: true,
        };
        let mut app = App {
            konabess: KonaBessWizard {
                step: 3,
                export: Some(KonaBessExport {
                    chip: "waipio".into(),
                    description: "test".into(),
                    table: GpuTable { groups: vec![] },
                }),
                ..KonaBessWizard::default()
            },
            ..App::default()
        };
        let prepared = KonaBessPrepared {
            vendor_boot: work_dir.join("vendor_boot.img"),
            vbmeta: work_dir.join("vbmeta.img"),
            backup_dir: root.path().join("backup_critical_1_konabess"),
            slot_suffix: "_b".into(),
            work_dir,
        };

        let _task = app.update_konabess(KonaBessMsg::KonaBessInspectionReady(
            KonaBessInspectionResult {
                prepared: prepared.clone(),
                candidates: vec![candidate],
                log: vec![],
            },
        ));

        assert!(app.konabess.target_popup_open);
        assert_eq!(app.konabess.candidates.len(), 1);
        assert_eq!(app.konabess.prepared, Some(prepared));
    }
}
