//! Errata Corrige detection logic
//!
//! This module handles detection of updated resources within the same week,
//! plus the downloaded-file registry that feeds it: the queue worker records
//! each successful download here (`record_downloaded_file`), and each poll
//! reconciles the registry against the fresh remote snapshot
//! (`process_errata`).

use crate::models::{DownloadedFile, ErrataChange, Resource, WeekIdentifier};
use crate::services::FileRetentionService;
use chrono::Utc;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Detect errata corrige changes by comparing local files with remote resources.
///
/// An errata corrige is detected when:
/// - A resource with the same ID exists locally
/// - The remote resource's created_at is newer than local downloaded_at
/// - Both are in the same week
pub fn detect_errata_changes(
    local_files: &[DownloadedFile],
    remote_resources: &[Resource],
) -> Vec<ErrataChange> {
    remote_resources
        .iter()
        .filter_map(|remote| {
            // Find local file with same resource ID in the same week
            local_files
                .iter()
                .find(|local| {
                    local.resource_id == remote.id
                        && local.week == remote.week()
                        && !local.is_superseded
                })
                .filter(|local| local.downloaded_at < remote.created_at)
                .map(|local| ErrataChange {
                    resource_id: remote.id,
                    old_file: local.clone(),
                    new_resource: remote.clone(),
                })
        })
        .collect()
}

/// Find resources that are new (not yet downloaded)
pub fn find_new_resources(
    local_files: &[DownloadedFile],
    remote_resources: &[Resource],
) -> Vec<Resource> {
    remote_resources
        .iter()
        .filter(|remote| {
            !local_files
                .iter()
                .any(|local| local.resource_id == remote.id && !local.is_superseded)
        })
        .cloned()
        .collect()
}

/// Upsert an entry into the registry, keyed by `resource_id` + `week` (pure).
///
/// A resource can only have one live record per week: a new download for the
/// same `(resource_id, week)` replaces whatever was there (including a
/// previously superseded entry), so re-downloading an errata corrige promotes
/// the fresh file back to the current record. Free-standing so it can be unit
/// tested without an `AppHandle`.
pub fn upsert_downloaded_file(registry: &mut Vec<DownloadedFile>, entry: DownloadedFile) {
    if let Some(existing) = registry
        .iter_mut()
        .find(|f| f.resource_id == entry.resource_id && f.week == entry.week)
    {
        *existing = entry;
    } else {
        registry.push(entry);
    }
}

/// Mark the registry entries targeted by `changes` as superseded (pure).
///
/// Only flips a matching, not-yet-superseded entry (same `resource_id` and the
/// change's `week`), so repeating the pass against an already-reconciled
/// registry is a stable no-op. Returns the ids actually flipped this call.
/// Free-standing for unit testing without an `AppHandle`.
fn mark_superseded(registry: &mut [DownloadedFile], changes: &[ErrataChange]) -> Vec<i64> {
    let mut marked = Vec::new();
    for change in changes {
        if let Some(entry) = registry.iter_mut().find(|f| {
            f.resource_id == change.resource_id && f.week == change.old_file.week && !f.is_superseded
        }) {
            entry.is_superseded = true;
            marked.push(change.resource_id);
        }
    }
    marked
}

/// Whether the registry holds a superseded file for `current_week` (pure).
///
/// Uses the state's notion of the current week (derived from
/// `latest_week` / `AppStatus.current_week`) rather than the wall-clock ISO
/// week, so the flag stays consistent with the rest of the status even when
/// the remote's latest week differs from today.
pub(crate) fn compute_has_superseded(
    registry: &[DownloadedFile],
    current_week: Option<&WeekIdentifier>,
) -> bool {
    match current_week {
        Some(week) => registry
            .iter()
            .any(|f| f.is_superseded && &f.week == week),
        None => false,
    }
}

