import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {UnlistenFn} from '@tauri-apps/api/event';
import {create} from 'zustand';

import {useCelebrationStore} from './celebrationStore';
import {useToastStore} from './toastStore';
import {errorMessage} from '../lib/utils';
import {tGlobal} from '../lib/i18n';
import {AppConfig, AppStatus, CategoryCount, DownloadCompletePayload, ErrataDetectedPayload, Resource, ResourceListResponse, ResourceStatus, SavingsResolvedPayload, SavingsStats, WeekIdentifier} from '../types';

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
  // Batched per-resource status (downloaded flag + cached sizes), keyed by
  // resource id. Populated by fetchResourcesStatus; consumed by useResource.
  resourceStatuses: Record<number, ResourceStatus>;
  // Full category catalog from the backend `categories/counts` fetch, kept
  // separate from `resources` so Settings can offer categories that aren't in
  // the current week. Loaded on init and refreshed by `categories-updated`.
  allCategories: CategoryCount[];
  archivedWeeks: WeekIdentifier[];
  isLoading: boolean;
  error: string|null;

  // Actions
  fetchInitialData: () => Promise<void>;
  // Registers the process-global Tauri event listeners exactly once for the
  // app's lifetime. Kept separate from fetchInitialData (which fetches data and
  // may run on every page mount) so navigation never re-registers listeners —
  // duplicated listeners made every event fire N times and inflated the
  // session savings counter. Call once from App.tsx.
  initEventListeners: () => Promise<void>;
  fetchResourcesStatus: () => Promise<void>;
  patchResourceStatus: (id: number, patch: Partial<ResourceStatus>) => void;
  updateConfig: (config: Partial<AppConfig>) => Promise<void>;
  forcePoll: () => Promise<void>;
  selectWorkDirectory: () => Promise<void>;
  togglePolling: (enabled: boolean) => Promise<void>;
  setPollingInterval: (minutes: number) => Promise<void>;
  setRetentionDays: (days: number|null) => Promise<void>;
  setAutostartEnabled: (enabled: boolean) => Promise<void>;
  fetchArchivedWeeks: () => Promise<void>;
  fetchSummary: () => Promise<void>;
  startDownload: (resource: Resource) => Promise<void>;
  pauseDownload: (resourceId: number) => Promise<void>;
  resumeDownload: (resource: Resource) => Promise<void>;
  cancelDownload: (resourceId: number) => Promise<void>;
}

// Config fields whose change can alter the derived downloaded/size state
// returned by get_resources_status (see updateConfig below).
const STATUS_AFFECTING_CONFIG_FIELDS =
    new Set<keyof AppConfig>(
        ['prefer_optimized', 'work_directory', 'auto_download_categories']);

// Simple debounce helper
function debounce<T extends (...args: never[]) => unknown>(
  func: T,
  wait: number
): (...args: Parameters<T>) => void {
  let timeout: ReturnType<typeof setTimeout> | null = null;
  return (...args: Parameters<T>) => {
    if (timeout) clearTimeout(timeout);
    timeout = setTimeout(() => func(...args), wait);
  };
}

// Tauri event listeners are process-global singletons registered against the
// webview; registering a set more than once makes every event run its handler
// once per copy. That is the root cause of the inflated "session saved" figure:
// each duplicate download-complete re-ran the incremental accumulator. These
// module-level guards ensure the set is registered exactly once for the app's
// lifetime, independent of how many times React mounts/remounts a page.
let eventListenersInitialized = false;
let registeredUnlisten: UnlistenFn[] = [];

// On a Vite hot-reload this module is replaced: tear down the old listeners and
// reset the guard so the fresh module re-registers exactly one set (without the
// stale copies from the previous module instance lingering on the webview).
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    registeredUnlisten.forEach((unlisten) => unlisten());
    registeredUnlisten = [];
    eventListenersInitialized = false;
  });
}

