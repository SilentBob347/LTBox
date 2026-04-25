//! End-to-end root pipeline: download → dump → patch → resign → flash.
//!
//! Orchestrates [`crate::magisk`], [`crate::ksu`], [`crate::avb`], and
//! `ltbox_device::edl`. Outputs land in `cfg.output_dir` (patched boot +
//! rebuilt vbmeta), then flash pushes them to the active slot.

use std::path::{Path, PathBuf};

// fs_err: io::Error Display includes the path, so bare `?` gives readable errors.
use fs_err as fs;

use ltbox_core::downloader::download_to_file;
use ltbox_core::github::GitHubClient;
use ltbox_core::i18n::tr;
use ltbox_core::live;
use ltbox_core::{LtboxError, Result};

use crate::{avb, gki, key_map, ksu, magisk};

/// Pick the avbtool-rs key_spec for re-signing.
/// `None` → unsigned (NONE algorithm); `Some(sha)` → `KEY_MAP` lookup,
/// hard error on miss (signing key rolled — add to the map).
fn resolve_signing_key(
    pubkey_sha1: Option<&str>,
    image_name: &str,
    log: &mut Vec<String>,
) -> Result<Option<String>> {
    let Some(sha) = pubkey_sha1 else {
        ltbox_core::live!(
            log,
            "[AVB] {image_name} {}",
            tr("log_avb_unsigned_skip_key")
        );
        return Ok(None);
    };
    if let Some(spec) = key_map::key_spec_for_pubkey(Some(sha)) {
        ltbox_core::live!(
            log,
            "[AVB] {image_name} {} {sha} → {} {spec}",
            tr("log_avb_pubkey"),
            tr("log_avb_bundled")
        );
        return Ok(Some(spec.to_string()));
    }
    Err(LtboxError::Avb(format!(
        "No signing key available for {image_name}: stock pubkey_sha1 = {sha} is not in the bundled KEY_MAP. If the device's signing key has rolled, add it to `ltbox_patch::key_map::KEY_MAP`."
    )))
}

/// Provider families carried through the GUI wizard state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootFamily {
    /// Magisk / forks — init_boot ramdisk injection.
    Magisk,
    /// KernelSU-style LKM — init_boot with ksuinit + kernelsu.ko.
    KernelSU,
    /// APatch — boot image via kptools + kpimg.
    APatch,
}

/// Provider inside the family to fetch from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootProvider {
    Magisk,
    MagiskFork,
    KernelSU,
    KernelSUNext,
    SukiSU,
    ReSukiSU,
    APatch,
    FolkPatch,
}

/// Release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootVersion {
    Stable,
    Nightly,
}

/// Root pipeline input — GUI wizard state converted into a flat struct.
pub struct RootPipelineConfig {
    pub family: RootFamily,
    pub provider: RootProvider,
    pub version: RootVersion,

    /// APK extraction + boot patching workspace. Cleaned on entry.
    pub work_dir: PathBuf,
    /// Where patched boot + vbmeta land.
    pub output_dir: PathBuf,
    /// EDL loader path (`xbl_s_devprg_ns.melf`).
    pub loader: PathBuf,
    /// Active slot (`_a` / `_b` / empty; empty → flash defaults to `_a`).
    pub slot_suffix: String,
    /// Magisk `PREINITDEVICE`. Empty → Magisk resolves at runtime.
    pub preinit_device: String,
    /// GKI-mode only: user-supplied AnyKernel3 zip.
    pub gki_kernel_zip: Option<PathBuf>,
    /// Device kernel version (`major.minor.patch` from `uname -r`) —
    /// used by KSU to pick the matching `.ko` release asset.
    pub kernel_version: Option<String>,
    /// GKI mode → patch `boot.img` via `gki::patch_boot` instead of the
    /// Magisk/KSU ramdisk path.
    pub gki_mode: bool,
    /// APatch / FolkPatch: `.kpm` modules to embed.
    pub kpm_paths: Vec<PathBuf>,
    /// APatch / FolkPatch: superkey (8..=63 ASCII alphanumeric).
    pub superkey: String,
    /// Magisk Forks: user-picked variant APK (local-APK-only in v2 parity).
    pub magisk_forks_apk: Option<PathBuf>,
    /// Nightly: manual workflow run ID. `None` → auto-detect latest.
    pub nightly_run_id: Option<u64>,
}

/// Per-provider `(workflow_file, default_branch)` for nightly runs.
/// Returns `None` for providers without a nightly channel (e.g. MagiskFork).
fn provider_workflow(provider: RootProvider) -> Option<(&'static str, &'static str)> {
    Some(match provider {
        RootProvider::Magisk => ("ci.yml", "master"),
        RootProvider::MagiskFork => return None,
        RootProvider::KernelSU => ("build-manager.yml", "main"),
        RootProvider::KernelSUNext => ("build-manager-ci.yml", "dev"),
        RootProvider::SukiSU => ("build-manager.yml", "main"),
        RootProvider::ReSukiSU => ("build-manager.yml", "main"),
        RootProvider::APatch => ("build.yml", "main"),
        RootProvider::FolkPatch => ("build.yml", "main"),
    })
}

/// Resolve `(repo, run_id)` for a nightly fetch. Manual IDs are validated
/// against the provider's workflow so bad IDs fail fast, not at nightly.link.
fn resolve_nightly_run(
    provider: RootProvider,
    manual_run_id: Option<u64>,
    log: &mut Vec<String>,
) -> Result<(&'static str, u64)> {
    let repo = provider_repo(provider).ok_or_else(|| {
        LtboxError::Patch(format!(
            "resolve_nightly_run: unsupported provider {provider:?}"
        ))
    })?;
    let (workflow_file, branch) = provider_workflow(provider).ok_or_else(|| {
        LtboxError::Patch(format!(
            "resolve_nightly_run: no workflow metadata for {provider:?}"
        ))
    })?;
    let client = GitHubClient::new(repo)?;

    let run_id = match manual_run_id {
        Some(id) => {
            ltbox_core::live!(
                log,
                "[Nightly] {repo}: {}",
                tr("log_nightly_validating_manual")
                    .replace("{id}", &id.to_string())
                    .replace("{workflow}", workflow_file)
                    .replace("{branch}", branch)
            );
            if !client.workflow_run_matches(id, workflow_file, Some(branch))? {
                return Err(LtboxError::Patch(format!(
                    "Manual run id {id} does not match workflow {workflow_file} on branch {branch} of {repo}"
                )));
            }
            id
        }
        None => {
            ltbox_core::live!(
                log,
                "[Nightly] {repo}: {}",
                tr("log_nightly_auto_detect")
                    .replace("{workflow}", workflow_file)
                    .replace("{branch}", branch)
            );
            client
                .latest_successful_run(workflow_file, Some(branch))?
                .ok_or_else(|| {
                    LtboxError::Patch(format!(
                        "No successful {workflow_file} run found on {repo}:{branch}"
                    ))
                })?
        }
    };
    ltbox_core::live!(
        log,
        "[Nightly] {repo}: {}",
        tr("log_nightly_using_run_id").replace("{id}", &run_id.to_string())
    );
    Ok((repo, run_id))
}

/// Build the `nightly.link` public-mirror URL. Response is always ZIP-wrapped.
fn nightly_artifact_url(repo: &str, run_id: u64, artifact_name: &str) -> String {
    let suffix = if artifact_name.ends_with(".zip") {
        ""
    } else {
        ".zip"
    };
    format!("https://nightly.link/{repo}/actions/runs/{run_id}/{artifact_name}{suffix}")
}

/// Which base partition this pipeline targets.
/// `"boot"` for GKI + APatch/FolkPatch (kernel-blob patching),
/// `"init_boot"` for Magisk / KSU (ramdisk injection).
pub fn boot_partition_base(family: RootFamily, gki_mode: bool) -> &'static str {
    if gki_mode || matches!(family, RootFamily::APatch) {
        "boot"
    } else {
        "init_boot"
    }
}

/// Resolve the GitHub repo slug for a given provider.
pub fn provider_repo(provider: RootProvider) -> Option<&'static str> {
    Some(match provider {
        RootProvider::Magisk => "topjohnwu/Magisk",
        RootProvider::MagiskFork => return None,
        RootProvider::KernelSU => "tiann/KernelSU",
        // Upstream moved to the KernelSU-Next org; the old `rifsxd/KernelSU-Next`
        // redirects but its release assets aren't mirrored, so pin the new slug.
        RootProvider::KernelSUNext => "KernelSU-Next/KernelSU-Next",
        RootProvider::SukiSU => "SukiSU-Ultra/SukiSU-Ultra",
        RootProvider::ReSukiSU => "ReSukiSU/ReSukiSU",
        RootProvider::APatch => "bmax121/APatch",
        RootProvider::FolkPatch => "LyraVoid/FolkPatch",
    })
}

