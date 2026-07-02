//! Data models for Church Helper Desktop
//!
//! These models represent the core domain entities used throughout the application.

use chrono::{DateTime, Datelike, IsoWeek, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User configuration persisted via tauri-plugin-store
///
/// `#[serde(default)]` at the struct level (not just on individual new
/// fields) so a settings.json saved by an older build of the app — missing
/// any number of fields added since — still deserializes successfully,
/// filling in `AppConfig::default()` for whatever is absent, instead of
/// failing to parse and silently resetting the user's whole config back to
/// defaults (see `test_deserialize_missing_new_fields_preserves_existing`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    /// Local folder where files are saved
    pub work_directory: Option<PathBuf>,
    /// Whether automatic polling is enabled
    pub polling_enabled: bool,
    /// Polling interval in minutes (1-1440)
    pub polling_interval_minutes: u32,
    /// Retention policy in days. None = KeepForever, Some(0) = Immediate delete
    /// Retention policy in days. None = KeepForever, Some(0) = Immediate delete
    pub retention_days: Option<u32>,
    /// Categories enabled for auto-download
    pub auto_download_categories: Vec<String>,
    /// Download mode (Queue or Parallel)
    pub download_mode: DownloadMode,
    /// Prefer optimized video URL when available
    pub prefer_optimized: bool,
    /// Whether the app should launch automatically at OS startup (opt-in)
    pub autostart_enabled: bool,
    /// Whether the one-time "the app keeps running in the tray" notice has
    /// already been shown (see `lib.rs`'s window `CloseRequested` handler).
    /// Covered by the struct-level `#[serde(default)]` above; kept here too
    /// for clarity at the field.
    pub tray_close_notice_shown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadMode {
    Queue,
    Parallel,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            work_directory: None,
            polling_enabled: true,
            polling_interval_minutes: 60, // Default: 1 hour
            retention_days: Some(7),      // Default: 7 days
            auto_download_categories: Vec::new(),
            download_mode: DownloadMode::Queue,
            prefer_optimized: true,         // Default: prefer optimized videos
            autostart_enabled: false,       // Default: disabled (opt-in)
            tray_close_notice_shown: false, // Default: not shown yet
        }
    }
}

impl AppConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.polling_interval_minutes < 1 || self.polling_interval_minutes > 1440 {
            return Err(ConfigValidationError::InvalidPollingInterval(
                self.polling_interval_minutes,
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValidationError {
    InvalidPollingInterval(u32),
}

/// A single optimized video variant produced by the re-encoder from a
/// resource's original zip (adr-0008: matching per provenienza).
///
/// Per `contract-resources-api`, the list is ordered by `size_bytes` desc by
/// the producer; `label` is a slug-or-human-readable name for the variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptimizedVideo {
    pub url: String,
    pub label: String,
    pub size_bytes: u64,
}

/// Resource from the API response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub id: i64,
    pub category: String,
    pub title: String,
    pub description: String,
    pub download_url: String,
    pub thumbnail_url: Option<String>,
    pub file_type: Option<String>,
    pub checksum: Option<String>,
    pub is_active: bool,
    #[serde(deserialize_with = "deserialize_naive_to_utc")]
    pub created_at: DateTime<Utc>,
    pub optimized_video_url: Option<String>,
    /// All optimized video variants available for this resource (adr-0008).
    /// Additive field: a missing key or an explicit JSON `null` (older
    /// servers, pre adr-0008) both deserialize to `None` — serde treats
    /// `Option<T>` struct fields as implicitly optional on deserialization,
    /// so no custom deserializer is needed here. Purely informational for
    /// the UI: `get_effective_download_url` below is intentionally
    /// unaffected by this field and keeps using only `optimized_video_url`
    /// (the producer's compat-default, always the first/largest element).
    pub optimized_videos: Option<Vec<OptimizedVideo>>,
}

fn deserialize_naive_to_utc<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    // Attempt to parse as RFC3339 (standard JSON format for DateTime<Utc>)
    if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Attempt to parse as NaiveDateTime (default for the API)
    // The format is ISO 8601 without timezone
    if let Ok(naive) = s.parse::<NaiveDateTime>() {
        return Ok(naive.and_utc());
    }

    Err(serde::de::Error::custom(format!(
        "Failed to parse datetime: {}",
        s
    )))
}
impl Resource {
    /// Check if the download URL is a YouTube link
    pub fn is_youtube(&self) -> bool {
        is_youtube_url(&self.download_url)
    }

    /// Get the week identifier for this resource
    pub fn week(&self) -> WeekIdentifier {
        WeekIdentifier::from_datetime(self.created_at)
    }

