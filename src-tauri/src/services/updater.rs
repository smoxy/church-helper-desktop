//! App self-update with a hand-built fallback between two update sources.
//!
//! - **Updater A** (primary): our endpoint
//!   `{api_base_url()}/api/desktop/update/{{target}}/{{arch}}/{{current_version}}`
//!   (contract D), with the anonymous `X-Install-Id` header and a 30s timeout.
//! - **Updater B** (fallback): the GitHub `latest.json` configured in
//!   `tauri.conf.json` (`plugins.updater.endpoints`), queried WITHOUT the
//!   header so the install id never reaches GitHub. Used ONLY when A fails
//!   (network error, timeout, 503 or any other non-2xx, undecodable body). A
//!   `204` from A is a deliberate hold and is respected: no fallback.
//!
//! Checks run once shortly after startup and then every
//! `AppConfig::update_check_interval_hours`, never two at once
//! (`AppState::update_check_in_flight`), and are deferred while downloads
//! are queued or running. An available update is downloaded in the
//! background and installed ONLY after explicit consent (never silently,
//! bl-desktop-autoupdate). A critical release (contract D: `critical: true`,
//! or the installed version below `min_supported`) ignores a previous
//! "Più tardi" and asks again — still with consent.

use crate::commands::AppState;
use crate::constants::api_base_url;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;
use tokio::time::{sleep, Duration};

/// Path (appended to `api_base_url()`) of our update endpoint. The
/// `{{...}}` placeholders are substituted by tauri-plugin-updater.
pub const UPDATE_ENDPOINT_PATH: &str =
    "/api/desktop/update/{{target}}/{{arch}}/{{current_version}}";
/// Request header carrying the anonymous install id (sent to A only).
pub const INSTALL_ID_HEADER: &str = "X-Install-Id";
/// tauri-plugin-store key (in `settings.json`, NOT inside `AppConfig`) holding
/// the anonymous install id.
pub const INSTALL_ID_STORE_KEY: &str = "install_id";
/// tauri-plugin-store key (in settings.json) holding the last update version
/// the user dismissed with "Più tardi", so it isn't re-offered every check.
const UPDATER_DECLINED_VERSION_KEY: &str = "updater_declined_version";
/// Sentinel value shipped in `tauri.conf.json` before the real signing public
/// key existed (see UPDATER_SETUP.md).
const UPDATER_PUBKEY_PLACEHOLDER: &str = "PLACEHOLDER_REPLACE_WITH_TAURI_SIGNER_PUBLIC_KEY";

/// Timeout of each update-manifest request (A and B).
const UPDATE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Delay before the first check, letting startup (initial poll, retention,
/// tray, window) settle first.
const STARTUP_DELAY: Duration = Duration::from_secs(10);
/// Retry delay when a check was deferred because downloads are active.
const BUSY_RETRY_DELAY: Duration = Duration::from_secs(15 * 60);

/// Our update endpoint for a given API base (pure, so it is testable without
/// touching the process environment). Debug builds with the
/// `CHURCH_HELPER_API_BASE` override therefore point at the stub instead of
/// skipping the check.
pub(crate) fn update_endpoint_for(base: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), UPDATE_ENDPOINT_PATH)
}

/// What to do once Updater A answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrimaryDecision {
    /// A offered a newer version: use it (B is never queried).
    UseUpdate,
    /// A answered and there is nothing to install: either a 200 manifest whose
    /// version is not newer, or a deliberate `204` hold. B is NOT queried —
    /// falling back on a 204 would bypass the pin/block policy.
    NoUpdate,
    /// A failed (network error/timeout, 503 or other non-2xx, bad body):
    /// fall back to GitHub (B), without the install id.
    FallBack,
}

/// Decision table on Updater A's `check()` result. tauri-plugin-updater maps
/// the contract D statuses as: `200` newer -> `Ok(Some)`, `200` same/older ->
/// `Ok(None)`, `204` -> `Ok(None)`, `503`/other non-2xx -> `Err(ReleaseNotFound)`,
/// unreachable/timeout/undecodable -> `Err(..)`.
pub(crate) fn decide_after_primary<T, E>(primary: &Result<Option<T>, E>) -> PrimaryDecision {
    match primary {
        Ok(Some(_)) => PrimaryDecision::UseUpdate,
        Ok(None) => PrimaryDecision::NoUpdate,
        Err(_) => PrimaryDecision::FallBack,
    }
}