export const useAppStore = create<AppState>(
    (set, get) => {
      // Create debounced versions for event listeners
      const debouncedFetchSummary = debounce(() => {
        get().fetchSummary();
      }, 300);
      const debouncedFetchStatuses = debounce(() => {
        get().fetchResourcesStatus();
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
      resourceStatuses: {},
      allCategories: [],

      fetchInitialData: async () => {
        try {
          set({isLoading: true, error: null});

          const
              [config, status, resources, summary, resourceStatuses,
               allCategories, savingsStats] =
                  await Promise.all([
                    invoke<AppConfig>('get_config'),
                    invoke<AppStatus>('get_status'),
                    invoke<Resource[]>('get_resources'),
                    invoke<ResourceSummary>('get_resource_summary'),
                    invoke<Record<number, ResourceStatus>>(
                        'get_resources_status'),
                    invoke<CategoryCount[]>('get_all_categories'),
                    invoke<SavingsStats>('get_savings_stats'),
                  ]);

          // Seed the persistent cross-session savings total once at startup;
          // every subsequent download-complete event keeps it in sync via its
          // own total_saved_bytes field (no re-fetch needed).
          useCelebrationStore.getState().setTotalSavedBytes(
              savingsStats.total_saved_bytes);

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
            resourceStatuses,
            allCategories,
            archivedWeeks,
            isLoading: false
          });
        } catch (e) {
          set({
            error: tGlobal('store.error.initFailed', {error: errorMessage(e)}),
            isLoading: false
          });
        }
      },

      initEventListeners: async () => {
        // Idempotent for the app's lifetime. The guard is flipped synchronously
        // *before* the first await so React StrictMode's double-invoke (dev)
        // can't slip a second registration through the await gap. Called once
        // from App.tsx — never from a page mount — so navigating between pages
        // never re-registers, and every event runs its handler exactly once.
        if (eventListenersInitialized) return;
        eventListenersInitialized = true;
        // Promise.allSettled (not Promise.all): a single failed `listen()`
        // call must not discard the UnlistenFns of the ones that *did*
        // succeed. With Promise.all, one rejection loses every fulfilled
        // handle silently, leaking those listeners registered against the
        // webview for the app's lifetime (no way to tear them down later).
        try {
          const results = await Promise.allSettled([
          // Listen for updates
          listen<ResourceListResponse>('resources-updated', (event) => {
            set({resources: event.payload.resources});
            // Also refresh status to update last poll time
            invoke<AppStatus>('get_status').then(status => set({status}));
            debouncedFetchSummary();
            debouncedFetchStatuses();
          }),

          listen<string>('poll-error', (event) => {
            set({error: `Poll error: ${event.payload}`});
          }),

          listen('poll-tick', () => {
            invoke<AppStatus>('get_status').then(status => set({status}));
          }),

          // Full category catalog refreshed by the backend after each poll.
          listen<CategoryCount[]>('categories-updated', (event) => {
            set({allCategories: event.payload});
          }),

          // Global download progress listener
          listen<{
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
          }),

          // Listen for queue status changes
          listen<QueueStatusPayload>('queue-status-changed', (event) => {
            set(state => {
              const {queued, active: _active} = event.payload;
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
          }),

          // Listen for download start from queue
          listen<number>('download-started', (event) => {
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
          }),

          // Listen for download completion from auto-download queue
          listen<DownloadCompletePayload>('download-complete', (event) => {
            const {
              id: resourceId,
              optimized,
              optimized_bytes: optimizedBytes,
              original_bytes: originalBytes,
              saved_bytes: savedBytes,
              total_saved_bytes: totalSavedBytes,
            } = event.payload;
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
            debouncedFetchStatuses();

            // Intervento B: celebrate optimized completions (manual AND
            // auto-download — the latter is the main "value delivered by
            // Rinoova" case and never reaches activeDownloads above). The
            // payload is now the authoritative, backend-computed source for
            // the sizes/savings (a best-effort HEAD on the backend side), so
            // no frontend refetch/cache lookup is needed and the celebration
            // never goes out with missing data just because the frontend
            // hadn't cached the sizes yet (collaudo bug).
            if (optimized) {
              const title = get().resources.find(r => r.id === resourceId)
                                ?.title ??
                  tGlobal('celebration.fallbackTitle');
              useCelebrationStore.getState().addCelebration({
                resourceId,
                title,
                originalBytes,
                optimizedBytes,
                savedBytes,
                totalSavedBytes,
              });
            }
          }),

          // Backend resolved an original file size that wasn't cached yet at
          // download-complete time (see queue.rs's detached background
          // task): upgrade the matching celebration panel from its generic
          // "no savings info" copy to the full savings layout, and fold the
          // saving into the session/total counters (counted exactly once
          // across download-complete + savings-resolved, never both).
          listen<SavingsResolvedPayload>('savings-resolved', (event) => {
            useCelebrationStore.getState().resolveSavings(event.payload);
          }),

          // Listen for download failures
          listen<{id: number, error: string}>('download-failed', (event) => {
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
          }),

          // Listen for download pause (emitted by the queue worker when a
          // download stops on the pause signal). Only reflect it if we track
          // the resource; a pause event for an unknown download is ignored.
          listen<number>('download-paused', (event) => {
            const resourceId = event.payload;
            set(state => {
              const current = state.activeDownloads[resourceId];
              if (!current) return state;
              return {
                activeDownloads: {
                  ...state.activeDownloads,
                  [resourceId]: {...current, status: 'paused'}
                }
              };
            });
          }),

          // Listen for download cancellation (an in-flight download stopped on
          // the cancel signal; removing a still-queued item does not emit this).
          // Drop it from the map.
          listen<number>('download-cancelled', (event) => {
            const resourceId = event.payload;
            set(state => {
              const {[resourceId]: _removed, ...rest} = state.activeDownloads;
              return {activeDownloads: rest};
            });
            debouncedFetchSummary();
            debouncedFetchStatuses();
          }),

          // Errata corrige detected: the backend has already archived the old
          // file, marked the registry, and re-queued the updated download
          // (see errata.rs::process_errata). The UI stays dumb — just refresh
          // status/resources from the backend and surface a non-invasive
          // toast; no client-side comparison logic.
          listen<ErrataDetectedPayload>('errata-detected', (event) => {
            invoke<AppStatus>('get_status').then(status => set({status}));
            invoke<Resource[]>('get_resources').then(resources => set({resources}));
            debouncedFetchSummary();
            debouncedFetchStatuses();

            const count = event.payload.resourceIds.length;
            useToastStore.getState().addToast(
                count === 1 ?
                    tGlobal('store.toast.errataSingle') :
                    tGlobal('store.toast.errataMultiple', {count}),
                'info');
          }),
          ]);

          const unlisten: UnlistenFn[] = [];
          results.forEach((result, index) => {
            if (result.status === 'fulfilled') {
              unlisten.push(result.value);
            } else {
              // Log and move on: the guard stays up (see below) so the
              // listeners that DID succeed are never re-registered — a retry
              // here would double-fire every event they already own.
              console.error(
                  `Failed to register event listener #${index}`,
                  result.reason);
            }
          });
          registeredUnlisten = unlisten;
          console.log(
              `[EventListeners] ${unlisten.length}/${
                  results.length} listeners active`);
        } catch (e) {
          // Unexpected failure building/awaiting the batch itself — distinct
          // from a per-listener rejection, which Promise.allSettled already
          // absorbs above without throwing. No listener is known to have
          // registered here, so it's safe to reset the guard and let a later
          // call retry the whole batch.
          eventListenersInitialized = false;
          registeredUnlisten = [];
          console.error('Failed to register event listeners', e);
        }
      },

      fetchResourcesStatus: async () => {
        try {
          const resourceStatuses =
              await invoke<Record<number, ResourceStatus>>(
                  'get_resources_status');
          set({resourceStatuses});
        } catch (e) {
          console.error('Failed to fetch resource statuses', e);
        }
      },

      // Reconcile a single batched entry in place (mount-time deletion check
      // and the file-missing reveal path in useResource): a full refetch
      // would re-stat every resource on disk just to correct one entry.
      patchResourceStatus: (id, patch) => {
        set(state => {
          const current = state.resourceStatuses[id] ??
              {downloaded: false, file_size: null, optimized_file_size: null};
          return {
            resourceStatuses:
                {...state.resourceStatuses, [id]: {...current, ...patch}}
          };
        });
      },

      updateConfig: async (updates) => {
        const currentConfig = get().config;
        if (!currentConfig) return;

        const newConfig = {...currentConfig, ...updates};
        try {
          await invoke('set_config', {config: newConfig});
          set({config: newConfig});
          // prefer_optimized / work_directory / auto-download changes alter
          // derived downloaded/size state, so refresh the batched statuses;
          // other fields (e.g. theme) don't affect it, skip the refetch.
          const updatedFields = Object.keys(updates);
          const affectsStatuses = updatedFields.some(
              field => STATUS_AFFECTING_CONFIG_FIELDS.has(
                  field as keyof AppConfig));
          if (affectsStatuses) debouncedFetchStatuses();
        } catch (e) {
          set({
            error: tGlobal(
                'store.error.configUpdateFailed', {error: errorMessage(e)})
          });
          throw e;
        }
      },

      forcePoll: async () => {
        try {
          set({isLoading: true});
          const response = await invoke<ResourceListResponse>('force_poll');
          const status = await invoke<AppStatus>('get_status');
          set({resources: response.resources, status, isLoading: false});
          debouncedFetchStatuses();
        } catch (e) {
          set({
            error: tGlobal('store.error.pollFailed', {error: errorMessage(e)}),
            isLoading: false
          });
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
          set({
            error: tGlobal(
                'store.error.selectDirFailed', {error: errorMessage(e)})
          });
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
          set({
            error: tGlobal(
                'store.error.togglePollingFailed', {error: errorMessage(e)})
          });
        }
      },

      setPollingInterval: async (minutes) => {
        try {
          await invoke('set_polling_interval', {minutes});
          const config = await invoke<AppConfig>('get_config');
          set({config});
        } catch (e) {
          set({
            error: tGlobal(
                'store.error.setIntervalFailed', {error: errorMessage(e)})
          });
        }
      },

      setRetentionDays: async (days) => {
        try {
          await invoke('set_retention_days', {days});
          const config = await invoke<AppConfig>('get_config');
          set({config});
        } catch (e) {
          set({
            error: tGlobal(
                'store.error.setRetentionFailed', {error: errorMessage(e)})
          });
        }
      },

      setAutostartEnabled: async (enabled) => {
        try {
          // Toggles the real OS-level autostart entry and persists the
          // preference (backend command set_autostart_enabled); re-throw so
          // the caller can show an accurate success/error toast instead of
          // assuming success (this changes OS state, not just app config).
          await invoke('set_autostart_enabled', {enabled});
          const config = await invoke<AppConfig>('get_config');
          set({config});
        } catch (e) {
          set({
            error: tGlobal(
                'store.error.setAutostartFailed', {error: errorMessage(e)})
          });
          throw e;
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
        } catch (error: unknown) {
          const message = errorMessage(error) || 'Download failed';

          set(state => ({
                activeDownloads: {
                  ...state.activeDownloads,
                  [resource.id]:
                      {progress: 0, status: 'error', error: message}
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
