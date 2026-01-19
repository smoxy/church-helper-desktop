export interface WeekIdentifier {
  year: number;
  week_number: number;
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
}

export interface AppConfig {
  work_directory: string|null;
  polling_enabled: boolean;
  polling_interval_minutes: number;
  retention_days: number|null;
  auto_download_categories: string[];
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
