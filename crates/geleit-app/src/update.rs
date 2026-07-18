//! Auto-update (APP-7, ADR-0013) — glue over `tauri-plugin-updater`.
//!
//! This is the app's **only** non-IMAP/SMTP network. It GETs a single static release feed, sending only
//! the running version + platform (to serve the right binary) and **no user data**; every update is
//! **signature-verified** (the public key is compiled in) before it can install, and installing is never
//! silent — the frontend always confirms. On-launch checking is gated by the `auto_update` setting.
//!
//! Excluded from mutants: it's glue over a plugin that needs a live updater endpoint + a real signed
//! artifact. The version comparison (is the feed's build newer?) is the plugin's own semver check, so
//! there's no logic of ours to mutation-test here; the flow is verified against a local feed instead.

use crate::ipc::AppState;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Wait this long after launch before the first check, so it never fights the app's own boot.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);

/// What the UI needs to describe an available update.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
}

/// On launch: unless the user has turned auto-checking off, look for an update once and — if a newer
/// signed build exists — tell the frontend (a calm "update available" prompt). Never installs on its
/// own. Runs for the app's lifetime on Tauri's async runtime.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        let state = app.state::<AppState>().inner().clone();
        if !crate::ipc::bool_setting(&state, "auto_update", true).await {
            return; // the user opted out of automatic checks — manual button only
        }
        if let Ok(Some(info)) = check(&app).await {
            let _ = app.emit("update-available", info);
        }
    });
}

/// Check the release feed. `Ok(None)` = up to date; `Ok(Some(info))` = a newer signed build is offered;
/// `Err` = the feed couldn't be reached or the config is missing (surfaced only when the user asked).
/// The request carries only the app version + platform — no user data.
pub(crate) async fn check(app: &tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app
        .updater()
        .map_err(|_| "Updates aren't set up in this build.".to_owned())?;
    match updater.check().await {
        Ok(Some(u)) => Ok(Some(UpdateInfo {
            version: u.version.clone(),
            notes: u.body.clone().unwrap_or_default(),
        })),
        Ok(None) => Ok(None),
        Err(_) => Err("Couldn't reach the update server.".to_owned()),
    }
}

/// Download, verify the signature of, and install the pending update, then relaunch into it. Re-checks
/// to obtain the update handle. `app.restart()` diverges, so a success never returns.
pub(crate) async fn install(app: &tauri::AppHandle) -> Result<(), String> {
    let updater = app
        .updater()
        .map_err(|_| "Updates aren't set up in this build.".to_owned())?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|_| "Couldn't reach the update server.".to_owned())?
    else {
        return Err("You're already up to date.".to_owned());
    };
    update
        .download_and_install(|_downloaded, _total| {}, || {})
        .await
        .map_err(|_| "Couldn't install the update.".to_owned())?;
    app.restart();
}