/// Whether the offered release is critical (contract D): `critical: true` in
/// the manifest, or the installed `current_version` is below `min_supported`.
/// Missing/malformed fields are simply "not critical" (GitHub's latest.json
/// has neither).
pub(crate) fn is_critical_update(raw_json: &serde_json::Value, current_version: &str) -> bool {
    if raw_json
        .get("critical")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return true;
    }
    let Some(min_supported) = raw_json
        .get("min_supported")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    match (
        semver::Version::parse(min_supported.trim().trim_start_matches('v')),
        semver::Version::parse(current_version.trim().trim_start_matches('v')),
    ) {
        (Ok(min), Ok(current)) => current < min,
        _ => {
            tracing::warn!(
                "Updater: cannot compare current {} with min_supported {}",
                current_version,
                min_supported
            );
            false
        }
    }
}

/// Whether to (download and) ask the user about `version`: always for a
/// critical release, otherwise only if it isn't the version they declined.
pub(crate) fn should_prompt(declined: Option<&str>, version: &str, critical: bool) -> bool {
    critical || declined != Some(version)
}

/// Whether `value` is a well-formed UUID v4 (the only install id we send).
fn is_valid_install_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .map(|id| id.get_version_num() == 4)
        .unwrap_or(false)
}

/// Resolve the install id from the persisted store value (pure): a valid
/// UUID v4 is reused as-is; anything else (absent, wrong type, malformed)
/// is replaced by a fresh v4. The bool says whether it must be persisted.
pub(crate) fn resolve_install_id(existing: Option<&serde_json::Value>) -> (String, bool) {
    if let Some(id) = existing.and_then(serde_json::Value::as_str) {
        if is_valid_install_id(id) {
            return (id.to_string(), false);
        }
        tracing::warn!("Updater: persisted install id is malformed, regenerating");
    }
    (uuid::Uuid::new_v4().to_string(), true)
}

/// Read (or create and persist) the anonymous install id from its own key in
/// `settings.json`. `None` only if the store is unavailable, in which case the
/// check simply goes out without the header.
fn get_or_create_install_id(app: &AppHandle) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    let store = match app.store("settings.json") {
        Ok(store) => store,
        Err(e) => {
            tracing::warn!("Updater: store unavailable, no install id: {}", e);
            return None;
        }
    };
    let (id, is_new) = resolve_install_id(store.get(INSTALL_ID_STORE_KEY).as_ref());
    if is_new {
        store.set(INSTALL_ID_STORE_KEY, serde_json::json!(id));
        if let Err(e) = store.save() {
            tracing::warn!("Updater: failed to persist install id: {}", e);
        }
    }
    Some(id)
}

/// Remove the install id header from the headers an `Update` carries over
/// from Updater A: `Update::download` reuses them to fetch the installer,
/// which lives on GitHub — the id must only ever reach our server.
pub(crate) fn strip_install_id_header(headers: &mut reqwest::header::HeaderMap) {
    headers.remove(INSTALL_ID_HEADER);
}

/// Whether `plugins.updater.pubkey` is still the placeholder (or missing).
fn updater_pubkey_is_placeholder(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|pubkey| pubkey.as_str())
        .is_none_or(|pubkey| pubkey == UPDATER_PUBKEY_PLACEHOLDER)
}

/// Result of one check attempt, from the scheduler's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckOutcome {
    /// Ran to completion (whatever the result): wait the full interval.
    Done,
    /// Skipped because downloads are active: retry sooner.
    Deferred,
}

