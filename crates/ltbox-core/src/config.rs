//! Configuration — loads config.json and provides typed accessors.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{LtboxError, Result};

/// Root provider configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub repo: Option<String>,
    pub tag: Option<String>,
    pub manager: Option<String>,
    pub manager_fallbacks: Option<Vec<String>>,
    pub workflow: Option<String>,
}

/// Region patterns (hex-encoded, PRC↔ROW).
#[derive(Debug, Clone, Deserialize)]
pub struct PatternConfig {
    pub row_dot: String,
    pub prc_dot: String,
    pub row_i: String,
    pub prc_i: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EdlConfig {
    pub loader_filename: String,
}

/// Top-level config.json structure.
#[derive(Debug, Clone, Deserialize)]
pub struct LtboxConfig {
    pub version: String,

    #[serde(default)]
    pub kernelsu: Option<ProviderConfig>,
    #[serde(rename = "kernelsu-next", default)]
    pub kernelsu_next: Option<ProviderConfig>,
    #[serde(rename = "sukisu-ultra", default)]
    pub sukisu_ultra: Option<ProviderConfig>,
    #[serde(default)]
    pub resukisu: Option<ProviderConfig>,
    #[serde(default)]
    pub apatch: Option<ProviderConfig>,
    #[serde(default)]
    pub folkpatch: Option<ProviderConfig>,
    #[serde(default)]
    pub magisk: Option<ProviderConfig>,

    #[serde(default)]
    pub edl: Option<EdlConfig>,

    #[serde(default)]
    pub patterns: Option<PatternConfig>,

    /// pubkey_sha1 → key filename
    #[serde(default)]
    pub key_map: HashMap<String, String>,

    /// country code → name
    #[serde(default)]
    pub country_codes: HashMap<String, String>,
}

impl LtboxConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            LtboxError::Config(format!("Cannot read config {}: {e}", path.display()))
        })?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn provider(&self, key: &str) -> Option<&ProviderConfig> {
        match key {
            "kernelsu" => self.kernelsu.as_ref(),
            "kernelsu-next" => self.kernelsu_next.as_ref(),
            "sukisu-ultra" => self.sukisu_ultra.as_ref(),
            "resukisu" => self.resukisu.as_ref(),
            "apatch" => self.apatch.as_ref(),
            "folkpatch" => self.folkpatch.as_ref(),
            "magisk" => self.magisk.as_ref(),
            _ => None,
        }
    }

    /// Decoded region pattern bytes.
    pub fn pattern_bytes(&self, name: &str) -> Option<Vec<u8>> {
        let patterns = self.patterns.as_ref()?;
        let hex = match name {
            "row_dot" => &patterns.row_dot,
            "prc_dot" => &patterns.prc_dot,
            "row_i" => &patterns.row_i,
            "prc_i" => &patterns.prc_i,
            _ => return None,
        };
        hex::decode(hex).ok()
    }

    pub fn resolve_key(&self, pubkey_sha1: &str, keys_dir: &Path) -> Option<PathBuf> {
        self.key_map
            .get(pubkey_sha1)
            .map(|filename| keys_dir.join(filename))
    }

    pub fn edl_loader(&self) -> &str {
        self.edl
            .as_ref()
            .map(|e| e.loader_filename.as_str())
            .unwrap_or("xbl_s_devprg_ns.melf")
    }
}

mod hex {
    pub fn decode(s: &str) -> std::result::Result<Vec<u8>, String> {
        if !s.len().is_multiple_of(2) {
            return Err("Odd hex length".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("Invalid hex at {i}: {e}"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_json() {
        let json = r#"{
            "version": "v2.6.5",
            "edl": { "loader_filename": "test.melf" },
            "patterns": { "row_dot": "2E524F57", "prc_dot": "2E505243", "row_i": "49524F57", "prc_i": "49505243" },
            "key_map": { "abc123": "testkey.pem" },
            "country_codes": { "US": "United States" }
        }"#;
        let config: LtboxConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.version, "v2.6.5");
        assert_eq!(config.edl_loader(), "test.melf");
        assert_eq!(config.pattern_bytes("row_dot").unwrap(), b".ROW");
        assert_eq!(config.pattern_bytes("prc_dot").unwrap(), b".PRC");
        assert!(config.resolve_key("abc123", Path::new("/keys")).is_some());
    }
}
