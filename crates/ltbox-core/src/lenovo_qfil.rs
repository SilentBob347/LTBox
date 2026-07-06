//! Lenovo PTSTPD `getPadFlashingMachine` client.
//!
//! Resolves a device MTM against the public flashing-machine endpoint and
//! returns the official QFIL (flash-tool) package for that model. Powers the
//! dashboard's "click firmware → QFIL Firmware" flow.
//!
//! CN-market only: the endpoint answers for MTMs of devices sold in mainland
//! China. The GUI gates on `SaleArea == CN` before calling this and points
//! global devices at Lenovo's Software Fix tool instead.
//!
//! The response envelope is `{ code, message, data: [ ... ] }` with `data` an
//! array of package entries. Only the first entry is surfaced (mirrors the
//! upstream tool). Chinese-text fields are raw UTF-8 and returned verbatim.

use crate::error::{LtboxError, Result};
use serde::Deserialize;

const ENDPOINT: &str = "https://ptstpd.lenovo.com.cn/home/ConfigurationQuery/getPadFlashingMachine";

/// Extraction password for the official QFIL package archive. A fixed
/// constant published in Lenovo's own flashing tool — not device-specific.
pub const PACKAGE_PASSWORD: &str = "FC(fv:SknR";

/// Slim view of one flashing-machine entry.
#[derive(Debug, Clone, Default)]
pub struct QfilPackage {
    /// Direct download URL for the flash-tool archive (`download_url`).
    pub download_url: String,
    /// Firmware version tag (`latest_version`).
    pub version: String,
    /// Archive file name (`server_version_name`).
    pub file_name: String,
    /// Model this package targets (`product_model`).
    pub product_model: String,
    /// SoC platform label (`platform`, e.g. "高通" / "联发科"). Raw UTF-8.
    pub platform: String,
    /// First-published unix time (`add_time`), seconds. `None` if absent.
    pub add_time: Option<i64>,
    /// Last-updated unix time (`upd_time`), seconds. `None` if absent.
    pub upd_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    data: Option<Vec<serde_json::Value>>,
}

/// Blocking POST against the endpoint with `{"mtm": mtm}`. Returns
/// `Ok(None)` when the MTM resolves to no package (empty `data`), and
/// `Ok(Some(_))` for the first package entry.
pub fn fetch_qfil_package(mtm: &str) -> Result<Option<QfilPackage>> {
    let mtm = mtm.trim();
    if mtm.is_empty() {
        return Err(LtboxError::Other("empty MTM".into()));
    }
    let agent = crate::downloader::build_agent();
    let mut resp = agent
        .post(ENDPOINT)
        .send_json(serde_json::json!({ "mtm": mtm }))
        .map_err(|e| LtboxError::Download(format!("Lenovo QFIL POST: {e}")))?;
    let env: Envelope = resp
        .body_mut()
        .read_json()
        .map_err(|e| LtboxError::Download(format!("Lenovo QFIL JSON: {e}")))?;
    if env.code != Some(200) {
        let msg = env.message.unwrap_or_else(|| "upstream error".to_string());
        return Err(LtboxError::Download(format!(
            "Lenovo QFIL code {:?}: {msg}",
            env.code
        )));
    }
    let Some(entry) = env.data.unwrap_or_default().into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(parse_entry(&entry)))
}

/// Pull the fields we surface from one `data` object. Missing / null values
/// become empty strings (or `None` for the timestamps) so the popup can skip
/// blank rows without branching on the upstream JSON shape.
fn parse_entry(v: &serde_json::Value) -> QfilPackage {
    let s = |k: &str| -> String {
        match v.get(k) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    };
    let i = |k: &str| -> Option<i64> {
        match v.get(k) {
            Some(serde_json::Value::Number(n)) => n.as_i64(),
            Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    };
    QfilPackage {
        download_url: s("download_url"),
        version: s("latest_version"),
        file_name: s("server_version_name"),
        product_model: s("product_model"),
        platform: s("platform"),
        add_time: i("add_time"),
        upd_time: i("upd_time"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entry_pulls_fields_and_times() {
        let v = serde_json::json!({
            "download_url": "http://example.invalid/x.7z",
            "latest_version": "TB000_ZUXOS_1.0",
            "server_version_name": "TB000_ZUXOS_1.0_Tool.7z",
            "product_model": "TB000FC",
            "platform": "高通",
            "add_time": 1_700_000_000i64,
            "upd_time": "1710000000",
            "flashing_machine_method": "ignored"
        });
        let p = parse_entry(&v);
        assert_eq!(p.download_url, "http://example.invalid/x.7z");
        assert_eq!(p.version, "TB000_ZUXOS_1.0");
        assert_eq!(p.file_name, "TB000_ZUXOS_1.0_Tool.7z");
        assert_eq!(p.product_model, "TB000FC");
        assert_eq!(p.platform, "高通");
        assert_eq!(p.add_time, Some(1_700_000_000));
        // String timestamps are coerced.
        assert_eq!(p.upd_time, Some(1_710_000_000));
    }

    #[test]
    fn parse_entry_tolerates_missing_fields() {
        let p = parse_entry(&serde_json::json!({}));
        assert!(p.download_url.is_empty());
        assert_eq!(p.add_time, None);
        assert_eq!(p.upd_time, None);
    }

    /// Live smoke test against the real endpoint. Set `QFIL_TEST_MTM` to a CN
    /// device MTM (a model code, not a serial) to exercise the full POST +
    /// parse path:
    /// `cargo test -p ltbox-core -- --ignored qfil_fetch_smoke --nocapture`
    #[test]
    #[ignore = "network; set QFIL_TEST_MTM to a CN device MTM"]
    fn qfil_fetch_smoke() {
        let mtm = std::env::var("QFIL_TEST_MTM")
            .expect("set QFIL_TEST_MTM to a CN device MTM to run this test");
        let pkg = fetch_qfil_package(&mtm)
            .expect("QFIL fetch failed")
            .expect("no package for this MTM");
        assert!(!pkg.download_url.is_empty(), "download_url must be present");
        eprintln!(
            "download_url={}\nversion={} file={}\nplatform={} upd_time={:?}",
            pkg.download_url, pkg.version, pkg.file_name, pkg.platform, pkg.upd_time
        );
    }
}
