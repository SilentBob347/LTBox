//! Persisted user settings (language, theme, recents).
//!
//! Lives in the user's config dir (outside the install tree so
//! replacing `ltbox.exe` keeps preferences).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const APP_DIR: &str = "ltbox";
const FILE_NAME: &str = "settings.json";

/// Maximum number of recent file / folder paths to remember.
pub const RECENT_MAX: usize = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentPaths {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub folders: Vec<String>,
}

impl RecentPaths {
    /// Push to the front, dedup, truncate to `RECENT_MAX`.
    /// Returns `true` iff the list changed.
    pub fn push_file(&mut self, path: &str) -> bool {
        push_front_dedup(&mut self.files, path)
    }

    pub fn push_folder(&mut self, path: &str) -> bool {
        push_front_dedup(&mut self.folders, path)
    }
}

fn push_front_dedup(list: &mut Vec<String>, path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let before = list.clone();
    list.retain(|p| p != path);
    list.insert(0, path.to_string());
    if list.len() > RECENT_MAX {
        list.truncate(RECENT_MAX);
    }
    list != &before
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSettings {
    #[serde(default = "default_language")]
    pub language: String,
    /// "system" | "light" | "dark". Blank in old configs — loader
    /// upgrades via the legacy `dark_mode` field below.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Legacy flag kept for upgrade compatibility. `theme` is the
    /// source of truth for new saves.
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub recent_paths: RecentPaths,
}

fn default_language() -> String {
    "en".to_string()
}

fn default_theme() -> String {
    String::new()
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: "system".to_string(),
            dark_mode: false,
            recent_paths: RecentPaths::default(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR).join(FILE_NAME))
}

/// Load settings. `Default` on missing / malformed / no config dir.
pub fn load() -> PersistedSettings {
    let Some(path) = config_path() else {
        return PersistedSettings::default();
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return PersistedSettings::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

/// Persist settings. Errors are swallowed so a read-only config dir
/// doesn't break the GUI.
pub fn save(settings: &PersistedSettings) {
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(&path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_follow_system() {
        let s = PersistedSettings::default();
        assert_eq!(s.language, "en");
        assert_eq!(s.theme, "system");
        assert!(!s.dark_mode);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: PersistedSettings = serde_json::from_str(r#"{"dark_mode": true}"#).unwrap();
        assert_eq!(s.language, "en");
        assert_eq!(s.theme, "");
        assert!(s.dark_mode);
    }

    #[test]
    fn theme_field_roundtrips() {
        let s: PersistedSettings = serde_json::from_str(r#"{"theme": "dark"}"#).unwrap();
        assert_eq!(s.theme, "dark");
    }
}
