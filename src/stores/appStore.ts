import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {create} from 'zustand';

import {AppConfig, AppStatus, Resource, ResourceListResponse, WeekIdentifier} from '../types';

export interface ActiveDownload {
  progress: number;
  status: 'pending'|'downloading'|'paused'|'completed'|'error';
  error?: string;
  integrity?: 'verified'|'mismatch'|'unknown';
  path?: string;
  hash?: string;
  // Added fields
  currentBytes?: number;
  totalBytes?: number;
  queuePosition?: number;
  startTime?: number;
}

export interface QueueStatusPayload {
  queued: Array<{id: number, position: number}>;
  active: number[];
}

export interface ResourceSummary {
  total: number;
  downloaded: number;
  active: number;
  queued: number;
}

interface AppState {
  config: AppConfig|null;
  status: AppStatus|null;
  resources: Resource[];
  activeDownloads: Record<number, ActiveDownload>;
  summary: ResourceSummary|null;
  archivedWeeks: WeekIdentifier[];
  isLoading: boolean;
  error: string|null;

  // Actions
  fetchInitialData: () => Promise<void>;
  updateConfig: (config: Partial<AppConfig>) => Promise<void>;
  forcePoll: () => Promise<void>;
  selectWorkDirectory: () => Promise<void>;
  togglePolling: (enabled: boolean) => Promise<void>;
  setPollingInterval: (minutes: number) => Promise<void>;
  setRetentionDays: (days: number|null) => Promise<void>;
  fetchArchivedWeeks: () => Promise<void>;
  fetchSummary: () => Promise<void>;
  startDownload: (resource: Resource) => Promise<void>;
  pauseDownload: (resourceId: number) => Promise<void>;
  resumeDownload: (resource: Resource) => Promise<void>;
  cancelDownload: (resourceId: number) => Promise<void>;
}

// Simple debounce helper
function debounce<T extends (...args: any[]) => any>(
  func: T,
  wait: number
): (...args: Parameters<T>) => void {
  let timeout: ReturnType<typeof setTimeout> | null = null;
  return (...args: Parameters<T>) => {
    if (timeout) clearTimeout(timeout);
    timeout = setTimeout(() => func(...args), wait);
  };
}