/// Ordered keyword preferences for picking a manager asset from a **stable
/// release** asset list. Keywords are case-insensitive substrings matched
/// against `.apk` asset names (e.g. `KernelSU_v3.2.4_32457-release.apk`).
///
/// Spoofed variants go first so providers that ship both `-spoofed` and
/// non-spoofed release APKs (KernelSU-Next today, SukiSU going forward)
/// land on the spoofed one. ReSukiSU has no stable channel, hence empty.
fn ksu_manager_stable_preferences(provider: RootProvider) -> &'static [&'static str] {
    match provider {
        RootProvider::KernelSU => &["-release.apk"],
        RootProvider::KernelSUNext => &["-spoofed", "-release.apk"],
        RootProvider::SukiSU => &["-spoofed", "-release.apk"],
        // ReSukiSU publishes no stable releases; GUI gates this off but we
        // also return empty here so a stray Stable call fails fast instead
        // of grabbing some unrelated asset.
        RootProvider::ReSukiSU => &[],
        _ => &[],
    }
}

/// Ordered keyword preferences for picking a manager artifact from a
/// **nightly workflow run**. Workflow artifact names are bare (no suffix),
/// so exact-match is the common case; substring fallback is the safety net.
fn ksu_manager_nightly_preferences(provider: RootProvider) -> &'static [&'static str] {
    match provider {
        RootProvider::KernelSU => &["manager"],
        // Upstream ships `manager-spoofed` + `manager`; prefer the spoofed
        // one for Play Integrity / Widevine preservation.
        RootProvider::KernelSUNext => &["manager-spoofed", "manager"],
        // SukiSU doesn't currently emit `manager-spoofed`, but upstream has
        // signalled intent — keep spoofed first so future runs pick it up
        // without code changes.
        RootProvider::SukiSU => &["manager-spoofed", "manager"],
        // ReSukiSU emits four variants; user preference is
        // release > debug, spoofed > plain, checked in that order.
        RootProvider::ReSukiSU => &[
            "Spoofed-Manager-release",
            "Manager-release",
            "Spoofed-Manager-debug",
            "Manager-debug",
        ],
        _ => &[],
    }
}

/// Pick a manager asset from `assets` using `preferred_keywords`.
///
/// Matching is two-tiered, both case-insensitive:
///
/// 1. Exact asset-name match against each preferred keyword, in order.
///    Handles nightly artifact names (bare, no suffix) cleanly.
/// 2. Substring match against each preferred keyword, in order.
///    Handles stable release `.apk` names whose keyword is only a fragment
///    (e.g. `-spoofed` inside `KernelSU_Next_v3.2.0-spoofed_33129-release.apk`).
///
/// The iteration order of `preferred_keywords` is the priority order —
/// earlier entries win even when later entries would also substring-match.
fn select_manager_asset(
    assets: &[(String, String)],
    preferred_keywords: &[&str],
) -> Option<(String, String)> {
    // Tier 1 — exact match (nightly artifact names).
    for keyword in preferred_keywords {
        if let Some(hit) = assets
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(keyword))
        {
            return Some(hit.clone());
        }
    }
    // Tier 2 — substring match (stable `.apk` names).
    for keyword in preferred_keywords {
        let keyword_lower = keyword.to_lowercase();
        if let Some(hit) = assets
            .iter()
            .find(|(name, _)| name.to_lowercase().contains(&keyword_lower))
        {
            return Some(hit.clone());
        }
    }
    None
}

/// Download latest Magisk APK into `dst_path`; returns the tag name.
pub fn download_latest_magisk_apk(
    provider: RootProvider,
    dst_path: &Path,
    log: &mut Vec<String>,
) -> Result<String> {
    let repo = provider_repo(provider)
        .ok_or_else(|| LtboxError::Patch("Magisk forks need a local APK for patching".into()))?;
    let client = GitHubClient::new(repo)?;
    let (tag, assets) = client.latest_release_assets()?;
    let (name, url) = assets
        .into_iter()
        .find(|(n, _)| {
            let lower = n.to_lowercase();
            lower.ends_with(".apk") && !lower.contains("debug")
        })
        .ok_or_else(|| LtboxError::Download(format!("No release APK on latest {repo}")))?;
    ltbox_core::live!(
        log,
        "[Magisk] {}",
        tr("log_release_latest_asset")
            .replace("{tag}", &tag)
            .replace("{name}", &name)
    );
    download_to_file(&url, dst_path, log)?;
    Ok(tag)
}

/// Download outer nightly ZIP → extract → move inner `.apk` onto `dst_apk`.
/// `rename` falls back to `copy` for cross-volume moves under WSL.
#[allow(clippy::too_many_arguments)]
fn fetch_nightly_apk_outer_zip(
    log_tag: &str,
    repo: &str,
    run_id: u64,
    artifact_name: &str,
    staging_name: &str,
    work_dir: &Path,
    dst_apk: &Path,
    log: &mut Vec<String>,
) -> Result<()> {
    let outer_zip_path = work_dir.join(format!("{staging_name}.zip"));
    let url = nightly_artifact_url(repo, run_id, artifact_name);
    download_to_file(&url, &outer_zip_path, log)?;

    let staging = work_dir.join(staging_name);
    if staging.exists() {
        fs::remove_dir_all(&staging).ok();
    }
    fs::create_dir_all(&staging)?;
    {
        let f = fs::File::open(&outer_zip_path)?;
        let mut archive = zip::ZipArchive::new(f)
            .map_err(|e| LtboxError::Patch(format!("{repo}: nightly artifact not a zip: {e}")))?;
        archive
            .extract(&staging)
            .map_err(|e| LtboxError::Patch(format!("{repo}: extract nightly zip: {e}")))?;
    }

    // Walk the extracted artifact recursively — some providers nest
    // their APK under `<artifact>/manager/`, `<arch>/`, or
    // `app-release-arm64-v8a/`. Old non-recursive `read_dir` skipped
    // those and reported "no .apk found after extract".
    let mut apk_candidates: Vec<PathBuf> = Vec::new();
    collect_apks_recursive(&staging, &mut apk_candidates);
    let apk_src = pick_preferred_apk_path(&apk_candidates)
        .cloned()
        .ok_or_else(|| {
            LtboxError::Patch(format!(
                "{repo} nightly artifact {artifact_name}: no .apk found after extract"
            ))
        })?;

    if dst_apk.exists() {
        fs::remove_file(dst_apk).ok();
    }
    fs::rename(&apk_src, dst_apk).or_else(|_| fs::copy(&apk_src, dst_apk).map(|_| ()))?;
    ltbox_core::live!(
        log,
        "[{log_tag}] {}",
        tr("log_staged_nightly_apk").replace("{path}", &dst_apk.display().to_string())
    );
    Ok(())
}

/// Fetch a nightly Magisk APK via `nightly.link`. Prefers `app-release` /
/// `apk-ng-release` artifacts over debug. `manual_run_id = None` →
/// latest successful `ci.yml` run on `master`.
pub fn download_magisk_apk_nightly(
    provider: RootProvider,
    manual_run_id: Option<u64>,
    work_dir: &Path,
    dst_path: &Path,
    log: &mut Vec<String>,
) -> Result<u64> {
    let (repo, run_id) = resolve_nightly_run(provider, manual_run_id, log)?;
    let client = GitHubClient::new(repo)?;
    let artifact_names = client.workflow_artifacts(run_id)?;
    if artifact_names.is_empty() {
        return Err(LtboxError::Patch(format!(
            "{repo} run {run_id} has no artifacts"
        )));
    }
    // Prefer release variants over debug artifacts.
    let preferred: &[&str] = &["app-release", "apk-ng-release"];
    let artifact_name = preferred
        .iter()
        .find_map(|p| {
            artifact_names
                .iter()
                .find(|n| n.to_lowercase().starts_with(p))
                .cloned()
        })
        .or_else(|| {
            artifact_names
                .iter()
                .find(|n| !n.to_lowercase().contains("debug"))
                .cloned()
        })
        .ok_or_else(|| {
            LtboxError::Patch(format!(
                "{repo} run {run_id}: no release APK artifact (got {artifact_names:?})"
            ))
        })?;
    ltbox_core::live!(
        log,
        "[Magisk] {repo} {}",
        tr("log_nightly_artifact").replace("{artifact}", &artifact_name)
    );
    fetch_nightly_apk_outer_zip(
        "Magisk",
        repo,
        run_id,
        &artifact_name,
        "magisk_nightly",
        work_dir,
        dst_path,
        log,
    )?;
    Ok(run_id)
}

