import {invoke} from '@tauri-apps/api/core';
import {useEffect, useMemo, useState} from 'react';

import {formatBytes} from '../lib/utils';
import {useAppStore} from '../stores/appStore';
import {useToastStore} from '../stores/toastStore';
import {AppConfig, OptimizedVideo, Resource} from '../types';

export function useResource(resource: Resource) {
  const [isDownloaded, setIsDownloaded] = useState<boolean>(false);
  const [isAutoDownloadEnabled, setIsAutoDownloadEnabled] =
      useState<boolean>(false);
  const [fileSize, setFileSize] = useState<string|null>(null);
  const [originalSizeBytes, setOriginalSizeBytes] = useState<number|null>(null);
  const [optimizedSizeBytes, setOptimizedSizeBytes] = useState<number|null>(null);
  const [preferOptimized, setPreferOptimized] = useState<boolean>(true);

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
  const progress = downloadState?.progress ?? null;
  const error = downloadState?.error ?? null;
  const integrity = downloadState?.integrity;

  // Initial check for status and config
  useEffect(
      () => {
        checkAutoDownload();
      },
      [
        resource
      ]);  // Re-check when resource changes

  // Separate effect for the "is it downloaded?" check and the file-size
  // fetch: both re-run when the user picks a different optimized video
  // (effectiveResource changes), so the downloaded/not-downloaded indicator
  // and the displayed size track the variant that was actually selected
  // instead of always the compat-default.
  useEffect(
      () => {
        checkStatus();
        fetchFileSize();
      },
      [
        effectiveResource
      ]);

  // Update displayed file size when preference changes
  useEffect(
      () => {
        // Determine which size to show based on preference and availability
        let sizeToShow: number | null = null;
        
        if (preferOptimized && optimizedSizeBytes) {
          sizeToShow = optimizedSizeBytes;
        } else if (originalSizeBytes) {
          sizeToShow = originalSizeBytes;
        }

        if (sizeToShow) {
          setFileSize(formatBytes(sizeToShow));
        } else {
          setFileSize(null);
        }
      },
      [
        preferOptimized, originalSizeBytes, optimizedSizeBytes
      ]);

  const checkStatus = async () => {
    try {
      const status = await invoke<boolean>(
          'check_resource_status', {resource: effectiveResource});
      setIsDownloaded(status);
    } catch (error) {
      console.error('Failed to check resource status:', error);
    }
  };

  const checkAutoDownload = async () => {
    try {
      const config = await invoke<AppConfig>('get_config');
      setIsAutoDownloadEnabled(
          config.auto_download_categories.includes(resource.category));
      setPreferOptimized(config.prefer_optimized);
    } catch (error) {
      console.error('Failed to check auto-download config:', error);
    }
  };

  const fetchFileSize = async () => {
    // Simple check for YouTube URLs
    const isYoutube = resource.download_url.includes('youtube.com') ||
        resource.download_url.includes('youtu.be');
    if (isYoutube) {
      setFileSize(null);
      setOriginalSizeBytes(null);
      setOptimizedSizeBytes(null);
      return;
    }

    try {
      // Fetch both sizes in parallel for better performance
      const [originalSize, optimizedSize] = await Promise.all([
        invoke<number>('get_file_size', { url: resource.download_url }),
        effectiveResource.optimized_video_url
          ? invoke<number>('get_file_size', { url: effectiveResource.optimized_video_url })
          : Promise.resolve(0)
      ]);

      setOriginalSizeBytes(originalSize > 0 ? originalSize : null);
      setOptimizedSizeBytes(optimizedSize > 0 ? optimizedSize : null);
    } catch (error) {
      console.error('Failed to fetch file size:', error);
      setOriginalSizeBytes(null);
      setOptimizedSizeBytes(null);
    }
  };

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
    // Re-check status to verify deletion?
    // checkStatus();
    // Actually cancelDownload updates state immediately.
  };

  const {addToast} = useToastStore();

  const toggleAutoDownload = async () => {
    try {
      const config = await invoke<AppConfig>('get_config');
      let newCategories = [...config.auto_download_categories];

      const checkEnabled =
          isAutoDownloadEnabled;  // Capture current state before toggle

      if (checkEnabled) {
        newCategories = newCategories.filter(c => c !== resource.category);
      } else {
        if (!newCategories.includes(resource.category)) {
          newCategories.push(resource.category);
        }
      }

      const newConfig = {...config, auto_download_categories: newCategories};
      await invoke('set_config', {config: newConfig});

      const newState = !checkEnabled;
      setIsAutoDownloadEnabled(newState);

      addToast(
          `Auto-download ${newState ? 'enabled' : 'disabled'} for "${
              resource.category}"`,
          'success');
    } catch (error) {
      console.error('Failed to toggle auto-download:', error);
      addToast(`Failed to toggle auto-download: ${error}`, 'error');
    }
  };

  return {
    isDownloaded,
    isDownloading,
    isPaused,
    isAutoDownloadEnabled,
    fileSize,
    originalSizeBytes,
    optimizedSizeBytes,
    error,
    progress,
    integrity,
    download,
    pause,
    resume,
    cancel,
    toggleAutoDownload,
    preferOptimized,
    optimizedVideos,
    selectedVideoUrl,
    selectVideo,
    resource
  };
}
