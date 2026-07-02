//! Polling service for background API fetching
//!
//! Runs a background task using tokio to periodically poll the API.

use crate::commands::AppState;
use crate::constants::api_base_url;
use crate::models::{CategoriesCountResponse, ResourceListResponse, WeekIdentifier};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;
use tokio::time::{interval, sleep, Duration};

/// Polling service that runs in the background
pub struct PollingService {
    /// Cancellation sender for the *current* spawned task, or `None` when
    /// stopped. A fresh channel is created on every `start`: this way the
    /// receiver owned by a running task always belongs to the channel that
    /// task subscribed to, so a `stop` (or a quick `stop`+`start` in
    /// `restart`) cancels exactly that task without the coalescing race a
    /// single reused `watch` channel had (a `send(true)` immediately followed
    /// by a `send(false)` could collapse into a single unseen `false`, leaving
    /// the old task alive and leaking).
    cancel_tx: Mutex<Option<watch::Sender<bool>>>,
    /// Whether polling is currently running. Written only by the control
    /// methods (`start`/`stop`); the spawned task never touches it, so a
    /// dying old task can't clobber the flag of a freshly started one.
    is_running: AtomicBool,
}

impl PollingService {
    /// Create a new polling service
    pub fn new() -> Self {
        Self {
            cancel_tx: Mutex::new(None),
            is_running: AtomicBool::new(false),
        }
    }

    /// Start the polling background task
    pub fn start(&self, app: AppHandle, interval_mins: u32) {
        if self.is_running.load(Ordering::SeqCst) {
            tracing::warn!("Polling already running, ignoring start request");
            return;
        }

        // Each task gets its own cancel channel so cancellation targets this
        // task specifically and is immune to `restart` timing (see struct doc).
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        match self.cancel_tx.lock() {
            Ok(mut guard) => *guard = Some(cancel_tx),
            Err(_) => {
                tracing::error!("Polling cancel lock poisoned, not starting");
                return;
            }
        }
        self.is_running.store(true, Ordering::SeqCst);

        tauri::async_runtime::spawn(async move {
            tracing::info!(
                "Polling service started with interval {} minutes",
                interval_mins
            );

            // Dedicated cancel receiver for the retry backoffs (used by the
            // initial poll and by every tick). The loop's `select!` below
            // already borrows `cancel_rx` in its own arm, so the retry helper
            // can't borrow that same receiver a second time; this clone observes
            // the identical cancel signal.
            let mut retry_cancel_rx = cancel_rx.clone();

            // Poll immediately on startup so the user sees fresh data within
            // seconds instead of waiting a full `interval_mins` for the first
            // fetch. Retries (cancellably) on a cold gateway; a cancel arriving
            // during a startup backoff exits before the loop even begins.
            tracing::info!("Performing initial poll on startup");
            if let PollCycle::Cancelled =
                poll_once_with_cancellable_retry(&app, &mut retry_cancel_rx).await
            {
                tracing::info!("Polling cancelled during initial poll");
                return;
            }

            let duration = Duration::from_secs(interval_mins as u64 * 60);
            let mut ticker = interval(duration);

            // `interval` fires its first tick immediately upon creation; consume
            // it here so the periodic ticks below stay spaced by `duration`
            // starting after the initial poll above, instead of firing a second
            // poll back-to-back with it.
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        tracing::debug!("Polling tick (interval: {} minutes)", interval_mins);

                        // The retry backoffs live here (not in `poll_once`) so
                        // they are cancellable: a cancel during a backoff breaks
                        // out immediately instead of stalling the task for up to
                        // the whole schedule, which would let a `restart` spawn a
                        // second overlapping poller.
                        if let PollCycle::Cancelled =
                            poll_once_with_cancellable_retry(&app, &mut retry_cancel_rx).await
                        {
                            tracing::info!("Polling cancelled during retry backoff");
                            break;
                        }
                    }
                    // Fires on `stop`/`restart` (value set to `true`) or if the
                    // sender is dropped (service dropped at shutdown): either
                    // way this task must exit. Cancellation while idling on the
                    // tick is caught here; cancellation mid-poll/backoff is
                    // caught by `retry_cancel_rx` inside the handler above.
                    _ = cancel_rx.changed() => {
                        tracing::info!("Polling service cancelled");
                        break;
                    }
                }
            }

