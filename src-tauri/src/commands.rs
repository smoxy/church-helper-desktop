//! Tauri commands for IPC communication with the frontend
//!
//! These commands implement the "Dumb UI, Smart Backend" architecture.

use crate::models::{AppConfig, AppStatus, Resource, ResourceListResponse, WeekIdentifier};
use crate::services::download::{STATUS_CANCELLED, STATUS_PAUSED};
use crate::services::DownloadQueue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, State};

/// Application state managed by Tauri
pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub current_week: RwLock<Option<WeekIdentifier>>,
    pub resources: RwLock<Vec<Resource>>,
    pub status: RwLock<AppStatus>,
    /// Signals to control active downloads (Pause/Cancel)
    pub download_signals: RwLock<HashMap<i64, Arc<AtomicU8>>>,
    /// Download queue service
    pub download_queue: Arc<DownloadQueue>,
}

/// Response for download command
#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadResponse {
    pub path: String,
    pub hash: String,
}

/// Open a native folder picker dialog
#[tauri::command]
pub async fn select_work_directory(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let path = app.dialog().file().blocking_pick_folder();

    Ok(path.map(|p| p.to_string()))
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: RwLock::new(AppConfig::default()),
            current_week: RwLock::new(None),
            resources: RwLock::new(Vec::new()),
            status: RwLock::new(AppStatus::default()),
            download_signals: RwLock::new(HashMap::new()),
            download_queue: Arc::new(DownloadQueue::new()),
        }
    }
}

/// API base URL
const API_BASE_URL: &str = "https://api.adventistyouth.it";

/// Get the current configuration
#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state
        .config
        .read()
        .map_err(|e| format!("Failed to read config: {}", e))?;
    Ok(config.clone())
}

/// Update the configuration
/// Update the configuration
/// Update the configuration
#[tauri::command]
pub async fn set_config(state: State<'_, AppState>, app: AppHandle, config: AppConfig) -> Result<(), String> {
    // Validate before saving
    config
        .validate()
        .map_err(|e| format!("Invalid config: {:?}", e))?;

    // Save to store
    use tauri_plugin_store::StoreExt;
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;
    
    let json = serde_json::to_value(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    store.set("config", json);
    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    // Update state
    {
        let mut current = state
            .config
            .write()
            .map_err(|e| format!("Failed to write config: {}", e))?;
        *current = config.clone();
    }

    // Trigger queue updates
    state.download_queue.update_mode(config.download_mode).await;
    state.download_queue.scan_and_queue(app).await;

    Ok(())
}

/// Get the current application status
#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Result<AppStatus, String> {
    let status = state
        .status
        .read()
        .map_err(|e| format!("Failed to read status: {}", e))?;
    Ok(status.clone())
}

/// Get the currently loaded resources
#[tauri::command]
pub fn get_resources(state: State<'_, AppState>) -> Result<Vec<Resource>, String> {
    let resources = state
        .resources
        .read()
        .map_err(|e| format!("Failed to read resources: {}", e))?;
    Ok(resources.clone())
}

/// Trigger an immediate poll of the API
#[tauri::command]
pub async fn force_poll(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ResourceListResponse, String> {
    // Fetch from API
    let client = reqwest::Client::new();
    let url = format!("{}/api/resources/latest-week", API_BASE_URL);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    let api_response: ResourceListResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Update state
    {
        let mut resources = state
            .resources
            .write()
            .map_err(|e| format!("Failed to update resources: {}", e))?;
        *resources = api_response.resources.clone();
    }

    // Update status
    {
        let mut status = state
            .status
            .write()
            .map_err(|e| format!("Failed to update status: {}", e))?;
        status.last_poll_time = Some(chrono::Utc::now());
        status.total_resources = api_response.resources.len();

        // Determine current week from resources
        if let Some(resource) = api_response.resources.first() {
            status.current_week = Some(resource.week());
        }
    }

    // Emit event to frontend
    let _ = app.emit("resources-updated", &api_response);

    // Check for auto-downloads after force poll
    tracing::debug!("Scanning resources for auto-download after force poll");
    state.download_queue.scan_and_queue(app.clone()).await;

    Ok(api_response)
}

/// Set the work directory
/// Set the work directory
#[tauri::command]
pub fn set_work_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    path: String,
) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    // Verify directory exists
    if !path_buf.exists() {
        return Err(format!("Directory does not exist: {}", path));
    }

    if !path_buf.is_dir() {
        return Err(format!("Path is not a directory: {}", path));
    }

    let mut config = state
        .config
        .write()
        .map_err(|e| format!("Failed to write config: {}", e))?;
    config.work_directory = Some(path_buf);

    // Save to store
    use tauri_plugin_store::StoreExt;
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;
    
    let json = serde_json::to_value(&*config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    store.set("config", json);
    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    Ok(())
}