/// RAII release of `AppState::update_check_in_flight`.
struct InFlightGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Run one update check: A, then B only if A failed, then (if an update is
/// offered) download + ask for consent + install + restart.
async fn check_once(app: &AppHandle) -> CheckOutcome {
    use tauri_plugin_updater::UpdaterExt;

    let state = app.state::<AppState>();
    if state
        .update_check_in_flight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        tracing::debug!("Updater: a check is already in flight, skipping");
        return CheckOutcome::Done;
    }
    let _guard = InFlightGuard(&state.update_check_in_flight);

    if updater_pubkey_is_placeholder(app) {
        tracing::info!(
            "Auto-update non configurato (pubkey placeholder): salto il check. Vedi UPDATER_SETUP.md"
        );
        return CheckOutcome::Done;
    }

    // Don't compete with (or restart in the middle of) material downloads.
    let active = state.download_queue.active_count();
    let queued = state.download_queue.queue_len().await;
    if active > 0 || queued > 0 {
        tracing::info!(
            "Updater: {} active / {} queued download(s), deferring the update check",
            active,
            queued
        );
        return CheckOutcome::Deferred;
    }

    // --- Updater A: our endpoint, with the anonymous install id ---------
    let endpoint = update_endpoint_for(&api_base_url());
    let install_id = get_or_create_install_id(app);
    let primary = async {
        let url = endpoint
            .parse::<tauri::Url>()
            .map_err(|e| format!("invalid update endpoint {endpoint}: {e}"))?;
        let mut builder = app
            .updater_builder()
            .clear_headers()
            .endpoints(vec![url])
            .map_err(|e| e.to_string())?
            .timeout(UPDATE_REQUEST_TIMEOUT);
        if let Some(id) = install_id.as_deref() {
            builder = builder
                .header(INSTALL_ID_HEADER, id)
                .map_err(|e| e.to_string())?;
        }
        let updater = builder.build().map_err(|e| e.to_string())?;
        updater.check().await.map_err(|e| e.to_string())
    }
    .await;

    let update = match decide_after_primary(&primary) {
        PrimaryDecision::UseUpdate => primary.ok().flatten(),
        PrimaryDecision::NoUpdate => {
            tracing::debug!("Updater: our endpoint reports no update to install (or a hold)");
            None
        }
        PrimaryDecision::FallBack => {
            if let Err(e) = &primary {
                tracing::warn!(
                    "Updater: our endpoint failed ({}), falling back to GitHub",
                    e
                );
            }
            // --- Updater B: tauri.conf.json endpoints (GitHub), NO header --
            let fallback = async {
                app.updater_builder()
                    .clear_headers()
                    .timeout(UPDATE_REQUEST_TIMEOUT)
                    .build()
                    .map_err(|e| e.to_string())?
                    .check()
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            match fallback {
                Ok(update) => update,
                Err(e) => {
                    // Expected with no network: never fatal.
                    tracing::warn!("Update check failed (fallback too): {}", e);
                    None
                }
            }
        }
    };

    if let Some(mut update) = update {
        strip_install_id_header(&mut update.headers);
        offer_update(app, update).await;
    }
    CheckOutcome::Done
}

/// Download `update` in the background, then ask for explicit consent before
/// installing and restarting. Never installs silently.
async fn offer_update(app: &AppHandle, update: tauri_plugin_updater::Update) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let critical = is_critical_update(&update.raw_json, &update.current_version);
    let declined = declined_update_version(app);
    if !should_prompt(declined.as_deref(), &update.version, critical) {
        tracing::info!(
            "Update {} was previously declined by the user, skipping",
            update.version
        );
        return;
    }

    tracing::info!(
        "Update available: {} -> {} (critical: {}), downloading in background",
        update.current_version,
        update.version,
        critical
    );

    let bytes = match update
        .download(
            |_chunk_size, _content_length| {},
            || tracing::debug!("Update download finished"),
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Update download failed: {}", e);
            return;
        }
    };

    tracing::info!(
        "Update {} downloaded, asking for confirmation before installing",
        update.version
    );

    let (title, message) = if critical {
        (
            "Aggiornamento importante",
            format!(
                "È disponibile un aggiornamento importante: versione {} (attuale: {}). Si consiglia di installarlo subito. L'app verrà riavviata.",
                update.version, update.current_version
            ),
        )
    } else {
        (
            "Aggiornamento disponibile",
            format!(
                "È disponibile la versione {} (attuale: {}). Vuoi installarla ora? L'app verrà riavviata.",
                update.version, update.current_version
            ),
        )
    };

    // `.show()`'s callback runs off this task: bridge it with a oneshot.
    let (confirmed_tx, confirmed_rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title(title)
        .kind(if critical {
            MessageDialogKind::Warning
        } else {
            MessageDialogKind::Info
        })
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Installa e riavvia".to_string(),
            "Più tardi".to_string(),
        ))
        .show(move |confirmed| {
            let _ = confirmed_tx.send(confirmed);
        });

    let confirmed = confirmed_rx.await.unwrap_or(false);
    if !confirmed {
        tracing::info!(
            "Update {} downloaded but installation postponed by the user",
            update.version
        );
        // Recorded even for a critical release: the next check re-prompts
        // anyway (critical ignores the declined version).
        persist_declined_update_version(app, &update.version);
        return;
    }

    tracing::info!("Installing update {} and restarting", update.version);
    if let Err(e) = update.install(bytes) {
        tracing::error!("Failed to install update {}: {}", update.version, e);
        show_update_error_dialog(app, &update.version, &e);
        return;
    }

    app.restart();
}

