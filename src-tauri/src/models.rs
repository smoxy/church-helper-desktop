//! Data models for Church Helper Desktop
//!
//! These models represent the core domain entities used throughout the application.

use chrono::{DateTime, Datelike, IsoWeek, NaiveDate, NaiveDateTime, Utc, Weekday};
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
    /// Whether the one-time OS notification about the app staying in the tray
    /// has already been shown (see `lib.rs`'s window `CloseRequested` handler).
    /// Renamed from `tray_close_notice_shown`: a settings.json carrying only the
    /// old key deserializes this to `false` (struct-level `#[serde(default)]`)
    /// and drops the stale key on the next write, so users whose old flag was
    /// burned get the OS notice once more — no migration needed.
    pub tray_close_os_notice_shown: bool,
    /// UI colour theme. `#[serde(default)]` so a settings.json from a build
    /// predating this field deserializes to `System` instead of failing.
    #[serde(default)]
    pub theme: ThemeSetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadMode {
    Queue,
    Parallel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum ThemeSetting {
    #[default]
    System,
    Light,
    Dark,
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
            prefer_optimized: true,   // Default: prefer optimized videos
            autostart_enabled: false, // Default: disabled (opt-in)
            tray_close_os_notice_shown: false, // Default: not shown yet
            theme: ThemeSetting::System, // Default: follow the OS
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
    /// Human-readable description. Additive-tolerant (adr-0003): an explicit
    /// JSON `null` or an entirely absent key both deserialize to `None`
    /// instead of failing the whole poll. `#[serde(default)]` makes the
    /// missing-key case explicit alongside `Option`'s implicit tolerance.
    #[serde(default)]
    pub description: Option<String>,
    pub download_url: String,
    pub thumbnail_url: Option<String>,
    pub file_type: Option<String>,
    pub checksum: Option<String>,
    pub is_active: bool,
    #[serde(deserialize_with = "deserialize_naive_to_utc")]
    pub created_at: DateTime<Utc>,
    /// True calendar week of this resource's content ("YYYY-MM-DD", from the
    /// newsletter subject), as opposed to `created_at` which is the DB
    /// *insert* timestamp. Additive field (adr-0003): the mail-parser ships
    /// it in a parallel fix, so older/degraded payloads omit it entirely.
    /// `week()` prefers this over `created_at` — during ingestion recovery
    /// (mail-parser stalled for weeks, then backfills in one batch) every
    /// recovered resource is inserted "today", so deriving the week from
    /// `created_at` alone would put stale content in the current week and
    /// `is_material_week_stale` would never fire.
    ///
    /// Uses `deserialize_lenient_week_date` rather than plain
    /// `Option<NaiveDate>`: a malformed value here must degrade to `None`
    /// instead of failing the whole `Vec<Resource>` deserialization — one bad
    /// `week_date` must never take down the rest of the poll.
    #[serde(default, deserialize_with = "deserialize_lenient_week_date")]
    pub week_date: Option<NaiveDate>,
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

/// Lenient `Option<NaiveDate>` deserializer for `Resource::week_date`. Any
/// value that isn't a valid "YYYY-MM-DD" string — the wrong JSON type (e.g. a
/// stray number) or a string that fails to parse (e.g. "boh") — degrades to
/// `None` rather than raising an error, so this best-effort field can never
/// break deserialization of the surrounding resource list.
///
/// Goes through `serde_json::Value` instead of `Option<String>` on purpose:
/// `Option<String>::deserialize` itself errors out on a non-string JSON
/// value, which would propagate the same as a hard parse failure. Routing
/// through `Value` first lets every "not a valid date" case — wrong type or
/// unparsable string — fall through to `None` uniformly.
fn deserialize_lenient_week_date<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| v.as_str().map(str::to_owned))
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()))
}

impl Resource {
    /// Check if the download URL is a YouTube link
    pub fn is_youtube(&self) -> bool {
        is_youtube_url(&self.download_url)
    }