export const useAppStore = create<AppState>(
    (set, get) => {
      // Create debounced version of fetchSummary for event listeners
      const debouncedFetchSummary = debounce(() => {
        get().fetchSummary();
      }, 300);

      return {
      config: null,
      status: null,
      resources: [],
      archivedWeeks: [],
      isLoading: true,
      error: null,
      activeDownloads: {},
      summary: null,

      fetchInitialData: async () => {
        try {
          set({isLoading: true, error: null});

          const [config, status, resources, summary] = await Promise.all([
            invoke<AppConfig>('get_config'),
            invoke<AppStatus>('get_status'),
            invoke<Resource[]>('get_resources'),
            invoke<ResourceSummary>('get_resource_summary'),
          ]);

          // If work directory is set, fetch archived weeks
          let archivedWeeks: WeekIdentifier[] = [];
          if (config.work_directory) {
            try {
              archivedWeeks =
                  await invoke<WeekIdentifier[]>('get_archived_weeks');
            } catch (e) {
              console.error('Failed to fetch archived weeks', e);
            }
          }

          set({
            config,
            status,
            resources,
            summary,
            archivedWeeks,
            isLoading: false
          });

          // Listen for updates
          await listen<ResourceListResponse>('resources-updated', (event) => {
            set({resources: event.payload.resources});
            // Also refresh status to update last poll time
            invoke<AppStatus>('get_status').then(status => set({status}));
            debouncedFetchSummary();
          });

          await listen<string>('poll-error', (event) => {
            set({error: `Poll error: ${event.payload}`});
          });

          await listen('poll-tick', () => {
            invoke<AppStatus>('get_status').then(status => set({status}));
          });

          // Global download progress listener
          await listen<{
            id: number,
            progress: number,
            current_bytes?: number,
            total_bytes?: number
          }>('download-progress', (event) => {
            set(state => {
              const current = state.activeDownloads[event.payload.id];
              // If paused, don't update progress (though backend shouldn't
              // emit)
              if (!current || current.status !== 'downloading') return state;

              return {
                activeDownloads: {
                  ...state.activeDownloads,
                  [event.payload.id]: {
                    ...current,
                    progress: event.payload.progress,
                    currentBytes: event.payload.current_bytes,
                    totalBytes: event.payload.total_bytes,
                    // Set start time if not set
                    startTime: current.startTime || Date.now()
                  }
                }
              };
            });
          });

          // Listen for queue status changes
          await listen<QueueStatusPayload>('queue-status-changed', (event) => {
            set(state => {
              const {queued, active} = event.payload;
              const newActiveDownloads = {...state.activeDownloads};

              // Update queue positions for queued items
              queued.forEach(item => {
                // If we know about this download (it's potentially in
                // pending/paused/downloading state)
                if (newActiveDownloads[item.id]) {
                  newActiveDownloads[item.id] = {
                    ...newActiveDownloads[item.id],
                    queuePosition: item.position,
                    status: 'pending'  // Ensure it's marked pending if in queue
                  };
                } else {
                  // New item from queue we didn't know about? Add it.
                  newActiveDownloads[item.id] = {
                    progress: 0,
                    status: 'pending',
                    queuePosition: item.position
                  };
                }
              });

              return {activeDownloads: newActiveDownloads};
            });
            debouncedFetchSummary();
          });

          // Listen for download start from queue
          await listen<number>('download-started', (event) => {
            const resourceId = event.payload;
            console.log(
                `[DownloadEvent] Received download-started for resource ${
                    resourceId}`);
            set(state => {
              const current = state.activeDownloads[resourceId];

              if (current) {
                console.log(
                    `[DownloadEvent] Resource ${resourceId} found (status: ${
                        current.status}), updating to downloading`);
                return {
                  activeDownloads: {
                    ...state.activeDownloads,
                    [resourceId]: {
                      ...current,
                      status: 'downloading',
                      // Reset error if retrying
                      error: undefined
                    }
                  }
                };
              }

              console.log(`[DownloadEvent] Adding new resource ${
                  resourceId} to activeDownloads`);
              return {
                activeDownloads: {
                  ...state.activeDownloads,
                  [resourceId]: {progress: 0, status: 'downloading'}
                }
              };
            });
            debouncedFetchSummary();
          });

          // Listen for download completion from auto-download queue
          await listen<number>('download-complete', (event) => {
            const resourceId = event.payload;
            set(state => {
              const current = state.activeDownloads[resourceId];
              if (!current) {
                // Auto-download completed for a resource not manually initiated
                // Don't add it to activeDownloads to avoid clutter
                return state;
              }

              // Update existing download to completed
              return {
                activeDownloads: {
                  ...state.activeDownloads,
                  [resourceId]: {...current, progress: 100, status: 'completed'}
                }
              };
            });
            debouncedFetchSummary();
          });

          // Listen for download failures
          await listen<{id: number, error: string}>('download-failed', (event) => {
            const { id: resourceId, error } = event.payload;
            console.log(`[DownloadEvent] Received download-failed for resource ${resourceId}: ${error}`);
            
            set(state => {
              const current = state.activeDownloads[resourceId];
              if (!current) {
                // Failed download for unknown resource - add it to show the error
                return {
                  activeDownloads: {
                    ...state.activeDownloads,
                    [resourceId]: {
                      progress: 0,
                      status: 'error',
                      error
                    }
                  }
                };
              }

              // Update existing download to error state
              return {
                activeDownloads: {
                  ...state.activeDownloads,
                  [resourceId]: {
                    ...current,
                    status: 'error',
                    error
                  }
                }
              };
            });
            debouncedFetchSummary();
          });

        } catch (e) {
          set({error: `Initialization failed: ${e}`, isLoading: false});
        }
      },

      updateConfig: async (updates) => {
        const currentConfig = get().config;
        if (!currentConfig) return;

        const newConfig = {...currentConfig, ...updates};
        try {
          await invoke('set_config', {config: newConfig});
          set({config: newConfig});
        } catch (e) {
          set({error: `Failed to update config: ${e}`});
          throw e;
        }
      },

      forcePoll: async () => {
        try {
          set({isLoading: true});
          const response = await invoke<ResourceListResponse>('force_poll');
          const status = await invoke<AppStatus>('get_status');
          set({resources: response.resources, status, isLoading: false});
        } catch (e) {
          set({error: `Manual poll failed: ${e}`, isLoading: false});
        }
      },

      selectWorkDirectory: async () => {
        try {
          const path = await invoke<string|null>('select_work_directory');
          if (path) {
            await invoke('set_work_directory', {path});
            // Refresh config and archived weeks
            const config = await invoke<AppConfig>('get_config');
            const archivedWeeks =
                await invoke<WeekIdentifier[]>('get_archived_weeks');
            set({config, archivedWeeks});
          }
        } catch (e) {
          set({error: `Failed to select directory: ${e}`});
        }
      },

      togglePolling: async (enabled) => {
        try {
          await invoke('set_polling_enabled', {enabled});
          // Refresh config and status
          const [config, status] = await Promise.all([
            invoke<AppConfig>('get_config'),
            invoke<AppStatus>('get_status'),
          ]);
          set({config, status});
        } catch (e) {
          set({error: `Failed to toggle polling: ${e}`});
        }
      },

      setPollingInterval: async (minutes) => {
        try {
          await invoke('set_polling_interval', {minutes});
          const config = await invoke<AppConfig>('get_config');
          set({config});
        } catch (e) {
          set({error: `Failed to set interval: ${e}`});
        }
      },

      setRetentionDays: async (days) => {
        try {
          await invoke('set_retention_days', {days});
          const config = await invoke<AppConfig>('get_config');
          set({config});
        } catch (e) {
          set({error: `Failed to set retention: ${e}`});
        }
      },

      fetchArchivedWeeks: async () => {
        try {
          const archivedWeeks =
              await invoke<WeekIdentifier[]>('get_archived_weeks');
          set({archivedWeeks});
        } catch (e) {
          // Silently fail if e.g. dir not set
          console.error(e);
        }
      },

      fetchSummary: async () => {
        try {
          const summary = await invoke<ResourceSummary>('get_resource_summary');
          set({summary});
        } catch (e) {
          console.error('Failed to fetch summary', e);
        }
      },

      startDownload: async (resource: Resource) => {
        const {activeDownloads} = get();
        // If already downloading, do nothing
        if (activeDownloads[resource.id]?.status === 'downloading') return;

        // Mark as downloading immediately
        set(state => ({
              activeDownloads: {
                ...state.activeDownloads,
                [resource.id]:
                    {progress: 0, status: 'downloading', error: undefined}
              }
            }));

        try {
          // Just trigger the download, queue and events handle the rest
          await invoke('download_resource', {resource});
          // The download-started, download-progress, and download-complete
          // events will update the state automatically
        } catch (error: any) {
          const errorMessage = typeof error === 'string' ?
              error :
              error.message || 'Download failed';

          set(state => ({
                activeDownloads: {
                  ...state.activeDownloads,
                  [resource.id]:
                      {progress: 0, status: 'error', error: errorMessage}
                }
              }));
        }
      },

      pauseDownload: async (resourceId: number) => {
        set(state => ({
              activeDownloads: {
                ...state.activeDownloads,
                [resourceId]:
                    {...state.activeDownloads[resourceId], status: 'paused'}
              }
            }));
        // Retry a few times in case the lock is temporarily held
        for (let attempt = 0; attempt < 3; attempt++) {
          try {
            await invoke('pause_download', {resourceId});
            return;
          } catch (e) {
            if (attempt < 2) {
              await new Promise(resolve => setTimeout(resolve, 50));
            } else {
              console.error('Failed to pause after retries', e);
            }
          }
        }
      },

      resumeDownload: async (resource: Resource) => {
        // Just call startDownload
        get().startDownload(resource);
      },

      cancelDownload: async (resourceId: number) => {
        // Remove from state immediately to update UI
        set(state => {
          const {[resourceId]: deleted, ...rest} = state.activeDownloads;
          return {activeDownloads: rest};
        });
        // Retry a few times in case the lock is temporarily held
        for (let attempt = 0; attempt < 3; attempt++) {
          try {
            await invoke('cancel_download', {resourceId});
            return;
          } catch (e) {
            if (attempt < 2) {
              await new Promise(resolve => setTimeout(resolve, 50));
            } else {
              console.error('Failed to cancel after retries', e);
            }
          }
        }
      }

    };
  });