/// Persist the version the user declined. Best-effort: a store failure only
/// means we may prompt again next time.
fn persist_declined_update_version(app: &AppHandle, version: &str) {
    use tauri_plugin_store::StoreExt;
    let store = match app.store("settings.json") {
        Ok(store) => store,
        Err(e) => {
            tracing::warn!(
                "Updater: failed to access store to record declined version: {}",
                e
            );
            return;
        }
    };
    store.set(UPDATER_DECLINED_VERSION_KEY, serde_json::json!(version));
    if let Err(e) = store.save() {
        tracing::warn!("Updater: failed to persist declined version: {}", e);
    }
}

/// The version the user last declined, if any.
fn declined_update_version(app: &AppHandle) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").ok()?;
    store
        .get(UPDATER_DECLINED_VERSION_KEY)
        .and_then(|value| value.as_str().map(str::to_string))
}

/// Surface an install failure that happened *after* the user consented.
fn show_update_error_dialog(app: &AppHandle, version: &str, error: &tauri_plugin_updater::Error) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    use tauri_plugin_updater::Error;

    let message = match error {
        Error::UnsupportedOs | Error::UnsupportedArch | Error::TargetNotFound(_) => format!(
            "L'aggiornamento automatico non è supportato su questa piattaforma. Scarica e installa manualmente la versione {}.",
            version
        ),
        _ => format!(
            "Installazione dell'aggiornamento {} non riuscita. Riprova più tardi.",
            version
        ),
    };

    app.dialog()
        .message(message)
        .title("Aggiornamento non riuscito")
        .kind(MessageDialogKind::Error)
        .blocking_show();
}

/// Hours to wait between periodic checks, read fresh from the config each
/// cycle (so a changed setting applies from the next wait), at least 1.
fn configured_interval(app: &AppHandle) -> Duration {
    let hours = app
        .state::<AppState>()
        .config
        .read()
        .map(|config| config.update_check_interval_hours)
        .unwrap_or(crate::models::DEFAULT_UPDATE_CHECK_INTERVAL_HOURS)
        .max(1);
    Duration::from_secs(u64::from(hours) * 3600)
}

/// Background scheduler: one check shortly after startup, then one every
/// `update_check_interval_hours` (sooner after a deferral). Stoppable, like
/// `PollingService`/`RetentionScheduler`, from `shutdown_and_exit`.
pub struct UpdateScheduler {
    cancel_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl UpdateScheduler {
    pub fn new() -> Self {
        Self {
            cancel_tx: Mutex::new(None),
        }
    }

    /// Spawn the scheduler task (no-op if already started).
    pub fn start(&self, app: AppHandle) {
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        match self.cancel_tx.lock() {
            Ok(mut guard) => {
                if guard.is_some() {
                    tracing::warn!("Update scheduler already running, ignoring start request");
                    return;
                }
                *guard = Some(cancel_tx);
            }
            Err(_) => {
                tracing::error!("Update scheduler lock poisoned, not starting");
                return;
            }
        }

        tauri::async_runtime::spawn(async move {
            let mut wait = STARTUP_DELAY;
            loop {
                tokio::select! {
                    _ = sleep(wait) => {}
                    _ = cancel_rx.changed() => break,
                }
                wait = match check_once(&app).await {
                    CheckOutcome::Deferred => BUSY_RETRY_DELAY,
                    CheckOutcome::Done => configured_interval(&app),
                };
                tracing::debug!("Next update check in {}s", wait.as_secs());
            }
            tracing::info!("Update scheduler stopped");
        });
    }

    /// Stop the scheduler task (idempotent).
    pub fn stop(&self) {
        if let Ok(mut guard) = self.cancel_tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(true);
            }
        }
    }
}

