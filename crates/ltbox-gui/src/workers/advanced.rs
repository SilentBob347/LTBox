//! Advanced-menu single-file workers: region convert, devinfo/country
//! patch, ARB patch, vbmeta rebuild, xml convert. Each takes one input
//! image and writes patched output. Extracted from the update_adv handler.

use crate::{AdvAction, DeviceRegion};

pub(crate) fn advanced_file_worker(
    input_path: String,
    action: AdvAction,
    adv_country: Option<String>,
    adv_region_target: Option<DeviceRegion>,
    adv_arb_index: Option<u64>,
    output_dir: std::path::PathBuf,
    action_label: String,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let input = std::path::Path::new(&input_path);
    let parent = input.parent().unwrap_or(std::path::Path::new("."));
    // Created eagerly so a no-op exec still
    // leaves a folder for the user to find.
    if action.produces_output() {
        let _ = std::fs::create_dir_all(&output_dir);
        ltbox_core::live!(
            log,
            "[Advanced] {}",
            ltbox_core::i18n::tr("live_advanced_output_folder")
                .replace("{path}", &output_dir.display().to_string())
        );
    }
    match action {
        AdvAction::ImageInfo => {
            return Err("Image Info uses a dedicated multi-file flow".to_string());
        }
        AdvAction::ConvertXml => {
            // `input` is now the folder holding the encrypted
            // `*.x` pack (picker moved from file→folder so
            // users don't have to repeat the dialog for each
            // file). Iterate every `*.x`, decrypt to `*.xml`
            // in `output_dir`.
            let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(input)
                .map_err(|e| format!("read_dir {}: {e}", input.display()))?
                .filter_map(|r| r.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.eq_ignore_ascii_case("x"))
                            .unwrap_or(false)
                })
                .collect();
            entries.sort();
            if entries.is_empty() {
                return Err(format!("No *.x files found under {}", input.display()));
            }
            for src in entries {
                let stem = src.file_stem().unwrap_or_default();
                let output = output_dir.join(stem).with_extension("xml");
                match ltbox_core::crypto::decrypt_file(&src, &output) {
                    Ok(size) => ltbox_core::live!(
                        log,
                        "[Crypto] {}",
                        ltbox_core::i18n::tr("live_crypto_decrypted")
                            .replace("{bytes}", &size.to_string())
                    ),
                    Err(e) => return Err(format!("Decryption failed: {e}")),
                }
            }
        }
        AdvAction::DetectArb => {
            // DetectArb routes through its dedicated
            // `AdvDetectArbExecStart` worker, not the
            // generic file-selected pipeline. Reaching
            // this arm means a stale code path triggered
            // it; surface a clear error instead of a
            // silent no-op.
            return Err(
                "DetectArb uses a dedicated worker — file pipeline should not run".to_string(),
            );
        }
        AdvAction::FlashPartitions
        | AdvAction::DumpPartitions
        | AdvAction::FlashPhysical
        | AdvAction::DumpPhysical => {
            ltbox_core::live!(
                log,
                "[Advanced] {}",
                ltbox_core::i18n::tr("live_advanced_use_dedicated")
            );
        }
        AdvAction::RegionConvert => {
            let Some(target_region) = adv_region_target else {
                return Err(
                    "No target region selected — pick PRC or ROW in the popup before starting"
                        .into(),
                );
            };
            if input
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| !s.eq_ignore_ascii_case("vendor_boot.img"))
                .unwrap_or(true)
            {
                return Err(
                                                                "Region Convert expects vendor_boot.img; select the firmware folder's vendor_boot.img"
                                                                    .to_string(),
                                                            );
            }
            let firmware_dir = parent;
            let sibling_vbmeta = firmware_dir.join("vbmeta.img");
            if !sibling_vbmeta.is_file() {
                return Err(format!(
                    "Region Convert requires vbmeta.img beside vendor_boot.img; missing {}",
                    sibling_vbmeta.display()
                ));
            }
            let target = target_region.to_region_target();
            match ltbox_patch::region::build_region_converted_boot_chain(
                firmware_dir,
                &output_dir,
                target,
                &ltbox_patch::region::RegionPatternSet::default(),
            ) {
                Ok(ltbox_patch::region::RegionBootChainBuild::Built(output)) => {
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        ltbox_core::i18n::tr("live_region_source_target")
                            .replace("{source}", &format!("{:?}", output.source_region))
                            .replace("{target}", &format!("{:?}", output.target))
                    );
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        ltbox_core::i18n::tr("live_region_patched")
                            .replace("{count}", &output.replacement_count.to_string())
                            .replace("{path}", &output.vendor_boot.display().to_string())
                    );
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        ltbox_core::i18n::tr("live_region_final_vbmeta_written")
                            .replace("{path}", &output.vbmeta.display().to_string())
                    );
                }
                Ok(ltbox_patch::region::RegionBootChainBuild::Skipped {
                    source_region,
                    target,
                }) => {
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        ltbox_core::i18n::tr("live_region_source_target")
                            .replace("{source}", &format!("{:?}", source_region))
                            .replace("{target}", &format!("{:?}", target))
                    );
                    ltbox_core::live!(
                        log,
                        "[Region] {}",
                        ltbox_core::i18n::tr("live_region_source_matches_target")
                    );
                }
                Err(e) => return Err(format!("Region conversion failed: {e}")),
            }
        }
        AdvAction::PatchDevinfo => {
            // Country code lives in both devinfo.img
            // + persist.img — folder picker, at
            // least one must exist.
            const KNOWN: &[&str] = &[
                "CN", "KR", "JP", "US", "GB", "DE", "FR", "IT", "ES", "NL", "AT", "BE", "BG", "HR",
                "CY", "CZ", "DK", "EE", "FI", "GR", "HU", "IE", "LV", "LT", "LU", "MT", "PL", "PT",
                "RO", "SK", "SI", "SE", "AU", "CA", "IN", "RU", "BR", "MX", "SA", "AE", "WW",
            ];
            const EU: &[&str] = &[
                "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE",
                "IT", "LV", "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
            ];
            let Some(new_code) = adv_country.as_deref() else {
                return Err(
                    "No target country code selected — pick one in the popup before starting"
                        .into(),
                );
            };
            if !input.is_dir() {
                return Err(format!(
                    "PatchDevinfo expects a folder containing devinfo.img + persist.img, got {}",
                    input.display()
                ));
            }
            let mut any_written = false;
            let mut any_found = false;
            for name in ["devinfo.img", "persist.img"] {
                let src = input.join(name);
                if !src.exists() {
                    ltbox_core::live!(
                        log,
                        "[Country] {}",
                        ltbox_core::i18n::tr("live_country_name_missing").replace("{name}", name)
                    );
                    continue;
                }
                any_found = true;
                ltbox_core::live!(
                    log,
                    "[Country] {}",
                    ltbox_core::i18n::tr("live_country_processing")
                        .replace("{path}", &src.display().to_string())
                );
                let detected = ltbox_patch::region::detect_country_code(&src, KNOWN)
                    .map_err(|e| format!("Country detect failed on {name}: {e}"))?;
                let Some(old_code) = detected else {
                    ltbox_core::live!(
                        log,
                        "[Country] {}",
                        ltbox_core::i18n::tr("live_country_no_code_detected")
                            .replace("{name}", name)
                    );
                    continue;
                };
                ltbox_core::live!(
                    log,
                    "[Country] {}",
                    ltbox_core::i18n::tr("live_country_detected")
                        .replace("{name}", name)
                        .replace("{old_code}", &old_code)
                );
                let stem = std::path::Path::new(name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| name.to_string());
                // v2 naming: `<stem>_modified.img`.
                let output = output_dir.join(format!("{stem}_modified.img"));
                match ltbox_patch::region::patch_country_code(
                    &src, &output, &old_code, new_code, EU,
                ) {
                    Ok(true) => {
                        ltbox_core::live!(
                            log,
                            "[Country] {}",
                            ltbox_core::i18n::tr("live_country_written")
                                .replace("{name}", name)
                                .replace("{old_code}", &old_code)
                                .replace("{new_code}", new_code)
                                .replace("{path}", &output.display().to_string())
                        );
                        any_written = true;
                    }
                    Ok(false) => ltbox_core::live!(
                        log,
                        "[Country] {}",
                        ltbox_core::i18n::tr("live_country_no_replacements")
                            .replace("{name}", name)
                    ),
                    Err(e) => return Err(format!("Country patch failed on {name}: {e}")),
                }
            }
            if !any_found {
                return Err(format!(
                    "Neither devinfo.img nor persist.img found in {}",
                    input.display()
                ));
            }
            if !any_written {
                ltbox_core::live!(
                    log,
                    "[Country] {}",
                    ltbox_core::i18n::tr("live_country_already_matches")
                );
            }
        }
        AdvAction::PatchArb => {
            // `input` is the firmware folder; user-picked
            // target rollback index lives on the wizard.
            let target = adv_arb_index
                .ok_or_else(|| "Patch Rollback Index: missing target index".to_string())?;
            let boot = input.join("boot.img");
            let vbmeta = input.join("vbmeta_system.img");
            if !boot.is_file() {
                return Err(format!("Missing boot.img in {}", input.display()));
            }
            if !vbmeta.is_file() {
                return Err(format!("Missing vbmeta_system.img in {}", input.display()));
            }
            // Read AVB info first so the abort guards (rollback
            // == 0 / 1) trip before any signing-key work runs.
            let boot_info = ltbox_patch::avb::extract_image_avb_info(&boot)
                .map_err(|e| format!("boot.img inspect failed: {e}"))?;
            let vbmeta_info = ltbox_patch::avb::extract_image_avb_info(&vbmeta)
                .map_err(|e| format!("vbmeta_system.img inspect failed: {e}"))?;
            if boot_info.rollback_index <= 1 {
                return Err(format!(
                    "boot.img rollback index is {} — refusing to patch",
                    boot_info.rollback_index
                ));
            }
            if vbmeta_info.rollback_index <= 1 {
                return Err(format!(
                    "vbmeta_system.img rollback index is {} — refusing to patch",
                    vbmeta_info.rollback_index
                ));
            }
            // Signing key resolution: only the two stock
            // testkeys embedded in avbtool-rs are supported.
            // Anything else aborts — user-supplied PEMs are
            // intentionally not consulted.
            let resolve_key = |info: &ltbox_patch::avb::AvbImageInfo,
                               label: &str|
             -> std::result::Result<&'static str, String> {
                ltbox_patch::key_map::key_spec_for_pubkey(
                                                                info.public_key_sha1.as_deref(),
                                                            )
                                                            .ok_or_else(|| {
                                                                format!(
                                                                    "{label}: signing key not recognized (pubkey {:?}); only testkey_rsa2048 / testkey_rsa4096 are supported",
                                                                    info.public_key_sha1
                                                                )
                                                            })
            };
            let boot_key = resolve_key(&boot_info, "boot.img")?;
            let vbmeta_key = resolve_key(&vbmeta_info, "vbmeta_system.img")?;
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("live_patch_arb_signing_key")
                    .replace("{name}", "boot.img")
                    .replace("{key}", boot_key)
            );
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("live_patch_arb_signing_key")
                    .replace("{name}", "vbmeta_system.img")
                    .replace("{key}", vbmeta_key)
            );
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("live_patch_arb_rollback_change")
                    .replace("{name}", "boot.img")
                    .replace("{old}", &boot_info.rollback_index.to_string())
                    .replace("{new}", &target.to_string())
            );
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("live_patch_arb_rollback_change")
                    .replace("{name}", "vbmeta_system.img")
                    .replace("{old}", &vbmeta_info.rollback_index.to_string())
                    .replace("{new}", &target.to_string())
            );
            let boot_out = output_dir.join("boot.img");
            let vbmeta_out = output_dir.join("vbmeta_system.img");
            // boot.img: NONE → add_hash_footer; signed → resign.
            std::fs::copy(&boot, &boot_out).map_err(|e| format!("copy boot.img: {e}"))?;
            if boot_info.algorithm == "NONE" {
                ltbox_patch::avb::add_hash_footer(
                    &boot_out,
                    &boot_info,
                    Some(boot_key),
                    Some(target),
                )
                .map_err(|e| format!("boot ARB add_hash_footer failed: {e}"))?;
            } else {
                ltbox_patch::avb::resign_image(
                    &boot_out,
                    boot_key,
                    &boot_info.algorithm,
                    Some(target),
                )
                .map_err(|e| format!("boot ARB resign failed: {e}"))?;
            }
            // vbmeta_system.img: always resign (chains require sig).
            std::fs::copy(&vbmeta, &vbmeta_out)
                .map_err(|e| format!("copy vbmeta_system.img: {e}"))?;
            ltbox_patch::avb::resign_image(
                &vbmeta_out,
                vbmeta_key,
                &vbmeta_info.algorithm,
                Some(target),
            )
            .map_err(|e| format!("vbmeta_system ARB resign failed: {e}"))?;
            ltbox_core::live!(
                log,
                "[ARB] {}",
                ltbox_core::i18n::tr("live_advanced_output_folder")
                    .replace("{path}", &output_dir.display().to_string())
            );
        }
        AdvAction::RebuildVbmeta => {
            // `resign_image` alone won't work — chain
            // hashes go stale once dtbo / init_boot /
            // vendor_boot move.
            let info = ltbox_patch::avb::extract_image_avb_info(input)
                .map_err(|e| format!("VBMeta inspect failed: {e}"))?;
            // Only the two stock testkeys embedded in
            // avbtool-rs are supported.
            let key_spec = ltbox_patch::key_map::key_spec_for_pubkey(
                                                            info.public_key_sha1.as_deref(),
                                                        )
                                                        .ok_or_else(|| {
                                                            format!(
                                                                "Rebuild vbmeta: signing key not recognized (pubkey {:?}); only testkey_rsa2048 / testkey_rsa4096 are supported",
                                                                info.public_key_sha1
                                                            )
                                                        })?;
            let alg: Option<&str> = if info.algorithm == "NONE" {
                // NONE → infer from the resolved key spec.
                Some(if key_spec.contains("2048") {
                    "SHA256_RSA2048"
                } else {
                    "SHA256_RSA4096"
                })
            } else {
                Some(info.algorithm.as_str())
            };

            // Advanced is file-only — user supplies
            // the chained images (v2 dumps them).
            let candidates: &[&str] = &[
                "dtbo.img",
                "dtbo_a.img",
                "dtbo_b.img",
                "init_boot.img",
                "init_boot_a.img",
                "init_boot_b.img",
                "vendor_boot.img",
                "vendor_boot_a.img",
                "vendor_boot_b.img",
                "boot.img",
                "boot_a.img",
                "boot_b.img",
            ];
            let mut chained: Vec<std::path::PathBuf> = Vec::new();
            for name in candidates {
                let p = parent.join(name);
                if p.exists() {
                    chained.push(p);
                }
            }
            if chained.is_empty() {
                ltbox_core::live!(
                    log,
                    "[AVB] {}",
                    ltbox_core::i18n::tr("live_avb_no_chained_fallback")
                );
                if let Err(e) = ltbox_patch::avb::resign_image(
                    input,
                    key_spec,
                    alg.unwrap_or("SHA256_RSA4096"),
                    Some(info.rollback_index),
                ) {
                    return Err(format!("Rebuild vbmeta fallback resign failed: {e}"));
                }
            } else {
                if chained.iter().any(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with("vendor_boot"))
                        .unwrap_or(false)
                }) {
                    ltbox_core::live!(
                        log,
                        "[AVB] {}",
                        ltbox_core::i18n::tr("live_avb_rebuild_warning")
                    );
                }
                let output = output_dir.join("vbmeta.rebuilt.img");
                let chained_refs: Vec<&std::path::Path> =
                    chained.iter().map(|p| p.as_path()).collect();
                let chained_names = chained
                    .iter()
                    .map(|p| p.file_name().and_then(|s| s.to_str()).unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join(", ");
                ltbox_core::live!(
                    log,
                    "[AVB] {}",
                    ltbox_core::i18n::tr("live_avb_rebuild_chained")
                        .replace("{count}", &chained.len().to_string())
                        .replace("{names}", &chained_names)
                );
                ltbox_core::live!(
                    log,
                    "[AVB] {}",
                    ltbox_core::i18n::tr("live_avb_rebuild_key_alg")
                        .replace("{key}", key_spec)
                        .replace("{alg}", alg.unwrap_or("(from original vbmeta)"))
                );
                if let Err(e) = ltbox_patch::avb::rebuild_vbmeta_with_chained_images(
                    &output,
                    input,
                    &chained_refs,
                    key_spec,
                    alg,
                ) {
                    return Err(format!("Rebuild vbmeta failed: {e}"));
                }
                ltbox_core::live!(
                    log,
                    "[AVB] {}",
                    ltbox_core::i18n::tr("live_avb_rebuilt_written")
                        .replace("{path}", &output.display().to_string())
                );
            }
        }
    }
    ltbox_core::live!(
        log,
        "[Advanced] {}",
        ltbox_core::i18n::tr("live_advanced_completed").replace("{action}", &action_label)
    );
    Ok(log)
}