/// Pull `assets/kpimg` out of a staged APatch/FolkPatch APK into `work_dir/kpimg`.
fn extract_kpimg_from_apk(
    repo: &str,
    apk_path: &Path,
    work_dir: &Path,
    log: &mut Vec<String>,
) -> Result<()> {
    let kpimg_dst = work_dir.join("kpimg");
    let f = fs::File::open(apk_path)?;
    let mut archive = zip::ZipArchive::new(f)
        .map_err(|e| LtboxError::Patch(format!("{repo}: APK not a zip: {e}")))?;
    let mut entry = archive
        .by_name("assets/kpimg")
        .map_err(|e| LtboxError::Patch(format!("{repo}: APK missing assets/kpimg: {e}")))?;
    let size = entry.size();
    let mut out = fs::File::create(&kpimg_dst)?;
    std::io::copy(&mut entry, &mut out)?;
    ltbox_core::live!(
        log,
        "[APatch] {}",
        tr("log_apatch_extracted_kpimg")
            .replace("{path}", &kpimg_dst.display().to_string())
            .replace("{bytes}", &size.to_string())
    );
    Ok(())
}

/// Fetch APatch/FolkPatch Stable APK → stash at `work_dir/apatch.apk`,
/// extract `assets/kpimg` → `work_dir/kpimg`.
pub fn download_apatch_payload(
    provider: RootProvider,
    work_dir: &Path,
    log: &mut Vec<String>,
) -> Result<String> {
    let repo = provider_repo(provider).ok_or_else(|| {
        LtboxError::Patch(format!(
            "download_apatch_payload: unsupported provider {provider:?}"
        ))
    })?;
    let client = GitHubClient::new(repo)?;
    let (tag, assets) = client.latest_release_assets()?;
    let (name, url) = assets
        .into_iter()
        .find(|(n, _)| n.to_lowercase().ends_with(".apk"))
        .ok_or_else(|| LtboxError::Download(format!("No release APK on latest {repo}")))?;
    ltbox_core::live!(
        log,
        "[APatch] {repo} {}",
        tr("log_release_latest_asset")
            .replace("{tag}", &tag)
            .replace("{name}", &name)
    );

    let apk_path = work_dir.join("apatch.apk");
    download_to_file(&url, &apk_path, log)?;
    extract_kpimg_from_apk(repo, &apk_path, work_dir, log)?;
    Ok(tag)
}

/// Fetch APatch/FolkPatch Nightly APK via `nightly.link` → extract kpimg.
/// `manual_run_id = None` → latest successful run on provider's workflow.
pub fn download_apatch_payload_nightly(
    provider: RootProvider,
    manual_run_id: Option<u64>,
    work_dir: &Path,
    log: &mut Vec<String>,
) -> Result<u64> {
    let (repo, run_id) = resolve_nightly_run(provider, manual_run_id, log)?;
    let client = GitHubClient::new(repo)?;
    let artifact_names = client.workflow_artifacts(run_id)?;
    if artifact_names.is_empty() {
        return Err(LtboxError::Patch(format!(
            "{repo} run {run_id} has no artifacts"
        )));
    }
    // Case-insensitive prefix match after stripping .zip/.apk.
    let prefix = match provider {
        RootProvider::APatch => "apatch",
        RootProvider::FolkPatch => "folkpatch",
        _ => "",
    };
    let artifact_name = artifact_names
        .iter()
        .find(|n| {
            let lower = n.to_lowercase();
            let stripped = lower
                .strip_suffix(".zip")
                .unwrap_or(&lower)
                .strip_suffix(".apk")
                .unwrap_or_else(|| lower.strip_suffix(".zip").unwrap_or(&lower));
            stripped.starts_with(prefix)
        })
        .cloned()
        .or_else(|| artifact_names.into_iter().next())
        .ok_or_else(|| {
            LtboxError::Patch(format!(
                "{repo} run {run_id}: no matching artifact for prefix {prefix:?}"
            ))
        })?;
    ltbox_core::live!(
        log,
        "[APatch] {repo} {}",
        tr("log_nightly_artifact").replace("{artifact}", &artifact_name)
    );
    // Canonical apk path so Stable / Nightly share downstream steps.
    let apk_path = work_dir.join("apatch.apk");
    fetch_nightly_apk_outer_zip(
        "APatch",
        repo,
        run_id,
        &artifact_name,
        "apatch_nightly",
        work_dir,
        &apk_path,
        log,
    )?;
    extract_kpimg_from_apk(repo, &apk_path, work_dir, log)?;
    Ok(run_id)
}

fn copy_apk_to(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_file(dst).ok();
    }
    fs::copy(src, dst)?;
    Ok(())
}

/// Recursive .apk hunt — extracted nightly artifacts often nest the
/// APK inside `<artifact>/manager/` or `arm64-v8a/`. `read_dir` alone
/// missed those entries and the wizard reported "no .apk found".
fn collect_apks_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_apks_recursive(&path, out);
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("apk"))
        {
            out.push(path);
        }
    }
}

/// Pick the most-likely-to-install APK from a candidate list. Tier 1
/// prefers `arm64-v8a` (LTBox-supported devices are arm64); Tier 2
/// falls back to any non-debug variant; Tier 3 surrenders and returns
/// whatever's first. Used by both the recursive filesystem path and
/// the in-zip member-name path so the selection rule is consistent
/// across the staging shapes the various providers ship.
fn apk_preference_score(name_lower: &str) -> u8 {
    if name_lower.contains("arm64-v8a")
        || name_lower.contains("arm64")
        || name_lower.contains("v8a")
    {
        return 3;
    }
    if name_lower.contains("debug") {
        return 0;
    }
    if name_lower.contains("release") {
        return 2;
    }
    1
}

fn pick_preferred_apk_path(paths: &[PathBuf]) -> Option<&PathBuf> {
    paths.iter().max_by_key(|p| {
        let s = p.to_string_lossy().to_lowercase();
        apk_preference_score(&s)
    })
}

fn pick_preferred_apk_name(names: &[String]) -> Option<&String> {
    names
        .iter()
        .max_by_key(|n| apk_preference_score(&n.to_lowercase()))
}

fn extract_first_apk_from_zip(
    archive_path: &Path,
    output_path: &Path,
    log_tag: &str,
    log: &mut Vec<String>,
) -> Result<bool> {
    let f = fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| {
        LtboxError::Patch(format!(
            "{}: APK container not a zip: {e}",
            archive_path.display()
        ))
    })?;
    // Pick `arm64-v8a` over generic / debug / x86 variants when the
    // container ships multiple split APKs (release ZIPs from KSU
    // family + ReSukiSU look like that). Falls back to first non-debug
    // APK, then the first APK at all.
    let member_names: Vec<String> = archive
        .file_names()
        .filter(|n| n.to_lowercase().ends_with(".apk") && !n.ends_with('/'))
        .map(|s| s.to_string())
        .collect();
    let Some(member_name) = pick_preferred_apk_name(&member_names).cloned() else {
        return Ok(false);
    };
    let mut entry = archive.by_name(&member_name).map_err(|e| {
        LtboxError::Patch(format!(
            "{}: read {member_name}: {e}",
            archive_path.display()
        ))
    })?;
    if output_path.exists() {
        fs::remove_file(output_path).ok();
    }
    let mut out = fs::File::create(output_path)?;
    std::io::copy(&mut entry, &mut out)?;
    ltbox_core::live!(
        log,
        "[{log_tag}] {}",
        tr("log_extracted_manager_apk")
            .replace("{member}", &member_name)
            .replace("{path}", &output_path.display().to_string())
    );
    Ok(true)
}

fn stage_manager_from_downloaded_asset(
    asset_path: &Path,
    manager_apk: &Path,
    log_tag: &str,
    log: &mut Vec<String>,
) -> Result<()> {
    if asset_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"))
    {
        copy_apk_to(asset_path, manager_apk)?;
        ltbox_core::live!(
            log,
            "[{log_tag}] {}",
            tr("log_staged_manager_apk").replace("{path}", &manager_apk.display().to_string())
        );
        return Ok(());
    }
    if extract_first_apk_from_zip(asset_path, manager_apk, log_tag, log)? {
        return Ok(());
    }
    Err(LtboxError::Patch(format!(
        "{log_tag}: manager artifact {} did not contain an APK",
        asset_path.display()
    )))
}

