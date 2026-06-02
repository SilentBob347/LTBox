//! Wizard model — the per-flow wizard state structs and their
//! navigation logic, extracted from `main.rs`.

use crate::{Family, NightlySource, Provider, RootMode, VerChoice, is_loader_file};

// Internal steps: 0=Family, 1=Mode, 2=Provider, 3=Version,
// 4=NightlySource, 5=Folder, 6=Confirm, 7=Flash, 8=APatch KPM.
// Mode auto-skips for non-KSU. GKI: steps 3/4 collapse into a kernel
// zip picker at 2. MagiskForks: skip Version, APK picker at 3. Nightly
// inserts 4 between Version and Folder.
#[derive(Default)]
pub(crate) struct RootWizard {
    pub(crate) step: usize,
    pub(crate) family: Option<Family>,
    pub(crate) mode: Option<RootMode>,
    pub(crate) provider: Option<Provider>,
    pub(crate) version: Option<VerChoice>,
    pub(crate) nightly_source: Option<NightlySource>,
    pub(crate) file_path: Option<String>, // GKI zip, MagiskForks APK, or manual nightly
    pub(crate) folder_path: Option<String>, // Firmware folder (loader + optional testkey)
    /// APatch: `.kpm` modules to embed. Multi-select + per-entry remove.
    pub(crate) kpm_paths: Vec<String>,
    /// APatch superkey. Secret — never echoed in confirm or any log.
    pub(crate) superkey: Option<String>,
    pub(crate) superkey_popup_open: bool,
    /// Buffer for the currently visible field in the superkey popup;
    /// reset between the first-entry and re-entry stages.
    pub(crate) superkey_buffer: String,
    /// First-entry value held while the popup waits for the user to
    /// re-enter their key on the second stage. `None` → still on the
    /// first-entry stage; `Some(v)` → on the verification stage and
    /// `superkey_buffer` will be compared against `v` on Confirm.
    pub(crate) superkey_first_entry: Option<String>,
    /// Nightly ManualInput: committed workflow run ID (1..=12 digits).
    /// Only meaningful when `nightly_source == Some(ManualInput)`.
    pub(crate) run_id: Option<String>,
    pub(crate) run_id_popup_open: bool,
    pub(crate) run_id_buffer: String,
    /// KernelSU LKM: normalized `major.minor` kernel version from ADB or manual popup.
    pub(crate) kernel_version: Option<String>,
    pub(crate) kernel_version_popup_open: bool,
    pub(crate) kernel_version_buffer: String,
}

