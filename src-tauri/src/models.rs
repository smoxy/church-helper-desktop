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
    /// UI language. `#[serde(default)]` so a settings.json from a build
    /// predating this field deserializes to `System` instead of failing.
    #[serde(default)]
    pub language: LanguageSetting,
    /// Also auto-download the NEXT week's material (`GET
    /// /api/resources/next-week`, i.e. Saturday S+7) for the enabled
    /// categories. Opt-in (default `false`): the next week's resources are
    /// still fetched and shown in the Dashboard's "next week" section, they
    /// just aren't queued automatically unless this is on.
    pub download_next_week: bool,
    /// Hours between two periodic update checks (besides the one at
    /// startup). Default 6; clamped to at least 1 by the scheduler.
    pub update_check_interval_hours: u32,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum LanguageSetting {
    #[default]
    System,
    Italian,
    English,
}

/// Global counter of bytes saved by downloading the optimized video variant
/// instead of the original (A1/A2: computed backend-side per download, see
/// `services::queue`). Persisted as the `stats` key of `settings.json` —
/// deliberately the settings store rather than `cache.json`, since the latter
/// is treated elsewhere as disposable/derived data and this counter must
/// survive both app restarts and version upgrades.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SavingsStats {
    pub total_saved_bytes: u64,
}

/// Default for [`AppConfig::update_check_interval_hours`].
pub const DEFAULT_UPDATE_CHECK_INTERVAL_HOURS: u32 = 6;
/// Upper bound accepted by [`AppConfig::validate`] for the update check
/// interval (one week).
pub const MAX_UPDATE_CHECK_INTERVAL_HOURS: u32 = 168;

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
            language: LanguageSetting::System, // Default: follow the OS
            download_next_week: false, // Default: opt-in only
            update_check_interval_hours: DEFAULT_UPDATE_CHECK_INTERVAL_HOURS,
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
        if self.update_check_interval_hours < 1
            || self.update_check_interval_hours > MAX_UPDATE_CHECK_INTERVAL_HOURS
        {
            return Err(ConfigValidationError::InvalidUpdateCheckInterval(
                self.update_check_interval_hours,
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValidationError {
    InvalidPollingInterval(u32),
    InvalidUpdateCheckInterval(u32),
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

/// Hover-scrub storyboard of a video resource: one JPEG sprite with `frames`
/// frames laid out on a `cols` x `rows` grid, each `frame_width` x
/// `frame_height` pixels, row-major. `timestamps` (seconds) has exactly one
/// entry per frame, or is empty when the server's list was unusable.
/// Additive metadata (shared contract, 2026-10-16): see
/// `deserialize_lenient_storyboard` for how malformed payloads degrade.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Storyboard {
    pub url: String,
    pub frames: u32,
    pub cols: u32,
    pub rows: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    #[serde(default)]
    pub timestamps: Vec<f64>,
}

/// One certain-data tag of a resource (`facet` is "categoria", "trimestre",
/// "numero", "data" or "durata" today). Kept as free strings so a facet
/// added later by the server still parses; the UI shows only those it knows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceTag {
    pub facet: String,
    pub value: String,
}

/// Resource from the API response.
///
/// `Serialize` is implemented by hand (below) because the IPC/cache JSON
/// also carries the computed `projection_date`; `Deserialize` ignores it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
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
    /// Poster image URL (official title card, else the re-encoder's first
    /// non-black frame). Lenient like the other metadata fields: a
    /// non-string value degrades to `None` instead of failing the poll.
    #[serde(default, deserialize_with = "deserialize_lenient_string")]
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
    /// Id of the resource this one IMMEDIATELY supersedes (errata corrige
    /// published under a new id, e.g. the source changed the file URL). Not
    /// the chain root: for A -> B -> C, C carries B's id. Additive field
    /// (adr-0003): absent on older servers, `None` then.
    #[serde(default)]
    pub supersedes_resource_id: Option<i64>,
    /// Raw source title (uppercase, as found), next to the human `title`.
    /// Additive metadata: absent, `null` or a non-string value -> `None`.
    #[serde(default, deserialize_with = "deserialize_lenient_string")]
    pub title_original: Option<String>,
    /// Video duration in seconds. Additive metadata: absent, `null`, a
    /// non-number or a non-positive / non-finite value -> `None`.
    #[serde(default, deserialize_with = "deserialize_lenient_duration")]
    pub duration_s: Option<f64>,
    /// Hover-scrub sprite. Additive metadata: absent or malformed -> `None`.
    #[serde(default, deserialize_with = "deserialize_lenient_storyboard")]
    pub storyboard: Option<Storyboard>,
    /// Certain-data tags. Additive metadata: absent / not a list -> empty;
    /// malformed entries are skipped one by one.
    #[serde(default, deserialize_with = "deserialize_lenient_tags")]
    pub tags: Vec<ResourceTag>,
}

/// Field-by-field mirror of the derived representation plus the computed
/// `projection_date` ("YYYY-MM-DD", see [`Resource::projection_date`]), so
/// the UI never re-derives the projection Saturday itself. Every stored
/// field must be listed here: `test_resource_serialize_roundtrip` fails if
/// one is forgotten.
impl Serialize for Resource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("Resource", 20)?;
        st.serialize_field("id", &self.id)?;
        st.serialize_field("category", &self.category)?;
        st.serialize_field("title", &self.title)?;
        st.serialize_field("description", &self.description)?;
        st.serialize_field("download_url", &self.download_url)?;
        st.serialize_field("thumbnail_url", &self.thumbnail_url)?;
        st.serialize_field("file_type", &self.file_type)?;
        st.serialize_field("checksum", &self.checksum)?;
        st.serialize_field("is_active", &self.is_active)?;
        st.serialize_field("created_at", &self.created_at)?;
        st.serialize_field("week_date", &self.week_date)?;
        st.serialize_field("optimized_video_url", &self.optimized_video_url)?;
        st.serialize_field("optimized_videos", &self.optimized_videos)?;
        st.serialize_field("supersedes_resource_id", &self.supersedes_resource_id)?;
        st.serialize_field("title_original", &self.title_original)?;
        st.serialize_field("duration_s", &self.duration_s)?;
        st.serialize_field("storyboard", &self.storyboard)?;
        st.serialize_field("tags", &self.tags)?;
        st.serialize_field(
            "projection_date",
            &self
                .projection_date()
                .map(|d| d.format("%Y-%m-%d").to_string()),
        )?;
        st.end()
    }
}

