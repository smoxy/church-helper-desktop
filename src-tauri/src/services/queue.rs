//! Download Queue Service
//!
//! Manages a queue of download tasks, executing them sequentially or in parallel
//! based on the configuration.

use crate::models::{DownloadMode, Resource, WeekIdentifier};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, Notify};

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
    /// Week each currently-active download belongs to, keyed by resource id
    /// (mirrors `active_ids`'s push/remove lifecycle). Only needed so
    /// `weeks_with_pending_downloads` can tell the archiving pass
    /// (bl-desktop-archiving-not-called) which week folders are unsafe to
    /// move right now; kept separate from `active_ids` because that field's
    /// shape (`Vec<i64>`) is part of the `queue-status-changed` wire event
    /// consumed by the frontend and must not change.
    active_weeks: Arc<Mutex<HashMap<i64, WeekIdentifier>>>,
    /// Wakes the worker when there may be new work: a task was queued, a slot
    /// was freed by a finished download, or the mode changed the concurrency
    /// limit. The worker parks on `notified()` whenever the queue is empty or
    /// at the concurrency limit, so it no longer busy-waits.
    notify: Arc<Notify>,
}

/// Pure enqueue guard (A2): a resource may be queued only if it is neither
/// already queued nor already downloading. Kept free-standing so it can be
/// unit-tested without an `AppHandle`.
fn can_enqueue(queue: &VecDeque<Resource>, active_ids: &[i64], id: i64) -> bool {
    !active_ids.contains(&id) && !queue.iter().any(|r| r.id == id)
}

/// Pure queue removal (A5): drops `id` from `queue` in place and reports
/// whether anything was actually removed. Free-standing for unit testing
/// without an `AppHandle`.
fn drain_queued(queue: &mut VecDeque<Resource>, id: i64) -> bool {
    let before = queue.len();
    queue.retain(|r| r.id != id);
    queue.len() != before
}

/// Concurrency limit implied by the download mode. Free-standing so the
/// worker's slot arithmetic can be unit-tested without spawning it.
fn concurrency_limit(mode: &DownloadMode) -> usize {
    match mode {
        DownloadMode::Queue => 1,
        DownloadMode::Parallel => 4,
    }
}

/// Pure savings computation (A1): bytes saved by downloading the optimized
/// variant instead of the original. `None` whenever either size is unknown,
/// or when the "original" doesn't actually turn out larger (a stale/wrong
/// cached size, or a "optimized" variant that isn't actually smaller) — a
/// saving is never reported as negative or zero. Free-standing so it's
/// unit-testable without an `AppHandle`.
fn compute_saved_bytes(original_bytes: Option<u64>, optimized_bytes: Option<u64>) -> Option<u64> {
    match (original_bytes, optimized_bytes) {
        (Some(original), Some(optimized)) if original > optimized => Some(original - optimized),
        _ => None,
    }
}

/// Cache-only read of the *original* (non-optimized) file's size, for the
/// immediate A1 savings computation at `download-complete` time. Never makes
/// a network request — a cache miss returns `None` rather than blocking the
/// event on a HEAD request; see `resolve_original_size_bytes` for the
/// network-fallback used by the detached `savings-resolved` follow-up.
/// Filters out the `u64::MAX` negative-cache sentinel (see
/// `AppState::file_size_cache`'s doc comment), since that means "never
/// successfully HEAD-ed", not "known to be 0 bytes".
fn cached_original_size_bytes(app: &AppHandle, url: &str) -> Option<u64> {
    let state = app.state::<crate::commands::AppState>();
    state
        .file_size_cache
        .read()
        .ok()
        .and_then(|cache| cache.get(url).copied())
        .filter(|&size| size != u64::MAX)
}

