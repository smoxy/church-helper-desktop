//! Church Helper Desktop - Tauri Application
//!
//! A cross-platform desktop application for managing weekly church resources.

pub mod commands;
pub mod constants;
pub mod error;
pub mod models;
pub mod services;

// Re-export commonly used types
pub use commands::AppState;
pub use error::{AppError, CommandError};
pub use models::{AppConfig, Resource, WeekIdentifier};
pub use services::{PollingService, RetentionScheduler};

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing for logging. Honor RUST_LOG when set (e.g.
    // `church_helper_desktop_lib=debug`), defaulting to `info` otherwise.
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // Flag appended to the command the OS uses to auto-launch the
            // app (Windows registry autorun value / Linux .desktop `Exec=`).
            // Checked at setup below to start hidden in the tray instead of
            // showing the main window (bl-desktop-close-to-tray).
            Some(vec!["--autostart"]),
        ))
        .setup(|app| {
            // Initialize application state
            let app_state = AppState::default();

            // Load config from store
            use tauri_plugin_store::StoreExt;

            // A settings.json whose raw bytes aren't valid JSON is silently
            // discarded by tauri-plugin-store on load: `get("config")` then
            // returns None and the defaults below would overwrite the file,
            // destroying the user's data without a trace. Detect that up front
            // (before the store swallows it) and back the file up. The
            // valid-JSON-but-unparseable `config` case is handled in the Err
            // arm below, once the store is open.
            if raw_settings_is_corrupt(app.handle()) {
                tracing::error!(
                    "settings.json is not valid JSON; backing it up before it is reset to defaults"
                );
                backup_corrupt_config(app.handle());
            }

            let store = app.store("settings.json")?;

            // Load the persisted config, tracking whether valid defaults must be
            // (re)written so both "no config yet" and "corrupt/unparseable
            // config" leave a valid file behind. Without that rewrite a bad file
            // would be re-detected and re-backed-up on every launch, piling up
            // `.bak-<ts>` copies.
            let mut config = AppConfig::default();
            let mut write_defaults = false;
            match store.get("config") {
                Some(json) => match serde_json::from_value::<AppConfig>(json.clone()) {
                    Ok(loaded_config) => {
                        tracing::info!("Loaded configuration from store");
                        config = loaded_config;
                    }
                    // Valid JSON but an incompatible `config` schema (much
                    // older/newer build): back the raw file up, then fall
                    // through to rewriting defaults so it doesn't recur.
                    Err(e) => {
                        tracing::error!(
                            "Failed to parse persisted configuration, backing up and resetting to defaults: {}",
                            e
                        );
                        backup_corrupt_config(app.handle());
                        write_defaults = true;
                    }
                },
                None => {
                    tracing::info!("Initializing default configuration");
                    write_defaults = true;
                }
            }
            if write_defaults {
                let json =
                    serde_json::to_value(&config).expect("Failed to serialize default config");
                store.set("config", json);
                store.save()?;
            }

            // Set config in state
            *app_state
                .config
                .write()
                .map_err(|e| format!("Failed to write initial config: {}", e))? = config.clone();

            // Sync status with config
            app_state
                .status
                .write()
                .map_err(|e| format!("Failed to write initial status: {}", e))?
                .polling_active = config.polling_enabled;

            // Sync the OS-level autostart entry with the saved preference.
            // The two can drift apart outside of our control (reinstall, OS
            // reset, user manually removing the registry/XDG autostart
            // entry), so reconcile on every startup instead of trusting the
            // config blindly.
            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart_manager = app.autolaunch();
                let currently_enabled = autostart_manager.is_enabled().unwrap_or(false);
                if config.autostart_enabled && !currently_enabled {
                    if let Err(e) = autostart_manager.enable() {
                        tracing::error!("Failed to sync autostart (enable): {}", e);
                    } else {
                        tracing::info!("Autostart enabled to match saved configuration");
                    }
                } else if !config.autostart_enabled && currently_enabled {
                    if let Err(e) = autostart_manager.disable() {
                        tracing::error!("Failed to sync autostart (disable): {}", e);
                    } else {
                        tracing::info!("Autostart disabled to match saved configuration");
                    }
                }
            }

            // Try to load cached resources
            let cache_store = app.store("cache.json")?;
            if let Some(json) = cache_store.get("resources") {
                if let Ok(cached_resources) = serde_json::from_value::<Vec<Resource>>(json.clone())
                {
                    *app_state
                        .resources
                        .write()
                        .map_err(|e| format!("Failed to write cached resources: {}", e))? =
                        cached_resources.clone();
                    tracing::info!("Loaded {} cached resources", cached_resources.len());

                    // Update status with cached data
                    let mut status = app_state
                        .status
                        .write()
                        .map_err(|e| format!("Failed to write status: {}", e))?;
                    status.total_resources = cached_resources.len();
                    if let Some(week) = models::latest_week(&cached_resources) {
                        status.current_week = Some(week);
                    }
                    status.material_week_stale =
                        models::is_material_week_stale(status.current_week.as_ref());
                }
            }

            // Try to load cached file sizes
            if let Some(json) = cache_store.get("file_size_cache") {
                if let Ok(cached_sizes) =
                    serde_json::from_value::<std::collections::HashMap<String, u64>>(json.clone())
                {
                    *app_state
                        .file_size_cache
                        .write()
                        .map_err(|e| format!("Failed to write cached file sizes: {}", e))? =
                        cached_sizes;
                    let cached_file_sizes_len = app_state
                        .file_size_cache
                        .read()
                        .map_err(|e| format!("Failed to read cached file sizes: {}", e))?
                        .len();
                    tracing::info!("Loaded {} cached file sizes", cached_file_sizes_len);
                }
            }

            // Try to load the errata registry (downloaded_files). Absent or
            // malformed → empty registry, never a startup error: a corrupt or
            // missing registry must not stop the app from launching.
            if let Some(json) = cache_store.get("downloaded_files") {
                match serde_json::from_value::<Vec<models::DownloadedFile>>(json.clone()) {
                    Ok(files) => {
                        let count = files.len();
                        *app_state
                            .downloaded_files
                            .write()
                            .map_err(|e| format!("Failed to write downloaded_files: {}", e))? =
                            files;
                        tracing::info!("Loaded {} downloaded-file registry entries", count);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse downloaded_files registry, starting empty: {}",
                            e
                        );
                    }
                }
            }

            // Reconcile has_superseded_files against the freshly loaded registry
            // so a supersession recorded in a previous session is reflected in
            // the status at startup, using the same week the status derives from
            // latest_week (set above) rather than the wall-clock week.
            {
                let current_week = app_state
                    .status
                    .read()
                    .map_err(|e| format!("Failed to read status: {}", e))?
                    .current_week
                    .clone();
                let has_superseded = {
                    let registry = app_state
                        .downloaded_files
                        .read()
                        .map_err(|e| format!("Failed to read downloaded_files: {}", e))?;
                    services::errata::compute_has_superseded(&registry, current_week.as_ref())
                };
                app_state
                    .status
                    .write()
                    .map_err(|e| format!("Failed to write status: {}", e))?
                    .has_superseded_files = has_superseded;
            }

            app.manage(app_state);

            tracing::info!("Church Helper Desktop initialized");

            // Check for auto-downloads of cached resources at startup
            if config.work_directory.is_some() && !config.auto_download_categories.is_empty() {
                let app_handle = app.handle().clone();
                tracing::debug!("Scanning cached resources for auto-download at startup");
                tauri::async_runtime::spawn(async move {
                    // Delay to give frontend time to register event listeners
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    tracing::debug!(
                        "Starting auto-download scan after frontend initialization delay"
                    );
                    let state = app_handle.state::<AppState>();
                    state
                        .download_queue
                        .scan_and_queue(app_handle.clone())
                        .await;
                });
            }

            // Auto-start polling if enabled. The handle is stored in
            // AppState (below) so it can be stopped cleanly when the user
            // exits from the tray menu instead of leaking an unstoppable
            // background task (bl-desktop-close-to-tray).
            let polling_service = if config.polling_enabled {
                let service = PollingService::new();
                service.start(app.handle().clone(), config.polling_interval_minutes);
                Some(service)
            } else {
                None
            };

            // Enforce the retention policy once at startup and then daily.
            // Independent of `polling_enabled`: retention is local disk
            // hygiene (archived weeks older than `retention_days` moved to
            // the system trash), not tied to remote polling being on.
            let retention_scheduler = RetentionScheduler::new();
            retention_scheduler.start(app.handle().clone());

            {
                let state = app.state::<AppState>();
                *state
                    .polling_service
                    .write()
                    .map_err(|e| format!("Failed to store polling service handle: {}", e))? =
                    polling_service;
                *state
                    .retention_scheduler
                    .write()
                    .map_err(|e| format!("Failed to store retention scheduler handle: {}", e))? =
                    Some(retention_scheduler);
            }

            // Set up the system tray (Apri / Esci) and make closing the
            // window hide it instead of terminating the process, so the
            // download queue (adr-0007) and background schedulers keep
            // running (bl-desktop-close-to-tray).
            //
            // Guarded against panics: on some Linux setups the underlying
            // tray-icon stack can panic (rather than return a clean `Err`)
            // when libappindicator3/libayatana-appindicator3 isn't
            // installed (see `setup_tray`'s doc comment below). Catching
            // that here degrades to "no tray icon" — logged as a warning —
            // instead of the whole app failing to start.
            let tray_available = match catch_unwind(AssertUnwindSafe(|| setup_tray(app.handle())))
            {
                Ok(Ok(())) => true,
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Tray icon setup failed, continuing without one (window close will exit the app instead of hiding it): {}",
                        e
                    );
                    false
                }
                Err(_) => {
                    tracing::warn!(
                        "Tray icon setup panicked (likely missing libappindicator3/libayatana-appindicator3 on this Linux system). Continuing without a tray icon: window close will exit the app instead of hiding it. Install your desktop environment's AppIndicator support to enable it."
                    );
                    false
                }
            };
            app.state::<AppState>()
                .tray_available
                .store(tray_available, Ordering::SeqCst);

            // Show the main window, unless this launch was triggered by the
            // OS autostart entry (see the `--autostart` flag registered with
            // the autostart plugin above): in that case stay hidden in the
            // tray, coordinating with bl-desktop-autostart so the app doesn't
            // pop up a window at every boot. Exception: without a tray there
            // is no "Apri" to reopen from, so show the window anyway rather
            // than stranding the user with an invisible, unreachable app.
            let launched_via_autostart = std::env::args().any(|arg| arg == "--autostart");
            if launched_via_autostart && tray_available {
                tracing::info!("Launched via autostart: starting hidden in the tray");
            } else if let Some(window) = app.get_webview_window("main") {
                window
                    .show()
                    .map_err(|e| format!("Failed to show main window: {}", e))?;
            }

            // Check for app updates in the background: download automatically
            // if one is found, then ask for explicit confirmation before
            // installing and restarting. Never installs silently (policy
            // from bl-desktop-autoupdate: notify + explicit consent only).
            if constants::is_api_base_overridden() {
                // Local test session against a stub backend: do not contact
                // the GitHub update endpoint (avoids noisy expected failures
                // and keeps test runs fully local).
                tracing::info!("API base override active: skipping update check");
            } else {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Let the rest of startup (initial poll, retention, tray,
                    // window) settle first.
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    check_for_updates(app_handle).await;
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window (X button, Alt+F4, ...) must not terminate
            // the process: it would kill the download queue (adr-0007) and
            // the background schedulers mid-flight. Hide it instead; the
            // only real exit path is the tray menu's "Esci"
            // (bl-desktop-close-to-tray).
            //
            // Exception: if the tray icon isn't available (see
            // `tray_available`, set at setup from `setup_tray`'s result),
            // there is no "Esci" to fall back on, so closing the window
            // must behave like a normal close (the process exits) instead
            // of hiding to a tray the user could never reopen.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let tray_available = window
                    .try_state::<AppState>()
                    .map(|state| state.tray_available.load(Ordering::SeqCst))
                    .unwrap_or(false);

                if !tray_available {
                    tracing::debug!("No tray icon available: allowing the window to close normally");
                    return;
                }

                api.prevent_close();
                if let Err(e) = window.hide() {
                    tracing::error!("Failed to hide window on close request: {}", e);
                } else {
                    maybe_notify_first_tray_close(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_config,
            commands::get_status,
            commands::get_resources,
            commands::get_all_categories,
            commands::force_poll,
            commands::select_work_directory,
            commands::set_work_directory,
            commands::set_polling_enabled,
            commands::set_polling_interval,
            commands::set_retention_days,
            commands::set_autostart_enabled,
            commands::get_archived_weeks,
            commands::is_resource_youtube,
            commands::download_resource,
            commands::pause_download,
            commands::cancel_download,
            commands::check_resource_status,
            commands::get_file_size,
            commands::get_resource_summary,
            commands::get_resources_status,
            commands::reveal_resource,
            commands::open_work_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Best-effort backup of the settings store whose persisted `config` failed
/// to deserialize, written next to the store as `settings.json.bak-<unix-ts>`
/// before the app proceeds with defaults, so a corrupt or
/// incompatible-schema config is preserved for inspection/recovery instead of
/// vanishing. Copies the whole on-disk store (not just the `config` key) so
/// other persisted keys survive too. Any failure here is logged and ignored:
/// it must never block startup.
/// Whether `raw` parses as arbitrary JSON. Pure, so the corrupt-config
/// detection is unit-testable without a store on disk.
fn is_valid_json(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw).is_ok()
}

/// Whether settings.json exists but its raw bytes are not valid JSON.
/// tauri-plugin-store silently discards such a file on load (yielding an empty
/// store), so this is checked *before* the store is opened, to preserve the
/// file via `backup_corrupt_config`. A missing file (first run) or an
/// unreadable one is not "corrupt" — there is nothing to back up.
fn raw_settings_is_corrupt(app: &tauri::AppHandle) -> bool {
    let Ok(path) = tauri_plugin_store::resolve_store_path(app, "settings.json") else {
        return false;
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => !is_valid_json(&raw),
        Err(_) => false,
    }
}

fn backup_corrupt_config(app: &tauri::AppHandle) {
    let store_path = match tauri_plugin_store::resolve_store_path(app, "settings.json") {
        Ok(path) => path,
        Err(e) => {
            tracing::error!(
                "Could not resolve settings store path to back up corrupt config: {}",
                e
            );
            return;
        }
    };

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let backup_name = format!(
        "{}.bak-{}",
        store_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings.json"),
        timestamp
    );
    let backup_path = store_path.with_file_name(backup_name);

    match std::fs::copy(&store_path, &backup_path) {
        Ok(_) => tracing::info!(
            "Backed up corrupt configuration to {}",
            backup_path.display()
        ),
        Err(e) => tracing::error!(
            "Failed to back up corrupt configuration to {}: {}",
            backup_path.display(),
            e
        ),
    }
}

/// Build the system tray icon with a minimal "Apri" (show) / "Esci" (quit)
/// menu.
///
/// Real exit only happens via "Esci" (`shutdown_and_exit`); closing the
/// window just hides it (see the `on_window_event` handler on the
/// `Builder`).
///
/// ## Linux note
/// GNOME does not natively support the tray icon protocol used here
/// (AppIndicator/StatusNotifierItem, via the `tray-icon`/`libappindicator3`
/// stack) — the icon simply won't be visible there unless the user installs
/// the "AppIndicator and KStatusNotifierItem Support" GNOME Shell extension.
/// This is a GNOME ecosystem limitation, not specific to this app; the tray
/// works out of the box on Windows and on most other Linux desktop
/// environments (KDE Plasma, XFCE, Cinnamon, MATE, ...).
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{TrayIconBuilder, TrayIconEvent};

    let show_item = MenuItem::with_id(app, "show", "Apri", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Church Helper Desktop")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(shutdown_and_exit(app_handle));
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // `DoubleClick` is Windows-only per Tauri's docs; on Linux,
            // restoring the window goes through the "Apri" menu item instead
            // (that platform's indicator protocol is menu-driven).
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_main_window(tray.app_handle());
            }
        });

    match app.default_window_icon() {
        Some(icon) => tray_builder = tray_builder.icon(icon.clone()),
        None => tracing::warn!("No default window icon available for the tray icon"),
    }

    tray_builder.build(app)?;

    Ok(())
}

