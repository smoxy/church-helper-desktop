export interface WeekIdentifier {
  year: number;
  week_number: number;
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
  optimized_video_url?: string|null;
  // Additive (adr-0008): absent/null on older servers. When present, ordered
  // by size_bytes desc by the producer; optimized_video_url is always the
  // first element (compat default). >1 elements means the desktop must let
  // the user choose which one to download (see ResourceDetail).
  optimized_videos?: OptimizedVideo[]|null;
}

export interface AppConfig {
  work_directory: string|null;
  polling_enabled: boolean;
  polling_interval_minutes: number;
  retention_days: number|null;
  auto_download_categories: string[];
  download_mode: 'Queue'|'Parallel';
  prefer_optimized: boolean;
  autostart_enabled: boolean;
}

export interface AppStatus {
  polling_active: boolean;
  last_poll_time: string|null;  // ISO date string
  total_resources: number;
  current_week: WeekIdentifier|null;
}

export interface ResourceListResponse {
  count: number;
  resources: Resource[];
}