    /// Get the effective download URL based on preference
    /// If prefer_optimized is true and optimized_video_url is available, returns that.
    /// Otherwise returns the standard download_url.
    pub fn get_effective_download_url(&self, prefer_optimized: bool) -> &str {
        if prefer_optimized {
            self.optimized_video_url
                .as_deref()
                .unwrap_or(&self.download_url)
        } else {
            &self.download_url
        }
    }
}

/// Check if a URL is a YouTube link
pub fn is_youtube_url(url: &str) -> bool {
    let url_lower = url.to_lowercase();
    // Match youtube.com or youtu.be domains specifically
    // Use domain boundary checking to avoid false positives like "notyoutube.com"
    url_lower.contains("://youtube.com")
        || url_lower.contains("://www.youtube.com")
        || url_lower.contains("://m.youtube.com")
        || url_lower.contains("://youtu.be")
}

/// API Response wrapper
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceListResponse {
    pub count: u32,
    pub resources: Vec<Resource>,
}

/// Week identifier for tracking current vs archived resources
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WeekIdentifier {
    pub year: i32,
    pub week_number: u32,
}

impl WeekIdentifier {
    pub fn new(year: i32, week_number: u32) -> Self {
        Self { year, week_number }
    }

    /// Create a WeekIdentifier from a DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        let iso_week: IsoWeek = dt.iso_week();
        Self {
            year: iso_week.year(),
            week_number: iso_week.week(),
        }
    }

    /// Get the current week
    pub fn current() -> Self {
        Self::from_datetime(Utc::now())
    }

    /// Format as directory name (e.g., "2026-W03")
    pub fn as_dir_name(&self) -> String {
        format!("{}-W{:02}", self.year, self.week_number)
    }
}

impl std::fmt::Display for WeekIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-W{:02}", self.year, self.week_number)
    }
}

/// Local state for tracking downloaded files
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadedFile {
    pub resource_id: i64,
    pub week: WeekIdentifier,
    pub local_path: PathBuf,
    pub downloaded_at: DateTime<Utc>,
    /// Original URL used for download
    pub source_url: String,
    /// Whether this file has been superseded by an errata corrige
    pub is_superseded: bool,
}

/// Represents a detected errata corrige change
#[derive(Debug, Clone, PartialEq)]
pub struct ErrataChange {
    pub resource_id: i64,
    pub old_file: DownloadedFile,
    pub new_resource: Resource,
}

