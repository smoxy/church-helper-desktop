//! Data models for Church Helper Desktop
//!
//! These models represent the core domain entities used throughout the application.

use chrono::{DateTime, Datelike, IsoWeek, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User configuration persisted via tauri-plugin-store
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    /// Local folder where files are saved
    pub work_directory: Option<PathBuf>,
    /// Whether automatic polling is enabled
    pub polling_enabled: bool,
    /// Polling interval in minutes (1-1440)
    pub polling_interval_minutes: u32,
    /// Retention policy in days. None = KeepForever, Some(0) = Immediate delete
    pub retention_days: Option<u32>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            work_directory: None,
            polling_enabled: true,
            polling_interval_minutes: 60, // Default: 1 hour
            retention_days: Some(7),      // Default: 7 days
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
    pub is_active: bool,
    #[serde(deserialize_with = "deserialize_naive_to_utc")]
    pub created_at: DateTime<Utc>,
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
            is_active: true,
            created_at: Utc::now(),
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
            is_active: true,
            created_at: dt,
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
}
