import {invoke} from '@tauri-apps/api/core';
import {useEffect, useMemo, useState} from 'react';

import {useAppStore} from '../stores/appStore';
import {useToastStore} from '../stores/toastStore';
import {errorMessage} from '../lib/utils';
import {OptimizedVideo, Resource} from '../types';

export function useResource(resource: Resource) {
  // Config and batched status come from the global store: no per-card IPC.
  const config = useAppStore(state => state.config);
  const updateConfig = useAppStore(state => state.updateConfig);
  const statusEntry =
      useAppStore(state => state.resourceStatuses[resource.id]);

  const isAutoDownloadEnabled =
      config?.auto_download_categories.includes(resource.category) ?? false;
  const preferOptimized = config?.prefer_optimized ?? true;

  // adr-0008: when a resource offers multiple optimized video variants
  // (e.g. several clips re-encoded from the same zip), the user must be
  // able to pick which one to download instead of silently getting the
  // producer's compat-default (the first/largest element). Selection state
  // lives here (the hook), never in the presentational picker component.
  const optimizedVideos: OptimizedVideo[] = resource.optimized_videos ?? [];
  const [selectedVideoUrl, setSelectedVideoUrl] = useState<string|null>(
      optimizedVideos[0]?.url ?? null);

  // Reset the selection to the compat-default (first element) whenever the
  // resource identity/choices change, so switching between resources (or a
  // resource list refresh) never leaves a stale selection from a previous
  // resource applied to a new one.
  useEffect(
      () => {
        setSelectedVideoUrl(resource.optimized_videos?.[0]?.url ?? null);
      },
      [resource]);

  // Only meaningful when the user actually wants optimized videos at all
  // (prefer_optimized) AND there is more than one candidate to choose from;
  // with 0 or 1 elements there is nothing to pick, so behavior must stay
  // identical to the pre-existing single-URL logic below.
  const hasVideoChoice = preferOptimized && optimizedVideos.length > 1;

  // The Resource used for status checks / downloads: identical to the
  // prop unless the user picked a non-default variant, in which case
  // optimized_video_url is overridden to the selected one. This reuses the
  // EXISTING download_resource command and get_effective_download_url logic
  // unchanged (adr-0007: no new download path) — it just feeds them a
  // different URL for this one call.
  const effectiveResource: Resource = useMemo(
      () => hasVideoChoice && selectedVideoUrl ?
          {...resource, optimized_video_url: selectedVideoUrl} :
          resource,
      [resource, hasVideoChoice, selectedVideoUrl]);

  // Get download state from global store
  const activeDownloads = useAppStore(state => state.activeDownloads);
  const startDownload = useAppStore(state => state.startDownload);
  const pauseDownloadAction = useAppStore(state => state.pauseDownload);
  const resumeDownloadAction = useAppStore(state => state.resumeDownload);
  const cancelDownloadAction = useAppStore(state => state.cancelDownload);

  const downloadState = activeDownloads[resource.id];
  const isDownloading = downloadState?.status === 'downloading';
  const isPaused = downloadState?.status === 'paused';
  const isPending = downloadState?.status === 'pending';
  const queuePosition = downloadState?.queuePosition ?? null;
  const progress = downloadState?.progress ?? null;
  const error = downloadState?.error ?? null;
  const integrity = downloadState?.integrity;

  // The batched status is computed for the compat-default URL. When the user
  // picks a non-default variant, that batched flag can be wrong, so (and only
  // then) fall back to a targeted check for the exact selected variant.
  const isNonDefaultVariant = hasVideoChoice && !!selectedVideoUrl &&
      selectedVideoUrl !== resource.optimized_video_url;
  const [variantDownloaded, setVariantDownloaded] =
      useState<boolean|null>(null);

  useEffect(
      () => {
        if (!isNonDefaultVariant) {
          setVariantDownloaded(null);
          return;
        }
        let cancelled = false;
        invoke<boolean>('check_resource_status', {resource: effectiveResource})
            .then(status => {
              if (!cancelled) setVariantDownloaded(status);
            })
            .catch(err => {
              console.error('Failed to check resource status:', err);
            });
        return () => {
          cancelled = true;
        };
      },
      [isNonDefaultVariant, effectiveResource]);

  const isDownloaded = isNonDefaultVariant ?
      (variantDownloaded ?? false) :
      (statusEntry?.downloaded ?? false);

  const download = async () => {
    // Determine explicitly if we can download.
    // Allow retrying if error or mismatch.
    if ((isDownloading || isPaused) && !error && integrity !== 'mismatch')
      return;

    // If mismatch or error, we might be retrying (which overwrites).
    // If paused, we should call resume instead, but download() handles
    // start/resume too if we map it properly. However, store.startDownload
    // handles resumption if partial file exists.
    await startDownload(effectiveResource);
  };

  const pause = async () => {
    if (!isDownloading) return;
    await pauseDownloadAction(resource.id);
  };

  const resume = async () => {
    if (!isPaused) return;
    // Resume the same variant that was originally selected/started (not the
    // compat-default), so the .part file (named after the URL) matches.
    await resumeDownloadAction(effectiveResource);
  };

  /** Dumb-component callback: records which optimized video the user picked
   *  when a resource offers more than one (adr-0008). Never invokes IPC
   *  directly — the actual download still goes through download()/the
   *  queue, unchanged. */
  const selectVideo = (url: string) => {
    setSelectedVideoUrl(url);
  };

  const cancel = async () => {
    await cancelDownloadAction(resource.id);
  };

  const {addToast} = useToastStore();

  const revealInFolder = async () => {
    try {
      await invoke('reveal_resource', {resource: effectiveResource});
    } catch (error) {
      addToast(`Impossibile aprire la cartella: ${errorMessage(error)}`, 'error');
    }
  };

  const toggleAutoDownload = async () => {
    if (!config) return;
    const wasEnabled = isAutoDownloadEnabled;
    const newCategories = wasEnabled ?
        config.auto_download_categories.filter(c => c !== resource.category) :
        [...config.auto_download_categories, resource.category];

    try {
      await updateConfig({auto_download_categories: newCategories});
      addToast(
          `Auto-download ${!wasEnabled ? 'enabled' : 'disabled'} for "${
              resource.category}"`,
          'success');
    } catch (error) {
      addToast(`Failed to toggle auto-download: ${errorMessage(error)}`, 'error');
    }
  };

  return {
    isDownloaded,
    isDownloading,
    isPaused,
    isPending,
    queuePosition,
    isAutoDownloadEnabled,
    error,
    progress,
    integrity,
    download,
    pause,
    resume,
    cancel,
    revealInFolder,
    toggleAutoDownload,
    preferOptimized,
    optimizedVideos,
    selectedVideoUrl,
    selectVideo,
    resource
  };
}
