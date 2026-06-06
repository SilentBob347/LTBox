//! Settings-view handler: language, theme, default-loader path.
//! Extracted from `main.rs`.
use crate::*;
use iced::Task;

impl App {
    /// Settings view — language pick, theme dropdown, default-loader
    /// path management. Each variant either updates `self.settings`
    /// and persists, or spawns the file picker `Task` for the loader
    /// path.
    pub(crate) fn update_settings(&mut self, msg: SettingsMsg) -> Task<Message> {
        match msg {
            SettingsMsg::SetLanguage(l) => {
                self.settings.language = l;
                self.translations = Translations::load(l);
                install_core_translator(l);
                self.persist_settings();
                Task::none()
            }
            SettingsMsg::SetThemeSeed(seed) => {
                self.theme_seed = seed;
                self.sync_runtime_theme();
                self.persist_settings();
                Task::none()
            }
            SettingsMsg::SettingsPickDefaultLoader => {
                let spec = loader_file_spec("picker_target_edl_loader");
                pickers::pick_file_for(spec, &self.recent_paths, |__v| {
                    Message::Settings(SettingsMsg::SettingsDefaultLoaderChosen(__v))
                })
            }
            SettingsMsg::SettingsDefaultLoaderChosen(path) => {
                if let Some(p) = path {
                    self.remember_recent(pickers::PickerKind::File, &p);
                    self.default_loader_path = Some(p);
                    self.persist_settings();
                }
                Task::none()
            }
            SettingsMsg::SettingsClearDefaultLoader => {
                self.default_loader_path = None;
                self.persist_settings();
                Task::none()
            }
        }
    }
}
