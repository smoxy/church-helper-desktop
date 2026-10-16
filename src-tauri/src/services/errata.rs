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

/// Detect errata corrige published under a NEW resource id (contract C:
/// `supersedes_resource_id`), e.g. the source changed the file URL so the
/// server created a new row instead of updating the old one.
///
/// Two complementary rules, both only ever targeting LIVE (not superseded)
/// registry entries, deduplicated per entry:
///
/// 1. **Explicit**: a remote resource R with `supersedes_resource_id == X`
///    supersedes the live entry for resource X (whatever week the entry was
///    recorded in). The server said so explicitly, so this holds even if X is
///    (transiently) still listed; the scan then skips X via
///    [`superseded_ids`] so it is never re-downloaded.
/// 2. **Belt-and-braces**: a live entry E (week W, category C, id X) whose id
///    is no longer in `remote`, while `remote` contains a resource R of the
///    same week W and category C that has NO live registry entry yet (i.e. a
///    replacement appeared). This covers chains where the intermediate link
///    was never downloaded here: registry holds A, server now lists only C
///    (C supersedes B, B superseded A). Entries without a recorded category
///    (written by older builds) are never matched by this rule unless
///    [`backfill_categories`] could fill it first, and since R must come
///    from `remote`, a week whose fetch failed (empty list) can never
///    trigger it.
///
/// Returned changes carry the OLD id in `resource_id` (so `mark_superseded`
/// flips the old entry) and the replacement in `new_resource`.
pub fn detect_superseded_by_new_id(
    local_files: &[DownloadedFile],
    remote_resources: &[Resource],
) -> Vec<ErrataChange> {
    use std::collections::HashSet;

    let remote_ids: HashSet<i64> = remote_resources.iter().map(|r| r.id).collect();
    let live_ids: HashSet<i64> = local_files
        .iter()
        .filter(|f| !f.is_superseded)
        .map(|f| f.resource_id)
        .collect();

    let mut changes: Vec<ErrataChange> = Vec::new();
    let mut targeted: HashSet<(i64, WeekIdentifier)> = HashSet::new();

    // Rule 1: explicit supersedes_resource_id.
    for remote in remote_resources {
        let Some(old_id) = remote.supersedes_resource_id else {
            continue;
        };
        if old_id == remote.id {
            continue; // malformed self-reference, ignore
        }
        for local in local_files
            .iter()
            .filter(|f| f.resource_id == old_id && !f.is_superseded)
        {
            if targeted.insert((local.resource_id, local.week.clone())) {
                changes.push(ErrataChange {
                    resource_id: local.resource_id,
                    old_file: local.clone(),
                    new_resource: remote.clone(),
                });
            }
        }
    }

    // Rule 2: same (week, category) slot taken over by a brand-new id.
    for local in local_files.iter().filter(|f| !f.is_superseded) {
        if remote_ids.contains(&local.resource_id) {
            continue;
        }
        let Some(category) = local.category.as_deref() else {
            continue;
        };
        if targeted.contains(&(local.resource_id, local.week.clone())) {
            continue;
        }
        let replacement = remote_resources
            .iter()
            .filter(|r| {
                r.category == category && r.week() == local.week && !live_ids.contains(&r.id)
            })
            .max_by_key(|r| r.id);
        if let Some(replacement) = replacement {
            targeted.insert((local.resource_id, local.week.clone()));
            changes.push(ErrataChange {
                resource_id: local.resource_id,
                old_file: local.clone(),
                new_resource: replacement.clone(),
            });
        }
    }

    changes
}

