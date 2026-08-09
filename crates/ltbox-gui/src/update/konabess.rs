//! Stage-C KonaBess wizard handlers. Device dump/patch/flash execution belongs
//! to Stage D and is intentionally absent.

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
                    // Confirm → Apply is disabled until Stage D exists.
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
                    self.konabess.back();
                }
                Task::none()
            }
            KonaBessMsg::KonaBessInspectionReady(inspected) => {
                self.konabess.apply_inspection_result(inspected);
                Task::none()
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
