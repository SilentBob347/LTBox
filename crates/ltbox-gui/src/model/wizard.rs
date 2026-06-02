//! Wizard model — the per-flow wizard state structs and their
//! navigation logic, extracted from `main.rs`.

use crate::{Family, NightlySource, Provider, RootMode, VerChoice};

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