/// Fill the missing `category` of registry entries written by builds that
/// did not record it (v0.3.0), so Rule 2 of [`detect_superseded_by_new_id`]
/// can see them too (pure). `remote` is the current server list (latest ∪
/// next).
///
/// - The entry's resource id is still listed: take that resource's category.
/// - Otherwise match by (week, file basename) against the listed resources
///   that are part of a superseded chain (`supersedes_resource_id` set): an
///   errata that kept the file name (same URL basename) reveals the slot the
///   old entry belonged to. The basename of the entry is taken from its local
///   path and its source URL; the remote one from the raw and the optimized
///   URL. Filled only when every match agrees on one category.
/// - No match, or ambiguous: left as is.
///
/// Returns how many entries were filled.
pub fn backfill_categories(registry: &mut [DownloadedFile], remote: &[Resource]) -> usize {
    use crate::services::download::extract_filename_from_url;
    use std::collections::HashSet;

    let mut filled = 0;
    for entry in registry.iter_mut().filter(|f| f.category.is_none()) {
        if let Some(listed) = remote.iter().find(|r| r.id == entry.resource_id) {
            entry.category = Some(listed.category.clone());
            filled += 1;
            continue;
        }

        let entry_names: HashSet<String> = entry
            .local_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .into_iter()
            .chain(extract_filename_from_url(&entry.source_url))
            .collect();
        if entry_names.is_empty() {
            continue;
        }

        let categories: HashSet<&str> = remote
            .iter()
            .filter(|r| r.supersedes_resource_id.is_some() && r.week() == entry.week)
            .filter(|r| {
                std::iter::once(r.download_url.as_str())
                    .chain(r.optimized_video_url.as_deref())
                    .filter_map(extract_filename_from_url)
                    .any(|name| entry_names.contains(&name))
            })
            .map(|r| r.category.as_str())
            .collect();
        if categories.len() == 1 {
            entry.category = categories.into_iter().next().map(str::to_string);
            filled += 1;
        }
    }
    filled
}