    /// Get the week identifier for this resource: `week_date` (the true
    /// content week, from the newsletter subject) when present, falling back
    /// to `created_at` (the DB insert timestamp) otherwise. See the doc
    /// comment on `Resource::week_date` for why the fallback alone isn't
    /// enough during ingestion recovery. Every downstream consumer — week
    /// folder naming, staleness, errata, retention — goes through this
    /// method, so all of them benefit automatically.
    pub fn week(&self) -> WeekIdentifier {
        match self.week_date {
            Some(date) => WeekIdentifier::from_naive_date(date),
            None => WeekIdentifier::from_datetime(self.created_at),
        }
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

/// One category and how many resources currently carry it, as returned by
/// `GET {API_BASE}/api/resources/categories/counts`. The name is shown in
/// Settings so the user can enable auto-download for categories that aren't
/// in the current week (and so typos in the source data surface in the debug
/// log): see `services::polling::refresh_categories`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoryCount {
    pub name: String,
    pub count: u64,
}

/// Response of the categories/counts endpoint. `#[serde(default)]` on both
/// fields so a partial or evolving payload (the endpoint ships in parallel)
/// still deserializes instead of failing the whole refresh — a missing
/// `categories` yields an empty list and the UI keeps its previous state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoriesCountResponse {
    #[serde(default)]
    pub categories: Vec<CategoryCount>,
    #[serde(default)]
    pub total: u64,
}

/// Week identifier for tracking current vs archived resources.
///
/// `PartialOrd`/`Ord` are derived from the field order (`year` then
/// `week_number`), i.e. chronological order, matching `latest_week`'s manual
/// `(year, week_number)` tuple comparison and used by
/// `is_material_week_stale` to compare against `WeekIdentifier::current()`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// Get the current week (ISO calendar week of `Utc::now()`)
    pub fn current() -> Self {
        Self::from_datetime(Utc::now())
    }

    /// Create a WeekIdentifier from a NaiveDate (the ISO calendar week that
    /// date falls in). Used by `Resource::week()` for `week_date`, which
    /// carries no time-of-day/timezone component.
    pub fn from_naive_date(date: NaiveDate) -> Self {
        let iso_week: IsoWeek = date.iso_week();
        Self {
            year: iso_week.year(),
            week_number: iso_week.week(),
        }
    }

    /// Format as a self-explanatory directory name carrying the Saturday date
    /// of that ISO week, e.g. "W19-2026-05-09" (year-month-day are the
    /// Saturday's, not necessarily `self.year` — they can differ from the ISO
    /// week-year right at a year boundary). Falls back to the dateless
    /// "W{week:02}-{year}" when `(year, week_number)` isn't a valid ISO
    /// week/Saturday combination (e.g. a week number a given year doesn't
    /// have), logging a warning since that should never happen for
    /// server-derived weeks.
    pub fn as_dir_name(&self) -> String {
        match NaiveDate::from_isoywd_opt(self.year, self.week_number, Weekday::Sat) {
            Some(saturday) => format!(
                "W{:02}-{}-{:02}-{:02}",
                self.week_number,
                saturday.year(),
                saturday.month(),
                saturday.day()
            ),
            None => {
                tracing::warn!(
                    "WeekIdentifier(year={}, week={}) has no valid ISO Saturday; falling back to dateless directory name",
                    self.year,
                    self.week_number
                );
                format!("W{:02}-{}", self.week_number, self.year)
            }
        }
    }

    /// Format as the legacy directory name (e.g. "2026-W03") used before
    /// `as_dir_name` gained the self-explanatory Saturday date. Still needed
    /// to resolve files/archives written by older builds — see
    /// `services::download::resolve_dest_path` and
    /// `services::retention`'s directory-name parsing.
    pub fn legacy_dir_name(&self) -> String {
        format!("{}-W{:02}", self.year, self.week_number)
    }
}

impl std::fmt::Display for WeekIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-W{:02}", self.year, self.week_number)
    }
}

/// Latest (maximum) ISO week among `resources`, `None` if empty. Used to
/// derive `current_week`, which guards the destructive archiving path, so it
/// must not depend on API response ordering.
pub fn latest_week(resources: &[Resource]) -> Option<WeekIdentifier> {
    resources
        .iter()
        .map(|r| r.week())
        .max_by(|a, b| (a.year, a.week_number).cmp(&(b.year, b.week_number)))
}

/// Whether the latest known material is older than the current ISO calendar
/// week — i.e. the mail-parser stalled and the app would otherwise show old
/// material without any indication (the regression that motivated this
/// field: two months without new mail, app still showing May's material).
/// `latest` is `None` (no resources at all) is deliberately never stale: the
/// absence of material is not the same as stale material.
pub fn is_material_week_stale(latest: Option<&WeekIdentifier>) -> bool {
    match latest {
        Some(week) => *week < WeekIdentifier::current(),
        None => false,
    }
}

