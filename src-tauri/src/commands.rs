//! Tauri commands for IPC communication with the frontend
//!
//! These commands implement the "Dumb UI, Smart Backend" architecture.

use crate::error::{CommandError, FileError};
use crate::models::{
    AppConfig, AppStatus, CategoryCount, DownloadedFile, Resource, ResourceListResponse,
    WeekIdentifier,
};
use crate::services::download::{STATUS_CANCELLED, STATUS_PAUSED};
use crate::services::{DownloadQueue, PollingService, RetentionScheduler};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, State};

/// Application state managed by Tauri
pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub current_week: RwLock<Option<WeekIdentifier>>,
    pub resources: RwLock<Vec<Resource>>,
    pub status: RwLock<AppStatus>,
    /// Full category catalog from `categories/counts`, independent of the
    /// current week's resources so Settings can list (and re-enable)
    /// categories that aren't downloadable right now. Refreshed on every poll
    /// by `services::refresh_categories`; empty until the first successful
    /// fetch and left untouched when that fetch fails (offline fallback).
    pub all_categories: RwLock<Vec<CategoryCount>>,
    /// Signals to control active downloads (Pause/Cancel)
    pub download_signals: RwLock<HashMap<i64, Arc<AtomicU8>>>,
    /// Registry of successfully downloaded files (errata corrige tracking).
    /// Persisted in the `downloaded_files` key of `cache.json`; the queue
    /// worker upserts an entry on each successful download and the errata
    /// pass (`services::errata::process_errata`) marks entries superseded.
    pub downloaded_files: RwLock<Vec<crate::models::DownloadedFile>>,
    /// Download queue service
    pub download_queue: Arc<DownloadQueue>,
    /// Cache for file sizes (keyed by download_url)
    /// Note: u64::MAX is used as a sentinel value for failed requests (negative cache)
    pub file_size_cache: RwLock<HashMap<String, u64>>,
    /// Shared HTTP client for all requests (connection pooling)
    pub shared_http_client: reqwest::Client,
    /// Handle to the background polling scheduler (`None` if
    /// `polling_enabled` is off), so it can be stopped cleanly on app exit
    /// (tray menu "Esci"). Set once at setup, taken and stopped on shutdown.
    pub polling_service: RwLock<Option<PollingService>>,
    /// Handle to the background retention scheduler, so it can be stopped
    /// cleanly on app exit alongside `polling_service`.
    pub retention_scheduler: RwLock<Option<RetentionScheduler>>,
    /// Whether the system tray icon was created successfully at setup.
    /// `false` on Linux systems missing libappindicator3/
    /// libayatana-appindicator3 (see `lib.rs::setup_tray`): the window's
    /// `CloseRequested` handler reads this to fall back to a normal close
    /// (process exits) instead of hiding to a tray icon that doesn't exist,
    /// which would otherwise strand the user with no way to reopen the app.
    pub tray_available: AtomicBool,
}

/// Response for download command
#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadResponse {
    pub path: String,
    pub hash: String,
}

/// Open a native folder picker dialog
#[tauri::command]
pub async fn select_work_directory(app: AppHandle) -> Result<Option<String>, CommandError> {
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
            all_categories: RwLock::new(Vec::new()),
            download_signals: RwLock::new(HashMap::new()),
            downloaded_files: RwLock::new(Vec::new()),
            download_queue: Arc::new(DownloadQueue::new()),
            file_size_cache: RwLock::new(HashMap::new()),
            shared_http_client: reqwest::Client::new(),
            polling_service: RwLock::new(None),
            retention_scheduler: RwLock::new(None),
            tray_available: AtomicBool::new(false),
        }
    }
}

/// Get the current configuration
#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, CommandError> {
    let config = state.config.read()?;
    Ok(config.clone())
}

