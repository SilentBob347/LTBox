//! KonaBess wizard handlers for stock-image inspection, target selection, and
//! the irreversible rebuild/flash continuation.

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
                pickers::FilePickSpec::single("picker_target_konabess_export").with_filter(
                    self.t("picker_target_konabess_export").to_string(),
                    &["txt"],
                ),
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
                        let phases =
                            self.begin_phased_op(View::KonaBess, OperationPhaseKind::KonaBess);
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
                let probable_dtb_index = result.prepared.probable_dtb_index;
                self.konabess.prepared = Some(result.prepared);
                let auto_confirm = self
                    .konabess
                    .apply_inspection_result(result.candidates, probable_dtb_index);
                if auto_confirm {
                    self.update_konabess(KonaBessMsg::KonaBessTargetConfirm)
                } else {
                    Task::none()
                }
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
                if self.busy {
                    return Task::none();
                }
                let Some(target_index) = self.konabess.confirm_target() else {
                    return Task::none();
                };
                let Some(prepared) = self.konabess.prepared.clone() else {
                    return Task::none();
                };
                let Some(loader) = self.konabess.loader_path.clone() else {
                    return Task::none();
                };
                let Some(export_path) = self.konabess.export_path.clone() else {
                    return Task::none();
                };

                self.konabess.next();
                let phases = self.begin_phased_op(View::KonaBess, OperationPhaseKind::KonaBess);
                let ll = self.live_labels();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ltbox_core::runtime::run_heavy(move || {
                                konabess_flash_worker(
                                    std::path::PathBuf::from(loader),
                                    std::path::PathBuf::from(export_path),
                                    prepared,
                                    target_index,
                                    ll,
                                    phases,
                                )
                            })
                            .and_then(|result| result)
                        })
                        .await
                        .unwrap_or_else(|_| Err(ltbox_core::i18n::tr("err_task_failed")))
                    },
                    |result| match result {
                        Ok(log) => Message::KonaBess(KonaBessMsg::KonaBessFlashDone(log)),
                        Err(error) => Message::OperationError(error),
                    },
                )
            }
            KonaBessMsg::KonaBessTargetDismiss => {
                self.konabess.dismiss_target_popup();
                let Some(loader) = self.konabess.loader_path.clone() else {
                    self.konabess.cleanup_prepared();
                    return Task::none();
                };
                self.begin_silent_op(View::KonaBess);
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            ltbox_core::runtime::run_heavy(move || {
                                konabess_cancel_worker(std::path::PathBuf::from(loader))
                            })
                        })
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or_default()
                    },
                    |log| Message::KonaBess(KonaBessMsg::KonaBessCancelDone(log)),
                )
            }
            KonaBessMsg::KonaBessCancelDone(log) => {
                self.flush_exec_done_log(log);
                self.end_silent_op();
                self.konabess.cleanup_prepared();
                Task::none()
            }
            KonaBessMsg::KonaBessFlashDone(log) => {
                self.flush_exec_done_log(log);
                self.end_op();
                self.konabess.cleanup_prepared();
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltbox_patch::konabess::{ClassifiedDtb, GpuTable, KonaBessExport, VendorBootDtbInfo};

    fn candidate(index: usize) -> ClassifiedDtb {
        ClassifiedDtb {
            info: VendorBootDtbInfo {
                index,
                model: Some("test".into()),
                chip: Some("waipio".into()),
                gpu_shape: None,
                table: None,
            },
            structurally_matches: true,
        }
    }

    fn app_ready_for_inspection_result() -> App {
        App {
            konabess: KonaBessWizard {
                step: 2,
                loader_path: Some("loader.elf".into()),
                export_path: Some("export.txt".into()),
                export: Some(KonaBessExport {
                    chip: "waipio".into(),
                    description: "test".into(),
                    table: GpuTable { groups: vec![] },
                }),
                ..KonaBessWizard::default()
            },
            ..App::default()
        }
    }

    fn prepared(root: &std::path::Path, probable_dtb_index: Option<usize>) -> KonaBessPrepared {
        let work_dir = root.join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        KonaBessPrepared {
            vendor_boot: work_dir.join("vendor_boot.img"),
            vbmeta: work_dir.join("vbmeta.img"),
            backup_dir: root.join("backup_konabess"),
            slot_suffix: "_b".into(),
            probable_dtb_index,
            work_dir,
        }
    }

    #[test]
    fn unique_probable_dtb_skips_popup_and_starts_existing_flash_path() {
        let root = tempfile::tempdir().unwrap();
        let mut app = app_ready_for_inspection_result();
        let prepared = prepared(root.path(), Some(4));

        let task = app.update_konabess(KonaBessMsg::KonaBessInspectionReady(
            KonaBessInspectionResult {
                prepared: prepared.clone(),
                candidates: vec![candidate(4)],
                log: vec![],
            },
        ));

        assert!(!app.konabess.target_popup_open);
        assert_eq!(app.konabess.candidates.len(), 1);
        assert_eq!(app.konabess.selected_target_index, Some(4));
        assert_eq!(app.konabess.prepared, Some(prepared));
        assert_eq!(app.konabess.step, 3);
        assert!(app.busy);
        assert_eq!(task.units(), 1);
    }

    #[test]
    fn non_unique_probable_dtb_cases_open_existing_target_popup() {
        let cases = [
            (None, vec![candidate(4)], None),
            (Some(99), vec![candidate(4)], None),
            (Some(4), vec![candidate(4), candidate(4)], Some(4)),
        ];

        for (probable_dtb_index, candidates, expected_selection) in cases {
            let root = tempfile::tempdir().unwrap();
            let mut app = app_ready_for_inspection_result();
            let task = app.update_konabess(KonaBessMsg::KonaBessInspectionReady(
                KonaBessInspectionResult {
                    prepared: prepared(root.path(), probable_dtb_index),
                    candidates,
                    log: vec![],
                },
            ));

            assert!(app.konabess.target_popup_open);
            assert_eq!(app.konabess.selected_target_index, expected_selection);
            assert_eq!(app.konabess.step, 2);
            assert!(!app.busy);
            assert_eq!(task.units(), 0);
        }
    }
}
