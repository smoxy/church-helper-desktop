//! Services layer for Church Helper Desktop
//!
//! This module contains all business logic services.

pub mod download;
pub mod errata;
pub mod polling;
pub mod retention;

pub use download::DownloadService;
pub use errata::detect_errata_changes;
pub use polling::PollingService;
pub use retention::FileRetentionService;