/// Persist `config` to the `config` key of the `settings.json` store. Shared by
/// every config-mutating command so the serialize-and-save path lives in one
/// place. Synchronous: never `.await` while a config lock is held.
fn persist_config(app: &AppHandle, config: &AppConfig) -> Result<(), CommandError> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json")?;

    let json = serde_json::to_value(config).map_err(|e| {
        CommandError::new(
            "config-serialize-failed",
            format!("Failed to serialize config: {e}"),
        )
    })?;

    store.set("config", json);
    store.save()?;
    Ok(())
}

/// Update the configuration
#[tauri::command]
pub async fn set_config(
    state: State<'_, AppState>,
    app: AppHandle,
    mut config: AppConfig,
) -> Result<(), CommandError> {
    // Validate before saving
    config
        .validate()
        .map_err(|e| CommandError::new("config-invalid", format!("Invalid config: {e:?}")))?;

    // `tray_close_os_notice_shown` is backend-owned (set once in lib.rs when the
    // window is first hidden to the tray); never let a stale value round-tripped
    // by the frontend overwrite it.
    {
        let current = state.config.read()?;
        config.tray_close_os_notice_shown = current.tray_close_os_notice_shown;
    }

    persist_config(&app, &config)?;

    // Update state
    {
        let mut current = state.config.write()?;
        *current = config.clone();
    }

    // Trigger queue updates
    state.download_queue.update_mode(config.download_mode).await;
    state.download_queue.scan_and_queue(app).await;

    Ok(())
}

/// Get the current application status
#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Result<AppStatus, CommandError> {
    let status = state.status.read()?;
    Ok(status.clone())
}

/// Get the currently loaded resources
#[tauri::command]
pub fn get_resources(state: State<'_, AppState>) -> Result<Vec<Resource>, CommandError> {
    let resources = state.resources.read()?;
    Ok(resources.clone())
}

/// Get the full category catalog (from the last successful `categories/counts`
/// fetch). Used by the UI's initial load; live updates arrive via the
/// `categories-updated` event.
#[tauri::command]
pub fn get_all_categories(state: State<'_, AppState>) -> Result<Vec<CategoryCount>, CommandError> {
    let categories = state.all_categories.read()?;
    Ok(categories.clone())
}

/// Trigger an immediate poll of the API. Thin wrapper over the shared
/// `services::poll_once` flow (the same one the background polling loop runs),
/// so the manual "refresh now" action and the periodic poll can never diverge.
#[tauri::command]
pub async fn force_poll(app: AppHandle) -> Result<ResourceListResponse, CommandError> {
    // `poll_once` still surfaces its failure as a flat string (it aggregates
    // HTTP/parse/lock failures across a whole cycle); wrap it under one stable
    // code while preserving the detailed message it built.
    crate::services::poll_once(&app)
        .await
        .map_err(|e| CommandError::new("poll-failed", e))
}

/// Set the work directory
#[tauri::command]
pub fn set_work_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    path: String,
) -> Result<(), CommandError> {
    let path_buf = validate_work_directory(&path)?;

    let mut config = state.config.write()?;
    config.work_directory = Some(path_buf);

    persist_config(&app, &config)
}

/// Validate a user-selected work directory, returning the resolved path or a
/// typed error the frontend can branch on (`work-dir-not-found` /
/// `not-a-directory`). Extracted from `set_work_directory` so the mapping from
/// filesystem state to error code is unit-testable without Tauri state.
fn validate_work_directory(path: &str) -> Result<PathBuf, CommandError> {
    let path_buf = PathBuf::from(path);

    if !path_buf.exists() {
        return Err(CommandError::new(
            "work-dir-not-found",
            format!("Directory does not exist: {path}"),
        ));
    }

    if !path_buf.is_dir() {
        return Err(CommandError::new(
            "not-a-directory",
            format!("Path is not a directory: {path}"),
        ));
    }

    Ok(path_buf)
}