/// Lenient `Option<String>`: any non-string value degrades to `None`.
fn deserialize_lenient_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| v.as_str().map(str::to_owned)))
}

/// Lenient duration: a finite, strictly positive JSON number, else `None`.
fn deserialize_lenient_duration<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| v.as_f64())
        .filter(|d| d.is_finite() && *d > 0.0))
}

/// Lenient storyboard: `None` unless the object parses and is coherent
/// (non-empty URL, positive sizes, `frames` within the `cols` x `rows`
/// grid). A `timestamps` list of the wrong length or with non-finite
/// values is dropped (emptied) rather than discarding the whole sprite.
fn deserialize_lenient_storyboard<'de, D>(deserializer: D) -> Result<Option<Storyboard>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| serde_json::from_value::<Storyboard>(v).ok())
        .and_then(sanitize_storyboard))
}

fn sanitize_storyboard(mut sb: Storyboard) -> Option<Storyboard> {
    let grid = u64::from(sb.cols) * u64::from(sb.rows);
    let coherent = !sb.url.trim().is_empty()
        && sb.frames > 0
        && sb.cols > 0
        && sb.rows > 0
        && u64::from(sb.frames) <= grid
        && sb.frame_width > 0
        && sb.frame_height > 0;
    if !coherent {
        return None;
    }
    let timestamps_ok = sb.timestamps.len() == sb.frames as usize
        && sb.timestamps.iter().all(|t| t.is_finite() && *t >= 0.0);
    if !timestamps_ok {
        sb.timestamps.clear();
    }
    Some(sb)
}