/// Best-effort resolution of the *original* (non-optimized) file's size, for
/// A1 savings reporting.
///
/// Checks `AppState::file_size_cache` first (filtering out its `u64::MAX`
/// negative-cache sentinel — see the field's doc comment — since that means
/// "never successfully HEAD-ed", not "known to be 0 bytes"). On a miss, falls
/// back to a HEAD request bounded by a short timeout so a slow/unreachable
/// origin can never stall the caller. Only a successful outcome is written
/// back to the cache: unlike `get_file_size`, this path deliberately does NOT
/// negative-cache a failure here, since a transient blip shouldn't poison a
/// future on-demand lookup (e.g. the resource detail view opening moments
/// later). Called only from the DETACHED `savings-resolved` background task
/// (see `start_worker`), never inline in the download body, so its up-to-5s
/// latency never delays `download-complete` or holds a worker slot.
async fn resolve_original_size_bytes(app: &AppHandle, url: &str) -> Option<u64> {
    let state = app.state::<crate::commands::AppState>();

    if let Ok(cache) = state.file_size_cache.read() {
        if let Some(&size) = cache.get(url) {
            if size != u64::MAX {
                return Some(size);
            }
        }
    }

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.shared_http_client.head(url).send(),
    )
    .await
    .ok()?
    .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())?;

    if let Ok(mut cache) = state.file_size_cache.write() {
        cache.insert(url.to_string(), size);
    }

    Some(size)
}

impl Default for DownloadQueue {
    fn default() -> Self {
        Self::new()
    }
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
            active_weeks: Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Weeks that currently have a download in flight — either actively
    /// downloading right now, or still queued waiting for a worker slot.
    /// Consulted by the archiving pass (bl-desktop-archiving-not-called) so
    /// it never moves a week's folder while a download could still be
    /// writing into it.
    pub async fn weeks_with_pending_downloads(&self) -> HashSet<WeekIdentifier> {
        let queue = self.queue.lock().await;
        let active_weeks = self.active_weeks.lock().await;
        let mut weeks: HashSet<WeekIdentifier> = queue.iter().map(|r| r.week()).collect();
        weeks.extend(active_weeks.values().cloned());
        weeks
    }

    /// Update the concurrency limit based on mode
    pub async fn update_mode(&self, mode: DownloadMode) {
        let changed = {
            let mut current_mode = self.mode.lock().await;
            if *current_mode != mode {
                *current_mode = mode;
                true
            } else {
                false
            }
        };
        // Raising the limit (e.g. Queue -> Parallel) frees slots, so the worker
        // must re-evaluate; a lower limit is a harmless spurious wake.
        if changed {
            self.notify.notify_one();
        }
    }

    /// Add a resource to the queue and trigger processing
    pub async fn add_task(&self, app: AppHandle, resource: Resource) {
        {
            let mut queue = self.queue.lock().await;
            let active = self.active_ids.lock().await;
            // A2: skip if already queued OR already downloading. Without the
            // `active_ids` check a poll landing mid-download would re-enqueue
            // the same resource — its `.part` doesn't trip `check_file_exists`,
            // so two tasks would write the same file concurrently.
            if can_enqueue(&queue, &active, resource.id) {
                queue.push_back(resource);
                tracing::info!("Added task to queue. Queue size: {}", queue.len());
            } else {
                tracing::trace!(
                    "Skipping enqueue for resource {}: already queued or active",
                    resource.id
                );
            }
        }
        self.emit_queue_status(&app).await;
        self.notify.notify_one();
        self.ensure_worker_started(app).await;
    }

    /// Add a resource to the queue with priority (for manual downloads)
    /// Priority tasks are added to the front of the queue
    pub async fn add_task_priority(&self, app: AppHandle, resource: Resource) {
        {
            let mut queue = self.queue.lock().await;
            let active = self.active_ids.lock().await;
            // A2: never front-jump a resource that's already downloading —
            // that would spawn a second concurrent write to the same file.
            // (Queue duplicates are handled below by `retain`.)
            if active.contains(&resource.id) {
                tracing::trace!(
                    "Skipping priority enqueue for resource {}: already active",
                    resource.id
                );
            } else {
                // Remove if already exists (to avoid duplicates)
                queue.retain(|r| r.id != resource.id);
                // Add to front for priority
                queue.push_front(resource);
            }
        }
        self.emit_queue_status(&app).await;
        self.notify.notify_one();
        self.ensure_worker_started(app).await;
    }

    /// Remove a still-queued resource and notify the frontend (A5).
    ///
    /// Returns `true` if an item was actually removed. Cancelling a resource
    /// that hasn't started downloading yet used to only set its download
    /// signal, which is a no-op for something not in `download_signals`, so
    /// the item stayed in the queue and reappeared on the next status emit.
    pub async fn remove_queued(&self, app: &AppHandle, id: i64) -> bool {
        let removed = {
            let mut queue = self.queue.lock().await;
            drain_queued(&mut queue, id)
        };
        if removed {
            self.emit_queue_status(app).await;
            self.notify.notify_one();
        }
        removed
    }