            tracing::info!("Polling service stopped");
        });
    }

    /// Stop the polling background task
    pub fn stop(&self) {
        let sender = match self.cancel_tx.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => {
                tracing::error!("Polling cancel lock poisoned during stop");
                None
            }
        };
        if let Some(tx) = sender {
            // Ignore the error: a dropped receiver just means the task already
            // exited on its own.
            let _ = tx.send(true);
        }
        self.is_running.store(false, Ordering::SeqCst);
    }

    /// Check if polling is currently running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Restart with a new interval
    pub fn restart(&self, app: AppHandle, new_interval_mins: u32) {
        self.stop();
        self.start(app, new_interval_mins);
    }
}

impl Default for PollingService {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait schedule for the polling loop's automatic retries of a failed poll
/// cycle: at most two extra attempts (10s then 30s) before giving up until the
/// next tick. A cold gateway routinely answers the first request with a
/// 504/HTML error page (which then fails the JSON decode); pausing and retrying
/// usually catches it once warm, sparing the user a blank screen — which on a
/// fresh install with a 60min interval would otherwise last an hour.
const POLL_RETRY_BACKOFFS: [Duration; 2] = [Duration::from_secs(10), Duration::from_secs(30)];

/// Result of a full poll cycle from the polling loop's perspective.
enum PollCycle {
    /// The cycle ended on its own — a poll succeeded, or every retry was spent.
    /// The loop moves on to the next tick.
    Finished,
    /// A cancel signal arrived during a retry backoff; the loop must exit.
    Cancelled,
}

/// Outcome of [`run_with_backoff`].
enum RetryOutcome<E> {
    /// One of the attempts succeeded.
    Succeeded,
    /// Every attempt failed; carries the last error for the caller to report.
    GaveUp(E),
    /// The cancel receiver fired during a backoff sleep — aborted immediately.
    Cancelled,
}

/// Run `attempt`, retrying on error along `backoffs`. The backoff sleeps sit
/// inside a `tokio::select!` on `cancel_rx`, so cancellation during a wait is
/// observed *immediately* rather than after the sleep elapses. Deliberately
/// free of any `AppHandle` so the retry/cancel control flow is unit-testable.
async fn run_with_backoff<F, Fut, T, E>(
    cancel_rx: &mut watch::Receiver<bool>,
    backoffs: &[Duration],
    mut attempt: F,
) -> RetryOutcome<E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_err = match attempt().await {
        Ok(_) => return RetryOutcome::Succeeded,
        Err(e) => e,
    };
    for &backoff in backoffs {
        tokio::select! {
            _ = sleep(backoff) => {}
            _ = cancel_rx.changed() => return RetryOutcome::Cancelled,
        }
        match attempt().await {
            Ok(_) => return RetryOutcome::Succeeded,
            Err(e) => last_err = e,
        }
    }
    RetryOutcome::GaveUp(last_err)
}

/// Run one `poll_once` cycle, retrying failures with the cancellable
/// `POLL_RETRY_BACKOFFS` schedule. `poll_once` itself is a single fail-fast
/// attempt, so `commands::force_poll` — which shares it — returns to the UI
/// immediately and never inherits these waits.
async fn poll_once_with_cancellable_retry(
    app: &AppHandle,
    cancel_rx: &mut watch::Receiver<bool>,
) -> PollCycle {
    match run_with_backoff(cancel_rx, &POLL_RETRY_BACKOFFS, || poll_once(app)).await {
        RetryOutcome::Succeeded => PollCycle::Finished,
        RetryOutcome::GaveUp(e) => {
            tracing::error!(
                "Polling gave up after {} retries: {}",
                POLL_RETRY_BACKOFFS.len(),
                e
            );
            let _ = app.emit("poll-error", e);
            PollCycle::Finished
        }
        RetryOutcome::Cancelled => PollCycle::Cancelled,
    }
}

