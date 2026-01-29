//! Download Queue Service
//!
//! Manages a queue of download tasks, executing them sequentially or in parallel
//! based on the configuration.

use crate::models::{DownloadMode, Resource};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

/// Queue service for managing downloads
pub struct DownloadQueue {
    queue: Arc<Mutex<VecDeque<Resource>>>,
    /// Track active downloads
    active_count: Arc<AtomicUsize>,
    mode: Arc<Mutex<DownloadMode>>,
    /// Flag to ensure worker is started only once
    worker_started: Arc<AtomicBool>,
    /// Track active download IDs for status updates
    active_ids: Arc<Mutex<Vec<i64>>>,
}

impl DownloadQueue {
    pub fn new() -> Self {
        // Default to Queue (1 concurrent) initially, updated via config
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            active_count: Arc::new(AtomicUsize::new(0)),
            mode: Arc::new(Mutex::new(DownloadMode::Queue)),
            worker_started: Arc::new(AtomicBool::new(false)),
            active_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Update the concurrency limit based on mode
    pub async fn update_mode(&self, mode: DownloadMode) {
        let mut current_mode = self.mode.lock().await;
        if *current_mode != mode {
            *current_mode = mode.clone();
            
            // Adjust semaphore permits
            // Note: Semaphore::add_permits increases capacity.
            // Semaphore doesn't support reducing capacity easily dynamically in this crate?
            // Actually, we can just replace the semaphore or use a different logic.
            // Since we can't easily resize a semaphore, we might just spawn tasks differently.
            //
            // Better approach: The `process_queue` loop checks the limit.
            // Or easier: Just use a fixed high limit for Parallel (e.g., 4) and 1 for Queue.
            // And `acquire` permits accordingly?
            //
            // Let's use a cleaner approach: a worker loop that pulls from queue.
        }
    }

    /// Add a resource to the queue and trigger processing
    pub async fn add_task(&self, app: AppHandle, resource: Resource) {
        {
            let mut queue = self.queue.lock().await;
            // Avoid duplicates
            if !queue.iter().any(|r| r.id == resource.id) {
                // Also check if already downloading in AppState?
                // The download service handles "already downloading" mostly, but good to check.
                queue.push_back(resource);
                tracing::info!("Added task to queue. Queue size: {}", queue.len());
            }
        }
        self.emit_queue_status(&app).await;
        self.ensure_worker_started(app).await;
    }

    /// Add a resource to the queue with priority (for manual downloads)
    /// Priority tasks are added to the front of the queue
    pub async fn add_task_priority(&self, app: AppHandle, resource: Resource) {
        {
            let mut queue = self.queue.lock().await;
            // Remove if already exists (to avoid duplicates)
            queue.retain(|r| r.id != resource.id);
            // Add to front for priority
            queue.push_front(resource);
        }
        self.emit_queue_status(&app).await;
        self.ensure_worker_started(app).await;
    }

    /// Emit current queue status to frontend
    async fn emit_queue_status(&self, app: &AppHandle) {
        let queue = self.queue.lock().await;
        let active = self.active_ids.lock().await;
        
        // Create list of queued items with their position
        let queued_items: Vec<serde_json::Value> = queue.iter()
            .enumerate()
            .map(|(i, r)| serde_json::json!({
                "id": r.id,
                "position": i + 1
            }))
            .collect();

        let payload = serde_json::json!({
            "queued": queued_items,
            "active": *active
        });

        if let Err(e) = app.emit("queue-status-changed", payload) {
            tracing::error!("Failed to emit queue-status-changed: {:?}", e);
        }
    }

    /// Ensure the worker is started (called once)
    async fn ensure_worker_started(&self, app: AppHandle) {
        // Check if worker already started
        if self.worker_started.compare_exchange(
            false,
            true,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ).is_ok() {
            // We successfully changed from false to true, so we start the worker
            self.start_worker(app).await;
        }
        // Otherwise, worker is already running, do nothing
    }

    /// scan resources and add to queue if matching auto-download criteria
    pub async fn scan_and_queue(&self, app: AppHandle) {
         let state = app.state::<crate::commands::AppState>();
         
         // Read config and resources
         let (config, resources) = {
             let config = state.config.read().unwrap().clone();
             let resources = state.resources.read().unwrap().clone();
             (config, resources)
         };

         tracing::debug!(
             "Scanning {} resources for auto-download. Enabled categories: {:?}",
             resources.len(),
             config.auto_download_categories
         );

         if let Some(work_dir) = &config.work_directory {
             let mut queued_count = 0;
             for resource in resources {
                 if config.auto_download_categories.contains(&resource.category) {
                     // Check if already downloaded
                      let is_downloaded = crate::services::download::DownloadService::check_file_exists(&resource, work_dir);
                     if !is_downloaded {
                         tracing::trace!("Queuing for auto-download: {} ({})", resource.title, resource.category);
                         self.add_task(app.clone(), resource).await;
                         queued_count += 1;
                     }
                 }
             }
             tracing::info!("Auto-download scan complete: {} resources queued", queued_count);
         } else {
             tracing::debug!("Auto-download scan skipped: work directory not configured");
         }
    }

    /// Start the queue worker (called once)
    async fn start_worker(&self, app: AppHandle) {
        let queue = self.queue.clone();
        let mode_lock = self.mode.clone();
        let active_count = self.active_count.clone();
        let active_ids = self.active_ids.clone();
        
        tracing::info!("Download queue worker started");
        
        // Spawn a detached task to manage coordination
        // This task never exits, continuously processing the queue
        tauri::async_runtime::spawn(async move {
            loop {
                // Determine concurrency limit
                let limit = {
                    let mode = mode_lock.lock().await;
                    match *mode {
                        DownloadMode::Queue => 1,
                        DownloadMode::Parallel => 4,
                    }
                };

                // Check if we can start more downloads
                let current_active = active_count.load(Ordering::SeqCst);
                
                if current_active >= limit {
                    // At capacity, wait before checking again
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    continue;
                }

                // Try to get next task from queue
                let resource = {
                    let mut q = queue.lock().await;
                    q.pop_front()
                };

                if let Some(resource) = resource {
                    // We have a task and have capacity, start it
                    active_count.fetch_add(1, Ordering::SeqCst);
                    
                    // Add to active IDs and emit update
                    {
                        let mut ids = active_ids.lock().await;
                        ids.push(resource.id);
                    }
                    
                    let active_count_clone = active_count.clone();
                    let active_ids_clone = active_ids.clone();
                    let app_clone = app.clone();
                    
                    // Emit status update immediately as queue changed (popped item) AND active changed
                    {
                        let q = queue.lock().await;
                        let a = active_ids.lock().await;
                        let queued_items: Vec<serde_json::Value> = q.iter()
                            .enumerate()
                            .map(|(i, r)| serde_json::json!({
                                "id": r.id,
                                "position": i + 1
                            }))
                            .collect();
                        let payload = serde_json::json!({
                            "queued": queued_items,
                            "active": *a
                        });
                        let _ = app_clone.emit("queue-status-changed", payload);
                    }

                    tauri::async_runtime::spawn(async move {
                         // Execute download
                         // Resolve state at the top level of the task
                         let state = app_clone.state::<crate::commands::AppState>();
                         
                         if let Ok(config) = crate::commands::get_config(state) {
                             if let Some(work_dir) = config.work_directory {
                                 let download_service = crate::services::DownloadService::new();
                                 let week_dir = resource.week().as_dir_name();
                                 let dest_dir = work_dir.join(week_dir);
                                 let prefer_optimized = config.prefer_optimized;
                                 
                                 if !dest_dir.exists() {
                                     let _ = std::fs::create_dir_all(&dest_dir);
                                 }
                                 
                                 // Register signal
                                 let signal = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(crate::services::download::STATUS_RUNNING));
                                 
                                 {
                                     let signal_state = app_clone.state::<crate::commands::AppState>();
                                     let signals_res = signal_state.download_signals.write();
                                     if let Ok(mut signals) = signals_res {
                                         signals.insert(resource.id, signal.clone());
                                     }
                                 }

                                 tracing::info!("Queue starting download: {}", resource.title);
                                 
                                 // Emit download started event to frontend
                                 if let Err(e) = app_clone.emit("download-started", resource.id) {
                                     tracing::error!("Failed to emit download-started event for {}: {:?}", resource.id, e);
                                 } else {
                                     tracing::trace!("Emitted download-started event for resource {}", resource.id);
                                 }
                                 
                                 match download_service.download_resource(&resource, &dest_dir, Some(&app_clone), Some(signal), prefer_optimized).await {
                                    Ok((path, hash)) => {
                                        tracing::info!("Download completed successfully: {} -> {:?} (hash: {})", resource.title, path, hash);
                                        let _ = app_clone.emit("download-complete", resource.id);
                                    },
                                    Err(e) => {
                                        tracing::error!("Download failed for {}: {}", resource.title, e);
                                        // Emit failure event too?
                                        let _ = app_clone.emit("download-failed", serde_json::json!({"id": resource.id, "error": e.to_string()}));
                                    }
                                 }
                                 
                                 // Cleanup signal
                                 {
                                     let signal_state = app_clone.state::<crate::commands::AppState>();
                                     let signals_res = signal_state.download_signals.write();
                                     if let Ok(mut signals) = signals_res {
                                         signals.remove(&resource.id);
                                     }
                                 }
                             }
                         }
                         
                        let previous = active_count_clone.fetch_sub(1, Ordering::SeqCst);
                        tracing::trace!("Download worker finished. Active count decremented from {} to {}", previous, previous - 1);
                        
                        // Remove from active IDs
                        {
                            let mut ids = active_ids_clone.lock().await;
                            if let Some(pos) = ids.iter().position(|&id| id == resource.id) {
                                ids.remove(pos);
                            }
                        }
                    });
                    
                    // In parallel mode, immediately check for more tasks
                    // In queue mode, the limit check will prevent starting another
                    continue;
                } else {
                    // Queue is empty, wait a bit before checking again
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                }
            }
        });
    }
    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::SeqCst)
    }

    pub async fn queue_len(&self) -> usize {
        self.queue.lock().await.len()
    }
}