impl Default for UpdateScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_points_at_the_given_api_base() {
        assert_eq!(
            update_endpoint_for("https://api.adventistyouth.it"),
            "https://api.adventistyouth.it/api/desktop/update/{{target}}/{{arch}}/{{current_version}}"
        );
        // Debug override (stub) and a trailing slash.
        assert_eq!(
            update_endpoint_for("http://127.0.0.1:8787/"),
            "http://127.0.0.1:8787/api/desktop/update/{{target}}/{{arch}}/{{current_version}}"
        );
        // The plugin substitutes the placeholders even after URL encoding.
        assert!(update_endpoint_for("http://127.0.0.1:8787")
            .parse::<tauri::Url>()
            .is_ok());
    }

    /// Contract D decision table, expressed on the plugin's `check()` result
    /// for each status of our endpoint.
    #[test]
    fn fallback_decision_table() {
        // 200 with a newer version.
        let newer: Result<Option<&str>, &str> = Ok(Some("0.4.0"));
        assert_eq!(decide_after_primary(&newer), PrimaryDecision::UseUpdate);
        // 200 with the same version (client compares) or 204 hold.
        let none: Result<Option<&str>, &str> = Ok(None);
        assert_eq!(decide_after_primary(&none), PrimaryDecision::NoUpdate);
        // 503 / any non-2xx (plugin: ReleaseNotFound), network error, timeout.
        for err in ["ReleaseNotFound (503)", "connection refused", "timed out"] {
            let failed: Result<Option<&str>, &str> = Err(err);
            assert_eq!(
                decide_after_primary(&failed),
                PrimaryDecision::FallBack,
                "{err}"
            );
        }
    }

    #[test]
    fn critical_flag_and_min_supported() {
        assert!(is_critical_update(
            &json!({"version": "0.4.0", "critical": true}),
            "0.4.0"
        ));
        assert!(!is_critical_update(
            &json!({"version": "0.4.0", "critical": false}),
            "0.3.0"
        ));
        // GitHub latest.json: no extra fields.
        assert!(!is_critical_update(&json!({"version": "0.4.0"}), "0.3.0"));
        // Installed below min_supported -> critical even with critical=false.
        assert!(is_critical_update(
            &json!({"critical": false, "min_supported": "0.4.0"}),
            "0.3.0"
        ));
        assert!(!is_critical_update(
            &json!({"min_supported": "0.3.0"}),
            "0.3.0"
        ));
        assert!(!is_critical_update(
            &json!({"min_supported": null}),
            "0.3.0"
        ));
        // Malformed values are not critical (and never panic).
        assert!(!is_critical_update(
            &json!({"min_supported": "boh"}),
            "0.3.0"
        ));
        assert!(!is_critical_update(&json!({"critical": "yes"}), "0.3.0"));
        // A "v" prefix is tolerated.
        assert!(is_critical_update(
            &json!({"min_supported": "v0.4.0"}),
            "0.3.9"
        ));
    }

    #[test]
    fn critical_release_re_prompts_a_declined_version() {
        assert!(!should_prompt(Some("0.4.0"), "0.4.0", false));
        assert!(should_prompt(Some("0.4.0"), "0.4.0", true));
        assert!(should_prompt(Some("0.3.5"), "0.4.0", false));
        assert!(should_prompt(None, "0.4.0", false));
    }

    #[test]
    fn install_id_is_created_once_then_reused() {
        // First run: nothing persisted -> new v4, must be persisted.
        let (first, persist) = resolve_install_id(None);
        assert!(persist);
        assert!(is_valid_install_id(&first));

        // Simulate the store round trip: the same id comes back and is reused
        // without being re-persisted.
        let stored = json!(first.clone());
        let (second, persist_again) = resolve_install_id(Some(&stored));
        assert_eq!(second, first);
        assert!(!persist_again);

        // Corrupt / wrong-typed / non-v4 values are replaced.
        for bad in [
            json!("not-a-uuid"),
            json!(42),
            json!("00000000-0000-1000-8000-000000000000"), // v1
        ] {
            let (fresh, persist) = resolve_install_id(Some(&bad));
            assert!(persist, "{bad} must be regenerated");
            assert!(is_valid_install_id(&fresh));
        }
    }

    #[test]
    fn install_id_header_is_stripped_before_the_installer_download() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            INSTALL_ID_HEADER,
            "6f1c0e0e-9a3b-4c55-8a0a-1b2c3d4e5f60".parse().unwrap(),
        );
        headers.insert(reqwest::header::ACCEPT, "application/json".parse().unwrap());
        strip_install_id_header(&mut headers);
        assert!(headers.get(INSTALL_ID_HEADER).is_none());
        assert!(headers.get("x-install-id").is_none());
        assert!(headers.get(reqwest::header::ACCEPT).is_some());
    }
}