/// Classify the latest-week HTTP status: any 2xx passes, anything else is a
/// clear `"API {status}"` error. Split out (with no I/O) so the retry policy is
/// unit-testable without a live server.
fn check_poll_status(status: reqwest::StatusCode) -> Result<(), String> {
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("API {}", status))
    }
}

/// Decode a latest-week body, logging the first ~300 chars on failure. The
/// caller passes the already-read body (via `response.text()`) precisely so a
/// malformed 2xx payload is available to diagnose here instead of being lost
/// inside `response.json()`.
fn parse_latest_week_body(body: &str) -> Result<ResourceListResponse, String> {
    serde_json::from_str(body).map_err(|e| {
        let preview: String = body.chars().take(300).collect();
        tracing::warn!("Poll decode failed: {}; body starts: {}", e, preview);
        format!("Failed to parse response: {}", e)
    })
}

/// Single latest-week fetch attempt: status is checked *before* decoding, and
/// the body is read as text first so a bad payload can be logged.
async fn fetch_latest_week(
    client: &reqwest::Client,
    url: &str,
) -> Result<ResourceListResponse, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    let status = response.status();
    if let Err(e) = check_poll_status(status) {
        tracing::warn!("Poll fetch returned non-success status: {}", status);
        return Err(e);
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    parse_latest_week_body(&body)
}