/// Show and focus the main window (tray "Apri" menu item / double-click).
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.show() {
            tracing::error!("Failed to show main window from tray: {}", e);
        }
        if let Err(e) = window.set_focus() {
            tracing::error!("Failed to focus main window from tray: {}", e);
        }
    } else {
        tracing::warn!("Tray 'show' requested but the main window doesn't exist");
    }
}

/// Shows a one-time notice the first time the window is closed to the tray
/// rather than exiting, so the user isn't left wondering where the app
/// went (2B review follow-up on bl-desktop-close-to-tray). Delivered as an
/// OS notification (the window is already hidden, so an in-app toast would
/// never be seen), then persists a flag in config so it never fires again,
/// even across restarts. A no-op (besides logging) on any failure to
/// notify or to read/write that flag — never worth blocking the close/hide
/// that already happened.
fn maybe_notify_first_tray_close(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let state = app.state::<AppState>();

    let already_shown = match state.config.read() {
        Ok(config) => config.tray_close_os_notice_shown,
        Err(e) => {
            tracing::error!("Tray close notice: failed to read config: {}", e);
            return;
        }
    };
    if already_shown {
        return;
    }

    // Only burn the one-time flag once the notification was actually handed
    // off successfully: on failure, leave it unset so we retry on the next
    // close instead of silently swallowing the user's only heads-up.
    if let Err(e) = app
        .notification()
        .builder()
        .title("Church Helper è ancora attivo")
        .body("L'app continua a funzionare nell'area di notifica. Usa \"Esci\" dal menu dell'icona per chiuderla del tutto.")
        .show()
    {
        tracing::warn!("Failed to show tray-close OS notification: {}", e);
        return;
    }

    let mut config = match state.config.write() {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Tray close notice: failed to write config: {}", e);
            return;
        }
    };
    config.tray_close_os_notice_shown = true;

    use tauri_plugin_store::StoreExt;
    let store = match app.store("settings.json") {
        Ok(store) => store,
        Err(e) => {
            tracing::error!("Tray close notice: failed to access store: {}", e);
            return;
        }
    };
    match serde_json::to_value(&*config) {
        Ok(json) => {
            store.set("config", json);
            if let Err(e) = store.save() {
                tracing::error!("Tray close notice: failed to persist flag: {}", e);
            }
        }
        Err(e) => tracing::error!("Tray close notice: failed to serialize config: {}", e),
    }
}