/// Toggle polling on/off
#[tauri::command]
pub fn set_polling_enabled(
    state: State<'_, AppState>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), CommandError> {
    // Update config + capture the interval to (re)start with and a config
    // snapshot to persist, then release the lock before touching the polling
    // service.
    let (interval, config_snapshot) = {
        let mut config = state.config.write()?;
        config.polling_enabled = enabled;
        (config.polling_interval_minutes, config.clone())
    };

    {
        let mut status = state.status.write()?;
        status.polling_active = enabled;
    }

    // Actually start/stop the background task so the UI toggle takes effect
    // (previously this command only flipped config/status flags, a no-op).
    {
        let mut guard = state.polling_service.write()?;
        if enabled {
            match guard.as_ref() {
                // Already running: nothing to do.
                Some(service) if service.is_running() => {}
                // Kept from a previous run but stopped: restart it in place.
                Some(service) => service.start(app.clone(), interval),
                // No service yet: create, start, and store it in the same
                // field `lib.rs`/shutdown use, so it can be stopped cleanly.
                None => {
                    let service = PollingService::new();
                    service.start(app.clone(), interval);
                    *guard = Some(service);
                }
            }
        } else if let Some(service) = guard.as_ref() {
            service.stop();
        }
    }

    persist_config(&app, &config_snapshot)
}

/// Set the polling interval in minutes
#[tauri::command]
pub fn set_polling_interval(
    state: State<'_, AppState>,
    app: AppHandle,
    minutes: u32,
) -> Result<(), CommandError> {
    if !(1..=1440).contains(&minutes) {
        return Err(CommandError::new(
            "invalid-polling-interval",
            "Polling interval must be between 1 and 1440 minutes",
        ));
    }

    let config_snapshot = {
        let mut config = state.config.write()?;
        config.polling_interval_minutes = minutes;
        config.clone()
    };

    // If polling is currently running, restart it so the new interval takes
    // effect immediately instead of only after the next app launch.
    {
        let guard = state.polling_service.read()?;
        if let Some(service) = guard.as_ref() {
            if service.is_running() {
                service.restart(app.clone(), minutes);
            }
        }
    }

    persist_config(&app, &config_snapshot)
}

/// Set the retention policy
#[tauri::command]
pub fn set_retention_days(
    state: State<'_, AppState>,
    app: AppHandle,
    days: Option<u32>,
) -> Result<(), CommandError> {
    let mut config = state.config.write()?;
    config.retention_days = days;

    persist_config(&app, &config)
}

/// Enable or disable launching the app automatically at OS startup.
///
/// Toggles the actual OS-level autostart entry (Windows registry autorun /
/// Linux XDG `.desktop` autostart, via tauri-plugin-autostart) first, and
/// only persists the preference to config/store if that succeeds — so a
/// failed OS-level toggle never leaves the saved config out of sync with
/// reality.
#[tauri::command]
pub fn set_autostart_enabled(
    state: State<'_, AppState>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), CommandError> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart_manager = app.autolaunch();
    if enabled {
        autostart_manager.enable().map_err(|e| {
            CommandError::new(
                "autostart-failed",
                format!("Failed to enable autostart: {e}"),
            )
        })?;
    } else {
        autostart_manager.disable().map_err(|e| {
            CommandError::new(
                "autostart-failed",
                format!("Failed to disable autostart: {e}"),
            )
        })?;
    }

    let mut config = state.config.write()?;
    config.autostart_enabled = enabled;

    persist_config(&app, &config)
}

/// Get archived weeks
#[tauri::command]
pub fn get_archived_weeks(state: State<'_, AppState>) -> Result<Vec<WeekIdentifier>, CommandError> {
    let config = state.config.read()?;

    let work_dir = config
        .work_directory
        .as_ref()
        .ok_or(FileError::WorkDirectoryNotSet)?;

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
) -> Result<(), CommandError> {
    let config = state.config.read()?.clone();

    let work_dir = config
        .work_directory
        .ok_or(FileError::WorkDirectoryNotSet)?;

    let dest_dir =
        crate::services::download::resolve_week_dir(&resource, &work_dir, config.prefer_optimized);

    if !dest_dir.exists() {
        std::fs::create_dir_all(&dest_dir)
            .map_err(|e| CommandError::new("create-directory-failed", e.to_string()))?;
    }

    // Add to queue with priority (manual downloads go first)
    state
        .download_queue
        .add_task_priority(app.clone(), resource)
        .await;

    Ok(())
}