/// Perform one full poll cycle: fetch the latest week (a single fail-fast
/// attempt), invalidate the file-size cache for changed/removed URLs, update
/// state and status, persist `cache.json`, emit UI events, refresh the category
/// catalog, reconcile errata, scan for auto-downloads, and archive past weeks
/// on a week change. Shared by the background polling loop and
/// `commands::force_poll` so the two entry points never drift; the background
/// loop wraps this in `poll_once_with_cancellable_retry` for retries, while
/// `force_poll` runs it once so the UI never blocks on a backoff.
pub async fn poll_once(app: &AppHandle) -> Result<ResourceListResponse, String> {
    let state = app.state::<AppState>();
    let url = format!("{}/api/resources/latest-week", api_base_url());

    let api_response = fetch_latest_week(&state.shared_http_client, &url).await?;

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
        let current_urls: std::collections::HashSet<String> = api_response
            .resources
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

    // Set when this poll's resources belong to a different week than the
    // last known one, so we can archive the now-previous week(s) below
    // (bl-desktop-archiving-not-called) once the status write lock (a
    // non-Send std::sync guard) is released and out of scope.
    let mut new_current_week: Option<WeekIdentifier> = None;
    {
        let mut status = state.status.write().map_err(|e| e.to_string())?;
        status.last_poll_time = Some(chrono::Utc::now());
        status.total_resources = api_response.resources.len();

        if let Some(week) = crate::models::latest_week(&api_response.resources) {
            if status.current_week.as_ref() != Some(&week) {
                new_current_week = Some(week.clone());
            }
            status.current_week = Some(week);
        }
        status.material_week_stale =
            crate::models::is_material_week_stale(status.current_week.as_ref());
    }

    // Emit event to frontend
    let _ = app.emit("resources-updated", &api_response);
    let _ = app.emit("poll-tick", ());

    // Second, independent GET for the full category catalog (best-effort:
    // its own errors never fail the poll).
    refresh_categories(app).await;

    // Save to cache
    use tauri_plugin_store::StoreExt;
    let store = app.store("cache.json").map_err(|e| e.to_string())?;
    let json = serde_json::to_value(&api_response.resources).map_err(|e| e.to_string())?;
    store.set("resources", json);

    // Save file size cache (exclude negative cache entries from persistence)
    let cache_snapshot = {
        let cache = state.file_size_cache.read().map_err(|e| e.to_string())?;
        cache
            .iter()
            .filter(|(_, &size)| size != u64::MAX) // Exclude negative cache
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

    // Reconcile the errata registry against this fresh snapshot BEFORE the
    // auto-download scan: any re-queued errata corrige lands in the queue
    // first, so the scan's own check_file_exists pass is deduped instead of
    // racing a second download of the same file (adr-0007).
    crate::services::process_errata(app, &api_response.resources).await;

    // Initial check for auto-downloads
    state.download_queue.scan_and_queue(app.clone()).await;

    // The current week just changed: archive the folders of the now-past
    // week(s) so enforce_retention (already scheduled daily) has something
    // to trash after retention_days (bl-desktop-archiving-not-called).
    if let Some(week) = new_current_week {
        tracing::info!("Current week changed to {}, archiving previous weeks", week);
        crate::services::archive_previous_weeks_once(app, &week).await;
    }

    Ok(api_response)
}

/// A parsed-but-empty categories response (`{}` or
/// `{"categories":[],"total":0}` both deserialize fine thanks to
/// `#[serde(default)]`) must be treated like a network/parse failure rather
/// than a legitimate "no categories" answer: the endpoint is not expected to
/// ever genuinely empty out, so an empty list is far more likely a backend
/// deploy hiccup or stub response than reality. Applying it would blank the
/// catalog and drop out-of-week categories from Settings.
fn is_empty_categories_response(parsed: &CategoriesCountResponse) -> bool {
    parsed.categories.is_empty()
}

/// Fetch the full category catalog (`categories/counts`) and publish it to the
/// UI. Called from `poll_once` (shared by the background loop and
/// `force_poll`), kept out of the resource-fetch error path on purpose —
/// this is a best-effort enrichment. On any network failure, parse failure,
/// *or* a parsed-but-empty payload (see `is_empty_categories_response`) it
/// logs and leaves `AppState::all_categories` untouched (last-known values
/// stay usable offline/during a backend deploy) without emitting
/// `categories-updated`, so the failure is invisible to the user.
pub async fn refresh_categories(app: &AppHandle) {
    let state = app.state::<AppState>();
    let url = format!("{}/api/resources/categories/counts", api_base_url());

    let response = match state.shared_http_client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Categories fetch failed, keeping last known: {}", e);
            return;
        }
    };

    let parsed: CategoriesCountResponse = match response.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Categories parse failed, keeping last known: {}", e);
            return;
        }
    };

    if is_empty_categories_response(&parsed) {
        tracing::debug!("Categories response parsed but empty, keeping last known");
        return;
    }

    // Per-category debug line so a typo in the source data (e.g. "vidoe")
    // shows up as its own bogus category for the operator to spot.
    for cat in &parsed.categories {
        tracing::debug!("categoria {}: {} risorse", cat.name, cat.count);
    }

    match state.all_categories.write() {
        Ok(mut guard) => *guard = parsed.categories.clone(),
        Err(e) => {
            tracing::warn!("Categories state lock poisoned, not updating: {}", e);
            return;
        }
    }

    let _ = app.emit("categories-updated", &parsed.categories);
}

#[cfg(test)]
mod tests {
    use super::*;

    // `start`/`restart` need an `AppHandle`, which can't be constructed in a
    // unit test, so the running-loop behavior is verified manually (see the
    // "Polling service started/stopped" log lines). These tests cover the
    // control-flag lifecycle that does not require a task to be spawned.

    #[test]
    fn new_service_is_not_running() {
        let service = PollingService::new();
        assert!(!service.is_running());
    }

    #[test]
    fn stop_when_idle_is_a_noop() {
        let service = PollingService::new();
        // Stopping a service that was never started must not panic and must
        // leave it stopped (idempotent).
        service.stop();
        service.stop();
        assert!(!service.is_running());
    }

    #[test]
    fn default_matches_new() {
        assert!(!PollingService::default().is_running());
    }

