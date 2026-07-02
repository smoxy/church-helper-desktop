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