/// Lenient tags: a non-list degrades to an empty list; each entry that is
/// not a `{facet, value}` pair of non-empty strings is skipped.
fn deserialize_lenient_tags<'de, D>(deserializer: D) -> Result<Vec<ResourceTag>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(serde_json::Value::Array(items)) = value else {
        return Ok(Vec::new());
    };
    Ok(items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<ResourceTag>(item).ok())
        .filter(|tag| !tag.facet.trim().is_empty() && !tag.value.trim().is_empty())
        .collect())
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

    /// The Saturday this resource is projected on: the Saturday of the ISO
    /// week returned by [`Resource::week`] (so `week_date` when it is that
    /// Saturday, else the Saturday of `created_at`'s UTC ISO week). It is
    /// always inside the week the file is filed under. `None` only for a
    /// week with no valid ISO Saturday, which `week()` never produces.
    pub fn projection_date(&self) -> Option<NaiveDate> {
        let week = self.week();
        NaiveDate::from_isoywd_opt(week.year, week.week_number, Weekday::Sat)
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
    /// Category of the downloaded resource. Used by the errata pass to spot a
    /// live file whose resource vanished from the server while a NEW resource
    /// of the same week + category appeared (errata with a new id). `None` for
    /// entries written by builds that predate the field, until the errata pass
    /// backfills it from the server list (`errata::backfill_categories`).
    #[serde(default)]
    pub category: Option<String>,
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
            supersedes_resource_id: None,
            title_original: None,
            duration_s: None,
            storyboard: None,
            tags: Vec::new(),
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
            supersedes_resource_id: None,
            title_original: None,
            duration_s: None,
            storyboard: None,
            tags: Vec::new(),
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
            supersedes_resource_id: None,
            title_original: None,
            duration_s: None,
            storyboard: None,
            tags: Vec::new(),
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
            language: LanguageSetting::Italian,
            download_next_week: true,
            update_check_interval_hours: 12,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    /// `download_next_week` is opt-in, and a settings.json written before the
    /// field (or `update_check_interval_hours`) existed must load with the
    /// defaults instead of resetting the user's config.
    #[test]
    fn test_next_week_and_update_interval_defaults_and_missing_keys() {
        let config = AppConfig::default();
        assert!(!config.download_next_week, "next week download is opt-in");
        assert_eq!(config.update_check_interval_hours, 6);

        let json = r#"{
            "work_directory": "/data",
            "polling_enabled": true,
            "polling_interval_minutes": 60,
            "retention_days": 7,
            "auto_download_categories": ["missioni"],
            "download_mode": "Queue",
            "prefer_optimized": true,
            "autostart_enabled": false,
            "tray_close_os_notice_shown": true,
            "theme": "System",
            "language": "Italian"
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!config.download_next_week);
        assert_eq!(config.update_check_interval_hours, 6);
        assert_eq!(
            config.auto_download_categories,
            vec!["missioni".to_string()]
        );
        assert!(config.tray_close_os_notice_shown);
    }

    #[test]
    fn test_config_validation_update_interval_bounds() {
        for hours in [0, MAX_UPDATE_CHECK_INTERVAL_HOURS + 1] {
            let config = AppConfig {
                update_check_interval_hours: hours,
                ..Default::default()
            };
            assert_eq!(
                config.validate(),
                Err(ConfigValidationError::InvalidUpdateCheckInterval(hours))
            );
        }
        for hours in [1, 6, MAX_UPDATE_CHECK_INTERVAL_HOURS] {
            let config = AppConfig {
                update_check_interval_hours: hours,
                ..Default::default()
            };
            assert!(config.validate().is_ok());
        }
    }

    /// Contract C: `supersedes_resource_id` is additive and nullable — absent
    /// (old server), `null` and a value must all deserialize.
    #[test]
    fn test_supersedes_resource_id_is_additive() {
        let base = r#""id": 7, "category": "missioni", "title": "T",
            "download_url": "https://example.com/7.zip", "thumbnail_url": null,
            "file_type": null, "is_active": true,
            "created_at": "2026-10-16T10:00:00""#;
        let absent: Resource = serde_json::from_str(&format!("{{{base}}}")).unwrap();
        assert_eq!(absent.supersedes_resource_id, None);
        let null: Resource =
            serde_json::from_str(&format!("{{{base}, \"supersedes_resource_id\": null}}")).unwrap();
        assert_eq!(null.supersedes_resource_id, None);
        let some: Resource =
            serde_json::from_str(&format!("{{{base}, \"supersedes_resource_id\": 5}}")).unwrap();
        assert_eq!(some.supersedes_resource_id, Some(5));
    }

    const META_BASE: &str = r#""id": 7, "category": "missioni", "title": "T",
            "download_url": "https://example.com/7.zip",
            "file_type": null, "is_active": true,
            "created_at": "2026-10-16T10:00:00""#;

    /// Shared metadata contract: a payload from a server that predates it
    /// (no thumbnail_url/storyboard/duration_s/tags/title_original at all)
    /// still parses, with every new field empty.
    #[test]
    fn test_metadata_fields_absent_are_tolerated() {
        let r: Resource = serde_json::from_str(&format!("{{{META_BASE}}}")).unwrap();
        assert_eq!(r.thumbnail_url, None);
        assert_eq!(r.storyboard, None);
        assert_eq!(r.duration_s, None);
        assert!(r.tags.is_empty());
        assert_eq!(r.title_original, None);
    }

    /// A well-formed payload fills every field as given.
    #[test]
    fn test_metadata_fields_well_formed() {
        let ts: Vec<String> = (0..15).map(|i| format!("{}.5", i * 20)).collect();
        let json = format!(
            r#"{{{META_BASE}, "thumbnail_url": "https://v.example/2026-W42/a.poster.jpg",
            "duration_s": 298.4, "title_original": "L'EFFETTO GREGGE",
            "storyboard": {{"url": "https://v.example/2026-W42/a.storyboard.jpg",
                "frames": 15, "cols": 5, "rows": 3, "frame_width": 320,
                "frame_height": 180, "timestamps": [{}]}},
            "tags": [{{"facet": "trimestre", "value": "2026-Q4"}},
                     {{"facet": "numero", "value": "03"}}]}}"#,
            ts.join(",")
        );
        let r: Resource = serde_json::from_str(&json).unwrap();
        assert_eq!(
            r.thumbnail_url.as_deref(),
            Some("https://v.example/2026-W42/a.poster.jpg")
        );
        assert_eq!(r.duration_s, Some(298.4));
        assert_eq!(r.title_original.as_deref(), Some("L'EFFETTO GREGGE"));
        let sb = r.storyboard.expect("storyboard");
        assert_eq!((sb.frames, sb.cols, sb.rows), (15, 5, 3));
        assert_eq!((sb.frame_width, sb.frame_height), (320, 180));
        assert_eq!(sb.timestamps.len(), 15);
        assert_eq!(
            r.tags,
            vec![
                ResourceTag {
                    facet: "trimestre".into(),
                    value: "2026-Q4".into()
                },
                ResourceTag {
                    facet: "numero".into(),
                    value: "03".into()
                },
            ]
        );
    }

    /// Malformed metadata never fails the resource (one bad field must not
    /// take down the whole poll): each degrades to its empty value.
    #[test]
    fn test_metadata_fields_malformed_are_tolerated() {
        let cases = [
            // wrong JSON types everywhere
            r#""thumbnail_url": 5, "duration_s": "4:58", "title_original": [],
               "storyboard": "x", "tags": {"facet": "numero"}"#,
            // incoherent storyboard (frames beyond the grid), bad numbers
            r#""duration_s": -3, "storyboard": {"url": "u", "frames": 16,
               "cols": 5, "rows": 3, "frame_width": 320, "frame_height": 180,
               "timestamps": []}, "tags": null"#,
            // storyboard missing required keys, zero duration
            r#""duration_s": 0, "storyboard": {"url": "u", "frames": 15}"#,
            // empty url, zero size
            r#""storyboard": {"url": " ", "frames": 15, "cols": 5, "rows": 3,
               "frame_width": 0, "frame_height": 180}"#,
        ];
        for extra in cases {
            let r: Resource = serde_json::from_str(&format!("{{{META_BASE}, {extra}}}"))
                .unwrap_or_else(|e| panic!("{extra}: {e}"));
            assert_eq!(r.thumbnail_url, None, "{extra}");
            assert_eq!(r.duration_s, None, "{extra}");
            assert_eq!(r.storyboard, None, "{extra}");
            assert!(r.tags.is_empty(), "{extra}");
            assert_eq!(r.title_original, None, "{extra}");
        }

        // Partially bad: malformed tag entries are skipped one by one, and a
        // timestamps list of the wrong length is dropped, keeping the sprite.
        let r: Resource = serde_json::from_str(&format!(
            r#"{{{META_BASE}, "tags": [{{"facet": "numero", "value": "03"}}, 7,
                {{"facet": "durata"}}, {{"facet": "", "value": "x"}}],
              "storyboard": {{"url": "u", "frames": 15, "cols": 5, "rows": 3,
                "frame_width": 320, "frame_height": 180, "timestamps": [1, 2]}}}}"#
        ))
        .unwrap();
        assert_eq!(r.tags.len(), 1);
        assert_eq!(r.tags[0].value, "03");
        let sb = r.storyboard.expect("sprite kept");
        assert!(sb.timestamps.is_empty());
    }

    /// The IPC/cache JSON carries every stored field (round-trip equality)
    /// plus the computed `projection_date`, which deserialization ignores.
    #[test]
    fn test_resource_serialize_roundtrip_and_projection_date() {
        let json = format!(
            r#"{{{META_BASE}, "week_date": "2026-10-17", "thumbnail_url": "https://p/x.jpg",
            "description": "d", "checksum": "abc", "optimized_video_url": "https://o/x.mp4",
            "optimized_videos": [{{"url": "https://o/x.mp4", "label": "720p", "size_bytes": 9}}],
            "supersedes_resource_id": 3, "duration_s": 298.4, "title_original": "T",
            "storyboard": {{"url": "u", "frames": 15, "cols": 5, "rows": 3,
                "frame_width": 320, "frame_height": 180, "timestamps": []}},
            "tags": [{{"facet": "numero", "value": "03"}}]}}"#
        );
        let r: Resource = serde_json::from_str(&json).unwrap();
        let value = serde_json::to_value(&r).unwrap();
        assert_eq!(value["projection_date"], "2026-10-17");
        let back: Resource = serde_json::from_value(value).unwrap();
        assert_eq!(back, r);
    }

    /// `projection_date` follows `week()`: the Saturday of `week_date`'s ISO
    /// week, else of `created_at`'s UTC ISO week (Sunday late evening and
    /// Monday just after midnight stay in their own ISO week).
    #[test]
    fn test_projection_date_same_rule_as_week() {
        let parse = |extra: &str| -> Resource {
            serde_json::from_str(&format!(
                r#"{{"id": 1, "category": "x", "title": "t", "download_url": "u",
                "file_type": null, "is_active": true, {extra}}}"#
            ))
            .unwrap()
        };
        let sat = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        let cases = [
            (r#""created_at": "2026-10-16T08:00:00Z""#, sat(2026, 10, 17)),
            (r#""created_at": "2026-10-18T23:30:00Z""#, sat(2026, 10, 17)),
            (r#""created_at": "2026-10-12T00:10:00Z""#, sat(2026, 10, 17)),
            (r#""created_at": "2026-10-17T12:00:00Z""#, sat(2026, 10, 17)),
            (
                r#""created_at": "2026-10-16T08:00:00Z", "week_date": "2026-10-10""#,
                sat(2026, 10, 10),
            ),
            // Year boundary: ISO 2026-W53 Saturday is 2027-01-02.
            (r#""created_at": "2026-12-31T10:00:00Z""#, sat(2027, 1, 2)),
        ];
        for (extra, expected) in cases {
            let r = parse(extra);
            assert_eq!(r.projection_date(), expected, "{extra}");
            let week = r.week();
            let p = r.projection_date().unwrap();
            assert_eq!(WeekIdentifier::from_naive_date(p), week, "{extra}");
        }
    }

    /// A registry entry written before `category` existed still loads.
    #[test]
    fn test_downloaded_file_missing_category_deserializes_to_none() {
        let json = r#"{
            "resource_id": 1,
            "week": {"year": 2026, "week_number": 42},
            "local_path": "/data/W42-2026-10-17/a.mp4",
            "downloaded_at": "2026-10-16T10:00:00Z",
            "source_url": "https://example.com/a.mp4",
            "is_superseded": false
        }"#;
        let entry: DownloadedFile = serde_json::from_str(json).unwrap();
        assert_eq!(entry.category, None);
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

    /// A settings.json written before the `language` field existed must
    /// deserialize to `LanguageSetting::System` (the `#[serde(default)]`
    /// default) rather than failing to parse.
    #[test]
    fn test_language_missing_key_deserializes_to_system() {
        let json = r#"{
            "work_directory": null,
            "polling_enabled": true,
            "polling_interval_minutes": 60,
            "retention_days": 7,
            "auto_download_categories": [],
            "download_mode": "Queue",
            "prefer_optimized": true,
            "autostart_enabled": false,
            "tray_close_os_notice_shown": false,
            "theme": "System"
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.language, LanguageSetting::System);
    }

    /// serde roundtrip for each `LanguageSetting` variant individually, so a
    /// regression that breaks one specific variant's (de)serialization isn't
    /// masked by the others in the full-config roundtrip test above.
    #[test]
    fn test_language_setting_serde_roundtrip_all_variants() {
        for variant in [
            LanguageSetting::System,
            LanguageSetting::Italian,
            LanguageSetting::English,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: LanguageSetting = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
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
