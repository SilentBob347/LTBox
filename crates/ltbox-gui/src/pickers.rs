//! File/folder picker categories and rfd helpers.
//!
//! Each *folder* picker kind owns its own MRU list in
//! [`settings_store::RecentPaths`] keyed by [`PickerKind::storage_key`].
//! File picks share a single `File` bucket — they're parameterised by
//! [`FilePickSpec`] at the call site (ext filter / single-multi /
//! description target) per user spec ("unify into one kind with only the
//! ext filter, single/multi, and `[X]을 선택하세요` description
//! customisable"), so per-spec buckets would fragment recents needlessly.
//!
//! `pick_folder_for` / `pick_files_for` seed the native dialog with the
//! kind's most-recent path so users land where they last worked.

use iced::Task;
use rfd::AsyncFileDialog;
use std::path::PathBuf;

use crate::settings_store::RecentPaths;

/// Picker category. Determines which recents bucket is used + which
/// dialog flavour (folder vs file) opens.
///
/// The 4 folder kinds map 1:1 to the user-facing Browse-button semantics:
/// "loader only", "loader + rawprogram", "full QFIL firmware", "encrypted
/// rawprogram (.x)". `OutputFolder` is a 5th convenience kind for
/// dump/save destinations — previously memory-less. `File` is the
/// unified file pick with per-call [`FilePickSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickerKind {
    /// Folder containing the fixed-name EDL loader `xbl_s_devprg_ns.melf`.
    LoaderFolder,
    /// Folder with loader + `rawprogram*.xml` (rescue-style firmware).
    /// Currently unused — Boot Recovery moved to a single-`.melf`
    /// file picker (matching the root flow) since it resolves
    /// vendor_boot / vbmeta via GPT, not rawprogram XML. Kept so the
    /// user's existing JSON-stored recents keyed `loader_rawprogram_folder`
    /// don't get orphaned and the storage_key contract stays stable.
    #[allow(dead_code)]
    LoaderRawprogramFolder,
    /// Full QFIL firmware folder (programmer + all XML + partition images).
    QfilFirmwareFolder,
    /// Folder with encrypted `rawprogram*.x` files.
    EncryptedRawprogramFolder,
    /// Output / save destination folder (dumps, log saves).
    OutputFolder,
    /// Unified file pick — customised by [`FilePickSpec`] per call.
    File,
}

impl PickerKind {
    /// Stable string used as the JSON key in the on-disk recents map.
    /// **Must not change** without a migration — renaming a key silently
    /// orphans the user's history.
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::LoaderFolder => "loader_folder",
            Self::LoaderRawprogramFolder => "loader_rawprogram_folder",
            Self::QfilFirmwareFolder => "qfil_firmware_folder",
            Self::EncryptedRawprogramFolder => "encrypted_rawprogram_folder",
            Self::OutputFolder => "output_folder",
            Self::File => "file",
        }
    }

    /// `true` iff the picker opens a folder dialog (vs a file dialog).
    pub fn is_folder(self) -> bool {
        !matches!(self, Self::File)
    }

    /// i18n key for the unified Browse-button description. Resolves to
    /// the localised `[X]을 선택하세요` (or equivalent) string.
    ///
    /// File picks use `FilePickSpec::target_i18n_key` instead since the
    /// `[X]` slot varies per call — not per kind.
    ///
    /// NOTE: currently unused by the view code — views still render the
    /// original `btn_browse_*` keys. Kept wired so localisation + view
    /// reshuffle can land as one follow-up without re-threading the enum.
    #[allow(dead_code)]
    pub fn browse_label_key(self) -> &'static str {
        match self {
            Self::LoaderFolder => "picker_browse_loader_folder",
            Self::LoaderRawprogramFolder => "picker_browse_loader_rawprogram_folder",
            Self::QfilFirmwareFolder => "picker_browse_qfil_firmware_folder",
            Self::EncryptedRawprogramFolder => "picker_browse_encrypted_rawprogram_folder",
            Self::OutputFolder => "picker_browse_output_folder",
            Self::File => "picker_browse_file", // unused — file picks use spec key
        }
    }
}

