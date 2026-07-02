//! File retention service
//!
//! Handles archiving of old week files and retention policy enforcement.

use crate::error::FileError;
use crate::models::WeekIdentifier;
use chrono::{Duration, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;
use tokio::time::{interval, Duration as TokioDuration};

/// Archive directory name
const ARCHIVE_DIR: &str = ".archive";
/// Superseded files subdirectory within week archive
const SUPERSEDED_DIR: &str = ".superseded";
/// How often the background scheduler re-checks the retention policy.
const RETENTION_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // once a day
/// Startup grace period before the first retention run, so it doesn't
/// contend with the rest of app initialization (frontend listener
/// registration, the initial poll). Mirrors the delay already used for the
/// auto-download scan in `lib.rs`.
const STARTUP_DELAY_SECS: u64 = 5;

/// Service for managing file retention and archiving
pub struct FileRetentionService {
    work_dir: PathBuf,
}

impl FileRetentionService {
    /// Create a new FileRetentionService
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    /// Get the archive directory path
    pub fn archive_dir(&self) -> PathBuf {
        self.work_dir.join(ARCHIVE_DIR)
    }

    /// Get the archive path for a specific week
    pub fn week_archive_path(&self, week: &WeekIdentifier) -> PathBuf {
        self.archive_dir().join(week.as_dir_name())
    }

    /// Get the superseded files path for a specific week
    pub fn superseded_path(&self, week: &WeekIdentifier) -> PathBuf {
        self.week_archive_path(week).join(SUPERSEDED_DIR)
    }

    /// Archive a file for a previous week
    ///
    /// Moves the file from work_dir to .archive/{week}/
    pub fn archive_file(
        &self,
        file_path: &Path,
        week: &WeekIdentifier,
    ) -> Result<PathBuf, FileError> {
        let archive_path = self.week_archive_path(week);

        // Create archive directory if it doesn't exist
        fs::create_dir_all(&archive_path).map_err(|e| FileError::CreateDirectoryFailed {
            path: archive_path.clone(),
            source: e,
        })?;

        // Get filename and construct destination
        let file_name = file_path
            .file_name()
            .ok_or_else(|| FileError::MoveFileFailed {
                from: file_path.to_path_buf(),
                to: archive_path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid filename"),
            })?;
        let dest_path = archive_path.join(file_name);

        // Move the file
        fs::rename(file_path, &dest_path).map_err(|e| FileError::MoveFileFailed {
            from: file_path.to_path_buf(),
            to: dest_path.clone(),
            source: e,
        })?;

        Ok(dest_path)
    }

    /// Move a superseded file to the superseded directory
    ///
    /// Moves from work_dir to .archive/{week}/.superseded/
    pub fn archive_superseded(
        &self,
        file_path: &Path,
        week: &WeekIdentifier,
    ) -> Result<PathBuf, FileError> {
        let superseded_path = self.superseded_path(week);

        // Create superseded directory if it doesn't exist
        fs::create_dir_all(&superseded_path).map_err(|e| FileError::CreateDirectoryFailed {
            path: superseded_path.clone(),
            source: e,
        })?;

        let file_name = file_path
            .file_name()
            .ok_or_else(|| FileError::MoveFileFailed {
                from: file_path.to_path_buf(),
                to: superseded_path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid filename"),
            })?;
        let dest_path = superseded_path.join(file_name);

        fs::rename(file_path, &dest_path).map_err(|e| FileError::MoveFileFailed {
            from: file_path.to_path_buf(),
            to: dest_path.clone(),
            source: e,
        })?;

        Ok(dest_path)
    }

    /// Get all archived weeks
    pub fn get_archived_weeks(&self) -> Vec<WeekIdentifier> {
        let archive_dir = self.archive_dir();

        if !archive_dir.exists() {
            return Vec::new();
        }

        fs::read_dir(&archive_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .filter_map(|e| parse_week_dir_name(e.file_name().to_str()?))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Enforce retention policy
    ///
    /// - retention_days = None: Keep forever
    /// - retention_days = Some(0): Delete immediately (move to trash)
    /// - retention_days = Some(n): Move to trash after n days
    ///
    /// Returns the number of weeks moved to trash
    pub fn enforce_retention(&self, retention_days: Option<u32>) -> Result<u32, FileError> {
        let retention_days = match retention_days {
            None => {
                tracing::debug!("Retention policy is 'keep forever', nothing to enforce");
                return Ok(0);
            }
            Some(days) => days,
        };

        let cutoff_date = Utc::now() - Duration::days(retention_days as i64);
        let archived_weeks = self.get_archived_weeks();
        tracing::debug!(
            "Enforcing retention policy: {} archived week(s) found in {:?}, retention_days={}, cutoff={}",
            archived_weeks.len(),
            self.archive_dir(),
            retention_days,
            cutoff_date.to_rfc3339()
        );
        let mut deleted_count = 0;

        for week in archived_weeks {
            let week_path = self.week_archive_path(&week);

            // Check if the week is old enough to delete
            if let Ok(metadata) = fs::metadata(&week_path) {
                if let Ok(modified) = metadata.modified() {
                    let modified_datetime: chrono::DateTime<Utc> = modified.into();

                    if modified_datetime < cutoff_date {
                        // Move to system trash
                        trash::delete(&week_path).map_err(|e| FileError::TrashFailed {
                            path: week_path.clone(),
                            source: e,
                        })?;
                        tracing::info!(
                            "Retention: moved archived week {} to trash (archived {}, older than {} day(s))",
                            week,
                            modified_datetime.to_rfc3339(),
                            retention_days
                        );
                        deleted_count += 1;
                    } else {
                        tracing::trace!(
                            "Retention: keeping archived week {} (archived {}, within {} day(s))",
                            week,
                            modified_datetime.to_rfc3339(),
                            retention_days
                        );
                    }
                }
            }
        }

        if deleted_count > 0 {
            tracing::info!(
                "Retention enforcement complete: {} archived week(s) moved to trash",
                deleted_count
            );
        } else {
            tracing::debug!("Retention enforcement complete: nothing old enough to trash");
        }

        Ok(deleted_count)
    }

    /// Check if there are superseded files for a given week
    pub fn has_superseded_files(&self, week: &WeekIdentifier) -> bool {
        let path = self.superseded_path(week);
        path.exists()
            && fs::read_dir(&path)
                .map(|rd| rd.count() > 0)
                .unwrap_or(false)
    }

    /// Get list of superseded files for a week
    pub fn get_superseded_files(&self, week: &WeekIdentifier) -> Vec<PathBuf> {
        let path = self.superseded_path(week);

        if !path.exists() {
            return Vec::new();
        }

        fs::read_dir(&path)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Move previous weeks' folders out of the work directory into
    /// `.archive/{week}/`, so `enforce_retention` (which only ever looks at
    /// `.archive/`) has something to actually act on
    /// (bl-desktop-archiving-not-called: previously `archive_file` had no
    /// production call site, so `.archive/` stayed empty forever and old
    /// week folders were never cleaned up).
    ///
    /// - Never touches `current_week`'s folder.
    /// - Never touches a week in `busy_weeks` (per the download queue —
    ///   caller passes `DownloadQueue::weeks_with_pending_downloads`), and as
    ///   an extra filesystem-level safety net never moves a `.part` file
    ///   (download.rs's own in-progress/resume marker) even inside an
    ///   otherwise-eligible week.
    /// - Idempotent: uses `archive_file` per file (rename), so a file
    ///   already moved by a previous run simply isn't found again on the
    ///   next one; a week folder left non-empty because some of its files
    ///   were skipped (still downloading) is revisited on a later call and
    ///   only its remaining files are moved.
    ///
    /// Returns the number of week folders that had at least one file moved.
    pub fn archive_previous_weeks(
        &self,
        current_week: &WeekIdentifier,
        busy_weeks: &HashSet<WeekIdentifier>,
    ) -> Result<u32, FileError> {
        if !self.work_dir.exists() {
            return Ok(0);
        }

        let entries = fs::read_dir(&self.work_dir).map_err(|e| FileError::ReadDirectoryFailed {
            path: self.work_dir.clone(),
            source: e,
        })?;

        let mut archived_weeks = 0u32;

        for entry in entries.filter_map(Result::ok) {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }

            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            // Never descend into `.archive`, `.superseded`, or any other
            // dotdir (defensive: only ever touch directories that parse as
            // a week name).
            if name.starts_with('.') {
                continue;
            }
            let Some(week) = parse_week_dir_name(&name) else {
                continue; // not a week-named directory, leave it alone
            };
            if &week == current_week {
                continue; // never touch the current week
            }
            if busy_weeks.contains(&week) {
                tracing::debug!(
                    "Archiving: skipping week {} for now, it has a download in flight",
                    week
                );
                continue;
            }

            let week_path = entry.path();
            let files = match fs::read_dir(&week_path) {
                Ok(files) => files,
                Err(e) => {
                    tracing::error!("Archiving: failed to read {}: {}", week_path.display(), e);
                    continue;
                }
            };

            let mut moved_any = false;
            let mut skipped_any = false;
            for file_entry in files.filter_map(Result::ok) {
                if !file_entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    // Unexpected nested directory: leave it alone rather
                    // than guessing what to do with it.
                    skipped_any = true;
                    continue;
                }
                let file_name = file_entry.file_name();
                if file_name.to_string_lossy().ends_with(".part") {
                    // In-progress/resumable download (services/download.rs):
                    // never move it, even if the queue itself doesn't (yet,
                    // or anymore) know about it.
                    skipped_any = true;
                    continue;
                }

                self.archive_file(&file_entry.path(), &week)?;
                moved_any = true;
            }

            if moved_any {
                archived_weeks += 1;
                tracing::info!(
                    "Archived week {} into {:?}",
                    week,
                    self.week_archive_path(&week)
                );
            }
            if !skipped_any {
                // Best-effort cleanup: only succeeds if truly empty, so a
                // concurrent write or a race with the check above is safe
                // to ignore.
                let _ = fs::remove_dir(&week_path);
            }
        }

        Ok(archived_weeks)
    }
}

/// Parse a directory name in format "YYYY-WNN" to WeekIdentifier
fn parse_week_dir_name(name: &str) -> Option<WeekIdentifier> {
    let parts: Vec<&str> = name.split("-W").collect();
    if parts.len() != 2 {
        return None;
    }

    let year: i32 = parts[0].parse().ok()?;
    let week: u32 = parts[1].parse().ok()?;

    if week >= 1 && week <= 53 {
        Some(WeekIdentifier::new(year, week))
    } else {
        None
    }
}

/// Background scheduler that periodically enforces the retention policy.
///
/// Mirrors `PollingService` (see `services/polling.rs`): runs once shortly
/// after startup and then every `RETENTION_CHECK_INTERVAL_SECS`, reading
/// `work_directory`/`retention_days` fresh from `AppState` on every run so
/// config changes (e.g. the user updating the retention policy in Settings)
/// take effect on the next scheduled run without needing a restart.
pub struct RetentionScheduler {
    /// Channel sender to signal cancellation
    cancel_tx: watch::Sender<bool>,
    /// Whether the scheduler is currently running
    is_running: Arc<AtomicBool>,
}

impl RetentionScheduler {
    /// Create a new retention scheduler
    pub fn new() -> Self {
        let (cancel_tx, _) = watch::channel(false);
        Self {
            cancel_tx,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the retention background task.
    ///
    /// Runs independently of `PollingService`/`polling_enabled`: retention is
    /// local disk hygiene, not tied to whether automatic remote polling is on.
    pub fn start(&self, app: AppHandle) {
        if self.is_running.load(Ordering::SeqCst) {
            tracing::warn!("Retention scheduler already running, ignoring start request");
            return;
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let mut cancel_rx = self.cancel_tx.subscribe();

        tauri::async_runtime::spawn(async move {
            tracing::info!(
                "Retention scheduler started (checks every {}h)",
                RETENTION_CHECK_INTERVAL_SECS / 3600
            );

            // Give the rest of app startup (frontend event listeners, the
            // initial poll, see bl-desktop-first-poll-skipped) a brief head
            // start before touching the filesystem.
            tokio::time::sleep(TokioDuration::from_secs(STARTUP_DELAY_SECS)).await;

            tracing::info!("Performing initial retention enforcement on startup");
            run_retention_once(&app).await;

            let mut ticker = interval(TokioDuration::from_secs(RETENTION_CHECK_INTERVAL_SECS));
            // `interval` fires its first tick immediately upon creation; consume
            // it here (same rationale as PollingService::start, see
            // bl-desktop-first-poll-skipped) so the periodic ticks below stay
            // spaced by the full interval starting after the initial run above,
            // instead of firing a second run back-to-back with it.
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !is_running.load(Ordering::SeqCst) {
                            break;
                        }
                        run_retention_once(&app).await;
                    }
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            tracing::info!("Retention scheduler cancelled");
                            break;
                        }
                    }
                }
            }

            is_running.store(false, Ordering::SeqCst);
            tracing::info!("Retention scheduler stopped");
        });
    }

    /// Stop the retention background task
    pub fn stop(&self) {
        if self.is_running.load(Ordering::SeqCst) {
            let _ = self.cancel_tx.send(true);
            self.is_running.store(false, Ordering::SeqCst);
        }
    }

    /// Check if the scheduler is currently running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }
}