/// Pause an active download
#[tauri::command]
pub fn pause_download(state: State<'_, AppState>, resource_id: i64) -> Result<(), CommandError> {
    // Use try_read to avoid blocking if a write lock is held
    let signals = state
        .download_signals
        .try_read()
        .map_err(|_| CommandError::new("signals-locked", "Download signals locked, try again"))?;
    if let Some(signal) = signals.get(&resource_id) {
        signal.store(STATUS_PAUSED, Ordering::Relaxed);
    }
    Ok(())
}

/// Cancel and delete an active download
#[tauri::command]
pub async fn cancel_download(
    state: State<'_, AppState>,
    app: AppHandle,
    resource_id: i64,
) -> Result<(), CommandError> {
    // A5: if the resource is still waiting in the queue, drop it there.
    // Setting the download signal would be a no-op for something not yet
    // active, so the item would otherwise reappear on the next status emit.
    if state.download_queue.remove_queued(&app, resource_id).await {
        return Ok(());
    }

    // Otherwise it's an in-flight download: signal cancellation.
    // Use try_read to avoid blocking if a write lock is held
    let signals = state
        .download_signals
        .try_read()
        .map_err(|_| CommandError::new("signals-locked", "Download signals locked, try again"))?;
    if let Some(signal) = signals.get(&resource_id) {
        signal.store(STATUS_CANCELLED, Ordering::Relaxed);
    }
    Ok(())
}

/// Check if a resource is already downloaded
#[tauri::command]
pub fn check_resource_status(
    state: State<'_, AppState>,
    resource: Resource,
) -> Result<bool, CommandError> {
    // Use try_read to avoid blocking if a write lock is held
    let config = state
        .config
        .try_read()
        .map_err(|_| CommandError::new("config-locked", "Config locked, try again"))?;

    if let Some(work_dir) = &config.work_directory {
        let dest_path = crate::services::download::resolve_dest_path(
            &resource,
            work_dir,
            config.prefer_optimized,
        );
        Ok(dest_path.exists())
    } else {
        Ok(false)
    }
}

/// Resolve the on-disk path of a resource's downloaded file.
///
/// Registry-first: an entry in `downloaded_files` records where the download
/// actually landed, which stays authoritative even if the URL-derived filename
/// later changes. Falls back to `resolve_dest_path` (work dir + week + URL
/// filename) for files that predate the registry or have no entry yet.
fn resolve_resource_path(state: &AppState, resource: &Resource) -> Result<PathBuf, CommandError> {
    {
        let registry = state.downloaded_files.read()?;
        if let Some(entry) = registry
            .iter()
            .rev()
            .find(|f| f.resource_id == resource.id && !f.is_superseded)
        {
            return Ok(entry.local_path.clone());
        }
    }

    let config = state.config.read()?;
    let work_dir = config
        .work_directory
        .as_ref()
        .ok_or(FileError::WorkDirectoryNotSet)?;
    Ok(crate::services::download::resolve_dest_path(
        resource,
        work_dir,
        config.prefer_optimized,
    ))
}

/// Reveal a downloaded resource in the system file manager, selecting the file
/// inside its containing folder. If selection isn't supported (some Linux file
/// managers) or the reveal otherwise fails, falls back to opening the week
/// directory that would contain it.
#[tauri::command]
pub fn reveal_resource(
    state: State<'_, AppState>,
    app: AppHandle,
    resource: Resource,
) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;

    let path = resolve_resource_path(state.inner(), &resource)?;

    if app.opener().reveal_item_in_dir(&path).is_err() {
        let dir = path.parent().unwrap_or(path.as_path());
        app.opener()
            .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
            // Bare detail only: the frontend toast already prefixes
            // "Impossibile aprire la cartella:" (useResource.ts), so prefixing
            // here too would double it.
            .map_err(|e| CommandError::new("reveal-failed", e.to_string()))?;
    }

    Ok(())
}

