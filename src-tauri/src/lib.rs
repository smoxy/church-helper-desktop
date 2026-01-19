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
            app.manage(AppState::default());

            tracing::info!("Church Helper Desktop initialized");

            // TODO: Load config from store and start polling if enabled
            // let state = app.state::<AppState>();
            // let config = state.config.read().unwrap();
            // if config.polling_enabled {
            //     let polling_service = PollingService::new();
            //     polling_service.start(app.handle().clone(), config.polling_interval_minutes);
            // }

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