fn download_ksu_manager_apk_stable(
    provider: RootProvider,
    work_dir: &Path,
    manager_apk: &Path,
    log: &mut Vec<String>,
) -> Result<String> {
    let repo = provider_repo(provider).ok_or_else(|| {
        LtboxError::Patch(format!(
            "download_ksu_manager_apk: unsupported provider {provider:?}"
        ))
    })?;
    let client = GitHubClient::new(repo)?;
    let (tag, assets) = client.latest_release_assets()?;
    let (name, url) = select_manager_asset(&assets, ksu_manager_stable_preferences(provider))
        .ok_or_else(|| LtboxError::Download(format!("No manager APK artifact on latest {repo}")))?;
    ltbox_core::live!(
        log,
        "[KSU] {repo} {}",
        tr("log_release_latest_asset")
            .replace("{tag}", &tag)
            .replace("{name}", &name)
    );
    let asset_path = work_dir.join(&name);
    download_to_file(&url, &asset_path, log)?;
    stage_manager_from_downloaded_asset(&asset_path, manager_apk, "KSU", log)?;
    Ok(tag)
}

fn download_ksu_manager_apk_nightly(
    provider: RootProvider,
    manual_run_id: Option<u64>,
    work_dir: &Path,
    manager_apk: &Path,
    log: &mut Vec<String>,
) -> Result<u64> {
    let (repo, run_id) = resolve_nightly_run(provider, manual_run_id, log)?;
    let client = GitHubClient::new(repo)?;
    let artifact_names = client.workflow_artifacts(run_id)?;
    let pairs: Vec<(String, String)> = artifact_names
        .iter()
        .map(|name| (name.clone(), String::new()))
        .collect();
    let (artifact_name, _) =
        select_manager_asset(&pairs, ksu_manager_nightly_preferences(provider)).ok_or_else(
            || {
                LtboxError::Patch(format!(
                    "{repo} run {run_id}: no manager APK artifact (got {artifact_names:?})"
                ))
            },
        )?;
    ltbox_core::live!(
        log,
        "[KSU] {repo} {}",
        tr("log_nightly_artifact").replace("{artifact}", &artifact_name)
    );
    fetch_nightly_apk_outer_zip(
        "KSU",
        repo,
        run_id,
        &artifact_name,
        "ksu_manager_nightly",
        work_dir,
        manager_apk,
        log,
    )?;
    Ok(run_id)
}

/// Stage the manager APK used for post-root control into `work_dir/manager.apk`.
pub fn stage_root_manager_apk(
    cfg: &RootPipelineConfig,
    log: &mut Vec<String>,
) -> Result<Option<PathBuf>> {
    fs::create_dir_all(&cfg.work_dir)?;
    let manager_apk = cfg.work_dir.join("manager.apk");
    if manager_apk.exists() {
        fs::remove_file(&manager_apk).ok();
    }

    if cfg.gki_mode {
        let Some(kernel_zip) = cfg.gki_kernel_zip.as_ref() else {
            return Ok(None);
        };
        return if extract_first_apk_from_zip(kernel_zip, &manager_apk, "GKI", log)? {
            Ok(Some(manager_apk))
        } else {
            ltbox_core::live!(log, "[GKI] {}", tr("log_gki_no_manager_apk"));
            Ok(None)
        };
    }

    match cfg.family {
        RootFamily::Magisk => match (cfg.provider, cfg.version) {
            (RootProvider::MagiskFork, _) => {
                let src = cfg.magisk_forks_apk.as_ref().ok_or_else(|| {
                    LtboxError::Patch("Magisk forks require a local APK — none supplied".into())
                })?;
                copy_apk_to(src, &manager_apk)?;
                ltbox_core::live!(
                    log,
                    "[Magisk] {}",
                    tr("log_magisk_staged_fork_apk")
                        .replace("{path}", &manager_apk.display().to_string())
                );
            }
            (_, RootVersion::Stable) => {
                download_latest_magisk_apk(cfg.provider, &manager_apk, log)?;
            }
            (_, RootVersion::Nightly) => {
                download_magisk_apk_nightly(
                    cfg.provider,
                    cfg.nightly_run_id,
                    &cfg.work_dir,
                    &manager_apk,
                    log,
                )?;
            }
        },
        RootFamily::KernelSU => match cfg.version {
            RootVersion::Stable => {
                download_ksu_manager_apk_stable(cfg.provider, &cfg.work_dir, &manager_apk, log)?;
            }
            RootVersion::Nightly => {
                download_ksu_manager_apk_nightly(
                    cfg.provider,
                    cfg.nightly_run_id,
                    &cfg.work_dir,
                    &manager_apk,
                    log,
                )?;
            }
        },
        RootFamily::APatch => {
            let apk_path = cfg.work_dir.join("apatch.apk");
            match cfg.version {
                RootVersion::Stable => {
                    download_apatch_payload(cfg.provider, &cfg.work_dir, log)?;
                }
                RootVersion::Nightly => {
                    download_apatch_payload_nightly(
                        cfg.provider,
                        cfg.nightly_run_id,
                        &cfg.work_dir,
                        log,
                    )?;
                }
            }
            copy_apk_to(&apk_path, &manager_apk)?;
            ltbox_core::live!(
                log,
                "[APatch] {}",
                tr("log_staged_manager_apk").replace("{path}", &manager_apk.display().to_string())
            );
        }
    }

    Ok(Some(manager_apk))
}

// KSU payload: `.ko` is a release asset (per-kernel), `ksuinit` is a
// workflow artifact fetched via `nightly.link` (GitHub API needs auth).

/// Reduce kernel version to `major.minor` for KSU asset matching
/// (e.g. `6.6.118` → `6.6`). Already-short strings pass through.
pub fn normalize_ksu_kernel_version(kver: &str) -> Option<String> {
    let trimmed = kver.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let minor_digits: String = minor.chars().take_while(|c| c.is_ascii_digit()).collect();
    if minor_digits.is_empty() {
        return None;
    }
    Some(format!("{major}.{minor_digits}"))
}

/// True iff `lower_filename` embeds `kver` between `-{kver}_` delimiters.
/// Prevents unanchored `"6.1"` from matching 6.10 / 6.11 / etc.
fn ksu_ko_kver_matches(lower_filename: &str, kver: &str) -> bool {
    let needle = format!("-{kver}_");
    lower_filename.contains(&needle)
}

fn select_ksu_release_ko_asset(
    assets: &[(String, String)],
    kver: &str,
) -> Option<(String, String)> {
    let want = kver.to_lowercase();
    assets
        .iter()
        .find(|(n, _)| {
            let lower = n.to_lowercase();
            lower.ends_with("_kernelsu.ko") && ksu_ko_kver_matches(&lower, &want)
        })
        .cloned()
}

fn select_ksu_nightly_ko_artifact(artifact_names: &[String], kver: &str) -> Option<String> {
    // Nightly artifact naming changed upstream: previous
    // `<branch>-<kver>_kernelsu.ko` style is gone, current builds emit
    // `<branch>-<kver>-lkm` (e.g. `android15-6.6-lkm`). Accept either
    // shape so the path keeps working through the next inevitable
    // rename, and for the same reason match on `-{kver}-lkm` (with
    // trailing `-`/end-of-string sentinel) so `6.1` doesn't pull
    // `6.10`/`6.11`/`6.12`.
    let want = kver.to_lowercase();
    let lkm_marker = format!("-{want}-lkm");
    artifact_names
        .iter()
        .find(|n| {
            let lower = n.to_lowercase();
            // Legacy: "*-{kver}_kernelsu.ko"
            if lower.contains("_kernelsu.ko") && ksu_ko_kver_matches(&lower, &want) {
                return true;
            }
            // Current: "android<api>-{kver}-lkm" (zip wrapper, real
            // .ko inside).
            lower.contains(&lkm_marker)
        })
        .cloned()
}