pub(crate) const ROOT_STEPS: &[&str] = &[
    "root_step_type",
    "root_step_mode",
    "root_step_provider",
    "root_step_version",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_NIGHTLY: &[&str] = &[
    "root_step_type",
    "root_step_mode",
    "root_step_provider",
    "root_step_version",
    "root_step_source",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_GKI: &[&str] = &[
    "root_step_type",
    "root_step_mode",
    "root_step_kernel",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_NOMODE: &[&str] = &[
    "root_step_type",
    "root_step_provider",
    "root_step_version",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_NOMODE_NIGHTLY: &[&str] = &[
    "root_step_type",
    "root_step_provider",
    "root_step_version",
    "root_step_source",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_FORKS: &[&str] = &[
    "root_step_type",
    "root_step_provider",
    "root_step_apk",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_APATCH: &[&str] = &[
    "root_step_type",
    "root_step_provider",
    "root_step_version",
    "root_step_kpm",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];
pub(crate) const ROOT_STEPS_APATCH_NIGHTLY: &[&str] = &[
    "root_step_type",
    "root_step_provider",
    "root_step_version",
    "root_step_source",
    "root_step_kpm",
    "root_step_folder",
    "root_step_confirm",
    "root_step_flash",
];

impl RootWizard {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// True on the final (flash/exec) step. Used to skip wizard reset
    /// when the user sidebar-bounces mid-operation.
    pub(crate) fn is_in_exec(&self) -> bool {
        self.step == 7
    }
    /// True on the confirm screen (step 6, before Flash). A sidebar
    /// bounce here preserves the wizard instead of resetting to step 0.
    pub(crate) fn is_on_confirm_step(&self) -> bool {
        self.step == 6
    }

    pub(crate) fn is_gki(&self) -> bool {
        self.mode == Some(RootMode::Gki)
    }
    pub(crate) fn is_forks(&self) -> bool {
        self.provider == Some(Provider::MagiskForks)
    }
    pub(crate) fn is_nightly(&self) -> bool {
        self.version == Some(VerChoice::Nightly)
    }
    pub(crate) fn is_apatch(&self) -> bool {
        self.family == Some(Family::APatch)
    }

    pub(crate) fn is_ksu_lkm(&self) -> bool {
        self.family == Some(Family::KernelSU) && self.mode == Some(RootMode::Lkm)
    }

    pub(crate) fn needs_ksu_lkm_kernel_version(&self) -> bool {
        self.is_ksu_lkm() && self.kernel_version.is_none()
    }

    pub(crate) fn active_steps(&self) -> &'static [&'static str] {
        if self.is_gki() {
            return ROOT_STEPS_GKI;
        }
        let has_modes = self.family.map(|f| f.has_modes()).unwrap_or(false);
        if self.is_forks() {
            return ROOT_STEPS_FORKS;
        }
        if self.is_apatch() {
            // APatch route: Version → KPM → Folder. Superkey popup
            // lives on the KPM→Folder edge, not as its own step.
            return if self.is_nightly() {
                ROOT_STEPS_APATCH_NIGHTLY
            } else {
                ROOT_STEPS_APATCH
            };
        }
        match (has_modes, self.is_nightly()) {
            (true, true) => ROOT_STEPS_NIGHTLY,
            (true, false) => ROOT_STEPS,
            (false, true) => ROOT_STEPS_NOMODE_NIGHTLY,
            (false, false) => ROOT_STEPS_NOMODE,
        }
    }

    pub(crate) fn display_step(&self) -> usize {
        // Map internal step index into the position within the active
        // route's label array. Comments at each branch show the mapping.
        let has_modes = self.family.map(|f| f.has_modes()).unwrap_or(false);
        if self.is_gki() {
            // 0,1,2,5,6,7 → 0..5
            return match self.step {
                0 => 0,
                1 => 1,
                2 => 2,
                5 => 3,
                6 => 4,
                7 => 5,
                _ => self.step,
            };
        }
        if self.is_forks() {
            // 0,2,3,5,6,7 → 0..5
            return match self.step {
                0 => 0,
                2 => 1,
                3 => 2,
                5 => 3,
                6 => 4,
                7 => 5,
                _ => self.step,
            };
        }
        if self.is_apatch() {
            // Stable: 0,2,3,8,5,6,7 → 0..6. Nightly: add 4 → 0..7.
            if self.is_nightly() {
                return match self.step {
                    0 => 0,
                    2 => 1,
                    3 => 2,
                    4 => 3,
                    8 => 4,
                    5 => 5,
                    6 => 6,
                    7 => 7,
                    _ => self.step,
                };
            }
            return match self.step {
                0 => 0,
                2 => 1,
                3 => 2,
                8 => 3,
                5 => 4,
                6 => 5,
                7 => 6,
                _ => self.step,
            };
        }
        if !has_modes {
            if self.is_nightly() {
                // 0,2,3,4,5,6,7 → 0..6
                return match self.step {
                    0 => 0,
                    2 => 1,
                    3 => 2,
                    4 => 3,
                    5 => 4,
                    6 => 5,
                    7 => 6,
                    _ => self.step,
                };
            }
            // 0,2,3,5,6,7 → 0..5
            return match self.step {
                0 => 0,
                2 => 1,
                3 => 2,
                5 => 3,
                6 => 4,
                7 => 5,
                _ => self.step,
            };
        }
        if self.is_nightly() {
            self.step
        } else {
            // 0,1,2,3,5,6,7 → 0..6
            match self.step {
                5 => 4,
                6 => 5,
                7 => 6,
                s => s,
            }
        }
    }

    pub(crate) fn next(&mut self) {
        match self.step {
            0 => {
                if let Some(f) = self.family
                    && !f.has_modes()
                {
                    self.mode = None;
                    self.step = 2;
                    return;
                }
                self.step = 1;
            }
            1 => self.step = 2,
            2 => {
                if self.is_gki() {
                    self.step = 5;
                    return;
                }
                self.step = 3;
            }
            3 => {
                if self.is_forks() {
                    self.step = 5;
                    return;
                }
                if self.is_nightly() {
                    self.step = 4;
                    return;
                }
                if self.is_apatch() {
                    self.step = 8;
                    return;
                }
                self.step = 5;
            }
            4 => {
                if self.is_apatch() {
                    self.step = 8;
                    return;
                }
                self.step = 5;
            }
            // Exit gated by superkey popup — caller sets step = 5 on confirm.
            8 => self.step = 5,
            5 => self.step = 6,
            6 => self.step = 7,
            _ => {}
        }
    }

    pub(crate) fn back(&mut self) {
        match self.step {
            1 => self.step = 0,
            2 => {
                if let Some(f) = self.family
                    && !f.has_modes()
                {
                    self.step = 0;
                    return;
                }
                self.step = 1;
            }
            3 => self.step = 2,
            4 => self.step = 3,
            5 => {
                // Folder → whichever sub-step populated the source.
                if self.is_gki() {
                    self.step = 2;
                    return;
                }
                if self.is_forks() {
                    self.step = 3;
                    return;
                }
                if self.is_apatch() {
                    self.step = 8;
                    return;
                }
                if self.is_nightly() {
                    self.step = 4;
                    return;
                }
                self.step = 3;
            }
            6 => self.step = 5,
            7 => self.step = 6,
            8 => {
                self.step = if self.is_nightly() { 4 } else { 3 };
            }
            _ => {}
        }
    }

    pub(crate) fn can_next(&self) -> bool {
        match self.step {
            0 => self.family.is_some(),
            1 => self.mode.is_some(),
            2 => {
                if self.is_gki() {
                    self.file_path.is_some()
                } else {
                    self.provider.is_some()
                }
            }
            3 => {
                if self.is_forks() {
                    self.file_path.is_some()
                } else {
                    self.version.is_some()
                }
            }
            4 => match self.nightly_source {
                // ManualInput also needs the popup's run ID committed.
                Some(NightlySource::AutoDetect) => true,
                Some(NightlySource::ManualInput) => {
                    self.run_id.as_deref().is_some_and(|s| !s.is_empty())
                }
                None => false,
            },
            5 => self.folder_path.is_some(),
            6 => true,
            // KPM embedding is optional — the actual gate is the
            // superkey popup on Next.
            8 => true,
            _ => false,
        }
    }
}

/// Linear-step wizard contract. Wizards whose `next` / `back` simply
/// walk a 0..step_count range share `reset` / `next` / `back` /
/// `is_in_exec` via this trait's default impls; only `step`,
/// `step_mut`, `step_count`, and `can_next` need per-impl bodies.
///
/// Not implemented for `RootWizard` because its non-linear step
/// numbering (steps skip around depending on family/mode) requires
/// custom navigation logic.
pub(crate) trait Wizard: Default {
    fn step(&self) -> usize;
    fn step_mut(&mut self) -> &mut usize;
    fn step_count(&self) -> usize;
    fn can_next(&self) -> bool;

    fn reset(&mut self) {
        *self = Self::default();
    }
    fn next(&mut self) {
        if self.step() < self.step_count() - 1 {
            *self.step_mut() += 1;
        }
    }
    fn back(&mut self) {
        if self.step() > 0 {
            *self.step_mut() -= 1;
        }
    }
    fn is_in_exec(&self) -> bool {
        self.step() == self.step_count() - 1
    }
    /// True on the confirm/start screen — the step immediately before
    /// exec. A sidebar bounce here preserves the wizard (the user returns
    /// to the confirm screen) instead of resetting to step 0.
    fn is_on_confirm_step(&self) -> bool {
        let n = self.step_count();
        n >= 2 && self.step() == n - 2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnrootType {
    MagiskLkm,
    APatchGki,
}
impl UnrootType {
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            Self::MagiskLkm => "unroottype_magisk_lkm",
            Self::APatchGki => "unroottype_apatch_gki",
        }
    }
    pub(crate) fn desc_key(&self) -> &'static str {
        match self {
            Self::MagiskLkm => "unroottype_magisk_lkm_desc",
            Self::APatchGki => "unroottype_apatch_gki_desc",
        }
    }
    pub(crate) fn folder_desc_key(&self) -> &'static str {
        match self {
            Self::MagiskLkm => "unroottype_magisk_lkm_folderdesc",
            Self::APatchGki => "unroottype_apatch_gki_folderdesc",
        }
    }
}

#[derive(Default)]
pub(crate) struct UnrootWizard {
    pub(crate) step: usize,
    pub(crate) unroot_type: Option<UnrootType>,
    pub(crate) folder_path: Option<String>,
    /// Loader file (`xbl_s_devprg_ns.melf`) for the EDL flash. Has
    /// its own wizard step. The Settings-level default loader
    /// auto-fills + auto-advances the loader step on Next from the
    /// method step (mirrors the Root wizard's step-5 fold-through);
    /// anyone without a default sees the explicit loader picker.
    pub(crate) loader_path: Option<String>,
}

pub(crate) const UNROOT_STEPS: &[&str] = &[
    "unroot_step_method",
    "unroot_step_loader",
    "unroot_step_folder",
    "unroot_step_confirm",
    "unroot_step_restore",
];

impl Wizard for UnrootWizard {
    fn step(&self) -> usize {
        self.step
    }
    fn step_mut(&mut self) -> &mut usize {
        &mut self.step
    }
    fn step_count(&self) -> usize {
        UNROOT_STEPS.len()
    }
    fn can_next(&self) -> bool {
        // Step indexes match `UNROOT_STEPS` — loader is its own step
        // (#1) so the folder step (#2) only gates on the backup folder
        // pick and doesn't have to bundle a loader sub-row.
        match self.step {
            0 => self.unroot_type.is_some(),
            1 => self.loader_path.is_some(),
            2 => self.folder_path.is_some(),
            3 => true,
            _ => false,
        }
    }
}

// =========================================================================
// Flash wizard state
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceRegion {
    Prc,
    Row,
}
impl DeviceRegion {
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            Self::Prc => "deviceregion_prc",
            Self::Row => "deviceregion_row",
        }
    }

    pub(crate) fn to_region_target(self) -> ltbox_patch::region::RegionTarget {
        match self {
            Self::Prc => ltbox_patch::region::RegionTarget::Prc,
            Self::Row => ltbox_patch::region::RegionTarget::Row,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlashTarget {
    OtherRegion,
    SameRegion,
}
impl FlashTarget {
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            Self::OtherRegion => "flashtarget_other",
            Self::SameRegion => "flashtarget_same",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DataMode {
    Keep,
    Wipe,
}
impl DataMode {
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            Self::Keep => "datamode_keep",
            Self::Wipe => "datamode_wipe",
        }
    }
}

#[derive(Default)]
pub(crate) struct FlashWizard {
    pub(crate) step: usize,
    pub(crate) device_region: Option<DeviceRegion>,
    pub(crate) target: Option<FlashTarget>,
    pub(crate) data_mode: Option<DataMode>,
    pub(crate) firmware_folder: Option<String>,
}

pub(crate) const FLASH_STEPS: &[&str] = &[
    "flash_step_region",
    "flash_step_target",
    "flash_step_data",
    "flash_step_folder",
    "flash_step_confirm",
    "flash_step_flash",
];

impl Wizard for FlashWizard {
    fn step(&self) -> usize {
        self.step
    }
    fn step_mut(&mut self) -> &mut usize {
        &mut self.step
    }
    fn step_count(&self) -> usize {
        FLASH_STEPS.len()
    }
    fn can_next(&self) -> bool {
        match self.step {
            0 => self.device_region.is_some(),
            1 => self.target.is_some(),
            2 => self.data_mode.is_some(),
            3 => self.firmware_folder.is_some(),
            4 => true,
            _ => false,
        }
    }
}

// =========================================================================
// System Update wizard state
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SysUpdateAction {
    Disable,
    Enable,
    Rescue,
}
impl SysUpdateAction {
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            Self::Disable => "sysupdate_disable",
            Self::Enable => "sysupdate_enable",
            Self::Rescue => "sysupdate_rescue",
        }
    }
    pub(crate) fn desc_key(&self) -> &'static str {
        match self {
            Self::Disable => "sysupdate_disable_desc",
            Self::Enable => "sysupdate_enable_desc",
            Self::Rescue => "sysupdate_rescue_desc",
        }
    }
}

