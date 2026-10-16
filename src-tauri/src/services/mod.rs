//! Services layer for Church Helper Desktop
//!
//! This module contains all business logic services.

pub mod download;
pub mod errata;
pub mod polling;
pub mod queue;
pub mod retention;
pub mod updater;

#[cfg(test)]
pub(crate) mod test_http;

pub use download::DownloadService;
pub use errata::{
    detect_errata_changes, detect_superseded_by_new_id, process_errata, record_downloaded_file,
    superseded_ids,
};
pub use polling::{poll_once, refresh_categories, PollingService};
pub use queue::DownloadQueue;
pub use retention::{archive_previous_weeks_once, FileRetentionService, RetentionScheduler};
pub use updater::UpdateScheduler;