/// Ids of resources in `resources` that another listed resource explicitly
/// supersedes (`supersedes_resource_id`). A server listing both the old and
/// the new row is a transient inconsistency: the old one must never be
/// (re-)downloaded, so the auto-download scan skips these ids.
pub fn superseded_ids(resources: &[Resource]) -> std::collections::HashSet<i64> {
    resources
        .iter()
        .filter_map(|r| r.supersedes_resource_id.filter(|&old| old != r.id))
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
            f.resource_id == change.resource_id
                && f.week == change.old_file.week
                && !f.is_superseded
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
        Some(week) => registry.iter().any(|f| f.is_superseded && &f.week == week),
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
            category: Some(resource.category.clone()),
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
    // is held across the archiving / re-queue work below. Entries written by
    // older builds get their category backfilled first (persisted under the
    // same write guard, like every other registry mutation).
    let registry_snapshot = match state.downloaded_files.write() {
        Ok(mut registry) => {
            let filled = backfill_categories(&mut registry, remote);
            if filled > 0 {
                tracing::info!("Errata: backfilled the category of {} entries", filled);
                persist_registry(app, &registry);
            }
            registry.clone()
        }
        Err(e) => {
            tracing::error!("Errata: failed to write downloaded_files: {}", e);
            return;
        }
    };

    let mut changes = detect_errata_changes(&registry_snapshot, remote);
    for change in detect_superseded_by_new_id(&registry_snapshot, remote) {
        let already = changes.iter().any(|c| {
            c.old_file.resource_id == change.old_file.resource_id
                && c.old_file.week == change.old_file.week
        });
        if !already {
            changes.push(change);
        }
    }
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
            week_date: None,
            optimized_video_url: None,
            optimized_videos: None,
            supersedes_resource_id: None,
            title_original: None,
            duration_s: None,
            storyboard: None,
            tags: Vec::new(),
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
            category: None,
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

        assert_eq!(
            registry.len(),
            1,
            "same (resource_id, week) must not duplicate"
        );
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

    // -- errata with a new id (contract C: supersedes_resource_id) ----------

    /// Week 42 of 2026: Saturday 2026-10-17.
    fn w42() -> WeekIdentifier {
        WeekIdentifier::new(2026, 42)
    }

    fn uicca_resource(id: i64, category: &str, supersedes: Option<i64>) -> Resource {
        let mut r = create_resource(id, Utc.with_ymd_and_hms(2026, 10, 16, 9, 0, 0).unwrap());
        r.category = category.to_string();
        r.week_date = chrono::NaiveDate::from_ymd_opt(2026, 10, 17);
        r.supersedes_resource_id = supersedes;
        r
    }

    fn live_entry(id: i64, category: Option<&str>) -> DownloadedFile {
        let mut f = create_downloaded_file(
            id,
            w42(),
            Utc.with_ymd_and_hms(2026, 10, 16, 10, 0, 0).unwrap(),
        );
        f.category = category.map(str::to_string);
        f
    }

    /// Simulates what the queue worker records once the replacement lands.
    fn downloaded(r: &Resource) -> DownloadedFile {
        let mut f = live_entry(r.id, Some(&r.category));
        f.downloaded_at = Utc.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap();
        f
    }

    #[test]
    fn test_explicit_supersedes_targets_the_old_entry() {
        let registry = vec![live_entry(1, Some("missioni"))];
        let b = uicca_resource(2, "missioni", Some(1));

        let changes = detect_superseded_by_new_id(&registry, std::slice::from_ref(&b));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource_id, 1, "the OLD id is flipped");
        assert_eq!(changes[0].new_resource.id, 2, "the replacement is queued");
    }

    /// Full chain A -> B -> C observed poll by poll: each step supersedes
    /// exactly the immediately preceding live entry, never twice.
    #[test]
    fn test_supersedes_chain_a_b_c_step_by_step() {
        let mut registry = vec![live_entry(1, Some("missioni"))];

        // Poll 1: server lists B (supersedes A).
        let b = uicca_resource(2, "missioni", Some(1));
        let changes = detect_superseded_by_new_id(&registry, std::slice::from_ref(&b));
        assert_eq!(mark_superseded(&mut registry, &changes), vec![1]);
        upsert_downloaded_file(&mut registry, downloaded(&b));

        // Same snapshot again: idempotent, nothing new.
        assert!(detect_superseded_by_new_id(&registry, std::slice::from_ref(&b)).is_empty());

        // Poll 2: server lists C (supersedes B, the immediate predecessor).
        let c = uicca_resource(3, "missioni", Some(2));
        let changes = detect_superseded_by_new_id(&registry, std::slice::from_ref(&c));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource_id, 2);
        assert_eq!(changes[0].new_resource.id, 3);
        assert_eq!(mark_superseded(&mut registry, &changes), vec![2]);
        upsert_downloaded_file(&mut registry, downloaded(&c));

        let live: Vec<i64> = registry
            .iter()
            .filter(|f| !f.is_superseded)
            .map(|f| f.resource_id)
            .collect();
        assert_eq!(live, vec![3], "only C stays live");
        assert!(detect_superseded_by_new_id(&registry, std::slice::from_ref(&c)).is_empty());
    }

    /// The intermediate link B was never seen here (offline during its
    /// lifetime): registry holds A, the server now lists only C (supersedes
    /// B). The explicit rule finds nothing (no B entry); the belt-and-braces
    /// rule must still retire A because its (week, category) slot was taken
    /// over by a new id.
    #[test]
    fn test_supersedes_chain_with_missed_intermediate_uses_belt_and_braces() {
        let registry = vec![live_entry(1, Some("missioni"))];
        let c = uicca_resource(3, "missioni", Some(2));

        let changes = detect_superseded_by_new_id(&registry, std::slice::from_ref(&c));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource_id, 1);
        assert_eq!(changes[0].new_resource.id, 3);
    }

    #[test]
    fn test_belt_and_braces_requires_a_replacement_in_the_same_slot() {
        let registry = vec![live_entry(1, Some("missioni"))];

        // Failed/empty fetch: nothing to compare against, never retire.
        assert!(detect_superseded_by_new_id(&registry, &[]).is_empty());

        // Only another category in that week: not a replacement.
        let other_cat = uicca_resource(5, "offerte", None);
        assert!(detect_superseded_by_new_id(&registry, &[other_cat]).is_empty());

        // Same category but another week: not a replacement.
        let mut next_week = uicca_resource(6, "missioni", None);
        next_week.week_date = chrono::NaiveDate::from_ymd_opt(2026, 10, 24);
        assert!(detect_superseded_by_new_id(&registry, &[next_week]).is_empty());

        // The old id is still listed: nothing disappeared.
        let still_there = uicca_resource(1, "missioni", None);
        let newcomer = uicca_resource(7, "missioni", None);
        assert!(detect_superseded_by_new_id(&registry, &[still_there, newcomer]).is_empty());
    }

    #[test]
    fn test_belt_and_braces_ignores_legacy_entries_and_already_live_replacements() {
        // Legacy entry without a recorded category: never matched.
        let legacy = vec![live_entry(1, None)];
        let newcomer = uicca_resource(7, "missioni", None);
        assert!(detect_superseded_by_new_id(&legacy, std::slice::from_ref(&newcomer)).is_empty());

        // The would-be replacement is already downloaded (live): not "new".
        let registry = vec![live_entry(1, Some("missioni")), downloaded(&newcomer)];
        assert!(detect_superseded_by_new_id(&registry, &[newcomer]).is_empty());
    }

    // -- category backfill for v0.3.0 entries (D7) ---------------------------

    /// v0.3.0 entry: no category, raw file saved under `basename`.
    fn legacy_entry(id: i64, basename: &str) -> DownloadedFile {
        let mut f = live_entry(id, None);
        f.local_path = PathBuf::from(format!("/work/W42-2026-10-17/{basename}"));
        f.source_url = format!("https://uicca.example/2026/10/{basename}");
        f
    }

    fn with_url(mut r: Resource, url: &str) -> Resource {
        r.download_url = url.to_string();
        r
    }

    #[test]
    fn test_backfill_from_listed_resource_id() {
        let mut registry = vec![legacy_entry(1, "mis-04-X.zip")];
        let a = uicca_resource(1, "missioni", None);

        assert_eq!(backfill_categories(&mut registry, &[a]), 1);
        assert_eq!(registry[0].category.as_deref(), Some("missioni"));
        // Idempotent: already filled entries are left alone.
        let a = uicca_resource(1, "missioni", None);
        assert_eq!(backfill_categories(&mut registry, &[a]), 0);
    }

    /// The v0.3.0 scenario from the clustering report: registry holds A
    /// (category None), the server now lists only C (supersedes B, never seen
    /// here) with the same basename. The backfill finds the slot through C,
    /// and Rule 2 then retires A in favour of C.
    #[test]
    fn test_backfill_by_week_and_basename_enables_rule_2_for_legacy_chain() {
        let mut registry = vec![legacy_entry(1, "mis-04-X.zip")];
        let c = with_url(
            uicca_resource(3, "missioni", Some(2)),
            "https://uicca.example/2026/11/mis-04-X.zip",
        );

        // Before the backfill Rule 2 cannot see the legacy entry.
        assert!(detect_superseded_by_new_id(&registry, std::slice::from_ref(&c)).is_empty());

        assert_eq!(
            backfill_categories(&mut registry, std::slice::from_ref(&c)),
            1
        );
        assert_eq!(registry[0].category.as_deref(), Some("missioni"));

        let changes = detect_superseded_by_new_id(&registry, std::slice::from_ref(&c));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource_id, 1);
        assert_eq!(changes[0].new_resource.id, 3);
    }

    #[test]
    fn test_backfill_leaves_unmatched_or_ambiguous_entries() {
        // Different basename: no match.
        let mut registry = vec![legacy_entry(1, "mis-04-X.zip")];
        let other = with_url(
            uicca_resource(3, "missioni", Some(2)),
            "https://uicca.example/2026/11/mis-04-Y.zip",
        );
        assert_eq!(backfill_categories(&mut registry, &[other]), 0);
        assert!(registry[0].category.is_none());

        // Same basename but not part of a superseded chain: no match.
        let plain = with_url(
            uicca_resource(3, "missioni", None),
            "https://uicca.example/2026/11/mis-04-X.zip",
        );
        assert_eq!(backfill_categories(&mut registry, &[plain]), 0);

        // Same basename in another week: no match.
        let mut next_week = with_url(
            uicca_resource(3, "missioni", Some(2)),
            "https://uicca.example/2026/11/mis-04-X.zip",
        );
        next_week.week_date = chrono::NaiveDate::from_ymd_opt(2026, 10, 24);
        assert_eq!(backfill_categories(&mut registry, &[next_week]), 0);

        // Two chains of different categories share the basename: ambiguous.
        let m = with_url(
            uicca_resource(3, "missioni", Some(2)),
            "https://uicca.example/a/mis-04-X.zip",
        );
        let d = with_url(
            uicca_resource(5, "decime", Some(4)),
            "https://uicca.example/b/mis-04-X.zip",
        );
        assert_eq!(backfill_categories(&mut registry, &[m, d]), 0);
        assert!(registry[0].category.is_none());

        // Nothing listed at all (failed fetch): nothing filled.
        assert_eq!(backfill_categories(&mut registry, &[]), 0);
    }

    #[test]
    fn test_superseded_ids_collects_explicit_predecessors() {
        let a = uicca_resource(1, "missioni", None);
        let b = uicca_resource(2, "missioni", Some(1));
        let selfref = uicca_resource(9, "offerte", Some(9));
        let ids = superseded_ids(&[a, b, selfref]);
        assert!(ids.contains(&1));
        assert!(!ids.contains(&2));
        assert!(!ids.contains(&9), "self-reference is ignored");
    }
}