/// Region target for Boot Recovery (Rescue). PRC/ROW hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RescueRegion {
    Prc,
    Row,
}

impl RescueRegion {
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::Prc => "rescue_region_prc",
            Self::Row => "rescue_region_row",
        }
    }
    pub(crate) fn to_target(self) -> ltbox_patch::region::RegionTarget {
        match self {
            Self::Prc => ltbox_patch::region::RegionTarget::Prc,
            Self::Row => ltbox_patch::region::RegionTarget::Row,
        }
    }
}

#[derive(Default)]
pub(crate) struct SysUpdateWizard {
    pub(crate) step: usize,
    pub(crate) action: Option<SysUpdateAction>,
    /// Rescue: firmware folder containing loader (`xbl_s_devprg_ns.melf`).
    pub(crate) rescue_folder: Option<String>,
    /// Rescue: selected target region. Set via popup between Folder and
    /// Confirm steps. May be pre-seeded from `inferred_flash_region`
    /// (PTSTPD `SaleArea`) before the popup opens — `rescue_region_confirmed`
    /// tracks whether the user explicitly clicked through.
    pub(crate) rescue_region: Option<RescueRegion>,
    /// Rescue: region popup overlay flag. Opens on Next press from the
    /// Folder step when the user hasn't yet confirmed a region pick.
    pub(crate) rescue_region_popup_open: bool,
    /// Rescue: true once the user has clicked a region radio in the
    /// popup. Distinguishes a pre-seeded `rescue_region` (initial
    /// preselect from `inferred_flash_region`) from a user-confirmed
    /// pick — preselect alone shouldn't skip the popup.
    pub(crate) rescue_region_confirmed: bool,
}

