//! EDL loader discovery + validation helpers, extracted from `main.rs`.

/// File-dialog / recent-chip extension filter for the EDL loader picker:
/// a stock `.melf` Firehose loader, or the `.xml` / encrypted `.x` Sahara
/// manifest (Y700 Gen 5). Single source so every loader picker + recents
/// chip row offers the same set.
pub(crate) const LOADER_PICKER_EXTS: &[&str] = &["melf", "mbn", "elf", "xml", "x"];

/// Locate the multi-image Sahara manifest in `dir`, case-insensitively.
/// Prefers the plaintext `qsahara_device_programmer.xml`; otherwise returns
/// the encrypted `qsahara_device_programmer.x` form, which
/// [`ltbox_device::edl::EdlSession::open`] decrypts at load time. `None`
/// when neither is present.
///
/// This only *locates* — it never decrypts or writes — so it is safe to
/// call from cheap UI gates (`can_next`) without side effects.
pub(crate) fn resolve_sahara_manifest(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let (mut plaintext, mut encrypted) = (None, None);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if ltbox_core::sahara_xml::is_manifest_filename(&p) {
                plaintext = Some(p);
            } else if ltbox_core::sahara_xml::is_encrypted_manifest_filename(&p) {
                encrypted = Some(p);
            }
        }
    }
    plaintext.or(encrypted)
}

/// Locate the EDL loader inside `dir`: the multi-image Sahara manifest
/// (plaintext `.xml` or encrypted `.x`) takes precedence over a single
/// `xbl_s_devprg_ns.melf`, since on a manifest device a stray `.melf` is
/// the wrong loader. Returns the path only — decryption of a `.x` manifest
/// happens in `EdlSession::open`.
pub(crate) fn find_edl_loader(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Some(manifest) = resolve_sahara_manifest(dir) {
        return Some(manifest);
    }
    let candidate = dir.join("xbl_s_devprg_ns.melf");
    if candidate.exists() {
        return Some(candidate);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("xbl_s_devprg_ns.melf")
            {
                return Some(entry.path());
            }
        }
    }
    None
}

pub(crate) fn is_loader_file(path: &std::path::Path) -> bool {
    // `.xml` covers TB323FU's `qsahara_device_programmer.xml` multi-
    // image manifest. `EdlSession::open` branches on the manifest
    // filename (case-insensitive) — any other `.xml` file would fail
    // there with a parse error rather than silently picking up the
    // single-loader path.
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "melf" | "mbn" | "elf" | "xml"
            )
        })
        .unwrap_or(false)
}

/// Whether `path`'s extension is one of the single-blob loader formats
/// (`.melf` / `.mbn` / `.elf`). Used by the TB323FU manifest-upgrade
/// gate to decide whether to look for a sibling manifest — `.xml` is
/// excluded so a manifest selection isn't recursively re-resolved.
pub(crate) fn is_melf_loader(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "melf" | "mbn" | "elf"))
        .unwrap_or(false)
}

/// True when `path`'s extension is the EDL loader form the given model needs:
/// TB323FU → `.xml` / `.x` (Sahara manifest); every other model → `.melf`.
/// Inspects only the file's own extension, not the images a manifest references.
pub(crate) fn loader_ext_fits_model(is_tb323fu: bool, path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if is_tb323fu {
        matches!(ext.as_deref(), Some("xml") | Some("x"))
    } else {
        ext.as_deref() == Some("melf")
    }
}

#[cfg(test)]
mod tests {
    use super::loader_ext_fits_model;
    use std::path::Path;

    #[test]
    fn loader_ext_fits_model_by_device() {
        // TB323FU needs the .xml / .x manifest.
        assert!(loader_ext_fits_model(
            true,
            Path::new("x/qsahara_device_programmer.xml")
        ));
        assert!(loader_ext_fits_model(true, Path::new("x/qsahara.x")));
        assert!(!loader_ext_fits_model(true, Path::new("x/prog.melf")));
        // Every other model needs the .melf single-blob (not .mbn / .elf / .xml).
        assert!(loader_ext_fits_model(false, Path::new("x/prog.melf")));
        assert!(!loader_ext_fits_model(false, Path::new("x/qsahara.xml")));
        assert!(!loader_ext_fits_model(false, Path::new("x/prog.mbn")));
    }
}