    /// Emit current queue status to frontend
    async fn emit_queue_status(&self, app: &AppHandle) {
        let queue = self.queue.lock().await;
        let active = self.active_ids.lock().await;

        // Create list of queued items with their position
        let queued_items: Vec<serde_json::Value> = queue
            .iter()
            .enumerate()
            .map(|(i, r)| {
                serde_json::json!({
                    "id": r.id,
                    "position": i + 1
                })
            })
            .collect();

        let payload = serde_json::json!({
            "queued": queued_items,
            "active": *active
        });

        if let Err(e) = app.emit("queue-status-changed", payload) {
            tracing::error!("Failed to emit queue-status-changed: {:?}", e);
        }
    }

    /// Ensure the worker is started (idempotent: the CAS lets exactly one
    /// caller win and spawn it).
    async fn ensure_worker_started(&self, app: AppHandle) {
        if self
            .worker_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.start_worker(app).await;
        }
    }

    /// scan resources and add to queue if matching auto-download criteria
    pub async fn scan_and_queue(&self, app: AppHandle) {
        let state = app.state::<crate::commands::AppState>();

        // Read config and resources. A poisoned lock is a non-recoverable
        // internal invariant break; log and skip this scan rather than panic
        // (no-unwrap guard) — the next poll/scan will retry.
        let (config, resources) = {
            let config = match state.config.read() {
                Ok(config) => config.clone(),
                Err(e) => {
                    tracing::error!("scan_and_queue: config lock poisoned, skipping scan: {}", e);
                    return;
                }
            };
            let resources = match state.resources.read() {
                Ok(resources) => resources.clone(),
                Err(e) => {
                    tracing::error!(
                        "scan_and_queue: resources lock poisoned, skipping scan: {}",
                        e
                    );
                    return;
                }
            };
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
                    let is_downloaded =
                        crate::services::download::DownloadService::check_file_exists(
                            &resource,
                            work_dir,
                            config.prefer_optimized,
                        );
                    if !is_downloaded {
                        tracing::trace!(
                            "Queuing for auto-download: {} ({})",
                            resource.title,
                            resource.category
                        );
                        self.add_task(app.clone(), resource).await;
                        queued_count += 1;
                    }
                }
            }
            tracing::info!(
                "Auto-download scan complete: {} resources queued",
                queued_count
            );
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
        let active_weeks = self.active_weeks.clone();
        let notify = self.notify.clone();

        tracing::info!("Download queue worker started");

        // Spawn a detached task to manage coordination. This task never exits;
        // instead of busy-waiting it parks on `notify.notified()` whenever it
        // can make no progress (queue empty or at the concurrency limit). The
        // producers (add_task*, remove_queued), a mode change, and every
        // finished download's `notify_one` wake it back up.
        tauri::async_runtime::spawn(async move {
            loop {
                // Determine concurrency limit
                let limit = {
                    let mode = mode_lock.lock().await;
                    concurrency_limit(&mode)
                };

                // Check if we can start more downloads
                let current_active = active_count.load(Ordering::SeqCst);

                if current_active >= limit {
                    // At capacity: park until a slot frees up or the limit
                    // grows. A `notify_one` issued before this point is latched
                    // by `Notify`, so a completion racing this check is not lost.
                    notify.notified().await;
                    continue;
                }

                // Try to get next task from queue. Register it in `active_ids`
                // AND `active_weeks` while still holding the queue lock, so the
                // transition out of the queue is atomic: a concurrent
                // `add_task` running `can_enqueue` never observes a window where
                // the resource is neither queued nor active (which would
                // re-enqueue it into a double download), and the archiving pass
                // (weeks_with_pending_downloads) never sees the week as free
                // while a folder is about to be written into. Lock order
                // queue→active_ids matches `add_task` to avoid deadlock.
                let resource = {
                    let mut q = queue.lock().await;
                    let popped = q.pop_front();
                    if let Some(resource) = &popped {
                        active_ids.lock().await.push(resource.id);
                        active_weeks
                            .lock()
                            .await
                            .insert(resource.id, resource.week());
                    }
                    popped
                };

                if let Some(resource) = resource {
                    // We have a task and have capacity, start it
                    active_count.fetch_add(1, Ordering::SeqCst);

                    let active_count_clone = active_count.clone();
                    let active_ids_clone = active_ids.clone();
                    let active_weeks_clone = active_weeks.clone();
                    let notify_clone = notify.clone();
                    let app_clone = app.clone();
                    // Separate handle for the supervisor: its cleanup must run
                    // even if `app_clone` is moved into the download body below.
                    let app_super = app.clone();
                    let resource_id = resource.id;

                    // Emit status update immediately as queue changed (popped item) AND active changed
                    {
                        let q = queue.lock().await;
                        let a = active_ids.lock().await;
                        let queued_items: Vec<serde_json::Value> = q
                            .iter()
                            .enumerate()
                            .map(|(i, r)| {
                                serde_json::json!({
                                    "id": r.id,
                                    "position": i + 1
                                })
                            })
                            .collect();
                        let payload = serde_json::json!({
                            "queued": queued_items,
                            "active": *a
                        });
                        let _ = app_clone.emit("queue-status-changed", payload);
                    }

                    // A4: supervise the download body so bookkeeping is ALWAYS
                    // reconciled — even if the body panics. Previously the
                    // `fetch_sub`/`active_ids` cleanup lived inside the body, so
                    // a panic left `active_count` permanently inflated and the
                    // worker stalled once it hit the concurrency limit.
                    tauri::async_runtime::spawn(async move {
                        let body = tauri::async_runtime::spawn(async move {
                            // Execute download
                            // Resolve state at the top level of the task
                            let state = app_clone.state::<crate::commands::AppState>();

                            if let Ok(config) = crate::commands::get_config(state) {
                                if let Some(work_dir) = config.work_directory {
                                    let download_service =
                                        crate::services::DownloadService::with_client(
                                            app_clone
                                                .state::<crate::commands::AppState>()
                                                .shared_http_client
                                                .clone(),
                                        );
                                    let prefer_optimized = config.prefer_optimized;
                                    let dest_dir = crate::services::download::resolve_week_dir(
                                        &resource,
                                        &work_dir,
                                        prefer_optimized,
                                    );

                                    if !dest_dir.exists() {
                                        let _ = std::fs::create_dir_all(&dest_dir);
                                    }

                                    // Register signal
                                    let signal =
                                        std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
                                            crate::services::download::STATUS_RUNNING,
                                        ));

                                    {
                                        let signal_state =
                                            app_clone.state::<crate::commands::AppState>();
                                        let signals_res = signal_state.download_signals.write();
                                        if let Ok(mut signals) = signals_res {
                                            signals.insert(resource.id, signal.clone());
                                        }
                                    }

                                    tracing::info!("Queue starting download: {}", resource.title);

                                    // Emit download started event to frontend
                                    if let Err(e) = app_clone.emit("download-started", resource.id)
                                    {
                                        tracing::error!(
                                            "Failed to emit download-started event for {}: {:?}",
                                            resource.id,
                                            e
                                        );
                                    } else {
                                        tracing::trace!(
                                            "Emitted download-started event for resource {}",
                                            resource.id
                                        );
                                    }

                                    match download_service
                                        .download_resource(
                                            &resource,
                                            &dest_dir,
                                            Some(&app_clone),
                                            Some(signal),
                                            prefer_optimized,
                                        )
                                        .await
                                    {
                                        Ok((path, hash)) => {
                                            tracing::info!("Download completed successfully: {} -> {:?} (hash: {})", resource.title, path, hash);

                                            // The frontend needs to know whether the
                                            // *actually downloaded* URL was an optimized
                                            // variant (auto-downloads never enter the
                                            // frontend's activeDownloads map, so it cannot
                                            // derive this itself). `resource` here is the
                                            // effective resource (manual variant picks
                                            // included), so this also covers non-default
                                            // picks.
                                            let optimized = resource
                                                .get_effective_download_url(prefer_optimized)
                                                != resource.download_url;

                                            // A1: savings are only meaningful when the
                                            // optimized variant is what actually landed
                                            // on disk — a non-optimized download has
                                            // nothing to compare against. Read before
                                            // `record_downloaded_file` moves `path`.
                                            let optimized_bytes = if optimized {
                                                tokio::fs::metadata(&path)
                                                    .await
                                                    .ok()
                                                    .map(|m| m.len())
                                            } else {
                                                None
                                            };

                                            // adr-0007 step 2: record the file in the
                                            // errata registry so a later poll can
                                            // detect it being superseded.
                                            crate::services::record_downloaded_file(
                                                &app_clone,
                                                &resource,
                                                path,
                                                prefer_optimized,
                                            );

                                            // A1: the original size is only ever read
                                            // from the shared HEAD-size cache here — NOT
                                            // fetched over the network. A blocking HEAD
                                            // request (up to 5s) used to run inline in
                                            // this task body, delaying both the
                                            // `download-complete` event and the worker
                                            // slot freeing (active_count isn't
                                            // decremented until this body returns). When
                                            // the size isn't cached yet, the event below
                                            // reports `original_bytes`/`saved_bytes` as
                                            // `null` and a DETACHED task (spawned further
                                            // down) resolves it in the background and
                                            // emits `savings-resolved` once it lands.
                                            let original_bytes = if optimized {
                                                cached_original_size_bytes(
                                                    &app_clone,
                                                    &resource.download_url,
                                                )
                                            } else {
                                                None
                                            };

                                            let saved_bytes = compute_saved_bytes(
                                                original_bytes,
                                                optimized_bytes,
                                            );

                                            // A2: fold this download's savings (if any)
                                            // into the persistent running total; when
                                            // there's nothing to add, still report its
                                            // current value so the UI's counter never
                                            // goes stale relative to the backend. Only
                                            // counted here when already known — a `None`
                                            // saved_bytes with `optimized` true is folded
                                            // in later by the detached resolution task
                                            // below instead, never both (no double count).
                                            let total_saved_bytes = match saved_bytes {
                                                Some(bytes) => crate::commands::add_saved_bytes(
                                                    &app_clone, bytes,
                                                ),
                                                None => crate::commands::current_saved_bytes(
                                                    &app_clone.state::<crate::commands::AppState>(),
                                                ),
                                            };

                                            let _ = app_clone.emit(
                                                "download-complete",
                                                serde_json::json!({
                                                    "id": resource.id,
                                                    "optimized": optimized,
                                                    "optimized_bytes": optimized_bytes,
                                                    "original_bytes": original_bytes,
                                                    "saved_bytes": saved_bytes,
                                                    "total_saved_bytes": total_saved_bytes,
                                                }),
                                            );

                                            // Original size wasn't cached: resolve it in
                                            // a task detached from this body (never
                                            // awaited here), so the up-to-5s HEAD request
                                            // cannot delay the worker slot freeing above.
                                            // Needs optimized_bytes too (already known) to
                                            // ever compute a saving.
                                            if optimized && original_bytes.is_none() {
                                                if let Some(optimized_bytes) = optimized_bytes {
                                                    let app_detached = app_clone.clone();
                                                    let download_url =
                                                        resource.download_url.clone();
                                                    let resource_id = resource.id;
                                                    tauri::async_runtime::spawn(async move {
                                                        let Some(resolved_original) =
                                                            resolve_original_size_bytes(
                                                                &app_detached,
                                                                &download_url,
                                                            )
                                                            .await
                                                        else {
                                                            return;
                                                        };
                                                        let resolved_saved = compute_saved_bytes(
                                                            Some(resolved_original),
                                                            Some(optimized_bytes),
                                                        );
                                                        // Counted exactly once: the
                                                        // immediate emission above only
                                                        // added to the total when
                                                        // saved_bytes was already known,
                                                        // which is not the branch we're
                                                        // in (original_bytes was None).
                                                        let resolved_total =
                                                            match resolved_saved {
                                                                Some(bytes) => {
                                                                    crate::commands::add_saved_bytes(
                                                                        &app_detached, bytes,
                                                                    )
                                                                }
                                                                None => {
                                                                    crate::commands::current_saved_bytes(
                                                                        &app_detached.state::<crate::commands::AppState>(),
                                                                    )
                                                                }
                                                            };
                                                        let _ = app_detached.emit(
                                                            "savings-resolved",
                                                            serde_json::json!({
                                                                "id": resource_id,
                                                                "saved_bytes": resolved_saved,
                                                                "original_bytes": resolved_original,
                                                                "total_saved_bytes": resolved_total,
                                                            }),
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                        Err(crate::error::DownloadError::Paused) => {
                                            tracing::info!("Download paused: {}", resource.title);
                                            let _ = app_clone.emit("download-paused", resource.id);
                                        }
                                        Err(crate::error::DownloadError::Cancelled) => {
                                            tracing::info!(
                                                "Download cancelled: {}",
                                                resource.title
                                            );
                                            let _ =
                                                app_clone.emit("download-cancelled", resource.id);
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Download failed for {}: {}",
                                                resource.title,
                                                e
                                            );
                                            let _ = app_clone.emit("download-failed", serde_json::json!({"id": resource.id, "error": e.to_string()}));
                                        }
                                    }
                                }
                            }
                        });

                        // A4: this cleanup runs unconditionally — including when
                        // the body panicked (surfaced here as a JoinError).
                        if let Err(join_err) = body.await {
                            tracing::error!(
                                "Download task for resource {} panicked: {:?}",
                                resource_id,
                                join_err
                            );
                            let _ = app_super.emit(
                                "download-failed",
                                serde_json::json!({"id": resource_id, "error": "internal error"}),
                            );
                        }

                        let previous = active_count_clone.fetch_sub(1, Ordering::SeqCst);
                        tracing::trace!(
                            "Download worker finished. Active count decremented from {} to {}",
                            previous,
                            previous.saturating_sub(1)
                        );
                        // A slot just freed: wake the worker so it can pull the
                        // next queued task. Must follow `fetch_sub` so the woken
                        // worker observes the decremented count.
                        notify_clone.notify_one();

                        // Remove from active IDs
                        {
                            let mut ids = active_ids_clone.lock().await;
                            if let Some(pos) = ids.iter().position(|&id| id == resource_id) {
                                ids.remove(pos);
                            }
                        }
                        {
                            let mut weeks = active_weeks_clone.lock().await;
                            weeks.remove(&resource_id);
                        }
                        // Guaranteed signal removal: the body registers the
                        // signal, so a panic before its own cleanup would leak
                        // it in `download_signals` without this.
                        {
                            let signal_state = app_super.state::<crate::commands::AppState>();
                            let signals_res = signal_state.download_signals.write();
                            if let Ok(mut signals) = signals_res {
                                signals.remove(&resource_id);
                            }
                        }
                    });

                    // In parallel mode, immediately check for more tasks
                    // In queue mode, the limit check will prevent starting another
                    continue;
                } else {
                    // Queue is empty: park until a producer enqueues something.
                    // An enqueue's `notify_one` racing this branch is latched by
                    // `Notify`, so the wakeup is not lost.
                    notify.notified().await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn make_resource(id: i64, year: i32, month: u32, day: u32) -> Resource {
        Resource {
            id,
            category: "test".to_string(),
            title: format!("Resource {}", id),
            description: None,
            download_url: format!("https://example.com/{}.zip", id),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).unwrap(),
            week_date: None,
            optimized_video_url: None,
            optimized_videos: None,
        }
    }

    #[tokio::test]
    async fn test_weeks_with_pending_downloads_empty_when_idle() {
        let dq = DownloadQueue::new();
        assert!(dq.weeks_with_pending_downloads().await.is_empty());
    }

    #[tokio::test]
    async fn test_weeks_with_pending_downloads_includes_queued_resources() {
        let dq = DownloadQueue::new();
        {
            let mut queue = dq.queue.lock().await;
            queue.push_back(make_resource(1, 2026, 1, 19)); // week 4
        }

        let weeks = dq.weeks_with_pending_downloads().await;
        assert_eq!(weeks.len(), 1);
        assert!(weeks.contains(&WeekIdentifier::new(2026, 4)));
    }

    #[tokio::test]
    async fn test_weeks_with_pending_downloads_includes_active_downloads() {
        let dq = DownloadQueue::new();
        {
            // Simulates what start_worker records once a download actually
            // starts (see the `active_weeks.insert` next to `ids.push`
            // above): by then the resource has already left `queue`.
            let mut active = dq.active_weeks.lock().await;
            active.insert(42, WeekIdentifier::new(2025, 52));
        }

        let weeks = dq.weeks_with_pending_downloads().await;
        assert_eq!(weeks.len(), 1);
        assert!(weeks.contains(&WeekIdentifier::new(2025, 52)));
    }

    #[test]
    fn test_can_enqueue_rejects_active_resource() {
        // A2: a resource currently downloading must not be re-queued, even
        // though it's not present in the (waiting) queue.
        let queue: VecDeque<Resource> = VecDeque::new();
        let active = vec![7_i64];
        assert!(!can_enqueue(&queue, &active, 7));
    }

    #[test]
    fn test_can_enqueue_rejects_already_queued_resource() {
        let mut queue: VecDeque<Resource> = VecDeque::new();
        queue.push_back(make_resource(3, 2026, 1, 19));
        let active: Vec<i64> = Vec::new();
        assert!(!can_enqueue(&queue, &active, 3));
    }

    #[test]
    fn test_can_enqueue_accepts_new_resource() {
        let mut queue: VecDeque<Resource> = VecDeque::new();
        queue.push_back(make_resource(1, 2026, 1, 19));
        let active = vec![2_i64];
        assert!(can_enqueue(&queue, &active, 3));
    }

    #[test]
    fn test_compute_saved_bytes_both_known_and_original_larger() {
        assert_eq!(compute_saved_bytes(Some(1000), Some(600)), Some(400));
    }

    #[test]
    fn test_compute_saved_bytes_original_missing_is_none() {
        // The A1 bug this guards against: no cached/HEAD-able original size
        // must not panic or fabricate a saving, just report unknown.
        assert_eq!(compute_saved_bytes(None, Some(600)), None);
    }

    #[test]
    fn test_compute_saved_bytes_optimized_missing_is_none() {
        assert_eq!(compute_saved_bytes(Some(1000), None), None);
    }

    #[test]
    fn test_compute_saved_bytes_both_missing_is_none() {
        assert_eq!(compute_saved_bytes(None, None), None);
    }

    #[test]
    fn test_compute_saved_bytes_original_not_larger_is_none() {
        // A stale/wrong cached "original" size that is actually smaller (or
        // equal) than what was downloaded must never report a saving.
        assert_eq!(compute_saved_bytes(Some(600), Some(600)), None);
        assert_eq!(compute_saved_bytes(Some(500), Some(600)), None);
    }

    #[test]
    fn test_concurrency_limit_matches_mode() {
        // The worker's slot arithmetic depends on these exact values (1 vs 4);
        // the busy-wait removal did not change them.
        assert_eq!(concurrency_limit(&DownloadMode::Queue), 1);
        assert_eq!(concurrency_limit(&DownloadMode::Parallel), 4);
    }

    #[test]
    fn test_drain_queued_removes_present_resource() {
        let mut queue: VecDeque<Resource> = VecDeque::new();
        queue.push_back(make_resource(1, 2026, 1, 19));
        queue.push_back(make_resource(2, 2026, 1, 19));

        assert!(drain_queued(&mut queue, 1));
        assert_eq!(queue.len(), 1);
        assert!(queue.iter().all(|r| r.id != 1));
    }

    #[test]
    fn test_drain_queued_reports_false_when_absent() {
        let mut queue: VecDeque<Resource> = VecDeque::new();
        queue.push_back(make_resource(1, 2026, 1, 19));

        assert!(!drain_queued(&mut queue, 99));
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn test_weeks_with_pending_downloads_merges_queued_and_active() {
        let dq = DownloadQueue::new();
        {
            let mut queue = dq.queue.lock().await;
            queue.push_back(make_resource(1, 2026, 1, 19)); // week 4
        }
        {
            let mut active = dq.active_weeks.lock().await;
            active.insert(2, WeekIdentifier::new(2025, 52));
        }

        let weeks = dq.weeks_with_pending_downloads().await;
        assert_eq!(weeks.len(), 2);
        assert!(weeks.contains(&WeekIdentifier::new(2026, 4)));
        assert!(weeks.contains(&WeekIdentifier::new(2025, 52)));
    }
}