impl Default for RetentionScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the current work directory/retention policy from `AppState` and
/// enforce the retention policy once. No-ops (with a debug log) if the work
/// directory isn't configured yet, matching how `scan_and_queue` treats a
/// missing work directory in `services/queue.rs`.
async fn run_retention_once(app: &AppHandle) {
    let state = app.state::<crate::commands::AppState>();
    let (work_dir, retention_days) = match state.config.read() {
        Ok(config) => (config.work_directory.clone(), config.retention_days),
        Err(e) => {
            tracing::error!("Retention: failed to read config: {}", e);
            return;
        }
    };

    let Some(work_dir) = work_dir else {
        tracing::debug!("Retention: work directory not configured yet, skipping");
        return;
    };

    // The filesystem scan + trash move is blocking I/O; run it off the async
    // runtime (same pattern used for the filesystem checks in
    // commands::get_resource_summary).
    let result = tauri::async_runtime::spawn_blocking(move || {
        FileRetentionService::new(work_dir).enforce_retention(retention_days)
    })
    .await;

    match result {
        Ok(Ok(_)) => {} // enforce_retention already logs a clear summary
        Ok(Err(e)) => tracing::error!("Retention enforcement failed: {}", e),
        Err(e) => tracing::error!("Retention enforcement task panicked: {}", e),
    }
}