/// Toggle polling on/off
/// Toggle polling on/off
#[tauri::command]
pub fn set_polling_enabled(
    state: State<'_, AppState>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut config = state
        .config
        .write()
        .map_err(|e| format!("Failed to write config: {}", e))?;
    config.polling_enabled = enabled;

    let mut status = state
        .status
        .write()
        .map_err(|e| format!("Failed to write status: {}", e))?;
    status.polling_active = enabled;

    // Save to store
    use tauri_plugin_store::StoreExt;
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;
    
    let json = serde_json::to_value(&*config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    store.set("config", json);
    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    Ok(())
}

/// Set the polling interval in minutes
/// Set the polling interval in minutes
#[tauri::command]
pub fn set_polling_interval(
    state: State<'_, AppState>,
    app: AppHandle,
    minutes: u32,
) -> Result<(), String> {
    if minutes < 1 || minutes > 1440 {
        return Err("Polling interval must be between 1 and 1440 minutes".to_string());
    }

    let mut config = state
        .config
        .write()
        .map_err(|e| format!("Failed to write config: {}", e))?;
    config.polling_interval_minutes = minutes;

    // Save to store
    use tauri_plugin_store::StoreExt;
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;
    
    let json = serde_json::to_value(&*config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    store.set("config", json);
    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    Ok(())
}

/// Set the retention policy
/// Set the retention policy
#[tauri::command]
pub fn set_retention_days(
    state: State<'_, AppState>,
    app: AppHandle,
    days: Option<u32>,
) -> Result<(), String> {
    let mut config = state
        .config
        .write()
        .map_err(|e| format!("Failed to write config: {}", e))?;
    config.retention_days = days;

    // Save to store
    use tauri_plugin_store::StoreExt;
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;
    
    let json = serde_json::to_value(&*config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    store.set("config", json);
    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    Ok(())
}

/// Get archived weeks
#[tauri::command]
pub fn get_archived_weeks(state: State<'_, AppState>) -> Result<Vec<WeekIdentifier>, String> {
    let config = state
        .config
        .read()
        .map_err(|e| format!("Failed to read config: {}", e))?;

    let work_dir = config
        .work_directory
        .as_ref()
        .ok_or("Work directory not configured")?;

    let service = crate::services::FileRetentionService::new(work_dir.clone());
    Ok(service.get_archived_weeks())
}

/// Check if a resource is a YouTube link
#[tauri::command]
pub fn is_resource_youtube(url: String) -> bool {
    crate::models::is_youtube_url(&url)
}

/// Download a specific resource
/// This adds the resource to the download queue with priority
#[tauri::command]
pub async fn download_resource(
    state: State<'_, AppState>,
    app: AppHandle,
    resource: Resource,
) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?.clone();

    let work_dir = config
        .work_directory
        .ok_or("Work directory not configured")?;
    
    let week_dir = resource.week().as_dir_name();
    let dest_dir = work_dir.join(week_dir);

    if !dest_dir.exists() {
        std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    }

    // Add to queue with priority (manual downloads go first)
    state.download_queue.add_task_priority(app.clone(), resource).await;

    Ok(())
}

/// Pause an active download
#[tauri::command]
pub fn pause_download(state: State<'_, AppState>, resource_id: i64) -> Result<(), String> {
    let signals = state.download_signals.read().map_err(|e| e.to_string())?;
    if let Some(signal) = signals.get(&resource_id) {
        signal.store(STATUS_PAUSED, Ordering::Relaxed);
    }
    Ok(())
}

/// Cancel and delete an active download
#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, resource_id: i64) -> Result<(), String> {
    let signals = state.download_signals.read().map_err(|e| e.to_string())?;
    if let Some(signal) = signals.get(&resource_id) {
        signal.store(STATUS_CANCELLED, Ordering::Relaxed);
    }
    Ok(())
}

/// Check if a resource is already downloaded
#[tauri::command]
pub fn check_resource_status(state: State<'_, AppState>, resource: Resource) -> Result<bool, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;

    if let Some(work_dir) = &config.work_directory {
        let week_dir = resource.week().as_dir_name();
        let dest_dir = work_dir.join(week_dir);

        let filename = crate::services::download::extract_filename_from_url(&resource.download_url)
            .unwrap_or_else(|| crate::services::download::sanitize_filename(&resource.title));

        let dest_path = dest_dir.join(filename);
        Ok(dest_path.exists())
    } else {
        Ok(false)
    }
}

/// Get the size of a file from its URL without downloading it
#[tauri::command]
pub async fn get_file_size(url: String) -> Result<u64, String> {
    let client = reqwest::Client::new();
    let response = client
        .head(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch headers: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Request failed with status: {}", response.status()));
    }

    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|val| val.to_str().ok())
        .and_then(|val| val.parse::<u64>().ok())
        .ok_or_else(|| "Content-Length header missing or invalid".to_string())?;

    Ok(content_length)
}
