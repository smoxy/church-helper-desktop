//! Church Helper Desktop - Tauri Application
//!
//! A cross-platform desktop application for managing weekly church resources.

pub mod commands;
pub mod error;
pub mod models;
pub mod services;

// Re-export commonly used types
pub use commands::AppState;
pub use error::AppError;
pub use models::{AppConfig, Resource, WeekIdentifier};
pub use services::PollingService;

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
                let json = serde_json::to_value(&config).expect("Failed to serialize default config");
                store.set("config", json);
                store.save()?;
            }

            // Set config in state
            *app_state.config.write().unwrap() = config.clone();
            
            // Sync status with config
            app_state.status.write().unwrap().polling_active = config.polling_enabled;

            // Try to load cached resources
            let cache_store = app.store("cache.json")?;
            if let Some(json) = cache_store.get("resources") {
                if let Ok(cached_resources) = serde_json::from_value::<Vec<Resource>>(json.clone()) {
                    *app_state.resources.write().unwrap() = cached_resources.clone();
                    tracing::info!("Loaded {} cached resources", cached_resources.len());
                    
                    // Update status with cached data
                    let mut status = app_state.status.write().unwrap();
                    status.total_resources = cached_resources.len();
                    if let Some(resource) = cached_resources.first() {
                         status.current_week = Some(resource.week());
                    }
                }
            }

            app.manage(app_state);

            tracing::info!("Church Helper Desktop initialized");

            // Auto-start polling if enabled
            if config.polling_enabled {
                let polling_service = PollingService::new();
                polling_service.start(app.handle().clone(), config.polling_interval_minutes);
            }

            Ok(())
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
            commands::get_archived_weeks,
            commands::is_resource_youtube,
            commands::download_resource,
            commands::pause_download,
            commands::cancel_download,
            commands::check_resource_status,
            commands::get_file_size,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