/// Archive the work directory's previous-week folders once, for the given
/// (already-updated) current week. Called from `services/polling.rs` and
/// `commands.rs` right after a poll determines the current week has
/// changed (bl-desktop-archiving-not-called): this is what makes
/// `enforce_retention` (already wired to run daily, see
/// `RetentionScheduler`) actually have a user-visible effect, since it only
/// ever acts on `.archive/`.
///
/// No-ops (with a debug log) if the work directory isn't configured yet,
/// mirroring `run_retention_once` above.
pub async fn archive_previous_weeks_once(app: &AppHandle, current_week: &WeekIdentifier) {
    let state = app.state::<crate::commands::AppState>();
    let work_dir = match state.config.read() {
        Ok(config) => config.work_directory.clone(),
        Err(e) => {
            tracing::error!("Archiving: failed to read config: {}", e);
            return;
        }
    };

    let Some(work_dir) = work_dir else {
        tracing::debug!("Archiving: work directory not configured yet, skipping");
        return;
    };

    // Consult the download queue so we never move a week folder out from
    // under a download that's still writing into it.
    let busy_weeks = state.download_queue.weeks_with_pending_downloads().await;
    let current_week = current_week.clone();

    // The filesystem scan + file moves are blocking I/O; run them off the
    // async runtime (same pattern as `run_retention_once` above).
    let result = tauri::async_runtime::spawn_blocking(move || {
        FileRetentionService::new(work_dir).archive_previous_weeks(&current_week, &busy_weeks)
    })
    .await;

    match result {
        Ok(Ok(_)) => {} // archive_previous_weeks already logs per-week on success
        Ok(Err(e)) => tracing::error!("Archiving previous weeks failed: {}", e),
        Err(e) => tracing::error!("Archiving previous weeks task panicked: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> (TempDir, FileRetentionService) {
        let temp_dir = TempDir::new().unwrap();
        let service = FileRetentionService::new(temp_dir.path().to_path_buf());
        (temp_dir, service)
    }

    #[test]
    fn test_archive_dir_path() {
        let (temp_dir, service) = setup_test_dir();
        let expected = temp_dir.path().join(".archive");
        assert_eq!(service.archive_dir(), expected);
    }

    #[test]
    fn test_week_archive_path() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);
        let expected = temp_dir.path().join(".archive").join("2026-W04");
        assert_eq!(service.week_archive_path(&week), expected);
    }

    #[test]
    fn test_superseded_path() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);
        let expected = temp_dir
            .path()
            .join(".archive")
            .join("2026-W04")
            .join(".superseded");
        assert_eq!(service.superseded_path(&week), expected);
    }

    #[test]
    fn test_parse_week_dir_name_valid() {
        assert_eq!(
            parse_week_dir_name("2026-W04"),
            Some(WeekIdentifier::new(2026, 4))
        );
        assert_eq!(
            parse_week_dir_name("2025-W52"),
            Some(WeekIdentifier::new(2025, 52))
        );
        assert_eq!(
            parse_week_dir_name("2024-W01"),
            Some(WeekIdentifier::new(2024, 1))
        );
    }

    #[test]
    fn test_parse_week_dir_name_invalid() {
        assert!(parse_week_dir_name("invalid").is_none());
        assert!(parse_week_dir_name("2026-04").is_none());
        assert!(parse_week_dir_name("2026-W00").is_none()); // Week 0 invalid
        assert!(parse_week_dir_name("2026-W54").is_none()); // Week 54 invalid
        assert!(parse_week_dir_name("abc-W04").is_none());
    }

    #[test]
    fn test_archive_file() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create a test file
        let test_file = temp_dir.path().join("test_file.zip");
        fs::write(&test_file, b"test content").unwrap();
        assert!(test_file.exists());

        // Archive it
        let archived_path = service.archive_file(&test_file, &week).unwrap();

        // Verify
        assert!(!test_file.exists()); // Original should be gone
        assert!(archived_path.exists()); // Archived should exist
        assert_eq!(
            archived_path,
            temp_dir.path().join(".archive/2026-W04/test_file.zip")
        );
    }

    #[test]
    fn test_archive_superseded() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create a test file
        let test_file = temp_dir.path().join("old_version.zip");
        fs::write(&test_file, b"old content").unwrap();

        // Archive as superseded
        let archived_path = service.archive_superseded(&test_file, &week).unwrap();

        assert!(!test_file.exists());
        assert!(archived_path.exists());
        assert_eq!(
            archived_path,
            temp_dir
                .path()
                .join(".archive/2026-W04/.superseded/old_version.zip")
        );
    }

    #[test]
    fn test_get_archived_weeks_empty() {
        let (_temp_dir, service) = setup_test_dir();
        assert!(service.get_archived_weeks().is_empty());
    }

    #[test]
    fn test_get_archived_weeks() {
        let (temp_dir, service) = setup_test_dir();

        // Create some archive directories
        let archive = temp_dir.path().join(".archive");
        fs::create_dir_all(archive.join("2026-W03")).unwrap();
        fs::create_dir_all(archive.join("2026-W04")).unwrap();
        fs::create_dir_all(archive.join("2025-W52")).unwrap();
        // Invalid directory should be ignored
        fs::create_dir_all(archive.join("invalid-dir")).unwrap();

        let weeks = service.get_archived_weeks();
        assert_eq!(weeks.len(), 3);
        assert!(weeks.contains(&WeekIdentifier::new(2026, 3)));
        assert!(weeks.contains(&WeekIdentifier::new(2026, 4)));
        assert!(weeks.contains(&WeekIdentifier::new(2025, 52)));
    }

    #[test]
    fn test_has_superseded_files_false() {
        let (_temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);
        assert!(!service.has_superseded_files(&week));
    }

    #[test]
    fn test_has_superseded_files_true() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create superseded file
        let superseded_dir = temp_dir.path().join(".archive/2026-W04/.superseded");
        fs::create_dir_all(&superseded_dir).unwrap();
        fs::write(superseded_dir.join("old_file.zip"), b"old").unwrap();

        assert!(service.has_superseded_files(&week));
    }

    #[test]
    fn test_get_superseded_files() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create superseded files
        let superseded_dir = temp_dir.path().join(".archive/2026-W04/.superseded");
        fs::create_dir_all(&superseded_dir).unwrap();
        fs::write(superseded_dir.join("old_v1.zip"), b"v1").unwrap();
        fs::write(superseded_dir.join("old_v2.zip"), b"v2").unwrap();

        let files = service.get_superseded_files(&week);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_retention_keep_forever() {
        let (_temp_dir, service) = setup_test_dir();
        let result = service.enforce_retention(None).unwrap();
        assert_eq!(result, 0);
    }

    /// Exercises the actual `Some(n)` trashing branch end-to-end (previously
    /// only the `None`/"keep forever" no-op path had coverage): an archived
    /// week older than `retention_days` must be moved to the system trash,
    /// while a recent one is left untouched. Regression guard for
    /// bl-desktop-retention-not-wired.
    #[test]
    fn test_enforce_retention_trashes_old_weeks_keeps_recent() {
        let (temp_dir, service) = setup_test_dir();

        let old_week = temp_dir.path().join(".archive/2025-W40");
        let recent_week = temp_dir.path().join(".archive/2026-W01");
        fs::create_dir_all(&old_week).unwrap();
        fs::create_dir_all(&recent_week).unwrap();

        // Backdate the "old" week's mtime well past a 7-day retention window;
        // leave the "recent" week at its just-created (now) mtime.
        let old_mtime =
            std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 60 * 60);
        fs::File::open(&old_week)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();

        let trashed_count = service.enforce_retention(Some(7)).unwrap();

        assert_eq!(
            trashed_count, 1,
            "only the week older than retention_days should be trashed"
        );
        assert!(
            !old_week.exists(),
            "old archived week should have been moved to the system trash"
        );
        assert!(
            recent_week.exists(),
            "recent archived week should be kept in place"
        );
    }

    /// A retention run must be safe to repeat: running it again immediately
    /// (e.g. the daily scheduler ticking, or startup + first scheduled tick in
    /// close succession) must not error or re-count weeks already trashed.
    #[test]
    fn test_enforce_retention_is_idempotent_across_repeated_runs() {
        let (temp_dir, service) = setup_test_dir();

        let old_week = temp_dir.path().join(".archive/2025-W40");
        fs::create_dir_all(&old_week).unwrap();
        let old_mtime =
            std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 60 * 60);
        fs::File::open(&old_week)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();

        let first_run = service.enforce_retention(Some(7)).unwrap();
        assert_eq!(first_run, 1);

        // Nothing left to evaluate: must be a stable, error-free no-op.
        let second_run = service.enforce_retention(Some(7)).unwrap();
        assert_eq!(second_run, 0);
    }

    // -- archive_previous_weeks (bl-desktop-archiving-not-called) -----------

    /// Regression guard for bl-desktop-archiving-not-called: previous weeks'
    /// folders (directly under the work directory) must actually move into
    /// `.archive/{week}/`, and the current week must be left alone.
    #[test]
    fn test_archive_previous_weeks_moves_non_current_keeps_current() {
        let (temp_dir, service) = setup_test_dir();
        let current = WeekIdentifier::new(2026, 4);
        let old1 = WeekIdentifier::new(2026, 3);
        let old2 = WeekIdentifier::new(2026, 2);

        for week in [&current, &old1, &old2] {
            let dir = temp_dir.path().join(week.as_dir_name());
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("video.mp4"), b"content").unwrap();
        }

        let archived = service
            .archive_previous_weeks(&current, &HashSet::new())
            .unwrap();

        assert_eq!(archived, 2, "both non-current weeks should be archived");
        assert!(
            temp_dir.path().join(current.as_dir_name()).exists(),
            "current week folder must be left in place at the top level"
        );
        assert!(
            !temp_dir.path().join(old1.as_dir_name()).exists(),
            "archived week folder must no longer exist at the top level"
        );
        assert!(service.week_archive_path(&old1).join("video.mp4").exists());
        assert!(service.week_archive_path(&old2).join("video.mp4").exists());
    }

    /// Re-running after everything has already been archived must be a
    /// stable no-op: no errors, nothing re-counted, no duplicate files.
    #[test]
    fn test_archive_previous_weeks_is_idempotent() {
        let (temp_dir, service) = setup_test_dir();
        let current = WeekIdentifier::new(2026, 4);
        let old = WeekIdentifier::new(2026, 3);

        let dir = temp_dir.path().join(old.as_dir_name());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("video.mp4"), b"content").unwrap();

        let first_run = service
            .archive_previous_weeks(&current, &HashSet::new())
            .unwrap();
        assert_eq!(first_run, 1);

        let second_run = service
            .archive_previous_weeks(&current, &HashSet::new())
            .unwrap();
        assert_eq!(
            second_run, 0,
            "nothing left at the top level to archive again"
        );
        assert!(service.week_archive_path(&old).join("video.mp4").exists());
    }

    /// A week with a download in flight (per the queue) must be left
    /// completely untouched, even though it isn't the current week.
    #[test]
    fn test_archive_previous_weeks_skips_busy_week() {
        let (temp_dir, service) = setup_test_dir();
        let current = WeekIdentifier::new(2026, 4);
        let busy = WeekIdentifier::new(2026, 3);

        let dir = temp_dir.path().join(busy.as_dir_name());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("video.mp4"), b"content").unwrap();

        let mut busy_weeks = HashSet::new();
        busy_weeks.insert(busy.clone());

        let archived = service
            .archive_previous_weeks(&current, &busy_weeks)
            .unwrap();

        assert_eq!(archived, 0);
        assert!(
            temp_dir
                .path()
                .join(busy.as_dir_name())
                .join("video.mp4")
                .exists(),
            "busy week's file must stay exactly where it was"
        );
        assert!(!service.week_archive_path(&busy).join("video.mp4").exists());
    }

    /// A `.part` file (services/download.rs's in-progress/resume marker)
    /// must never be moved, even inside an otherwise-archivable week —
    /// belt-and-suspenders alongside the queue-based `busy_weeks` check.
    /// Other, completed files in the same folder are still archived, and
    /// the week folder itself is left in place (not fully archived yet).
    #[test]
    fn test_archive_previous_weeks_never_moves_part_files() {
        let (temp_dir, service) = setup_test_dir();
        let current = WeekIdentifier::new(2026, 4);
        let old = WeekIdentifier::new(2026, 3);

        let dir = temp_dir.path().join(old.as_dir_name());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("done.mp4"), b"complete").unwrap();
        fs::write(dir.join("still-downloading.mp4.part"), b"partial").unwrap();

        let archived = service
            .archive_previous_weeks(&current, &HashSet::new())
            .unwrap();

        assert_eq!(
            archived, 1,
            "the week still counts as archived (moved >=1 file)"
        );
        assert!(service.week_archive_path(&old).join("done.mp4").exists());
        assert!(
            dir.join("still-downloading.mp4.part").exists(),
            ".part file must never be moved"
        );
        assert!(
            dir.exists(),
            "week folder must stay in place while a .part file remains"
        );
    }

    /// Once a week folder is fully archived (no files skipped), the
    /// now-empty source folder is cleaned up instead of being left behind.
    #[test]
    fn test_archive_previous_weeks_removes_now_empty_source_folder() {
        let (temp_dir, service) = setup_test_dir();
        let current = WeekIdentifier::new(2026, 4);
        let old = WeekIdentifier::new(2026, 3);

        let dir = temp_dir.path().join(old.as_dir_name());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("video.mp4"), b"content").unwrap();

        service
            .archive_previous_weeks(&current, &HashSet::new())
            .unwrap();

        assert!(
            !dir.exists(),
            "fully-archived week folder should be removed from the top level"
        );
    }

    /// Directories that aren't week-named (notes, `.git`, or the `.archive`
    /// tree itself) must never be touched by the scan.
    #[test]
    fn test_archive_previous_weeks_ignores_non_week_directories() {
        let (temp_dir, service) = setup_test_dir();
        let current = WeekIdentifier::new(2026, 4);

        let notes_dir = temp_dir.path().join("notes");
        fs::create_dir_all(&notes_dir).unwrap();
        fs::write(notes_dir.join("readme.txt"), b"keep me").unwrap();

        let archived = service
            .archive_previous_weeks(&current, &HashSet::new())
            .unwrap();

        assert_eq!(archived, 0);
        assert!(notes_dir.join("readme.txt").exists());
    }

    /// No work directory yet (not configured / not created on disk): must
    /// no-op cleanly rather than erroring, mirroring `get_archived_weeks`.
    #[test]
    fn test_archive_previous_weeks_missing_work_dir_is_a_noop() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("does-not-exist");
        let service = FileRetentionService::new(missing);

        let archived = service
            .archive_previous_weeks(&WeekIdentifier::new(2026, 4), &HashSet::new())
            .unwrap();
        assert_eq!(archived, 0);
    }
}