/// File-picker call parameters. Only these fields vary between file
/// pickers (per user: "ext filter + single/multi + `[X]을 선택하세요`
/// description — unify the rest"); the dialog itself is always the same
/// `File` kind so recents stay in one bucket.
#[derive(Debug, Clone)]
pub struct FilePickSpec {
    /// Extensions without the leading dot, e.g. `["img", "bin"]`.
    /// Empty = no filter (native "All files").
    pub exts: Vec<String>,
    /// Human-readable filter label shown in the dialog's type dropdown.
    pub filter_label: String,
    /// `true` for multi-select (`pick_files`), `false` for single (`pick_file`).
    pub multi: bool,
    /// i18n key that fills the `[X]` slot in the localised
    /// `[X]을 선택하세요` description above the Browse button.
    ///
    /// Threaded through but not yet consumed by views — same deferred-
    /// rollout note as [`PickerKind::browse_label_key`].
    #[allow(dead_code)]
    pub target_i18n_key: &'static str,
}

impl FilePickSpec {
    /// Single-file, no filter, custom description target.
    pub fn single(target_i18n_key: &'static str) -> Self {
        Self {
            exts: Vec::new(),
            filter_label: String::new(),
            multi: false,
            target_i18n_key,
        }
    }

    /// Multi-file, no filter, custom description target.
    pub fn multi(target_i18n_key: &'static str) -> Self {
        Self {
            exts: Vec::new(),
            filter_label: String::new(),
            multi: true,
            target_i18n_key,
        }
    }

    /// Attach an ext filter (fluent builder). Both `filter_label` and
    /// `exts` must be set for the filter to register with rfd.
    pub fn with_filter(mut self, filter_label: impl Into<String>, exts: &[&str]) -> Self {
        self.filter_label = filter_label.into();
        self.exts = exts.iter().map(|s| (*s).to_string()).collect();
        self
    }
}

/// `Task<Message>` that opens a folder-picker for `kind`, seeded with
/// that kind's most-recent path (falls back to OS default). Sends
/// `on_pick(Some(path))` / `on_pick(None)` on close.
pub fn pick_folder_for<M: 'static + Send>(
    kind: PickerKind,
    recents: &RecentPaths,
    on_pick: impl 'static + Send + Fn(Option<String>) -> M,
) -> Task<M> {
    debug_assert!(
        kind.is_folder(),
        "pick_folder_for called with file kind {kind:?}"
    );
    let start_dir: Option<PathBuf> = recents.most_recent(kind.storage_key()).map(PathBuf::from);
    Task::perform(
        async move {
            let mut dialog = AsyncFileDialog::new();
            if let Some(sd) = start_dir.filter(|p| p.is_dir()) {
                dialog = dialog.set_directory(sd);
            }
            dialog
                .pick_folder()
                .await
                .map(|f| f.path().to_string_lossy().to_string())
        },
        on_pick,
    )
}

/// Build the rfd dialog with `spec`'s filter + the `File` kind's last
/// directory as starting dir. Extracted so single and multi variants
/// share identical setup.
fn build_file_dialog(spec: &FilePickSpec, recents: &RecentPaths) -> AsyncFileDialog {
    let mut dialog = AsyncFileDialog::new();
    if !spec.exts.is_empty() && !spec.filter_label.is_empty() {
        let exts: Vec<&str> = spec.exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&spec.filter_label, &exts);
    }
    if let Some(sd) = recents
        .most_recent(PickerKind::File.storage_key())
        .map(PathBuf::from)
        .filter(|p| p.exists())
    {
        // Recent may be a file path — rfd wants a directory, so normalise
        // to parent when needed. Missing/non-dir parent falls through to
        // the OS default (no set_directory call).
        let dir = if sd.is_dir() {
            sd
        } else {
            sd.parent().map(PathBuf::from).unwrap_or(sd)
        };
        if dir.is_dir() {
            dialog = dialog.set_directory(dir);
        }
    }
    dialog
}