    #[test]
    fn empty_categories_response_is_flagged_as_empty() {
        let empty: CategoriesCountResponse = serde_json::from_str(r#"{"categories":[],"total":0}"#)
            .expect("empty categories must parse");
        assert!(is_empty_categories_response(&empty));

        let bare: CategoriesCountResponse =
            serde_json::from_str("{}").expect("bare object must parse");
        assert!(is_empty_categories_response(&bare));
    }

    #[test]
    fn non_empty_categories_response_is_not_flagged_as_empty() {
        let json = r#"{"categories":[{"name":"decime","count":12}],"total":12}"#;
        let parsed: CategoriesCountResponse =
            serde_json::from_str(json).expect("categories must parse");
        assert!(!is_empty_categories_response(&parsed));
    }

    #[test]
    fn success_status_passes() {
        assert!(check_poll_status(reqwest::StatusCode::OK).is_ok());
        assert!(check_poll_status(reqwest::StatusCode::NO_CONTENT).is_ok());
    }

    #[test]
    fn non_success_status_is_a_clear_error() {
        // The cold-gateway case from the field: a 504 must surface as a clear,
        // status-bearing error rather than an opaque decode failure.
        let err = check_poll_status(reqwest::StatusCode::GATEWAY_TIMEOUT)
            .expect_err("504 must be an error");
        assert!(err.starts_with("API "), "error should name the API: {err}");
        assert!(err.contains("504"), "error should carry the status: {err}");
    }

    #[test]
    fn valid_body_decodes() {
        let parsed = parse_latest_week_body(r#"{"count":0,"resources":[]}"#)
            .expect("well-formed body must decode");
        assert_eq!(parsed.count, 0);
        assert!(parsed.resources.is_empty());
    }

    #[test]
    fn html_error_page_body_fails_to_decode() {
        // The gateway's 504 HTML page is what actually blew up `response.json()`
        // in production; here it must fail decode cleanly (and be logged).
        let err = parse_latest_week_body("<html><body>504 Gateway Timeout</body></html>")
            .expect_err("HTML body must not decode as the API response");
        assert!(err.starts_with("Failed to parse response"), "got: {err}");
    }

    #[tokio::test]
    async fn backoff_returns_on_first_success_without_retrying() {
        let (_tx, mut rx) = watch::channel(false);
        let mut calls = 0;
        let outcome: RetryOutcome<String> = run_with_backoff(
            &mut rx,
            &[Duration::from_secs(10), Duration::from_secs(30)],
            || {
                calls += 1;
                async { Ok::<(), String>(()) }
            },
        )
        .await;
        assert!(matches!(outcome, RetryOutcome::Succeeded));
        assert_eq!(calls, 1, "a first-attempt success must not retry");
    }

    #[tokio::test]
    async fn backoff_gives_up_after_exhausting_the_schedule() {
        let (_tx, mut rx) = watch::channel(false);
        let mut calls = 0;
        // Tiny backoffs keep the test fast; no cancel is ever sent.
        let outcome: RetryOutcome<i32> = run_with_backoff(
            &mut rx,
            &[Duration::from_millis(1), Duration::from_millis(1)],
            || {
                calls += 1;
                let n = calls;
                async move { Err::<(), i32>(n) }
            },
        )
        .await;
        match outcome {
            RetryOutcome::GaveUp(last) => assert_eq!(last, 3, "carries the last error"),
            _ => panic!("expected GaveUp"),
        }
        assert_eq!(calls, 3, "initial attempt + two retries");
    }

    #[tokio::test]
    async fn backoff_cancels_immediately_instead_of_waiting_it_out() {
        let (tx, mut rx) = watch::channel(false);
        // Cancel up front: the backoff `select!` must observe it and return
        // without sleeping out the (deliberately huge) backoff.
        tx.send(true).unwrap();
        let mut calls = 0;
        let outcome: RetryOutcome<()> =
            run_with_backoff(&mut rx, &[Duration::from_secs(3600)], || {
                calls += 1;
                async { Err::<(), ()>(()) }
            })
            .await;
        assert!(matches!(outcome, RetryOutcome::Cancelled));
        assert_eq!(calls, 1, "only the initial attempt runs before the cancel");
    }
}
