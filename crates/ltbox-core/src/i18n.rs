//! i18n — JSON string tables with English fallback. Mirrors v2 `i18n.py`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use crate::error::{LtboxError, Result};

static STRINGS: OnceLock<StringTable> = OnceLock::new();

type TranslatorFn = dyn Fn(&str) -> String + Send + Sync;

/// Optional translator installed by the GUI so backend crates can emit
/// localized log lines without depending on the GUI's language state.
static TRANSLATOR: OnceLock<RwLock<Box<TranslatorFn>>> = OnceLock::new();

/// Install or swap the translator. Live language switches are supported
/// via the RwLock-held boxed closure.
pub fn set_translator<F>(f: F)
where
    F: Fn(&str) -> String + Send + Sync + 'static,
{
    let new_box: Box<TranslatorFn> = Box::new(f);
    match TRANSLATOR.get() {
        Some(lock) => {
            if let Ok(mut guard) = lock.write() {
                *guard = new_box;
            }
        }
        None => {
            let _ = TRANSLATOR.set(RwLock::new(new_box));
        }
    }
}

/// Resolve `key` via the registered translator or the built-in table.
/// Falls back to the key itself so calls are never destructive.
pub fn tr(key: &str) -> String {
    if let Some(lock) = TRANSLATOR.get()
        && let Ok(guard) = lock.read()
    {
        return guard(key);
    }
    get_string(key)
}

struct StringTable {
    strings: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

/// Initialize i18n. `lang_dir` must contain `en.json`, `ko.json`, etc.
pub fn load_lang(lang: &str, lang_dir: &Path) -> Result<()> {
    let fallback = load_json(&lang_dir.join("en.json"))?;
    let strings = if lang == "en" {
        fallback.clone()
    } else {
        let path = lang_dir.join(format!("{lang}.json"));
        if path.exists() {
            load_json(&path)?
        } else {
            fallback.clone()
        }
    };

    let _ = STRINGS.set(StringTable { strings, fallback });
    Ok(())
}

/// Localized string. Falls back to English, then the key itself.
pub fn get_string(key: &str) -> String {
    match STRINGS.get() {
        Some(table) => table
            .strings
            .get(key)
            .or_else(|| table.fallback.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string()),
        None => key.to_string(),
    }
}

/// `<exe_dir>/lang` (LTBox distribution: `bin/ltbox/lang/`).
pub fn default_lang_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("lang")
}

fn load_json(path: &Path) -> Result<HashMap<String, String>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        LtboxError::Config(format!("Cannot read language file {}: {e}", path.display()))
    })?;
    let map: HashMap<String, String> = serde_json::from_str(&content)?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn get_string_returns_key_when_not_loaded() {
        assert_eq!(get_string("nonexistent_key"), "nonexistent_key");
    }

    #[test]
    fn load_and_get_string() {
        let dir = tempfile::tempdir().unwrap();
        let en = dir.path().join("en.json");
        fs::write(&en, r#"{"hello": "world", "foo": "bar"}"#).unwrap();

        // STRINGS is a global OnceLock, so just verify JSON loading.
        let map = load_json(&en).unwrap();
        assert_eq!(map.get("hello").unwrap(), "world");
        assert_eq!(map.get("foo").unwrap(), "bar");
    }
}