/// Single-file pick — delivers `Option<String>`. Preferred over
/// [`pick_files_for`] when `spec.multi == false` so callers don't have
/// to unwrap a 1-element Vec.
pub fn pick_file_for<M: 'static + Send>(
    spec: FilePickSpec,
    recents: &RecentPaths,
    on_pick: impl 'static + Send + Fn(Option<String>) -> M,
) -> Task<M> {
    debug_assert!(
        !spec.multi,
        "pick_file_for called with multi=true spec; use pick_files_for"
    );
    let dialog = build_file_dialog(&spec, recents);
    Task::perform(
        async move {
            dialog
                .pick_file()
                .await
                .map(|h| h.path().to_string_lossy().to_string())
        },
        on_pick,
    )
}

/// Multi-file pick — delivers `Option<Vec<String>>`. `None` on cancel;
/// an empty Vec should be treated as "no selection" too (rfd on some
/// platforms reports empty selection as `Some(vec![])`).
pub fn pick_files_for<M: 'static + Send>(
    spec: FilePickSpec,
    recents: &RecentPaths,
    on_pick: impl 'static + Send + Fn(Option<Vec<String>>) -> M,
) -> Task<M> {
    debug_assert!(
        spec.multi,
        "pick_files_for called with multi=false spec; use pick_file_for"
    );
    let dialog = build_file_dialog(&spec, recents);
    Task::perform(
        async move {
            dialog.pick_files().await.map(|handles| {
                handles
                    .into_iter()
                    .map(|h| h.path().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
            })
        },
        on_pick,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_keys_are_unique_and_stable() {
        let all = [
            PickerKind::LoaderFolder,
            PickerKind::LoaderRawprogramFolder,
            PickerKind::QfilFirmwareFolder,
            PickerKind::EncryptedRawprogramFolder,
            PickerKind::OutputFolder,
            PickerKind::File,
        ];
        let mut keys: Vec<&str> = all.iter().map(|k| k.storage_key()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), all.len(), "storage_key collision");

        // Spot-check stable literals — renaming any of these breaks user
        // recents on upgrade. This test fails loudly if someone edits one.
        assert_eq!(PickerKind::LoaderFolder.storage_key(), "loader_folder");
        assert_eq!(PickerKind::File.storage_key(), "file");
    }

    #[test]
    fn is_folder_only_false_for_file() {
        for k in [
            PickerKind::LoaderFolder,
            PickerKind::LoaderRawprogramFolder,
            PickerKind::QfilFirmwareFolder,
            PickerKind::EncryptedRawprogramFolder,
            PickerKind::OutputFolder,
        ] {
            assert!(k.is_folder());
        }
        assert!(!PickerKind::File.is_folder());
    }

    #[test]
    fn spec_builder_sets_filter_and_keeps_target() {
        let s = FilePickSpec::single("picker_target_partition_image")
            .with_filter("Partition image", &["img", "bin"]);
        assert!(!s.multi);
        assert_eq!(s.target_i18n_key, "picker_target_partition_image");
        assert_eq!(s.exts, vec!["img".to_string(), "bin".to_string()]);
        assert_eq!(s.filter_label, "Partition image");
    }

    #[test]
    fn spec_multi_builder() {
        let s =
            FilePickSpec::multi("picker_target_kpm_modules").with_filter("KPM modules", &["kpm"]);
        assert!(s.multi);
        assert_eq!(s.exts, vec!["kpm".to_string()]);
    }

    #[test]
    fn spec_without_filter_leaves_exts_empty() {
        let s = FilePickSpec::single("picker_target_any");
        assert!(s.exts.is_empty());
        assert!(s.filter_label.is_empty());
    }
}