/// Clean shutdown triggered from the tray menu's "Esci": stop the polling
/// and retention background schedulers, pause (not cancel) any active
/// downloads so their `.part` file is kept for resume instead of being
/// deleted or torn mid-write (see `services/download.rs`: `STATUS_PAUSED`
/// keeps the `.part` file, `STATUS_CANCELLED` deletes it), give in-flight
/// tasks a brief moment to observe the signals, then actually terminate the
/// process.
///
/// This is the ONLY path that really exits the app — closing the window via
/// the `CloseRequested` handler just hides it.
async fn shutdown_and_exit(app: tauri::AppHandle) {
    tracing::info!("Exit requested from tray menu (Esci), shutting down cleanly");
    let state = app.state::<AppState>();

    if let Ok(mut guard) = state.polling_service.write() {
        if let Some(service) = guard.take() {
            service.stop();
        }
    }
    if let Ok(mut guard) = state.retention_scheduler.write() {
        if let Some(scheduler) = guard.take() {
            scheduler.stop();
        }
    }

    // adr-0007: the queue is the only path for downloads, so this is the one
    // place that needs a clean stop before the process is torn down.
    if let Ok(signals) = state.download_signals.read() {
        if !signals.is_empty() {
            tracing::info!("Pausing {} active download(s) before exit", signals.len());
        }
        for signal in signals.values() {
            signal.store(
                crate::services::download::STATUS_PAUSED,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }

    // Give spawned tasks a brief moment to observe the stop/pause signals
    // (polling/retention loops check on their next `select!` iteration; an
    // active download observes the pause signal on its next received HTTP
    // chunk) before the process is actually torn down.
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    tracing::info!("Clean shutdown complete, exiting");
    app.exit(0);
}

/// Check for a new app version against the configured updater endpoint
/// (`plugins.updater.endpoints` in `tauri.conf.json`, currently the GitHub
/// Release `latest.json` manifest). If one is available, download it in the
/// background, then show a confirmation dialog before installing and
/// restarting — the user always has the final say, there is no silent
/// install (bl-desktop-autoupdate).
///
/// No-ops quietly (debug/warn log only) if the check or download fails, e.g.
/// no network connectivity: this must never crash or block startup.
async fn check_for_updates(app: tauri::AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    use tauri_plugin_updater::UpdaterExt;

    if updater_pubkey_is_placeholder(&app) {
        tracing::info!(
            "Auto-update non configurato (pubkey placeholder): salto il check. Vedi UPDATER_SETUP.md"
        );
        return;
    }

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(e) => {
            tracing::warn!("Updater not available: {}", e);
            return;
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            tracing::debug!("No update available");
            return;
        }
        Err(e) => {
            // Expected in the common case: no network, or the endpoint isn't
            // reachable/configured yet (placeholder pubkey, see
            // UPDATER_SETUP.md). Never fatal.
            tracing::warn!("Update check failed: {}", e);
            return;
        }
    };

    // The user already dismissed this exact version with "Più tardi": don't
    // re-download it or nag again on every launch. A newer version won't
    // match the persisted string and will prompt as usual.
    if update_version_declined(&app, &update.version) {
        tracing::info!(
            "Update {} was previously declined by the user, skipping",
            update.version
        );
        return;
    }

    tracing::info!(
        "Update available: {} -> {}, downloading in background",
        update.current_version,
        update.version
    );

    let download_result = update
        .download(
            |_chunk_size, _content_length| {},
            || tracing::debug!("Update download finished"),
        )
        .await;

    let bytes = match download_result {
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

    // Explicit consent required before touching anything on disk: ask via a
    // native dialog (tauri-plugin-dialog, already used elsewhere for the
    // folder picker). `.show()`'s callback runs off the calling task, so
    // bridge it back into this async function with a oneshot channel.
    let (confirmed_tx, confirmed_rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(format!(
            "È disponibile la versione {} (attuale: {}). Vuoi installarla ora? L'app verrà riavviata.",
            update.version, update.current_version
        ))
        .title("Aggiornamento disponibile")
        .kind(MessageDialogKind::Info)
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
        persist_declined_update_version(&app, &update.version);
        return;
    }

    tracing::info!("Installing update {} and restarting", update.version);
    if let Err(e) = update.install(bytes) {
        tracing::error!("Failed to install update {}: {}", update.version, e);
        show_update_error_dialog(&app, &update.version, &e);
        return;
    }

    app.restart();
}

/// Sentinel value shipped in `tauri.conf.json` until the real signing public
/// key is generated (see UPDATER_SETUP.md).
const UPDATER_PUBKEY_PLACEHOLDER: &str = "PLACEHOLDER_REPLACE_WITH_TAURI_SIGNER_PUBLIC_KEY";

/// Whether `plugins.updater.pubkey` is still the placeholder (or missing):
/// in that state the endpoint has no signed `latest.json` yet, so checking
/// would only produce noisy plugin-level errors.
fn updater_pubkey_is_placeholder(app: &tauri::AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|pubkey| pubkey.as_str())
        .is_none_or(|pubkey| pubkey == UPDATER_PUBKEY_PLACEHOLDER)
}

/// tauri-plugin-store key (in settings.json) holding the last update version
/// the user dismissed with "Più tardi", so it isn't re-offered every launch.
const UPDATER_DECLINED_VERSION_KEY: &str = "updater_declined_version";

/// Persist the version the user declined via the update dialog. Best-effort:
/// a store failure only means we may prompt again next launch.
fn persist_declined_update_version(app: &tauri::AppHandle, version: &str) {
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

/// Whether the user already declined this exact version.
fn update_version_declined(app: &tauri::AppHandle, version: &str) -> bool {
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store("settings.json") else {
        return false;
    };
    store
        .get(UPDATER_DECLINED_VERSION_KEY)
        .and_then(|value| value.as_str().map(str::to_string))
        .is_some_and(|declined| declined == version)
}

/// Surface an install failure that happened *after* the user consented, so it
/// doesn't fail silently. Distinguishes the platform-not-supported case
/// (nothing the user can retry) from a transient failure.
fn show_update_error_dialog(
    app: &tauri::AppHandle,
    version: &str,
    error: &tauri_plugin_updater::Error,
) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_json_is_accepted() {
        assert!(is_valid_json(r#"{"config":{"polling_enabled":true}}"#));
        assert!(is_valid_json("{}"));
        assert!(is_valid_json("[1,2,3]"));
    }

    #[test]
    fn corrupt_json_is_rejected() {
        // The exact shapes tauri-plugin-store silently swallows on load: a
        // truncated/malformed object, an empty file, and a stray HTML page.
        assert!(!is_valid_json("{ this is : not json"));
        assert!(!is_valid_json(""));
        assert!(!is_valid_json("<html>504</html>"));
    }
}