/// Recompute `AppStatus.has_superseded_files` from `registry`, using the
/// current week already recorded in the status. Reads the current week and
/// writes the flag in two short, non-overlapping lock scopes (no lock held
/// across the other).
fn refresh_superseded_status(app: &AppHandle, registry: &[DownloadedFile]) {
    let state = app.state::<crate::commands::AppState>();
    let current_week = match state.status.read() {
        Ok(status) => status.current_week.clone(),
        Err(e) => {
            tracing::error!("Errata: failed to read status: {}", e);
            return;
        }
    };
    let has_superseded = compute_has_superseded(registry, current_week.as_ref());
    match state.status.write() {
        Ok(mut status) => status.has_superseded_files = has_superseded,
        Err(e) => tracing::error!("Errata: failed to update status: {}", e),
    };
}

/// Persist the whole registry snapshot to the `downloaded_files` key of
/// `cache.json`. Best-effort: logs on failure, never panics (persistence must
/// not take down a background poll/download).
fn persist_registry(app: &AppHandle, registry: &[DownloadedFile]) {
    use tauri_plugin_store::StoreExt;
    let store = match app.store("cache.json") {
        Ok(store) => store,
        Err(e) => {
            tracing::error!("Registry: failed to access cache store: {}", e);
            return;
        }
    };
    match serde_json::to_value(registry) {
        Ok(json) => {
            store.set("downloaded_files", json);
            if let Err(e) = store.save() {
                tracing::error!("Registry: failed to save downloaded_files: {}", e);
            }
        }
        Err(e) => tracing::error!("Registry: failed to serialize downloaded_files: {}", e),
    }
}

/// Producer (adr-0007 step 2): record a successfully downloaded file into the
/// registry and persist it. Called by the queue worker on the `Ok(...)` branch
/// of a download. Fully synchronous (no `.await`): the `std::sync` write guard
/// is held across the disk persist so a mutation and its on-disk snapshot stay
/// atomic — the last mutator under the exclusive lock is the last writer to
/// disk, preventing a concurrent producer/poll from overwriting it with a
/// stale snapshot.
pub fn record_downloaded_file(
    app: &AppHandle,
    resource: &Resource,
    local_path: PathBuf,
    prefer_optimized: bool,
) {
    let state = app.state::<crate::commands::AppState>();
    let snapshot = {
        let mut registry = match state.downloaded_files.write() {
            Ok(registry) => registry,
            Err(e) => {
                tracing::error!("Registry: failed to write downloaded_files: {}", e);
                return;
            }
        };
        let entry = DownloadedFile {
            resource_id: resource.id,
            week: resource.week(),
            local_path,
            downloaded_at: Utc::now(),
            source_url: resource
                .get_effective_download_url(prefer_optimized)
                .to_string(),
            is_superseded: false,
        };
        upsert_downloaded_file(&mut registry, entry);
        persist_registry(app, &registry);
        registry.clone()
    };
    // Re-downloading an errata corrige promotes a fresh, non-superseded record
    // (see upsert_downloaded_file), so the week may no longer have any
    // superseded file: reconcile the status flag against the new snapshot.
    refresh_superseded_status(app, &snapshot);
}

