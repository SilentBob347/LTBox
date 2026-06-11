use super::*;

/// Advanced "Change Country Code": rewrite the device's country code over EDL
/// and reset to system. Mirrors `flash_worker`'s country phase but standalone —
/// no firmware package, just a user-picked country + EDL loader. Per model:
/// TB320FC / TB323FU touch `oemowninfo`; all others `devinfo` + `persist`.
pub(crate) fn change_country_worker(
    conn: ConnectionStatus,
    device_model: String,
    target_code: String,
    loader: std::path::PathBuf,
    ll: LiveLabels,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    live!(
        log,
        "[Country] {}",
        tr_args!(
            "live_flash_country_patch_target",
            target = target_code.as_str()
        )
    );
    // Create scratch + backup dirs BEFORE entering EDL: a setup failure here
    // must not strand the device in EDL (EdlSession has no reset-on-drop).
    let work_dir = ltbox_core::app_paths::work_dir_for("change_country");
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| tr_args!("err_country_work_dir_failed", error = e.to_string()))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let critical_backup = ltbox_core::app_paths::backup_dir_for(&format!("backup_critical_{ts}"));
    std::fs::create_dir_all(&critical_backup)
        .map_err(|e| tr_args!("err_country_backup_dir_failed", error = e.to_string()))?;
    transition_to_edl(conn, &ll, &mut log)?;
    let mut session = open_edl_session(&loader, true, &mut log)?;
    let outcome = run_country_change(
        &mut session,
        &work_dir,
        &critical_backup,
        &device_model,
        None,
        &target_code,
        &ll,
        &mut log,
    );
    // Reset to system regardless (don't strand the device in EDL), then surface
    // any failure: for the standalone op the country change IS the operation, so
    // a partial failure must not report success.
    session.reset_tolerant(&mut log);
    outcome?;
    live!(
        log,
        "[Country] {}",
        ltbox_core::i18n::tr("live_country_change_done")
    );
    ltbox_core::app_paths::clean_work_dirs();
    Ok(log)
}