pub fn download_ksu_payload(
    provider: RootProvider,
    kernel_version: Option<&str>,
    staging_dir: &Path,
    log: &mut Vec<String>,
) -> Result<()> {
    use std::io::{Read, Write};

    let repo = provider_repo(provider)
        .ok_or_else(|| LtboxError::Patch(format!("Unknown KSU provider: {provider:?}")))?;
    let client = GitHubClient::new(repo)?;
    let (tag, assets) = client.latest_release_assets()?;
    live!(log, "[KSU] Latest release: {tag}");

    // -------- 1. Per-kernel `.ko` from release assets --------
    // KSU tags assets by kernel branch (`android15-6.6_kernelsu.ko`);
    // strip patch suffix from device kver before matching.
    let kver = kernel_version
        .and_then(normalize_ksu_kernel_version)
        .ok_or_else(|| {
            LtboxError::Download(
                "KernelSU LKM requires a kernel version such as `6.1`; no safe module fallback is allowed."
                    .into(),
            )
        })?;
    let (ko_name, ko_url) = select_ksu_release_ko_asset(&assets, &kver).ok_or_else(|| {
        LtboxError::Download(format!(
            "No `_kernelsu.ko` release asset on latest {repo} matching kernel `{kver}`."
        ))
    })?;
    live!(log, "[KSU] Downloading LKM: {ko_name}");
    fs::create_dir_all(staging_dir)?;
    let ko_path = staging_dir.join("kernelsu.ko");
    download_to_file(&ko_url, &ko_path, log)?;

    // -------- 2. `ksuinit` binary via nightly.link --------
    let run_id = client.workflow_run_for_tag(&tag).map_err(|e| {
        LtboxError::Download(format!(
            "No workflow run found for tag {tag} on {repo}: {e}"
        ))
    })?;
    let artifacts = client.workflow_artifacts(run_id).map_err(|e| {
        LtboxError::Download(format!("Cannot list artifacts for run {run_id}: {e}"))
    })?;
    let ksuinit_artifact = artifacts
        .iter()
        .find(|n| n.to_lowercase().starts_with("ksuinit"))
        .cloned()
        .ok_or_else(|| {
            LtboxError::Download(format!(
                "No `ksuinit*` workflow artifact on run {run_id} of {repo}"
            ))
        })?;
    let nightly_url = format!(
        "https://nightly.link/{repo}/actions/runs/{run_id}/{ksuinit_artifact}.zip",
        repo = repo,
        run_id = run_id,
        ksuinit_artifact = ksuinit_artifact,
    );
    live!(
        log,
        "[KSU] Downloading ksuinit artifact: {ksuinit_artifact}"
    );
    let tmp_zip = staging_dir.join(format!("{ksuinit_artifact}.zip"));
    download_to_file(&nightly_url, &tmp_zip, log)?;

    let file = fs::File::open(&tmp_zip)
        .map_err(|e| LtboxError::Patch(format!("open ksuinit zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| LtboxError::Patch(format!("ksuinit zip read: {e}")))?;
    let member_name: Option<String> = archive
        .file_names()
        .find(|n| n.ends_with("ksuinit") && !n.ends_with('/'))
        .map(|s| s.to_string());
    let member_name = member_name.ok_or_else(|| {
        LtboxError::Patch(format!(
            "`ksuinit` entry missing from {ksuinit_artifact}.zip"
        ))
    })?;
    let mut entry = archive
        .by_name(&member_name)
        .map_err(|e| LtboxError::Patch(format!("ksuinit zip entry: {e}")))?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf).map_err(LtboxError::Io)?;
    drop(entry);

    // magiskboot expects `init`, not `ksuinit`.
    let mut out = fs::File::create(staging_dir.join("init"))?;
    out.write_all(&buf)?;
    let _ = fs::remove_file(&tmp_zip);
    live!(log, "[KSU] Staged init ({} bytes) + kernelsu.ko", buf.len());
    Ok(())
}

/// Download `.ko` + `init` from a KSU nightly run into `staging_dir`.
/// LKM selection requires an exact kernel major.minor match.
/// `manual_run_id = None` → latest successful run on provider's workflow.
pub fn download_ksu_payload_nightly(
    provider: RootProvider,
    kernel_version: Option<&str>,
    manual_run_id: Option<u64>,
    staging_dir: &Path,
    log: &mut Vec<String>,
) -> Result<u64> {
    use std::io::{Read, Write};

    let (repo, run_id) = resolve_nightly_run(provider, manual_run_id, log)?;
    let client = GitHubClient::new(repo)?;
    let artifact_names = client.workflow_artifacts(run_id)?;
    if artifact_names.is_empty() {
        return Err(LtboxError::Patch(format!(
            "{repo} run {run_id} has no artifacts"
        )));
    }

    fs::create_dir_all(staging_dir)?;
    let kver = kernel_version
        .and_then(normalize_ksu_kernel_version)
        .ok_or_else(|| {
            LtboxError::Patch(
                "KernelSU Nightly LKM requires a kernel version such as `6.1`; no safe module fallback is allowed."
                    .into(),
            )
        })?;

    // -------- 1. Kernel `.ko` --------
    let ko_artifact = select_ksu_nightly_ko_artifact(&artifact_names, &kver).ok_or_else(|| {
        LtboxError::Patch(format!(
            "{repo} run {run_id}: no *_kernelsu.ko artifact matching kernel {kver} (artifacts={artifact_names:?})"
        ))
    })?;
    live!(log, "[KSU] nightly LKM artifact: {ko_artifact}");
    let ko_zip_path = staging_dir.join("ksu_nightly_lkm.zip");
    let ko_url = nightly_artifact_url(repo, run_id, &ko_artifact);
    download_to_file(&ko_url, &ko_zip_path, log)?;
    {
        let f = fs::File::open(&ko_zip_path)?;
        let mut archive = zip::ZipArchive::new(f)
            .map_err(|e| LtboxError::Patch(format!("{repo}: LKM artifact not a zip: {e}")))?;
        // First `.ko` entry → staging_dir/kernelsu.ko.
        let member_name: String = archive
            .file_names()
            .find(|n| n.to_lowercase().ends_with(".ko"))
            .map(|s| s.to_string())
            .ok_or_else(|| {
                LtboxError::Patch(format!("{repo} {ko_artifact}: no .ko entry in zip"))
            })?;
        let mut entry = archive
            .by_name(&member_name)
            .map_err(|e| LtboxError::Patch(format!("{repo} {ko_artifact}: {e}")))?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        drop(entry);
        fs::write(staging_dir.join("kernelsu.ko"), &buf)?;
    }
    let _ = fs::remove_file(&ko_zip_path);

    // -------- 2. ksuinit → `init` --------
    let init_artifact = artifact_names
        .iter()
        .find(|n| n.to_lowercase().starts_with("ksuinit"))
        .cloned()
        .ok_or_else(|| {
            LtboxError::Patch(format!(
                "{repo} run {run_id}: no ksuinit artifact (got {artifact_names:?})"
            ))
        })?;
    live!(log, "[KSU] nightly ksuinit artifact: {init_artifact}");
    let init_zip_path = staging_dir.join("ksu_nightly_init.zip");
    let init_url = nightly_artifact_url(repo, run_id, &init_artifact);
    download_to_file(&init_url, &init_zip_path, log)?;
    {
        let f = fs::File::open(&init_zip_path)?;
        let mut archive = zip::ZipArchive::new(f)
            .map_err(|e| LtboxError::Patch(format!("{repo}: ksuinit artifact not a zip: {e}")))?;
        let member_name: String = archive
            .file_names()
            .find(|n| n.ends_with("ksuinit") && !n.ends_with('/'))
            .map(|s| s.to_string())
            .ok_or_else(|| {
                LtboxError::Patch(format!("{repo} {init_artifact}: no ksuinit entry in zip"))
            })?;
        let mut entry = archive
            .by_name(&member_name)
            .map_err(|e| LtboxError::Patch(format!("{repo} {init_artifact}: {e}")))?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        drop(entry);
        let mut out = fs::File::create(staging_dir.join("init"))?;
        out.write_all(&buf)?;
    }
    let _ = fs::remove_file(&init_zip_path);
    live!(log, "[KSU] staged nightly init + kernelsu.ko");
    Ok(run_id)
}

/// Pre-fetch every per-family root payload into `cfg.work_dir` so the
/// long network steps live in Phase 2 (before the EDL reboot)
/// alongside the manager APK download. The GUI calls this back-to-back
/// with [`stage_root_manager_apk`] before transitioning to EDL —
/// `build_patched_artifacts` then runs offline.
///
/// Idempotent on the per-family payload files we own:
/// * Magisk: `magisk.apk` + extracted `magiskinit` / `magisk` / etc.
/// * KSU LKM: `kernelsu.ko` + `init`.
/// * APatch: handled by [`stage_root_manager_apk`] (downloads the APK
///   and extracts `kpimg` in one shot), so we no-op here.
/// * GKI: AnyKernel3 zip is the user's input, no fetch needed.
pub fn stage_root_payload(cfg: &RootPipelineConfig, log: &mut Vec<String>) -> Result<()> {
    fs::create_dir_all(&cfg.work_dir)?;
    if cfg.gki_mode {
        return Ok(());
    }
    match cfg.family {
        RootFamily::Magisk => {
            // Skip if already extracted from a prior call.
            if cfg.work_dir.join("magiskinit").exists() {
                return Ok(());
            }
            let apk_path = cfg.work_dir.join("magisk.apk");
            let manager_apk = cfg.work_dir.join("manager.apk");
            // Reuse stage_root_manager_apk's bytes when available
            // — saves a duplicate ~10 MB fetch in the common path.
            if !apk_path.exists() {
                if matches!(cfg.provider, RootProvider::MagiskFork) {
                    let src = cfg.magisk_forks_apk.as_ref().ok_or_else(|| {
                        LtboxError::Patch("Magisk forks require a local APK — none supplied".into())
                    })?;
                    if !src.exists() {
                        return Err(LtboxError::Patch(format!(
                            "Magisk forks APK does not exist: {}",
                            src.display()
                        )));
                    }
                    fs::copy(src, &apk_path)
                        .map_err(|e| LtboxError::Patch(format!("stage forks APK: {e}")))?;
                } else if manager_apk.exists() {
                    fs::copy(&manager_apk, &apk_path).map_err(|e| {
                        LtboxError::Patch(format!("magisk.apk copy from manager.apk: {e}"))
                    })?;
                } else {
                    match cfg.version {
                        RootVersion::Stable => {
                            download_latest_magisk_apk(cfg.provider, &apk_path, log)?;
                        }
                        RootVersion::Nightly => {
                            download_magisk_apk_nightly(
                                cfg.provider,
                                cfg.nightly_run_id,
                                &cfg.work_dir,
                                &apk_path,
                                log,
                            )?;
                        }
                    }
                }
            }
            live!(
                log,
                "[Magisk] Extracting payload from APK (magisk, magiskinit, init-ld, stub.apk)"
            );
            magisk::extract_apk_payload(&apk_path, &cfg.work_dir)?;
        }
        RootFamily::KernelSU => {
            // Skip if both files already on disk from a prior call.
            let ko = cfg.work_dir.join("kernelsu.ko");
            let init = cfg.work_dir.join("init");
            if ko.exists() && init.exists() {
                return Ok(());
            }
            match cfg.version {
                RootVersion::Stable => {
                    live!(log, "[KSU] Fetching latest Stable LKM zip from GitHub…");
                    download_ksu_payload(
                        cfg.provider,
                        cfg.kernel_version.as_deref(),
                        &cfg.work_dir,
                        log,
                    )?;
                }
                RootVersion::Nightly => {
                    live!(
                        log,
                        "[KSU] Fetching Nightly payload (run_id={:?})…",
                        cfg.nightly_run_id
                    );
                    download_ksu_payload_nightly(
                        cfg.provider,
                        cfg.kernel_version.as_deref(),
                        cfg.nightly_run_id,
                        &cfg.work_dir,
                        log,
                    )?;
                }
            }
        }
        RootFamily::APatch => {
            // stage_root_manager_apk for APatch already downloads the
            // APK and extracts kpimg via download_apatch_payload — no
            // additional payload fetch needed here.
        }
    }
    Ok(())
}

/// Offline pipeline outcome — everything before the EDL flash step.
pub struct PatchedArtifacts {
    pub patched_boot: PathBuf,
    /// `None` when the original vbmeta can stay (no chain).
    pub patched_vbmeta: Option<PathBuf>,
    pub manager_apk: Option<PathBuf>,
    /// Target partition name (`init_boot_a`, `boot_a`, …).
    pub boot_partition: String,
    pub vbmeta_partition: Option<String>,
}

/// Build patched artifacts: fetch payload, patch, resign, rebuild vbmeta,
/// move finals into `output_dir`. Caller must have already dumped stock
/// images into `cfg.work_dir` (GUI reuses the EDL session for flash).
pub fn build_patched_artifacts(
    cfg: &RootPipelineConfig,
    log: &mut Vec<String>,
) -> Result<PatchedArtifacts> {
    fs::create_dir_all(&cfg.work_dir)?;
    fs::create_dir_all(&cfg.output_dir)?;

    // GKI → boot.img; LKM → init_boot.img. GUI dump step picks the right one.
    let base_part = boot_partition_base(cfg.family, cfg.gki_mode);
    let stock_filename = if base_part == "boot" {
        "boot.img"
    } else {
        "init_boot.img"
    };
    let stock_boot_src = cfg.work_dir.join(stock_filename);
    let vbmeta_src = cfg.work_dir.join("vbmeta.img");
    if !stock_boot_src.exists() {
        return Err(LtboxError::Patch(format!(
            "work_dir is missing the stock {stock_filename} dump"
        )));
    }
    if !vbmeta_src.exists() {
        return Err(LtboxError::Patch(
            "work_dir is missing the stock vbmeta.img dump".into(),
        ));
    }
    // Defensive: GUI Phase 2 prefetches the manager APK + payload
    // before EDL, but headless callers (and the stable test
    // surface) shouldn't have to remember the order. Both helpers
    // are idempotent against already-staged files.
    let staged_manager_apk = cfg.work_dir.join("manager.apk");
    if !cfg.gki_mode && !staged_manager_apk.exists() {
        stage_root_manager_apk(cfg, log)?;
    }
    if !cfg.gki_mode {
        stage_root_payload(cfg, log)?;
    }

    let patched_boot = if cfg.gki_mode {
        // GKI: swap kernel blob from user's AnyKernel3 zip — no GitHub fetch.
        let kernel_zip = cfg.gki_kernel_zip.as_ref().ok_or_else(|| {
            LtboxError::Patch("GKI mode requires a custom kernel zip — none supplied".into())
        })?;
        live!(log, "[GKI] Kernel zip: {}", kernel_zip.display());
        gki::patch_boot(&cfg.work_dir, kernel_zip, log)?
    } else {
        match cfg.family {
            RootFamily::Magisk => {
                live!(log, "[Magisk] Patching init_boot.img ramdisk…");
                magisk::patch_init_boot(&cfg.work_dir, &cfg.preinit_device, log)?
            }
            RootFamily::KernelSU => {
                live!(
                    log,
                    "[KSU] Patching init_boot.img — swapping init + staging kernelsu.ko…"
                );
                ksu::patch_init_boot(&cfg.work_dir, log)?
            }
            RootFamily::APatch => {
                live!(
                    log,
                    "[APatch] Patching boot.img via kptools-rs (kpm_count={}, superkey_len={})",
                    cfg.kpm_paths.len(),
                    cfg.superkey.len()
                );
                crate::apatch::patch_boot(&cfg.work_dir, &cfg.kpm_paths, &cfg.superkey, log)?
            }
        }
    };

    let final_boot = cfg.output_dir.join(stock_filename);
    if final_boot.exists() {
        fs::remove_file(&final_boot).ok();
    }
    fs::rename(&patched_boot, &final_boot)?;
    ltbox_core::live!(
        log,
        "[Root] {} {} {} {}",
        tr("log_root_patched"),
        stock_filename,
        tr("log_root_ready_at"),
        final_boot.display()
    );

    // Re-apply AVB footer. Algorithm + rollback index copied from stock to
    // preserve device's rollback state. Signing key via `KEY_MAP` on stock pubkey.
    let stock_info = avb::extract_image_avb_info(&stock_boot_src)?;
    let boot_key = resolve_signing_key(stock_info.public_key_sha1.as_deref(), stock_filename, log)?;
    avb::erase_footer(&final_boot).ok();
    avb::add_hash_footer(
        &final_boot,
        &stock_info,
        boot_key.as_deref(),
        Some(stock_info.rollback_index),
    )?;
    ltbox_core::live!(
        log,
        "[AVB] {} {} ({} rollback={}, key={})",
        tr("log_avb_refootered"),
        stock_filename,
        stock_info.algorithm,
        stock_info.rollback_index,
        boot_key.as_deref().unwrap_or("(unsigned)"),
    );

    // Rebuild vbmeta with fresh hash descriptor. vbmeta pubkey may differ
    // from boot pubkey — second `KEY_MAP` lookup against the stock vbmeta.
    let stock_vbmeta_info = avb::extract_image_avb_info(&vbmeta_src)?;
    let vbmeta_key = resolve_signing_key(
        stock_vbmeta_info.public_key_sha1.as_deref(),
        "vbmeta.img",
        log,
    )?;
    let final_vbmeta = cfg.output_dir.join("vbmeta.img");
    match vbmeta_key.as_deref() {
        Some(key) => {
            avb::rebuild_vbmeta_with_chained_images(
                &final_vbmeta,
                &vbmeta_src,
                &[&final_boot],
                key,
                None,
            )?;
            ltbox_core::live!(
                log,
                "[AVB] {} {} at {} (key={key})",
                tr("log_avb_rebuilt_vbmeta"),
                stock_filename,
                final_vbmeta.display(),
            );
        }
        None => {
            // Unsigned vbmeta: copy stock through. Stale chain hash is fine
            // since NONE-algorithm bootloaders skip verification.
            fs::copy(&vbmeta_src, &final_vbmeta)?;
            ltbox_core::live!(
                log,
                "[AVB] {} {}",
                tr("log_avb_vbmeta_unsigned_copied"),
                final_vbmeta.display(),
            );
        }
    }

    // Empty slot suffix → default to `_a`.
    let suffix = if cfg.slot_suffix.is_empty() {
        "_a".to_string()
    } else {
        cfg.slot_suffix.clone()
    };

    Ok(PatchedArtifacts {
        patched_boot: final_boot,
        patched_vbmeta: Some(final_vbmeta),
        manager_apk: staged_manager_apk.exists().then_some(staged_manager_apk),
        boot_partition: format!("{base_part}{suffix}"),
        vbmeta_partition: Some(format!("vbmeta{suffix}")),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        RootProvider, apk_preference_score, download_ksu_manager_apk_nightly,
        download_ksu_manager_apk_stable, download_ksu_payload, download_ksu_payload_nightly,
        ksu_ko_kver_matches, normalize_ksu_kernel_version, pick_preferred_apk_name,
        pick_preferred_apk_path, select_ksu_nightly_ko_artifact, select_ksu_release_ko_asset,
        select_manager_asset,
    };
    use std::path::PathBuf;

    #[test]
    fn apk_preference_arm64_v8a_wins() {
        let names = vec![
            "app-x86-release.apk".to_string(),
            "app-arm64-v8a-release.apk".to_string(),
            "app-armeabi-v7a-release.apk".to_string(),
        ];
        assert_eq!(
            pick_preferred_apk_name(&names).map(|s| s.as_str()),
            Some("app-arm64-v8a-release.apk")
        );
    }

    #[test]
    fn apk_preference_release_beats_debug() {
        let names = vec!["app-debug.apk".to_string(), "app-release.apk".to_string()];
        assert_eq!(
            pick_preferred_apk_name(&names).map(|s| s.as_str()),
            Some("app-release.apk")
        );
    }

    #[test]
    fn apk_preference_falls_back_to_first_when_no_hints() {
        let names = vec!["foo.apk".to_string(), "bar.apk".to_string()];
        // Both score 1, max_by_key returns the last on ties — accept either
        // since neither carries an arm64/release/debug hint.
        let pick = pick_preferred_apk_name(&names).unwrap();
        assert!(names.contains(pick));
    }

    #[test]
    fn apk_preference_no_candidates_returns_none() {
        let empty: Vec<String> = Vec::new();
        assert!(pick_preferred_apk_name(&empty).is_none());
    }

    #[test]
    fn apk_preference_path_picks_arm64_v8a_in_subdir() {
        let paths = vec![
            PathBuf::from("staging/app-debug.apk"),
            PathBuf::from("staging/manager/arm64-v8a/app-release.apk"),
            PathBuf::from("staging/manager/x86/app-release.apk"),
        ];
        assert_eq!(
            pick_preferred_apk_path(&paths),
            Some(&PathBuf::from("staging/manager/arm64-v8a/app-release.apk"))
        );
    }

    #[test]
    fn apk_preference_score_orders_correctly() {
        assert!(
            apk_preference_score("app-arm64-v8a-release.apk")
                > apk_preference_score("app-release.apk")
        );
        assert!(apk_preference_score("app-release.apk") > apk_preference_score("app-debug.apk"));
        assert!(apk_preference_score("app-release.apk") > apk_preference_score("foo.apk"));
    }

    #[test]
    fn exact_major_minor_matches() {
        assert!(ksu_ko_kver_matches("android15-6.1_kernelsu.ko", "6.1"));
        assert!(ksu_ko_kver_matches("android14-5.15_kernelsu.ko", "5.15"));
    }

    #[test]
    fn longer_minor_does_not_match_shorter_prefix() {
        // Regression: unanchored `contains("6.1")` used to match 6.10/6.11/etc.
        assert!(!ksu_ko_kver_matches("android15-6.10_kernelsu.ko", "6.1"));
        assert!(!ksu_ko_kver_matches("android15-6.11_kernelsu.ko", "6.1"));
        assert!(!ksu_ko_kver_matches("android15-6.12_kernelsu.ko", "6.1"));
        assert!(!ksu_ko_kver_matches("android15-6.13_kernelsu.ko", "6.1"));
    }

    #[test]
    fn different_major_does_not_match() {
        assert!(!ksu_ko_kver_matches("android15-5.15_kernelsu.ko", "6.1"));
        assert!(!ksu_ko_kver_matches("android14-6.1_kernelsu.ko", "5.15"));
    }

    #[test]
    fn missing_leading_dash_does_not_match() {
        // `-{kver}_` boundary is required; bare `6.1_kernelsu.ko` is not a stock layout.
        assert!(!ksu_ko_kver_matches("6.1_kernelsu.ko", "6.1"));
    }

    #[test]
    fn ksu_kernel_version_normalizes_to_major_minor() {
        assert_eq!(normalize_ksu_kernel_version("6.1"), Some("6.1".to_string()));
        assert_eq!(
            normalize_ksu_kernel_version("6.1.75"),
            Some("6.1".to_string())
        );
        assert_eq!(
            normalize_ksu_kernel_version("  5.15.149-android14  "),
            Some("5.15".to_string())
        );
    }

    #[test]
    fn ksu_kernel_version_rejects_missing_or_malformed_input() {
        assert_eq!(normalize_ksu_kernel_version(""), None);
        assert_eq!(normalize_ksu_kernel_version("6"), None);
        assert_eq!(normalize_ksu_kernel_version("six.one"), None);
    }

    #[test]
    fn ksu_release_asset_selection_requires_matching_kernel() {
        let assets = vec![
            (
                "android14-5.15_kernelsu.ko".to_string(),
                "https://example.invalid/5.15.ko".to_string(),
            ),
            (
                "android15-6.6_kernelsu.ko".to_string(),
                "https://example.invalid/6.6.ko".to_string(),
            ),
        ];

        let picked = select_ksu_release_ko_asset(&assets, "6.6").expect("6.6 asset");
        assert_eq!(picked.0, "android15-6.6_kernelsu.ko");
        assert!(select_ksu_release_ko_asset(&assets, "6.1").is_none());
    }

    #[test]
    fn ksu_nightly_artifact_selection_does_not_fallback_to_any_module() {
        let artifacts = vec![
            "android14-5.15_kernelsu.ko".to_string(),
            "ksuinit-arm64.zip".to_string(),
        ];

        assert_eq!(
            select_ksu_nightly_ko_artifact(&artifacts, "5.15"),
            Some("android14-5.15_kernelsu.ko".to_string())
        );
        assert_eq!(select_ksu_nightly_ko_artifact(&artifacts, "6.1"), None);
    }

    #[test]
    fn ksu_nightly_artifact_selection_picks_new_lkm_naming() {
        // Real artifact list emitted by 2026 KernelSU / KSU-Next /
        // SukiSU / ReSukiSU nightlies — bare `<branch>-<kver>-lkm`
        // wrapper instead of the old `*_kernelsu.ko` filename.
        let artifacts = vec![
            "manager".to_string(),
            "ksud-aarch64-linux-android".to_string(),
            "android16-6.12-lkm".to_string(),
            "android15-6.6-lkm".to_string(),
            "android14-5.15-lkm".to_string(),
            "android14-6.1-lkm".to_string(),
            "android13-5.10-lkm".to_string(),
            "ksuinit".to_string(),
        ];

        assert_eq!(
            select_ksu_nightly_ko_artifact(&artifacts, "6.6"),
            Some("android15-6.6-lkm".to_string())
        );
        assert_eq!(
            select_ksu_nightly_ko_artifact(&artifacts, "5.15"),
            Some("android14-5.15-lkm".to_string())
        );
        // 6.1 must not steal 6.10 / 6.11 / 6.12 — kver match anchors
        // both sides via the surrounding `-` markers.
        assert_eq!(
            select_ksu_nightly_ko_artifact(&artifacts, "6.1"),
            Some("android14-6.1-lkm".to_string())
        );
        // No 4.x in this artifact set.
        assert_eq!(select_ksu_nightly_ko_artifact(&artifacts, "4.14"), None);
    }

    #[test]
    fn ksu_manager_asset_selection_prefers_provider_names() {
        let assets = vec![
            (
                "random-debug.apk".to_string(),
                "https://example.invalid/debug.apk".to_string(),
            ),
            (
                "manager-spoofed.zip".to_string(),
                "https://example.invalid/manager-spoofed.zip".to_string(),
            ),
            (
                "manager.zip".to_string(),
                "https://example.invalid/manager.zip".to_string(),
            ),
        ];

        let picked = select_manager_asset(&assets, &["manager-spoofed.zip", "manager.zip"])
            .expect("manager asset");
        assert_eq!(picked.0, "manager-spoofed.zip");
    }

    /// Network-dependent end-to-end probe of every LKM provider's
    /// manager-APK fetch path (Stable + Nightly auto). Each iteration
    /// uses an isolated tempdir so failures don't poison subsequent
    /// runs. Marked `#[ignore]` so CI / `cargo test` skip it; run
    /// locally with:
    ///
    ///     cargo test -p ltbox-patch --lib -- --ignored --nocapture lkm_manager_download_smoke
    ///
    /// Pass criteria per provider/channel:
    /// 1. Function returns `Ok(_)`.
    /// 2. `manager.apk` exists at the expected path.
    /// 3. The file is non-empty (full APK download / extraction).
    #[test]
    #[ignore = "hits GitHub releases + nightly.link; run manually"]
    fn lkm_manager_download_smoke() {
        let providers: &[(RootProvider, &str)] = &[
            (RootProvider::KernelSU, "tiann/KernelSU"),
            (RootProvider::KernelSUNext, "KernelSU-Next/KernelSU-Next"),
            (RootProvider::SukiSU, "SukiSU-Ultra/SukiSU-Ultra"),
            (RootProvider::ReSukiSU, "ReSukiSU/ReSukiSU"),
        ];

        let mut report: Vec<(String, String)> = Vec::new();

        for (provider, repo) in providers.iter().copied() {
            // ----- Stable -----
            let stable_label = format!("{repo} stable");
            // ReSukiSU has no Stable releases — expect Err.
            if matches!(provider, RootProvider::ReSukiSU) {
                report.push((
                    stable_label.clone(),
                    "skipped (no Stable channel)".to_string(),
                ));
            } else {
                let tmp = tempfile::tempdir().expect("tempdir");
                let manager_apk = tmp.path().join("manager.apk");
                let mut log = Vec::new();
                let result =
                    download_ksu_manager_apk_stable(provider, tmp.path(), &manager_apk, &mut log);
                let outcome = match result {
                    Ok(tag) => match (
                        manager_apk.exists(),
                        std::fs::metadata(&manager_apk)
                            .map(|m| m.len())
                            .unwrap_or(0),
                    ) {
                        (true, n) if n > 0 => format!("OK tag={tag} size={n}"),
                        (true, _) => "FAIL: manager.apk empty".to_string(),
                        (false, _) => "FAIL: manager.apk missing".to_string(),
                    },
                    Err(e) => format!("FAIL: {e}"),
                };
                eprintln!("[{stable_label}] {outcome}");
                report.push((stable_label, outcome));
            }

            // ----- Nightly auto-detect -----
            let nightly_label = format!("{repo} nightly");
            let tmp = tempfile::tempdir().expect("tempdir");
            let manager_apk = tmp.path().join("manager.apk");
            let mut log = Vec::new();
            let result = download_ksu_manager_apk_nightly(
                provider,
                None,
                tmp.path(),
                &manager_apk,
                &mut log,
            );
            let outcome = match result {
                Ok(run_id) => match (
                    manager_apk.exists(),
                    std::fs::metadata(&manager_apk)
                        .map(|m| m.len())
                        .unwrap_or(0),
                ) {
                    (true, n) if n > 0 => format!("OK run={run_id} size={n}"),
                    (true, _) => "FAIL: manager.apk empty".to_string(),
                    (false, _) => "FAIL: manager.apk missing".to_string(),
                },
                Err(e) => format!("FAIL: {e}"),
            };
            eprintln!("[{nightly_label}] {outcome}");
            report.push((nightly_label, outcome));
        }

        eprintln!("\n=== LKM manager-APK download report ===");
        for (label, outcome) in &report {
            eprintln!("  {label}: {outcome}");
        }
        eprintln!();

        let failures: Vec<&(String, String)> = report
            .iter()
            .filter(|(_, o)| o.starts_with("FAIL"))
            .collect();
        assert!(
            failures.is_empty(),
            "{} provider/channel combinations failed: {:#?}",
            failures.len(),
            failures
        );
    }

    /// Network-dependent probe for the full `download_ksu_payload`
    /// path — `.ko` (kernel module) + `ksuinit` artifact extraction —
    /// against kernel `6.6` for every KSU-family provider that ships
    /// release artifacts.
    ///
    ///     cargo test -p ltbox-patch --lib -- --ignored --nocapture lkm_payload_download_smoke
    #[test]
    #[ignore = "hits GitHub releases + nightly.link; run manually"]
    fn lkm_payload_download_smoke() {
        const KVER: &str = "6.6";
        let providers: &[(RootProvider, &str)] = &[
            (RootProvider::KernelSU, "tiann/KernelSU"),
            (RootProvider::KernelSUNext, "KernelSU-Next/KernelSU-Next"),
            (RootProvider::SukiSU, "SukiSU-Ultra/SukiSU-Ultra"),
        ];

        let mut report: Vec<(String, String)> = Vec::new();

        for (provider, repo) in providers.iter().copied() {
            let label = format!("{repo} payload k{KVER}");
            let tmp = tempfile::tempdir().expect("tempdir");
            let mut log = Vec::new();
            let result = download_ksu_payload(provider, Some(KVER), tmp.path(), &mut log);
            let outcome = match result {
                Ok(()) => {
                    let ko = tmp.path().join("kernelsu.ko");
                    let init = tmp.path().join("init");
                    let ko_n = std::fs::metadata(&ko).map(|m| m.len()).unwrap_or(0);
                    let init_n = std::fs::metadata(&init).map(|m| m.len()).unwrap_or(0);
                    if ko.exists() && ko_n > 0 && init.exists() && init_n > 0 {
                        format!("OK ko={ko_n} init={init_n}")
                    } else {
                        format!(
                            "FAIL: ko_exists={} ko_size={} init_exists={} init_size={}",
                            ko.exists(),
                            ko_n,
                            init.exists(),
                            init_n
                        )
                    }
                }
                Err(e) => format!("FAIL: {e}"),
            };
            eprintln!("[{label}] {outcome}");
            report.push((label, outcome));
        }

        eprintln!("\n=== LKM payload download report ===");
        for (label, outcome) in &report {
            eprintln!("  {label}: {outcome}");
        }
        eprintln!();

        let failures: Vec<&(String, String)> = report
            .iter()
            .filter(|(_, o)| o.starts_with("FAIL"))
            .collect();
        assert!(
            failures.is_empty(),
            "{} provider payloads failed: {:#?}",
            failures.len(),
            failures
        );
    }

    /// Nightly counterpart to `lkm_payload_download_smoke` — exercises
    /// `download_ksu_payload_nightly` so the per-kernel `.ko` artifact
    /// selection + ksuinit extraction get checked against every
    /// provider's actual nightly run, including ReSukiSU which has no
    /// Stable channel and is the only path that's actually used in
    /// production for that fork.
    ///
    ///     cargo test -p ltbox-patch --lib -- --ignored --nocapture lkm_payload_nightly_download_smoke
    #[test]
    #[ignore = "hits GitHub releases + nightly.link; run manually"]
    fn lkm_payload_nightly_download_smoke() {
        const KVER: &str = "6.6";
        let providers: &[(RootProvider, &str)] = &[
            (RootProvider::KernelSU, "tiann/KernelSU"),
            (RootProvider::KernelSUNext, "KernelSU-Next/KernelSU-Next"),
            (RootProvider::SukiSU, "SukiSU-Ultra/SukiSU-Ultra"),
            (RootProvider::ReSukiSU, "ReSukiSU/ReSukiSU"),
        ];

        let mut report: Vec<(String, String)> = Vec::new();

        for (provider, repo) in providers.iter().copied() {
            let label = format!("{repo} nightly payload k{KVER}");
            let tmp = tempfile::tempdir().expect("tempdir");
            let mut log = Vec::new();
            let result =
                download_ksu_payload_nightly(provider, Some(KVER), None, tmp.path(), &mut log);
            let outcome = match result {
                Ok(run_id) => {
                    let ko = tmp.path().join("kernelsu.ko");
                    let init = tmp.path().join("init");
                    let ko_n = std::fs::metadata(&ko).map(|m| m.len()).unwrap_or(0);
                    let init_n = std::fs::metadata(&init).map(|m| m.len()).unwrap_or(0);
                    if ko.exists() && ko_n > 0 && init.exists() && init_n > 0 {
                        format!("OK run={run_id} ko={ko_n} init={init_n}")
                    } else {
                        format!(
                            "FAIL: ko_exists={} ko_size={} init_exists={} init_size={}",
                            ko.exists(),
                            ko_n,
                            init.exists(),
                            init_n
                        )
                    }
                }
                Err(e) => format!("FAIL: {e}"),
            };
            eprintln!("[{label}] {outcome}");
            report.push((label, outcome));
        }

        eprintln!("\n=== LKM nightly payload download report ===");
        for (label, outcome) in &report {
            eprintln!("  {label}: {outcome}");
        }
        eprintln!();

        let failures: Vec<&(String, String)> = report
            .iter()
            .filter(|(_, o)| o.starts_with("FAIL"))
            .collect();
        assert!(
            failures.is_empty(),
            "{} nightly payloads failed: {:#?}",
            failures.len(),
            failures
        );
    }
}
