//! Polling service for background API fetching
//!
//! Runs a background task using tokio to periodically poll the API.

use crate::commands::AppState;
use crate::models::ResourceListResponse;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;
use tokio::time::{interval, Duration};

/// API base URL
const API_BASE_URL: &str = "https://api.adventistyouth.it";

/// Polling service that runs in the background
pub struct PollingService {
    /// Channel sender to signal cancellation
    cancel_tx: watch::Sender<bool>,
    /// Whether polling is currently running
    is_running: Arc<AtomicBool>,
}

impl PollingService {
    /// Create a new polling service
    pub fn new() -> Self {
        let (cancel_tx, _) = watch::channel(false);
        Self {
            cancel_tx,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the polling background task
    pub fn start(&self, app: AppHandle, interval_mins: u32) {
        if self.is_running.load(Ordering::SeqCst) {
            tracing::warn!("Polling already running, ignoring start request");
            return;
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let mut cancel_rx = self.cancel_tx.subscribe();

        tauri::async_runtime::spawn(async move {
            let duration = Duration::from_secs(interval_mins as u64 * 60);
            let mut ticker = interval(duration);

            // Skip the first immediate tick
            ticker.tick().await;

            tracing::info!("Polling service started with interval {} minutes", interval_mins);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        tracing::trace!("Polling tick");
                        
                        // Check if we should still be running
                        if !is_running.load(Ordering::SeqCst) {
                            break;
                        }

                        // Perform the poll
                        if let Err(e) = poll_api(&app).await {
                            tracing::error!("Polling failed: {}", e);
                            let _ = app.emit("poll-error", e.to_string());
                        }
                    }
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            tracing::info!("Polling service cancelled");
                            break;
                        }
                    }
                }
            }

            is_running.store(false, Ordering::SeqCst);
            tracing::info!("Polling service stopped");
        });
    }

    /// Stop the polling background task
    pub fn stop(&self) {
        if self.is_running.load(Ordering::SeqCst) {
            let _ = self.cancel_tx.send(true);
            self.is_running.store(false, Ordering::SeqCst);
        }
    }

    /// Check if polling is currently running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Restart with a new interval
    pub fn restart(&self, app: AppHandle, new_interval_mins: u32) {
        self.stop();
        // Reset the cancel channel
        let _ = self.cancel_tx.send(false);
        self.start(app, new_interval_mins);
    }
}

impl Default for PollingService {
    fn default() -> Self {
        Self::new()
    }
}

/// Perform a single poll of the API
async fn poll_api(app: &AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = app.state::<AppState>();
    let url = format!("{}/api/resources/latest-week", API_BASE_URL);

    let response = state.shared_http_client.get(&url).send().await?;
    let api_response: ResourceListResponse = response.json().await?;

    // Get old resources for cache invalidation
    let old_resources = {
        let resources = state.resources.read().map_err(|e| e.to_string())?;
        resources.clone()
    };
    
    // Update resources
    {
        let mut resources = state.resources.write().map_err(|e| e.to_string())?;
        *resources = api_response.resources.clone();
    }

    // Invalidate cache for changed/removed URLs
    {
        let mut cache = state.file_size_cache.write().map_err(|e| e.to_string())?;
        
        // Build a map of old URLs by resource ID
        let old_url_map: std::collections::HashMap<i64, String> = old_resources
            .iter()
            .map(|r| (r.id, r.download_url.clone()))
            .collect();
        
        // Build a set of current URLs
        let current_urls: std::collections::HashSet<String> = api_response.resources
            .iter()
            .map(|r| r.download_url.clone())
            .collect();
        
        // Remove cache entries for URLs that changed or no longer exist
        for new_resource in &api_response.resources {
            if let Some(old_url) = old_url_map.get(&new_resource.id) {
                if old_url != &new_resource.download_url {
                    // Same resource ID but URL changed (errata corrige)
                    cache.remove(old_url);
                    tracing::debug!("Invalidated cache for changed URL: {}", old_url);
                }
            }
        }
        
        // Remove cache entries for URLs that no longer exist
        let keys_to_remove: Vec<String> = cache
            .keys()
            .filter(|url| !current_urls.contains(*url))
            .cloned()
            .collect();
        
        for key in &keys_to_remove {
            cache.remove(key);
        }
        
        if !keys_to_remove.is_empty() {
            tracing::debug!("Removed {} stale cache entries", keys_to_remove.len());
        }
    }

    {
        let mut status = state.status.write().map_err(|e| e.to_string())?;
        status.last_poll_time = Some(chrono::Utc::now());
        status.total_resources = api_response.resources.len();

        if let Some(resource) = api_response.resources.first() {
            status.current_week = Some(resource.week());
        }
    }

    // Emit event to frontend
    app.emit("resources-updated", &api_response)?;
    app.emit("poll-tick", ())?;

    // Save to cache
    use tauri_plugin_store::StoreExt;
    let store = app.store("cache.json").map_err(|e| e.to_string())?;
    let json = serde_json::to_value(&api_response.resources).map_err(|e| e.to_string())?;
    store.set("resources", json);
    
    // Save file size cache (exclude negative cache entries from persistence)
    let cache_snapshot = {
        let cache = state.file_size_cache.read().map_err(|e| e.to_string())?;
        cache.iter()
            .filter(|(_, &size)| size != u64::MAX)  // Exclude negative cache
            .map(|(k, v)| (k.clone(), *v))
            .collect::<std::collections::HashMap<String, u64>>()
    };
    let cache_json = serde_json::to_value(&cache_snapshot).map_err(|e| e.to_string())?;
    store.set("file_size_cache", cache_json);
    
    store.save().map_err(|e| e.to_string())?;

    tracing::info!(
        "Poll completed: {} resources fetched",
        api_response.resources.len()
    );

    // Initial check for auto-downloads
    state.download_queue.scan_and_queue(app.clone()).await;

    Ok(())
}