/// Local state for tracking downloaded files.
///
/// Persisted as the `downloaded_files` key of `cache.json` (the errata
/// registry). `resource_id`, `week`, `local_path` and `downloaded_at` are the
/// identity/essential fields and are always written; `source_url` and
/// `is_superseded` carry `#[serde(default)]` so a registry snapshot written by
/// an older build that predates them still deserializes (empty string / false)
/// instead of failing to parse and wiping the whole registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadedFile {
    pub resource_id: i64,
    pub week: WeekIdentifier,
    pub local_path: PathBuf,
    pub downloaded_at: DateTime<Utc>,
    /// Original URL used for download
    #[serde(default)]
    pub source_url: String,
    /// Whether this file has been superseded by an errata corrige
    #[serde(default)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppStatus {
    pub polling_active: bool,
    pub last_poll_time: Option<DateTime<Utc>>,
    pub current_week: Option<WeekIdentifier>,
    pub total_resources: usize,
    pub pending_downloads: usize,
    pub has_superseded_files: bool,
    /// True when `current_week`'s material is older than the current ISO
    /// calendar week (see `is_material_week_stale`). `#[serde(default)]` so
    /// this additive field never breaks (de)serialization for a build that
    /// predates it (contract: IPC field, frontend-consumed).
    #[serde(default)]
    pub material_week_stale: bool,
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
            !config.tray_close_os_notice_shown,
            "tray close notice must default to not-yet-shown"
        );
    }

    /// Upgrading from a settings.json saved before this field existed must
    /// not fail to deserialize the rest of the config (see the field's
    /// `#[serde(default)]` doc comment in the struct definition).
    #[test]
    fn test_tray_close_os_notice_shown_missing_key_deserializes_to_false() {
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
        assert!(!config.tray_close_os_notice_shown);
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
        assert!(!config.tray_close_os_notice_shown);
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
            description: Some("A test".to_string()),
            download_url: "https://youtube.com/watch?v=abc".to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: Utc::now(),
            week_date: None,
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

    /// `as_dir_name` must carry the real Saturday of that ISO week (verified
    /// independently via `chrono::NaiveDate::from_isoywd_opt`, not just
    /// hand-computed), so the directory name is self-explanatory.
    #[test]
    fn test_week_identifier_as_dir_name() {
        let week = WeekIdentifier::new(2026, 3);
        let saturday = NaiveDate::from_isoywd_opt(2026, 3, Weekday::Sat).unwrap();
        assert_eq!(
            week.as_dir_name(),
            format!(
                "W03-{}-{:02}-{:02}",
                saturday.year(),
                saturday.month(),
                saturday.day()
            )
        );
        assert_eq!(week.as_dir_name(), "W03-2026-01-17");

        let week2 = WeekIdentifier::new(2025, 52);
        let saturday2 = NaiveDate::from_isoywd_opt(2025, 52, Weekday::Sat).unwrap();
        assert_eq!(
            week2.as_dir_name(),
            format!(
                "W52-{}-{:02}-{:02}",
                saturday2.year(),
                saturday2.month(),
                saturday2.day()
            )
        );
        assert_eq!(week2.as_dir_name(), "W52-2025-12-27");

        // Example from the spec: W19 2026 -> Saturday 2026-05-09.
        let week3 = WeekIdentifier::new(2026, 19);
        assert_eq!(week3.as_dir_name(), "W19-2026-05-09");
    }

    /// Invalid `(year, week_number)` combinations (no such ISO week/Saturday)
    /// must fall back to the dateless format instead of panicking.
    #[test]
    fn test_week_identifier_as_dir_name_invalid_week_falls_back_to_dateless() {
        // Week 0 and week 60 are never valid ISO week numbers, for any year.
        assert!(NaiveDate::from_isoywd_opt(2026, 0, Weekday::Sat).is_none());
        assert!(NaiveDate::from_isoywd_opt(2026, 60, Weekday::Sat).is_none());

        let week = WeekIdentifier::new(2026, 0);
        assert_eq!(week.as_dir_name(), "W00-2026");

        let week2 = WeekIdentifier::new(2026, 60);
        assert_eq!(week2.as_dir_name(), "W60-2026");
    }

    #[test]
    fn test_week_identifier_legacy_dir_name() {
        let week = WeekIdentifier::new(2026, 3);
        assert_eq!(week.legacy_dir_name(), "2026-W03");

        let week2 = WeekIdentifier::new(2025, 52);
        assert_eq!(week2.legacy_dir_name(), "2025-W52");
    }

    #[test]
    fn test_week_identifier_ord_is_chronological() {
        assert!(WeekIdentifier::new(2025, 52) < WeekIdentifier::new(2026, 1));
        assert!(WeekIdentifier::new(2026, 19) < WeekIdentifier::new(2026, 27));
        assert_eq!(WeekIdentifier::new(2026, 19), WeekIdentifier::new(2026, 19));
    }

    // -- is_material_week_stale ---------------------------------------------

    /// Material from W19 shown while the calendar is at W27 (the exact
    /// 2-month-stale-mail-parser regression this field guards against) must
    /// be flagged stale.
    #[test]
    fn test_material_week_stale_true_when_latest_precedes_current_calendar_week() {
        // W19 2026 (May) is necessarily before "now" for any real wall-clock
        // time this test can run at, so this exercises the real
        // `WeekIdentifier::current()` (Utc::now()) comparison end to end,
        // matching the spec's "W19 with today in W27" scenario.
        let old = WeekIdentifier::new(2026, 19);
        assert!(old < WeekIdentifier::current());
        assert!(is_material_week_stale(Some(&old)));
    }

    #[test]
    fn test_material_week_stale_false_when_latest_is_current_week() {
        let today = WeekIdentifier::current();
        assert!(!is_material_week_stale(Some(&today)));
    }

    #[test]
    fn test_material_week_stale_false_when_no_resources() {
        assert!(!is_material_week_stale(None));
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
            description: None,
            download_url: "https://example.com/file.zip".to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: dt,
            week_date: None,
            optimized_video_url: None,
            optimized_videos: None,
        };
        let week = resource.week();
        assert_eq!(week.year, 2026);
        assert_eq!(week.week_number, 4);
    }

    /// Minimal JSON payload for a `Resource`, with `week_date` injected as
    /// given by `week_date_json_fragment` (e.g. `"week_date": "2026-05-09"`,
    /// `"week_date": null`, or `""` for an absent key). `created_at` is fixed
    /// at 2026-07-01 (ISO week 27) so tests can assert whether `week()`
    /// followed `week_date` or fell back to it.
    fn resource_json_with_week_date(week_date_json_fragment: &str) -> String {
        let week_date_field = if week_date_json_fragment.is_empty() {
            String::new()
        } else {
            format!(r#""week_date": {},"#, week_date_json_fragment)
        };
        format!(
            r#"{{
                "id": 1,
                "category": "video",
                "title": "Test",
                "download_url": "https://example.com/1.zip",
                "is_active": true,
                "created_at": "2026-07-01T00:00:00Z",
                {week_date_field}
                "optimized_video_url": null
            }}"#
        )
    }

    /// A resource whose newsletter subject dates it to May (week 19) but
    /// that was only inserted into the DB in July (week 27, the ingestion
    /// recovery scenario) must report its true content week, not the insert
    /// week.
    #[test]
    fn test_week_date_present_and_valid_takes_precedence_over_created_at() {
        let json = resource_json_with_week_date(r#""2026-05-09""#);
        let resource: Resource =
            serde_json::from_str(&json).expect("payload with a valid week_date must deserialize");

        assert_eq!(resource.week_date, NaiveDate::from_ymd_opt(2026, 5, 9));
        let week = resource.week();
        assert_eq!(week.year, 2026);
        assert_eq!(week.week_number, 19, "must use week_date, not created_at");
    }

    /// An explicit JSON `null` and an entirely absent `week_date` key must
    /// both deserialize to `None` and fall back to `created_at` for `week()`
    /// — the pre-mail-parser-fix behaviour, unchanged.
    #[test]
    fn test_week_date_null_or_missing_falls_back_to_created_at() {
        for json in [
            resource_json_with_week_date("null"),
            resource_json_with_week_date(""),
        ] {
            let resource: Resource = serde_json::from_str(&json)
                .expect("payload with null/absent week_date must deserialize");
            assert_eq!(resource.week_date, None);
            let week = resource.week();
            assert_eq!(week.year, 2026);
            assert_eq!(
                week.week_number, 27,
                "must fall back to created_at's ISO week"
            );
        }
    }

    /// A malformed `week_date` (wrong format, or the wrong JSON type
    /// entirely) must never fail deserialization of the resource — it
    /// degrades to `None` and `week()` falls back to `created_at`, exactly
    /// like the null/missing case above.
    #[test]
    fn test_week_date_malformed_does_not_break_deserialization_and_falls_back() {
        for json in [
            resource_json_with_week_date(r#""boh""#),
            resource_json_with_week_date("12345"),
        ] {
            let resource: Resource = serde_json::from_str(&json)
                .expect("a malformed week_date must not break Resource deserialization");
            assert_eq!(resource.week_date, None);
            assert_eq!(resource.week().week_number, 27);
        }
    }

    /// A malformed `week_date` on one resource in a batch must not take
    /// down the rest of the poll: `ResourceListResponse` (the real payload
    /// shape) must still deserialize fully.
    #[test]
    fn test_week_date_malformed_does_not_break_batch_deserialization() {
        let json = format!(
            r#"{{ "count": 1, "resources": [{}] }}"#,
            resource_json_with_week_date(r#""boh""#)
        );
        let response: ResourceListResponse = serde_json::from_str(&json)
            .expect("a malformed week_date must not break the whole resources list");
        assert_eq!(response.resources.len(), 1);
        assert_eq!(response.resources[0].week_date, None);
    }

    /// End-to-end (conceptual) staleness regression: a resource inserted
    /// *today* (created_at = Utc::now()) but whose true content is from May
    /// (week_date in the past) must still surface as stale via
    /// `is_material_week_stale`/`latest_week`, because both go through
    /// `week()`, which now prefers `week_date`. Before this feature, this
    /// exact scenario (recovery batch dumped in one INSERT after the
    /// mail-parser stalled) was invisible: `created_at` alone always looked
    /// current.
    #[test]
    fn test_material_week_stale_true_when_week_date_is_old_but_created_at_is_today() {
        let resource = Resource {
            id: 1,
            category: "test".to_string(),
            title: "Test".to_string(),
            description: None,
            download_url: "https://example.com/file.zip".to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at: Utc::now(),
            week_date: NaiveDate::from_ymd_opt(2026, 5, 9),
            optimized_video_url: None,
            optimized_videos: None,
        };

        let latest = latest_week(&[resource]);
        assert_eq!(latest, Some(WeekIdentifier::new(2026, 19)));
        assert!(
            is_material_week_stale(latest.as_ref()),
            "week_date must win over created_at for staleness, even though created_at is today"
        );
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
            tray_close_os_notice_shown: true,
            theme: ThemeSetting::Dark,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    /// A settings.json written before the `theme` field existed must
    /// deserialize to `ThemeSetting::System` (the `#[serde(default)]` default)
    /// rather than failing to parse.
    #[test]
    fn test_theme_missing_key_deserializes_to_system() {
        let json = r#"{
            "work_directory": null,
            "polling_enabled": true,
            "polling_interval_minutes": 60,
            "retention_days": 7,
            "auto_download_categories": [],
            "download_mode": "Queue",
            "prefer_optimized": true,
            "autostart_enabled": false,
            "tray_close_os_notice_shown": false
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.theme, ThemeSetting::System);
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

    #[test]
    fn test_categories_count_response_parsing() {
        let json = r#"{
            "categories": [
                {"name": "decime", "count": 12},
                {"name": "video", "count": 3}
            ],
            "total": 15
        }"#;

        let response: CategoriesCountResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 15);
        assert_eq!(response.categories.len(), 2);
        assert_eq!(response.categories[0].name, "decime");
        assert_eq!(response.categories[0].count, 12);
        assert_eq!(response.categories[1].name, "video");
    }

    #[test]
    fn test_categories_count_response_empty_and_missing_fields() {
        // Empty categories with a total present.
        let empty: CategoriesCountResponse = serde_json::from_str(r#"{"categories":[],"total":0}"#)
            .expect("empty categories must parse");
        assert!(empty.categories.is_empty());
        assert_eq!(empty.total, 0);

        // Both fields absent (serde(default)): degrades to an empty list and
        // zero total instead of failing the whole refresh.
        let bare: CategoriesCountResponse =
            serde_json::from_str("{}").expect("missing fields must default");
        assert!(bare.categories.is_empty());
        assert_eq!(bare.total, 0);
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

    /// contract-resources-api / adr-0003 (additive-only): `description` is
    /// tolerant. An explicit JSON `null` must deserialize to `None` and never
    /// fail the whole poll.
    #[test]
    fn test_description_null_deserializes_to_none() {
        let json = r#"{
            "id": 1,
            "category": "decime",
            "title": "Decime",
            "description": null,
            "download_url": "https://example.com/file.zip",
            "thumbnail_url": null,
            "file_type": null,
            "is_active": true,
            "created_at": "2026-01-17T23:51:02.358083"
        }"#;

        let resource: Resource = serde_json::from_str(json).unwrap();
        assert_eq!(resource.description, None);
    }

    /// Same tolerance for a server that omits the `description` key entirely
    /// (`#[serde(default)]`): it must deserialize to `None`, not error out.
    #[test]
    fn test_description_missing_key_deserializes_to_none() {
        let json = r#"{
            "id": 1,
            "category": "decime",
            "title": "Decime",
            "download_url": "https://example.com/file.zip",
            "thumbnail_url": null,
            "file_type": null,
            "is_active": true,
            "created_at": "2026-01-17T23:51:02.358083"
        }"#;

        let resource: Resource = serde_json::from_str(json).unwrap();
        assert_eq!(resource.description, None);
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
