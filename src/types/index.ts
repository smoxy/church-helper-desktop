export interface WeekIdentifier {
  year: number;
  week_number: number;
}

// Structured error carried across the Tauri IPC boundary. Mirrors the Rust
// `CommandError` struct (src-tauri/src/error.rs): a command's rejected `invoke`
// resolves with this JSON object as its reason. `code` is a stable kebab-case
// identifier the UI can branch on (e.g. 'work-dir-not-set', 'api-unreachable');
// `message` is the human-readable detail to show the user. Use `errorMessage()`
// / `isCommandError()` from lib/utils to consume caught values safely.
export interface CommandError {
  code: string;
  message: string;
}

// A single optimized video variant produced by the re-encoder from a
// resource's original zip (adr-0008: matching per provenienza). Mirrors
// `OptimizedVideo` in src-tauri/src/models.rs exactly.
export interface OptimizedVideo {
  url: string;
  label: string;
  size_bytes: number;
}

export interface Resource {
  id: number;
  category: string;
  title: string;
  description: string|null;
  download_url: string;
  thumbnail_url: string|null;
  file_type: string|null;
  checksum?: string|null;
  is_active: boolean;
  created_at: string;  // ISO date string
  // True calendar week of the content ("YYYY-MM-DD", from the newsletter
  // subject), as opposed to created_at (the DB insert timestamp). Additive
  // (adr-0003): absent/null on servers that predate the mail-parser field.
  // Mirrors the backend's `week_date: Option<NaiveDate>` (src-tauri/src/models.rs);
  // the UI only reads this, week derivation stays entirely backend-side.
  week_date: string|null;
  optimized_video_url?: string|null;
  // Additive (adr-0008): absent/null on older servers. When present, ordered
  // by size_bytes desc by the producer; optimized_video_url is always the
  // first element (compat default). >1 elements means the desktop must let
  // the user choose which one to download (see ResourceDetail).
  optimized_videos?: OptimizedVideo[]|null;
}

// UI colour theme. Mirrors the Rust `ThemeSetting` enum (src-tauri/src/models.rs).
export type ThemeSetting = 'System'|'Light'|'Dark';

// UI language. Mirrors the Rust `LanguageSetting` enum (src-tauri/src/models.rs).
export type LanguageSetting = 'System'|'Italian'|'English';

export interface AppConfig {
  work_directory: string|null;
  polling_enabled: boolean;
  polling_interval_minutes: number;
  retention_days: number|null;
  auto_download_categories: string[];
  download_mode: 'Queue'|'Parallel';
  prefer_optimized: boolean;
  autostart_enabled: boolean;
  // Whether the one-time OS notice about the app staying in the tray has
  // already been shown (backend-owned; set once in lib.rs on first close-to-tray).
  tray_close_os_notice_shown: boolean;
  theme: ThemeSetting;
  language: LanguageSetting;
}

export interface AppStatus {
  polling_active: boolean;
  last_poll_time: string|null;  // ISO date string
  current_week: WeekIdentifier|null;
  total_resources: number;
  pending_downloads: number;
  has_superseded_files: boolean;
  // True when the material currently available belongs to a week earlier
  // than the calendar's current week (i.e. the backend hasn't found this
  // week's resources yet). Drives the "material not up to date" banner on
  // the Dashboard; the UI only reads this flag, it never derives it.
  material_week_stale: boolean;
}

export interface ResourceListResponse {
  count: number;
  resources: Resource[];
}

// One category and its resource count from the `categories/counts` endpoint.
// Mirrors the Rust `CategoryCount` struct (src-tauri/src/models.rs). Delivered
// to the UI by the `get_all_categories` command and the `categories-updated`
// event so Settings can list categories beyond the current week's resources.
export interface CategoryCount {
  name: string;
  count: number;
}

// Payload of the `errata-detected` event, emitted by the backend after a poll
// finds one or more resources superseded by an errata corrige. Mirrors the
// `serde_json::json!({ "resourceIds": ... })` payload in
// src-tauri/src/services/errata.rs::process_errata.
export interface ErrataDetectedPayload {
  resourceIds: number[];
}

// Payload of the `download-complete` event. Mirrors the
// `serde_json::json!({ "id", "optimized", ... })` payload emitted in
// src-tauri/src/services/queue.rs when a download finishes: `optimized` is
// true when the actually-downloaded URL was an optimized variant (computed
// backend-side via Resource::get_effective_download_url, the only reliable
// source — the frontend cannot derive this for auto-downloads, which never
// enter activeDownloads).
//
// The size/savings fields are only ever populated when `optimized` is true
// (savings are computed against the original size, so a non-optimized
// download has nothing to compare against), and `original_bytes` is
// deliberately CACHE-ONLY here (never a network request) so this event is
// never delayed by a HEAD request: it's `null` whenever the original size
// wasn't already cached at completion time. `saved_bytes` is `null`
// whenever either size is unknown, or when the original isn't actually
// larger than what was downloaded. `total_saved_bytes` is the persistent
// global counter's value *after* this download's contribution (if any) was
// applied, so the UI never needs a separate fetch to stay in sync. When
// `original_bytes` is `null` for an optimized download, a later
// `savings-resolved` event (see `SavingsResolvedPayload`) fills it in.
export interface DownloadCompletePayload {
  id: number;
  optimized: boolean;
  optimized_bytes: number|null;
  original_bytes: number|null;
  saved_bytes: number|null;
  total_saved_bytes: number;
}

// Payload of the `savings-resolved` event, emitted by a task DETACHED from
// the download body (src-tauri/src/services/queue.rs::start_worker) when a
// `download-complete` event reported `original_bytes: null` (the size wasn't
// cached yet): a best-effort HEAD request (5s timeout) resolves it in the
// background, without delaying the worker slot or the completion event.
// Mirrors the `serde_json::json!({ "id", "saved_bytes", ... })` payload.
// Upgrades the matching celebration panel (by `id` === `Celebration.resourceId`)
// from its generic "no savings info" copy to the full savings layout. The
// backend guarantees this download's `saved_bytes` is folded into
// `total_saved_bytes` exactly once across the two events — either in the
// preceding `download-complete` (when already known) or here, never both.
export interface SavingsResolvedPayload {
  id: number;
  saved_bytes: number|null;
  original_bytes: number|null;
  total_saved_bytes: number;
}

// Result of the `get_savings_stats` command. Mirrors the Rust `SavingsStats`
// struct (src-tauri/src/models.rs): the persistent, cross-session running
// total of bytes saved by optimized downloads (see `add_saved_bytes` in
// src-tauri/src/commands.rs). Used to seed `celebrationStore`'s
// `totalSavedBytes` on startup; subsequent updates arrive via each
// `download-complete` event's `total_saved_bytes` field, no re-fetch needed.
export interface SavingsStats {
  total_saved_bytes: number;
}

// Batched per-resource status returned by the `get_resources_status` command.
// Mirrors the Rust `ResourceStatus` struct (src-tauri/src/commands.rs). The
// backing HashMap<i64, _> serializes its integer keys as strings, so the
// command result is consumed as Record<number, ResourceStatus>. Sizes come
// only from the cached HEAD sizes (null when unknown/uncached).
export interface ResourceStatus {
  downloaded: boolean;
  file_size: number|null;
  optimized_file_size: number|null;
}
