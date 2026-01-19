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
}

interface AppState {
  config: AppConfig|null;
  status: AppStatus|null;
  resources: Resource[];
  activeDownloads: Record<number, ActiveDownload>;
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
  startDownload: (resource: Resource) => Promise<void>;
  pauseDownload: (resourceId: number) => Promise<void>;
  resumeDownload: (resource: Resource) => Promise<void>;
  cancelDownload: (resourceId: number) => Promise<void>;
}

export const useAppStore = create<AppState>(
    (set, get) => ({
      config: null,
      status: null,
      resources: [],
      archivedWeeks: [],
      isLoading: true,
      error: null,
      activeDownloads: {},

      fetchInitialData: async () => {
        try {
          set({isLoading: true, error: null});

          const [config, status, resources] = await Promise.all([
            invoke<AppConfig>('get_config'),
            invoke<AppStatus>('get_status'),
            invoke<Resource[]>('get_resources'),
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

          set({config, status, resources, archivedWeeks, isLoading: false});

          // Listen for updates
          await listen<ResourceListResponse>('resources-updated', (event) => {
            set({resources: event.payload.resources});
            // Also refresh status to update last poll time
            invoke<AppStatus>('get_status').then(status => set({status}));
          });

          await listen<string>('poll-error', (event) => {
            set({error: `Poll error: ${event.payload}`});
          });

          await listen('poll-tick', () => {
            invoke<AppStatus>('get_status').then(status => set({status}));
          });

          // Global download progress listener
          await listen<{id: number, progress: number}>(
              'download-progress', (event) => {
                set(state => {
                  const current = state.activeDownloads[event.payload.id];
                  // If paused, don't update progress (though backend shouldn't
                  // emit)
                  if (!current || current.status !== 'downloading')
                    return state;

                  return {
                    activeDownloads: {
                      ...state.activeDownloads,
                      [event.payload.id]:
                          {...current, progress: event.payload.progress}
                    }
                  };
                });
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

      startDownload: async (resource: Resource) => {
        const {activeDownloads} = get();
        // If already downloading, do nothing
        if (activeDownloads[resource.id]?.status === 'downloading') return;

        // Determine initial progress (preserve if resuming)
        const initialProgress = activeDownloads[resource.id]?.progress || 0;

        set(state => ({
              activeDownloads: {
                ...state.activeDownloads,
                [resource.id]: {
                  progress: initialProgress,
                  status: 'downloading',
                  error: undefined
                }
              }
            }));

        try {
          const result = await invoke<{path: string, hash: string}>(
              'download_resource', {resource});

          let integrity: ActiveDownload['integrity'] = 'unknown';
          if (resource.checksum && resource.checksum.trim() !== '') {
            // Simple check: backend hash is hex string. Check equality.
            // Assuming case-insensitive comparison might be safer
            integrity =
                result.hash.toLowerCase() === resource.checksum.toLowerCase() ?
                'verified' :
                'mismatch';
          } else {
            // If return hash is "youtube-shortcut", implicit success/verified
            if (result.hash === 'youtube-shortcut') {
              integrity = 'verified';
            }
          }

          set(state => ({
                activeDownloads: {
                  ...state.activeDownloads,
                  [resource.id]: {
                    progress: 100,
                    status: 'completed',
                    integrity,
                    path: result.path,
                    hash: result.hash
                  }
                }
              }));
        } catch (error: any) {
          const errorMessage = typeof error === 'string' ?
              error :
              error.message || 'Download failed';

          // Check if error message contains "cancelled"
          const isCancelled = errorMessage.toLowerCase().includes('cancelled');

          if (isCancelled) {
            // This branch entered if backend returns error Cancelled.
            // We need to check if user intended pause or cancel.
            // Usually pauseDownload() updates state before backend returns.
            // So if state is PAUSED, keep it.
            // If state is CANCELLING (removed), this callback might fire?
            // If removed, activeDownloads[id] is undefined.

            const current = get().activeDownloads[resource.id];
            if (current && current.status === 'paused') {
              // Remain paused
              return;
            }
            if (!current) {
              // It was removed (cancelled), do nothing
              return;
            }
          }

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
        try {
          await invoke('pause_download', {resourceId});
        } catch (e) {
          console.error('Failed to pause', e);
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
        try {
          await invoke('cancel_download', {resourceId});
        } catch (e) {
          console.error('Failed to cancel', e);
        }
      }

    }));