/// Get the size of a file from its URL without downloading it
#[tauri::command]
pub async fn get_file_size(state: State<'_, AppState>, url: String) -> Result<u64, CommandError> {
    // Check cache first
    {
        let cache = state.file_size_cache.read()?;
        if let Some(&size) = cache.get(&url) {
            if size == u64::MAX {
                // Negative cache hit - this URL previously failed
                tracing::debug!("Cache hit (negative) for file size: {}", url);
                return Err(CommandError::new(
                    "file-size-unavailable",
                    "File size unavailable (cached failure)",
                ));
            }
            tracing::debug!("Cache hit for file size: {}", url);
            return Ok(size);
        }
    }

    // Cache miss - fetch from remote
    tracing::debug!("Cache miss for file size, fetching: {}", url);
    let response = state
        .shared_http_client
        .head(&url)
        .send()
        .await
        .map_err(|e| {
            // Cache negative result to avoid repeated failures
            let _ = state.file_size_cache.write().map(|mut cache| {
                cache.insert(url.clone(), u64::MAX);
                tracing::debug!("Cached negative result (request failed) for: {}", url);
            });
            CommandError::new(
                "head-request-failed",
                format!("Failed to fetch headers: {e}"),
            )
        })?;

    if !response.status().is_success() {
        // Cache negative result for non-success status
        let _ = state.file_size_cache.write().map(|mut cache| {
            cache.insert(url.clone(), u64::MAX);
            tracing::debug!(
                "Cached negative result (status {}) for: {}",
                response.status(),
                url
            );
        });
        return Err(CommandError::new(
            "http-status-error",
            format!("Request failed with status: {}", response.status()),
        ));
    }

    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|val| val.to_str().ok())
        .and_then(|val| val.parse::<u64>().ok());

    match content_length {
        Some(size) => {
            // Save successful result to cache
            let mut cache = state.file_size_cache.write()?;
            cache.insert(url.clone(), size);
            tracing::debug!("Cached file size for: {}", url);
            Ok(size)
        }
        None => {
            // Cache negative result for missing/invalid Content-Length
            let _ = state.file_size_cache.write().map(|mut cache| {
                cache.insert(url.clone(), u64::MAX);
                tracing::debug!("Cached negative result (no Content-Length) for: {}", url);
            });
            Err(CommandError::new(
                "content-length-missing",
                "Content-Length header missing or invalid",
            ))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub total: usize,
    pub downloaded: usize,
    pub active: usize,
    pub queued: usize,
}

/// Batched per-resource status for the UI. `file_size`/`optimized_file_size`
/// come exclusively from the cached HEAD sizes (never a network request); a
/// missing or sentinel-cached (`u64::MAX`) entry serializes as `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub downloaded: bool,
    pub file_size: Option<u64>,
    pub optimized_file_size: Option<u64>,
}

/// Read a cached HEAD size, treating the `u64::MAX` failure sentinel (negative
/// cache) as "unknown" so it never leaks to the UI as a real size.
fn cached_size(size_cache: &HashMap<String, u64>, url: &str) -> Option<u64> {
    size_cache
        .get(url)
        .copied()
        .filter(|&size| size != u64::MAX)
}