pub(crate) const SYSUPDATE_STEPS_COMPACT: &[&str] = &[
    "sysupdate_step_action",
    "sysupdate_step_confirm",
    "sysupdate_step_execute",
];

pub(crate) const SYSUPDATE_STEPS_RESCUE: &[&str] = &[
    "sysupdate_step_action",
    "sysupdate_step_folder",
    "sysupdate_step_confirm",
    "sysupdate_step_execute",
];

impl SysUpdateWizard {
    /// Rescue gets an extra Folder step — distinct step list keeps the
    /// other actions (Disable/Enable) on their short 3-step flow.
    pub(crate) fn steps(&self) -> &'static [&'static str] {
        if matches!(self.action, Some(SysUpdateAction::Rescue)) {
            SYSUPDATE_STEPS_RESCUE
        } else {
            SYSUPDATE_STEPS_COMPACT
        }
    }
    pub(crate) fn is_rescue(&self) -> bool {
        matches!(self.action, Some(SysUpdateAction::Rescue))
    }
}

impl Wizard for SysUpdateWizard {
    fn step(&self) -> usize {
        self.step
    }
    fn step_mut(&mut self) -> &mut usize {
        &mut self.step
    }
    fn step_count(&self) -> usize {
        self.steps().len()
    }
    fn can_next(&self) -> bool {
        if self.is_rescue() {
            // Rescue flow: Action → Folder → Confirm → Exec.
            match self.step {
                0 => self.action.is_some(),
                1 => self
                    .rescue_folder
                    .as_deref()
                    .map(std::path::Path::new)
                    .is_some_and(|p| {
                        is_loader_file(p)
                            || ltbox_core::sahara_xml::is_encrypted_manifest_filename(p)
                    }),
                2 => self.rescue_region.is_some(),
                _ => false,
            }
        } else {
            match self.step {
                0 => self.action.is_some(),
                1 => true,
                _ => false,
            }
        }
    }
}
