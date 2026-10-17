//! Queries over the connected device and its model capabilities.

use crate::*;

impl App {
    /// Label key for the live connection. Identical to
    /// [`ConnectionStatus::label_key`] except that a Fastboot endpoint
    /// reporting `is-userspace` is named fastbootd — the two are the same
    /// connection everywhere else, so only the label distinguishes them.
    pub(crate) fn connection_label_key(&self) -> &'static str {
        if self.device.connection == ConnectionStatus::Fastboot && self.device.fastboot_userspace {
            "conn_fastbootd"
        } else {
            self.device.connection.label_key()
        }
    }

    /// The connected dual-USB-C model whose port guide is currently eligible
    /// to open, or `None`. Eligible when the model has the dual-USB capability
    /// and the user has neither permanently dismissed ("don't show again")
    /// nor session-closed ("close") it.
    pub(crate) fn dual_usb_advisory_model(&self) -> Option<&str> {
        let m = self.device.model.as_str();
        let hidden = |list: &[String]| list.iter().any(|x| x.eq_ignore_ascii_case(m));
        if !m.is_empty()
            && is_dual_usbc_model(m)
            && !hidden(&self.dual_usb_advisory_dismissed)
            && !hidden(&self.dual_usb_advisory_closed)
        {
            Some(m)
        } else {
            None
        }
    }

    /// Whether the Dashboard's rollback cell opens the floor breakdown.
    ///
    /// Both halves matter. Floors are only ever populated by a
    /// bootloader-mode poll, so their presence *is* the "in bootloader"
    /// test. The model check keeps an exempt SKU (TB322FC) from offering
    /// a breakdown behind a cell that reads "No" — the two would
    /// contradict each other even if its bootloader did report two
    /// populated locations.
    #[cfg(test)]
    pub(crate) fn rollback_detail_available(&self) -> bool {
        self.device.rollback_floors.is_some() && is_rollback_protected_model(&self.device.model)
    }

    pub(crate) fn is_nav_enabled(&self, view: View) -> bool {
        // About is informational and device/platform-independent — always on.
        if matches!(view, View::About) {
            return true;
        }
        if self.device.platform_supported == Some(false) {
            return matches!(view, View::Dashboard | View::SystemUpdate | View::Settings);
        }
        true
    }

    /// The same model policy used by the workers and patch pipeline.
    pub(crate) fn model_capabilities(&self) -> &'static ltbox_core::model::ModelCapabilities {
        ltbox_core::model::capabilities(&self.device.model)
    }

    /// Firmware GBL policy follows the target; read-only rollback applies
    /// whenever either the device or target requires it. Before the folder is
    /// known, show the connected device's supported choices.
    pub(crate) fn flash_rollback_policy(&self) -> ltbox_core::model::RollbackPolicy {
        use ltbox_core::model::{RollbackPolicy, fingerprint_capabilities};
        let device = self.model_capabilities().rollback;
        let target = self
            .flash
            .firmware_identity
            .as_ref()
            .and_then(|identity| identity.fingerprint.as_deref())
            .and_then(|fp| {
                fingerprint_capabilities(fp)
                    .map(|profile| profile.rollback)
                    .reduce(|a, b| {
                        if a == RollbackPolicy::ReadOnly || b == RollbackPolicy::ReadOnly {
                            RollbackPolicy::ReadOnly
                        } else if a == RollbackPolicy::Gbl || b == RollbackPolicy::Gbl {
                            RollbackPolicy::Gbl
                        } else {
                            a
                        }
                    })
            });
        if device == RollbackPolicy::ReadOnly || target == Some(RollbackPolicy::ReadOnly) {
            RollbackPolicy::ReadOnly
        } else {
            target.unwrap_or(device)
        }
    }

    pub(crate) fn is_xiaoxin_pro13(&self) -> bool {
        ltbox_core::model::is_xiaoxin_pro13_model(&self.device.model)
    }

    /// Which images the unroot folder picker should name.
    ///
    /// What a root backup holds is decided by the root run, not by the wizard
    /// card the user picks here: the target follows the route and the model,
    /// and vbmeta or vbmeta_system is only in the folder when rebuilt. Asking
    /// the same two functions the root run asked keeps the picker from naming a
    /// file that run never wrote — a chained `boot` target leaves no owner image
    /// at all, and only the TB320FC family roots to `boot` outside the GKI
    /// route.
    pub(crate) fn unroot_folder_desc(&self, unroot_type: UnrootType) -> &str {
        let target = ltbox_patch::root_pipeline::resolve_root_image_target(
            ltbox_patch::root_pipeline::RootFamily::Magisk,
            matches!(unroot_type, UnrootType::APatchGki),
            &self.device.model,
        );
        // The testkey efisp/GBL route leaves vbmeta out of the backup even for
        // a target vbmeta would normally hash, so it overrides the rebuild rule.
        let with_vbmeta = !root_skips_avb_postprocess(&self.device.model)
            && ltbox_patch::root_pipeline::root_run_rebuilds_vbmeta(target, &self.device.model);
        match (target, with_vbmeta) {
            (ltbox_patch::root_pipeline::RootImageTarget::Boot, true) => {
                self.t("unroot_folderdesc_boot_vbmeta")
            }
            (ltbox_patch::root_pipeline::RootImageTarget::Boot, false) => {
                self.t("unroot_folderdesc_boot")
            }
            (ltbox_patch::root_pipeline::RootImageTarget::InitBoot, true) => {
                self.t("unroot_folderdesc_init_boot_vbmeta")
            }
            (ltbox_patch::root_pipeline::RootImageTarget::InitBoot, false) => {
                self.t("unroot_folderdesc_init_boot")
            }
        }
    }

    pub(crate) fn advanced_picker_exts(&self) -> (&'static str, &'static [&'static str]) {
        if matches!(
            self.adv_wizard.action,
            Some(AdvAction::DetectArb | AdvAction::PatchDevinfo)
        ) {
            ("EDL loader", self.loader_picker_exts())
        } else {
            self.adv_wizard.accepted_exts()
        }
    }

    /// Whether the polled device is a TB322FC. PRC-only SKU — the Flash
    /// wizard hides ROW + OtherRegion as disabled cards so the user
    /// cannot pick a region or cross-region flash target that the
    /// hardware doesn't ship with.
    pub(crate) fn is_prc_only(&self) -> bool {
        self.model_capabilities().prc_only
    }

    /// True when the dashboard poll has placed the device in a mode
    /// any wizard can transition out of (`ensure_*` helpers + the
    /// flash/sysupdate bridges). Gates every wizard's final "Start"
    /// button — `None` and `AdbUnauthorized` mean the operation can't
    /// even start, so spawning a worker that would immediately bail
    /// with "no device" is noise.
    pub(crate) fn device_reachable(&self) -> bool {
        matches!(
            self.device.connection,
            ConnectionStatus::Adb
                | ConnectionStatus::AdbRecovery
                | ConnectionStatus::Fastboot
                | ConnectionStatus::Edl
        )
    }
}