/// Pure computation of per-resource status. A resource counts as `downloaded`
/// when the errata registry has a matching, not-yet-superseded entry
/// (`resource_id` + `week`) whose `local_path` still exists on disk, OR — as a
/// fallback when no such registry entry exists — the effective destination file
/// is present (`check_file_exists`). Sizes are looked up only in `size_cache`;
/// no network I/O happens here. With `work_dir` `None` every resource is
/// `downloaded = false`.
pub(crate) fn compute_resources_status(
    resources: &[Resource],
    registry: &[DownloadedFile],
    work_dir: Option<&Path>,
    prefer_optimized: bool,
    size_cache: &HashMap<String, u64>,
) -> HashMap<i64, ResourceStatus> {
    let mut statuses = HashMap::with_capacity(resources.len());

    for resource in resources {
        let downloaded = match work_dir {
            Some(work_dir) => {
                let week = resource.week();
                let registry_hit = registry.iter().any(|entry| {
                    entry.resource_id == resource.id
                        && entry.week == week
                        && !entry.is_superseded
                        && entry.local_path.exists()
                });
                registry_hit
                    || crate::services::download::DownloadService::check_file_exists(
                        resource,
                        work_dir,
                        prefer_optimized,
                    )
            }
            None => false,
        };

        let file_size = cached_size(size_cache, &resource.download_url);
        let optimized_file_size = resource
            .optimized_video_url
            .as_deref()
            .and_then(|url| cached_size(size_cache, url));

        statuses.insert(
            resource.id,
            ResourceStatus {
                downloaded,
                file_size,
                optimized_file_size,
            },
        );
    }

    statuses
}

#[tauri::command]
pub async fn get_resources_status(
    state: State<'_, AppState>,
) -> Result<HashMap<i64, ResourceStatus>, CommandError> {
    // Snapshot everything under short read locks, then compute off the async
    // runtime. No lock guard is ever held across the await (spawn_blocking).
    let (resources, registry, work_dir, prefer_optimized, size_cache) = {
        let resources = state.resources.read()?.clone();
        let registry = state.downloaded_files.read()?.clone();
        let (work_dir, prefer_optimized) = {
            let config = state.config.read()?;
            (config.work_directory.clone(), config.prefer_optimized)
        };
        let size_cache = state.file_size_cache.read()?.clone();
        (resources, registry, work_dir, prefer_optimized, size_cache)
    };

    tauri::async_runtime::spawn_blocking(move || {
        compute_resources_status(
            &resources,
            &registry,
            work_dir.as_deref(),
            prefer_optimized,
            &size_cache,
        )
    })
    .await
    .map_err(|e| CommandError::new("task-join-failed", e.to_string()))
}

