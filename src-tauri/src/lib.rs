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
pub use error::AppError;
pub use models::{AppConfig, Resource, WeekIdentifier};
pub use services::{PollingService, RetentionScheduler};

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

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
            let store = app.store("settings.json")?;

            // Try to load existing config
            let mut config = AppConfig::default();
            if let Some(json) = store.get("config") {
                if let Ok(loaded_config) = serde_json::from_value(json.clone()) {
                    tracing::info!("Loaded configuration from store");
                    config = loaded_config;
                }
            } else {
                // Save default config if not exists
                tracing::info!("Initializing default configuration");
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
                    if let Some(resource) = cached_resources.first() {
                        status.current_week = Some(resource.week());
                    }
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
            setup_tray(app.handle())?;

            // Show the main window, unless this launch was triggered by the
            // OS autostart entry (see the `--autostart` flag registered with
            // the autostart plugin above): in that case stay hidden in the
            // tray, coordinating with bl-desktop-autostart so the app doesn't
            // pop up a window at every boot.
            let launched_via_autostart = std::env::args().any(|arg| arg == "--autostart");
            if launched_via_autostart {
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
            {
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    tracing::error!("Failed to hide window on close request: {}", e);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_config,
            commands::get_status,
            commands::get_resources,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
        return;
    }

    tracing::info!("Installing update {} and restarting", update.version);
    if let Err(e) = update.install(bytes) {
        tracing::error!("Failed to install update {}: {}", update.version, e);
        return;
    }

    app.restart();
}