/// Consumer (adr-0007 step 3): reconcile the registry against the fresh remote
/// snapshot at the end of a successful poll.
///
/// For every detected errata corrige it archives the now-stale local file
/// (`FileRetentionService::archive_superseded`; an error is logged, never
/// fatal), marks the registry entry superseded and persists, and — if the
/// resource's category is enabled for auto-download — re-queues the new file
/// via the download queue (never a direct download, adr-0007). Updates
/// `AppStatus.has_superseded_files` for the current week and, if anything
/// changed, emits `errata-detected` with the affected resource ids.
///
/// Must run right before `scan_and_queue` in the poll path: the re-queue below
/// lands the resource in the queue first, so `scan_and_queue`'s own
/// `check_file_exists` pass is deduped by the queue instead of racing a second
/// download of the same file.
pub async fn process_errata(app: &AppHandle, remote: &[Resource]) {
    let state = app.state::<crate::commands::AppState>();

    // Snapshot the registry and the config bits we need up front, so no lock
    // is held across the archiving / re-queue work below.
    let registry_snapshot = match state.downloaded_files.read() {
        Ok(registry) => registry.clone(),
        Err(e) => {
            tracing::error!("Errata: failed to read downloaded_files: {}", e);
            return;
        }
    };

    let changes = detect_errata_changes(&registry_snapshot, remote);
    if changes.is_empty() {
        // No new supersessions, but still reconcile the flag: a re-download
        // since the last poll may have cleared the last superseded entry for
        // the current week.
        refresh_superseded_status(app, &registry_snapshot);
        return;
    }

    let (work_dir, auto_categories) = match state.config.read() {
        Ok(config) => (
            config.work_directory.clone(),
            config.auto_download_categories.clone(),
        ),
        Err(e) => {
            tracing::error!("Errata: failed to read config: {}", e);
            return;
        }
    };

    let Some(work_dir) = work_dir else {
        tracing::debug!("Errata: work directory not configured, skipping reconciliation");
        return;
    };

    tracing::info!("Errata: {} change(s) detected, reconciling", changes.len());

    // Archive each stale file and collect the resources to re-download.
    let service = FileRetentionService::new(work_dir);
    let mut to_redownload: Vec<Resource> = Vec::new();
    for change in &changes {
        match service.archive_superseded(&change.old_file.local_path, &change.old_file.week) {
            Ok(archived) => tracing::info!(
                "Errata: archived superseded file for resource {} -> {:?}",
                change.resource_id,
                archived
            ),
            Err(e) => tracing::error!(
                "Errata: failed to archive superseded file for resource {}: {}",
                change.resource_id,
                e
            ),
        }
        if auto_categories.contains(&change.new_resource.category) {
            to_redownload.push(change.new_resource.clone());
        }
    }

    // Mark superseded and persist under the same write guard: no `.await`
    // runs between the mutation and the disk write, so the last mutator holds
    // the exclusive lock through its own persist and a concurrent producer
    // cannot overwrite the file with a stale snapshot (lost update).
    let (snapshot, marked_ids) = {
        let mut registry = match state.downloaded_files.write() {
            Ok(registry) => registry,
            Err(e) => {
                tracing::error!("Errata: failed to write downloaded_files: {}", e);
                return;
            }
        };
        let marked = mark_superseded(&mut registry, &changes);
        persist_registry(app, &registry);
        (registry.clone(), marked)
    };

    // Reflect superseded files of the current week in the status, using the
    // week already tracked in the status (latest_week), not the wall clock.
    refresh_superseded_status(app, &snapshot);

    // Re-queue the updated files through the queue only (adr-0007).
    for resource in to_redownload {
        state.download_queue.add_task(app.clone(), resource).await;
    }

    if !marked_ids.is_empty() {
        if let Err(e) = app.emit(
            "errata-detected",
            serde_json::json!({ "resourceIds": marked_ids }),
        ) {
            tracing::error!("Errata: failed to emit errata-detected: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WeekIdentifier;
    use chrono::{TimeZone, Utc};
    use std::path::PathBuf;

    fn create_resource(id: i64, created_at: chrono::DateTime<Utc>) -> Resource {
        Resource {
            id,
            category: "test".to_string(),
            title: format!("Resource {}", id),
            description: Some("Test resource".to_string()),
            download_url: format!("https://example.com/file_{}.zip", id),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at,
            optimized_video_url: None,
            optimized_videos: None,
        }
    }

    fn create_downloaded_file(
        resource_id: i64,
        week: WeekIdentifier,
        downloaded_at: chrono::DateTime<Utc>,
    ) -> DownloadedFile {
        DownloadedFile {
            resource_id,
            week,
            local_path: PathBuf::from(format!("/downloads/file_{}.zip", resource_id)),
            downloaded_at,
            source_url: format!("https://example.com/file_{}.zip", resource_id),
            is_superseded: false,
        }
    }

    #[test]
    fn test_no_errata_when_no_local_files() {
        let local: Vec<DownloadedFile> = vec![];
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let remote = vec![create_resource(1, dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_no_errata_when_same_timestamp() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        let local = vec![create_downloaded_file(1, week, dt)];
        let remote = vec![create_resource(1, dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_errata_detected_when_remote_is_newer() {
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let local = vec![create_downloaded_file(1, week.clone(), original_dt)];
        let remote = vec![create_resource(1, updated_dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource_id, 1);
        assert_eq!(changes[0].old_file.downloaded_at, original_dt);
        assert_eq!(changes[0].new_resource.created_at, updated_dt);
    }

    #[test]
    fn test_no_errata_when_local_is_newer() {
        // This shouldn't happen in practice, but test defensive behavior
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let remote_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let local = vec![create_downloaded_file(1, week, original_dt)];
        let remote = vec![create_resource(1, remote_dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_multiple_errata_detected() {
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let local = vec![
            create_downloaded_file(1, week.clone(), original_dt),
            create_downloaded_file(2, week.clone(), original_dt),
            create_downloaded_file(3, week.clone(), original_dt),
        ];
        let remote = vec![
            create_resource(1, updated_dt),  // Updated
            create_resource(2, original_dt), // Not updated
            create_resource(3, updated_dt),  // Updated
        ];

        let changes = detect_errata_changes(&local, &remote);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| c.resource_id == 1));
        assert!(changes.iter().any(|c| c.resource_id == 3));
        assert!(!changes.iter().any(|c| c.resource_id == 2));
    }

    #[test]
    fn test_superseded_files_are_ignored() {
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let mut superseded_file = create_downloaded_file(1, week, original_dt);
        superseded_file.is_superseded = true;
        let local = vec![superseded_file];
        let remote = vec![create_resource(1, updated_dt)];

        // Should NOT detect errata for already superseded files
        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_find_new_resources_empty_local() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let remote = vec![create_resource(1, dt), create_resource(2, dt)];

        let new = find_new_resources(&[], &remote);
        assert_eq!(new.len(), 2);
    }

    #[test]
    fn test_find_new_resources_some_existing() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        let local = vec![create_downloaded_file(1, week, dt)];
        let remote = vec![
            create_resource(1, dt),
            create_resource(2, dt),
            create_resource(3, dt),
        ];

        let new = find_new_resources(&local, &remote);
        assert_eq!(new.len(), 2);
        assert!(new.iter().any(|r| r.id == 2));
        assert!(new.iter().any(|r| r.id == 3));
        assert!(!new.iter().any(|r| r.id == 1));
    }

    #[test]
    fn test_find_new_resources_ignores_superseded() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        let mut superseded = create_downloaded_file(1, week, dt);
        superseded.is_superseded = true;
        let local = vec![superseded];
        let remote = vec![create_resource(1, dt)];

        // Resource 1 should be considered new since existing one is superseded
        let new = find_new_resources(&local, &remote);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].id, 1);
    }

    /// Guards the idempotency requirement from bl-desktop-first-poll-skipped: an
    /// immediate poll on app startup (instead of waiting a full interval) means
    /// two near-simultaneous polls (e.g. the app restarted twice in a row) can
    /// run this created_at/downloaded_at comparison against an unchanged remote
    /// snapshot. Repeating the comparison must be a pure, stable read: no
    /// resource should be "discovered" as new or as an errata corrige more than
    /// once, and results must not change between runs with the same input.
    #[test]
    fn test_repeated_poll_detection_is_idempotent() {
        let downloaded_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(downloaded_dt);

        let local = vec![
            create_downloaded_file(1, week.clone(), downloaded_dt), // still current
            create_downloaded_file(2, week.clone(), downloaded_dt), // has an errata corrige
        ];
        let remote = vec![
            create_resource(1, downloaded_dt),
            create_resource(2, updated_dt),
            create_resource(3, downloaded_dt), // brand new resource
        ];

        // Simulate two back-to-back polls (e.g. immediate startup poll fired
        // twice in a row) by running detection twice against the same snapshot.
        let errata_first = detect_errata_changes(&local, &remote);
        let errata_second = detect_errata_changes(&local, &remote);
        assert_eq!(
            errata_first, errata_second,
            "repeated detection against the same snapshot must be stable"
        );
        assert_eq!(
            errata_first.len(),
            1,
            "only resource 2 has an errata corrige"
        );
        assert_eq!(errata_first[0].resource_id, 2);

        let new_first = find_new_resources(&local, &remote);
        let new_second = find_new_resources(&local, &remote);
        assert_eq!(
            new_first, new_second,
            "repeated detection against the same snapshot must be stable"
        );
        assert_eq!(new_first.len(), 1, "only resource 3 is new");
        assert_eq!(new_first[0].id, 3);
    }

    #[test]
    fn test_upsert_inserts_new_entry() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);
        let mut registry: Vec<DownloadedFile> = Vec::new();

        upsert_downloaded_file(&mut registry, create_downloaded_file(1, week, dt));

        assert_eq!(registry.len(), 1);
        assert_eq!(registry[0].resource_id, 1);
    }

    #[test]
    fn test_upsert_replaces_same_resource_and_week() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let later = Utc.with_ymd_and_hms(2026, 1, 19, 15, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        // Start with a superseded entry (as after an errata corrige).
        let mut old = create_downloaded_file(1, week.clone(), dt);
        old.is_superseded = true;
        let mut registry = vec![old];

        // Re-download promotes a fresh, non-superseded record in its place.
        upsert_downloaded_file(&mut registry, create_downloaded_file(1, week, later));

        assert_eq!(registry.len(), 1, "same (resource_id, week) must not duplicate");
        assert!(!registry[0].is_superseded);
        assert_eq!(registry[0].downloaded_at, later);
    }

    #[test]
    fn test_upsert_keeps_distinct_weeks_separate() {
        let dt_w4 = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let dt_w5 = Utc.with_ymd_and_hms(2026, 1, 26, 10, 0, 0).unwrap();
        let mut registry = vec![create_downloaded_file(
            1,
            WeekIdentifier::from_datetime(dt_w4),
            dt_w4,
        )];

        upsert_downloaded_file(
            &mut registry,
            create_downloaded_file(1, WeekIdentifier::from_datetime(dt_w5), dt_w5),
        );

        assert_eq!(
            registry.len(),
            2,
            "same resource in a different week is a separate record"
        );
    }

    #[test]
    fn test_mark_superseded_flips_matching_entry_once() {
        let downloaded_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(downloaded_dt);

        let mut registry = vec![create_downloaded_file(1, week.clone(), downloaded_dt)];
        let remote = vec![create_resource(1, updated_dt)];
        let changes = detect_errata_changes(&registry, &remote);
        assert_eq!(changes.len(), 1);

        let marked = mark_superseded(&mut registry, &changes);
        assert_eq!(marked, vec![1]);
        assert!(registry[0].is_superseded);

        // Idempotent: re-detecting now yields nothing (superseded is ignored),
        // and re-marking flips nothing further.
        let changes_again = detect_errata_changes(&registry, &remote);
        assert!(changes_again.is_empty());
        let marked_again = mark_superseded(&mut registry, &changes_again);
        assert!(marked_again.is_empty());
    }

    #[test]
    fn test_compute_has_superseded_scopes_to_current_week() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);
        let other_week = WeekIdentifier::new(2025, 52);

        let mut superseded = create_downloaded_file(1, week.clone(), dt);
        superseded.is_superseded = true;
        let registry = vec![superseded];

        // Flag is set only for the week the superseded file belongs to.
        assert!(compute_has_superseded(&registry, Some(&week)));
        assert!(!compute_has_superseded(&registry, Some(&other_week)));
        // No current week → nothing to reflect.
        assert!(!compute_has_superseded(&registry, None));
    }

    #[test]
    fn test_compute_has_superseded_ignores_live_entries() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);
        let registry = vec![create_downloaded_file(1, week.clone(), dt)];

        // A live (non-superseded) entry must not raise the flag.
        assert!(!compute_has_superseded(&registry, Some(&week)));
    }

    #[test]
    fn test_mark_superseded_ignores_unknown_change() {
        let downloaded_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(downloaded_dt);

        // Registry holds resource 2, but the change targets resource 1.
        let mut registry = vec![create_downloaded_file(2, week.clone(), downloaded_dt)];
        let change = ErrataChange {
            resource_id: 1,
            old_file: create_downloaded_file(1, week, downloaded_dt),
            new_resource: create_resource(1, updated_dt),
        };

        let marked = mark_superseded(&mut registry, &[change]);
        assert!(marked.is_empty());
        assert!(!registry[0].is_superseded, "unrelated entry must stay live");
    }
}