#[tauri::command]
pub async fn get_resource_summary(
    state: State<'_, AppState>,
) -> Result<ResourceSummary, CommandError> {
    // Clone data that needs to be used after await points or potentially long operations
    // This avoids holding non-Send RwLockGuard across await points
    let (resources, registry, work_dir, prefer_optimized) = {
        let resources = state.resources.read()?.clone();
        let registry = state.downloaded_files.read()?.clone();
        let (work_dir, prefer_optimized) = {
            let config = state.config.read()?;
            (config.work_directory.clone(), config.prefer_optimized)
        };
        (resources, registry, work_dir, prefer_optimized)
    };

    // Now we can await without holding the lock guards
    let active = state.download_queue.active_count();
    let queued = state.download_queue.queue_len().await;
    let total = resources.len();

    // Reuse the same registry-first-OR-fs logic as the batched status command;
    // the size cache is irrelevant to the downloaded count, so pass an empty one.
    let downloaded = tauri::async_runtime::spawn_blocking(move || {
        let empty_cache = HashMap::new();
        compute_resources_status(
            &resources,
            &registry,
            work_dir.as_deref(),
            prefer_optimized,
            &empty_cache,
        )
        .values()
        .filter(|status| status.downloaded)
        .count()
    })
    .await
    .map_err(|e| CommandError::new("task-join-failed", e.to_string()))?;

    Ok(ResourceSummary {
        total,
        downloaded,
        active,
        queued,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    fn make_resource(id: i64, url: &str) -> Resource {
        Resource {
            id,
            category: "video".to_string(),
            title: format!("Resource {id}"),
            description: None,
            download_url: url.to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap(),
            optimized_video_url: None,
            optimized_videos: None,
        }
    }

    fn make_downloaded(
        resource: &Resource,
        local_path: PathBuf,
        superseded: bool,
    ) -> DownloadedFile {
        DownloadedFile {
            resource_id: resource.id,
            week: resource.week(),
            local_path,
            downloaded_at: resource.created_at,
            source_url: resource.download_url.clone(),
            is_superseded: superseded,
        }
    }

    /// Write a real file at the resource's derived destination path so that
    /// `check_file_exists` (the fs fallback) sees it.
    fn create_dest_file(work_dir: &Path, resource: &Resource) -> PathBuf {
        let dest = crate::services::download::resolve_dest_path(resource, work_dir, true);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, b"x").unwrap();
        dest
    }

    #[test]
    fn test_validate_work_directory_ok_for_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let resolved = validate_work_directory(&tmp.path().to_string_lossy()).unwrap();
        assert_eq!(resolved, tmp.path());
    }

    #[test]
    fn test_validate_work_directory_missing_path_is_typed_error() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let err = validate_work_directory(&missing.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, "work-dir-not-found");
        assert!(err.message.contains("Directory does not exist"));
    }

    #[test]
    fn test_validate_work_directory_file_is_not_a_directory() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let err = validate_work_directory(&file.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, "not-a-directory");
    }

    #[test]
    fn test_registry_hit_with_existing_file_is_downloaded() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();
        let r = make_resource(1, "https://example.com/file1.mp4");

        // A real file recorded by the registry at a path distinct from the
        // derived dest (which is never created): only the registry can see it.
        let reg_path = wd.join("registry-copy.mp4");
        std::fs::write(&reg_path, b"x").unwrap();
        let registry = vec![make_downloaded(&r, reg_path, false)];

        let out = compute_resources_status(&[r], &registry, Some(wd), true, &HashMap::new());
        assert!(out[&1].downloaded);
    }

    #[test]
    fn test_registry_hit_missing_file_and_no_fs_is_not_downloaded() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();
        let r = make_resource(2, "https://example.com/file2.mp4");

        // Registry points at a non-existent path and no derived dest exists.
        let registry = vec![make_downloaded(&r, wd.join("missing.mp4"), false)];

        let out = compute_resources_status(&[r], &registry, Some(wd), true, &HashMap::new());
        assert!(!out[&2].downloaded);
    }

    #[test]
    fn test_superseded_entry_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();
        let r = make_resource(3, "https://example.com/file3.mp4");

        // Superseded entry whose file exists must NOT count; no fs dest exists.
        let sup_path = wd.join("superseded.mp4");
        std::fs::write(&sup_path, b"x").unwrap();
        let registry = vec![make_downloaded(&r, sup_path, true)];

        let out = compute_resources_status(&[r], &registry, Some(wd), true, &HashMap::new());
        assert!(!out[&3].downloaded);
    }

    #[test]
    fn test_empty_registry_with_file_on_disk_is_downloaded() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();
        let r = make_resource(4, "https://example.com/file4.mp4");
        create_dest_file(wd, &r);

        let out = compute_resources_status(&[r], &[], Some(wd), true, &HashMap::new());
        assert!(out[&4].downloaded);
    }

    #[test]
    fn test_different_week_falls_back_to_fs() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();
        let r = make_resource(5, "https://example.com/file5.mp4");

        // Registry entry for the same id but a DIFFERENT week: it must not
        // match, so the decision falls back to the fs check.
        let mut other_week = r.week();
        other_week.week_number += 1;
        let reg_path = wd.join("otherweek.mp4");
        std::fs::write(&reg_path, b"x").unwrap();
        let registry = vec![DownloadedFile {
            resource_id: r.id,
            week: other_week,
            local_path: reg_path,
            downloaded_at: r.created_at,
            source_url: r.download_url.clone(),
            is_superseded: false,
        }];

        // No derived dest yet → not downloaded despite the other-week file.
        let out = compute_resources_status(
            std::slice::from_ref(&r),
            &registry,
            Some(wd),
            true,
            &HashMap::new(),
        );
        assert!(
            !out[&5].downloaded,
            "different-week entry must not register a hit"
        );

        // Now the fs fallback finds the file in the resource's own week.
        create_dest_file(wd, &r);
        let out = compute_resources_status(&[r], &registry, Some(wd), true, &HashMap::new());
        assert!(out[&5].downloaded, "fs fallback finds the file");
    }

    #[test]
    fn test_size_cache_sentinel_maps_to_none() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();

        let mut r = make_resource(6, "https://example.com/file6.mp4");
        r.optimized_video_url = Some("https://example.com/file6-opt.mp4".to_string());

        let mut cache = HashMap::new();
        // Real size for the original, sentinel (failed HEAD) for the optimized.
        cache.insert(r.download_url.clone(), 1234u64);
        cache.insert("https://example.com/file6-opt.mp4".to_string(), u64::MAX);

        let out = compute_resources_status(&[r], &[], Some(wd), true, &cache);
        assert_eq!(out[&6].file_size, Some(1234));
        assert_eq!(out[&6].optimized_file_size, None);
    }

    #[test]
    fn test_work_dir_none_is_all_false() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();
        let r = make_resource(7, "https://example.com/file7.mp4");

        // A registry entry with an existing file is present, but work_dir None
        // forces every resource to false.
        let reg_path = wd.join("present.mp4");
        std::fs::write(&reg_path, b"x").unwrap();
        let registry = vec![make_downloaded(&r, reg_path, false)];

        let out = compute_resources_status(&[r], &registry, None, true, &HashMap::new());
        assert!(!out[&7].downloaded);
    }

    #[test]
    fn test_basename_collision_registry_disambiguates() {
        let tmp = TempDir::new().unwrap();
        let wd = tmp.path();

        // Two resources whose URLs share the same basename → identical derived
        // dest path in the same week (a real-world collision).
        let a = make_resource(20, "https://a.example.com/shared.mp4");
        let b = make_resource(21, "https://b.example.com/shared.mp4");
        let shared_dest = crate::services::download::resolve_dest_path(&a, wd, true);
        assert_eq!(
            shared_dest,
            crate::services::download::resolve_dest_path(&b, wd, true),
            "test premise: both resources derive the same dest path"
        );

        // Legacy fs-only behavior (empty registry): the single file at the
        // shared derived path makes BOTH resources look downloaded. This is the
        // documented over-count the registry is meant to disambiguate.
        std::fs::create_dir_all(shared_dest.parent().unwrap()).unwrap();
        std::fs::write(&shared_dest, b"x").unwrap();
        let legacy = compute_resources_status(
            &[a.clone(), b.clone()],
            &[],
            Some(wd),
            true,
            &HashMap::new(),
        );
        assert!(legacy[&20].downloaded);
        assert!(
            legacy[&21].downloaded,
            "without a registry the fs fallback counts both (legacy behavior)"
        );

        // Registry-disambiguated: the actual download was recorded for A at its
        // real saved path (distinct from the colliding derived name). Re-deriving
        // B's filename collides but no file exists there, so only A is downloaded.
        std::fs::remove_file(&shared_dest).unwrap();
        let actual_a = wd.join(a.week().as_dir_name()).join("actual-a.mp4");
        std::fs::write(&actual_a, b"x").unwrap();
        let registry = vec![make_downloaded(&a, actual_a, false)];

        let out = compute_resources_status(&[a, b], &registry, Some(wd), true, &HashMap::new());
        assert!(out[&20].downloaded, "registry hit for A");
        assert!(
            !out[&21].downloaded,
            "B has no registry entry and no file at its derived path"
        );
    }
}