/// Application status for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub polling_active: bool,
    pub last_poll_time: Option<DateTime<Utc>>,
    pub current_week: Option<WeekIdentifier>,
    pub total_resources: usize,
    pub pending_downloads: usize,
    pub has_superseded_files: bool,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            polling_active: false,
            last_poll_time: None,
            current_week: None,
            total_resources: 0,
            pending_downloads: 0,
            has_superseded_files: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.polling_enabled);
        assert_eq!(config.polling_interval_minutes, 60);
        assert_eq!(config.retention_days, Some(7));
        assert!(config.work_directory.is_none());
        assert!(
            !config.autostart_enabled,
            "autostart must default to disabled (opt-in only)"
        );
        assert!(
            !config.tray_close_notice_shown,
            "tray close notice must default to not-yet-shown"
        );
    }

    /// Upgrading from a settings.json saved before this field existed must
    /// not fail to deserialize the rest of the config (see the field's
    /// `#[serde(default)]` doc comment in the struct definition).
    #[test]
    fn test_tray_close_notice_shown_missing_key_deserializes_to_false() {
        let json = r#"{
            "work_directory": null,
            "polling_enabled": true,
            "polling_interval_minutes": 60,
            "retention_days": 7,
            "auto_download_categories": [],
            "download_mode": "Queue",
            "prefer_optimized": true,
            "autostart_enabled": false
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!config.tray_close_notice_shown);
    }

    /// BLOCKER (audit): a settings.json written by an older build is
    /// missing *every* field added since (not just the newest one). With
    /// only per-field `#[serde(default)]` attributes, any field the older
    /// build didn't know about yet is absent too, so this must be covered
    /// by the struct-level `#[serde(default)]` instead. This deserializes
    /// a config containing only the very first fields the app ever had,
    /// and checks both that it succeeds and that the fields which *are*
    /// present are preserved verbatim (not silently reset alongside the
    /// missing ones).
    #[test]
    fn test_deserialize_missing_new_fields_preserves_existing() {
        let json = r#"{
            "work_directory": "/home/user/church-media",
            "polling_enabled": false,
            "polling_interval_minutes": 30,
            "retention_days": 14,
            "auto_download_categories": ["sermons"],
            "download_mode": "Parallel"
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();

        // Present fields must be preserved exactly, not overwritten by
        // `AppConfig::default()`.
        assert_eq!(
            config.work_directory,
            Some(PathBuf::from("/home/user/church-media"))
        );
        assert!(!config.polling_enabled);
        assert_eq!(config.polling_interval_minutes, 30);
        assert_eq!(config.retention_days, Some(14));
        assert_eq!(config.auto_download_categories, vec!["sermons".to_string()]);
        assert_eq!(config.download_mode, DownloadMode::Parallel);

        // Fields absent from the old JSON fall back to their defaults
        // instead of failing to deserialize.
        assert_eq!(
            config.prefer_optimized,
            AppConfig::default().prefer_optimized
        );
        assert!(!config.autostart_enabled);
        assert!(!config.tray_close_notice_shown);
    }

    #[test]
    fn test_config_validation_valid() {
        let config = AppConfig {
            polling_interval_minutes: 30,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_interval_zero() {
        let config = AppConfig {
            polling_interval_minutes: 0,
            ..Default::default()
        };
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidPollingInterval(0))
        );
    }

    #[test]
    fn test_config_validation_invalid_interval_too_high() {
        let config = AppConfig {
            polling_interval_minutes: 1441,
            ..Default::default()
        };
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidPollingInterval(1441))
        );
    }

    #[test]
    fn test_youtube_url_detection() {
        // YouTube URLs
        assert!(is_youtube_url("https://www.youtube.com/watch?v=abc123"));
        assert!(is_youtube_url("https://youtube.com/watch?v=abc123"));
        assert!(is_youtube_url("https://youtu.be/abc123"));
        assert!(is_youtube_url("http://www.youtube.com/embed/abc123"));
        assert!(is_youtube_url("HTTPS://YOUTUBE.COM/watch?v=ABC")); // Case insensitive

        // Non-YouTube URLs
        assert!(!is_youtube_url("https://example.com/file.zip"));
        assert!(!is_youtube_url("https://vimeo.com/123456"));
        assert!(!is_youtube_url("https://notyoutube.com/video"));
    }

    #[test]
    fn test_resource_is_youtube() {
        let youtube_resource = Resource {
            id: 1,
            category: "video".to_string(),
            title: "Test Video".to_string(),
            description: "A test".to_string(),
            download_url: "https://youtube.com/watch?v=abc".to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: Utc::now(),
            optimized_video_url: None,
            optimized_videos: None,
        };
        assert!(youtube_resource.is_youtube());

        let file_resource = Resource {
            download_url: "https://example.com/file.zip".to_string(),
            ..youtube_resource.clone()
        };
        assert!(!file_resource.is_youtube());
    }

    #[test]
    fn test_week_identifier_from_datetime() {
        // January 19, 2026 is in week 4 of 2026
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);
        assert_eq!(week.year, 2026);
        assert_eq!(week.week_number, 4);
    }

    #[test]
    fn test_week_identifier_as_dir_name() {
        let week = WeekIdentifier::new(2026, 3);
        assert_eq!(week.as_dir_name(), "2026-W03");

        let week2 = WeekIdentifier::new(2025, 52);
        assert_eq!(week2.as_dir_name(), "2025-W52");
    }

    #[test]
    fn test_week_identifier_display() {
        let week = WeekIdentifier::new(2026, 4);
        assert_eq!(format!("{}", week), "2026-W04");
    }

    #[test]
    fn test_resource_week() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let resource = Resource {
            id: 1,
            category: "test".to_string(),
            title: "Test".to_string(),
            description: "".to_string(),
            download_url: "https://example.com/file.zip".to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: dt,
            optimized_video_url: None,
            optimized_videos: None,
        };
        let week = resource.week();
        assert_eq!(week.year, 2026);
        assert_eq!(week.week_number, 4);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = AppConfig {
            work_directory: Some(PathBuf::from("/home/user/documents")),
            polling_enabled: false,
            polling_interval_minutes: 120,
            retention_days: None, // Keep forever
            auto_download_categories: vec!["decime".to_string(), "video".to_string()],
            download_mode: DownloadMode::Parallel,
            prefer_optimized: false,
            autostart_enabled: true,
            tray_close_notice_shown: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_resource_list_response_parsing() {
        let json = r#"{
            "count": 2,
            "resources": [
                {
                    "id": 1,
                    "category": "decime",
                    "title": "Decime e offerte",
                    "description": "Test",
                    "download_url": "https://example.com/file.zip",
                    "thumbnail_url": "https://example.com/thumb.jpg",
                    "file_type": null,
                    "is_active": true,
                    "created_at": "2026-01-17T23:51:02.358083"
                },
                {
                    "id": 2,
                    "category": "video",
                    "title": "Video",
                    "description": "Test video",
                    "download_url": "https://youtube.com/watch?v=abc",
                    "thumbnail_url": null,
                    "file_type": null,
                    "is_active": true,
                    "created_at": "2026-01-18T10:00:00Z"
                }
            ]
        }"#;

        let response: ResourceListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.count, 2);
        assert_eq!(response.resources.len(), 2);
        assert!(!response.resources[0].is_youtube());
        assert!(response.resources[1].is_youtube());
    }

    /// contract-resources-api / adr-0008: `optimized_videos` is additive and
    /// must be tolerated when entirely absent from the payload (pre-adr-0008
    /// / old server), not just when present-but-null.
    #[test]
    fn test_optimized_videos_missing_key_deserializes_to_none() {
        let json = r#"{
            "id": 1,
            "category": "decime",
            "title": "Decime",
            "description": "Test",
            "download_url": "https://example.com/file.zip",
            "thumbnail_url": null,
            "file_type": null,
            "is_active": true,
            "created_at": "2026-01-17T23:51:02.358083"
        }"#;

        let resource: Resource = serde_json::from_str(json).unwrap();
        assert_eq!(resource.optimized_videos, None);
        assert_eq!(resource.optimized_video_url, None);
    }

    /// Same tolerance, but for a server that emits the field explicitly as
    /// `null` (e.g. no optimized variant for this specific resource) rather
    /// than omitting the key.
    #[test]
    fn test_optimized_videos_null_deserializes_to_none() {
        let json = r#"{
            "id": 1,
            "category": "decime",
            "title": "Decime",
            "description": "Test",
            "download_url": "https://example.com/file.zip",
            "thumbnail_url": null,
            "file_type": null,
            "is_active": true,
            "created_at": "2026-01-17T23:51:02.358083",
            "optimized_video_url": null,
            "optimized_videos": null
        }"#;

        let resource: Resource = serde_json::from_str(json).unwrap();
        assert_eq!(resource.optimized_videos, None);
    }

    /// adr-0008 "multi-video": a zip resource produces several optimized
    /// variants. Verifies field mapping (url/label/size_bytes) and that
    /// `get_effective_download_url` stays on the compat-default
    /// `optimized_video_url` (first/largest element) regardless of how many
    /// variants `optimized_videos` carries — the full list is UI-only.
    #[test]
    fn test_optimized_videos_multi_element_parses_all_fields() {
        let json = r#"{
            "id": 102,
            "category": "missioni",
            "title": "Missioni dal Mondo",
            "description": "Archivio zip",
            "download_url": "https://example.com/missioni_102.zip",
            "thumbnail_url": null,
            "file_type": "zip",
            "is_active": true,
            "created_at": "2026-06-29T11:14:03+02:00",
            "optimized_video_url": "https://example.com/files/servizio-completo.mp4",
            "optimized_videos": [
                {
                    "url": "https://example.com/files/servizio-completo.mp4",
                    "label": "Servizio completo",
                    "size_bytes": 8100000
                },
                {
                    "url": "https://example.com/files/intervista-pastore.mp4",
                    "label": "Intervista al pastore",
                    "size_bytes": 4950000
                },
                {
                    "url": "https://example.com/files/saluti-finali.mp4",
                    "label": "Saluti finali",
                    "size_bytes": 2150000
                }
            ]
        }"#;

        let resource: Resource = serde_json::from_str(json).unwrap();
        let videos = resource
            .optimized_videos
            .as_ref()
            .expect("optimized_videos should be Some for a multi-video resource");
        assert_eq!(videos.len(), 3);
        assert_eq!(videos[0].label, "Servizio completo");
        assert_eq!(videos[0].size_bytes, 8_100_000);
        assert_eq!(
            videos[0].url,
            "https://example.com/files/servizio-completo.mp4"
        );
        assert_eq!(videos[1].label, "Intervista al pastore");
        assert_eq!(videos[2].label, "Saluti finali");

        // get_effective_download_url is unaffected by the full list: it
        // still resolves solely from optimized_video_url (compat default).
        assert_eq!(
            resource.get_effective_download_url(true),
            "https://example.com/files/servizio-completo.mp4"
        );
        assert_eq!(
            resource.get_effective_download_url(false),
            "https://example.com/missioni_102.zip"
        );
    }
}
