import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {useEffect, useState} from 'react';

import {AppConfig, Resource} from '../types';

export function useResource(resource: Resource) {
  const [isDownloaded, setIsDownloaded] = useState<boolean>(false);
  const [isDownloading, setIsDownloading] = useState<boolean>(false);
  const [isAutoDownloadEnabled, setIsAutoDownloadEnabled] =
      useState<boolean>(false);

  const [fileSize, setFileSize] = useState<string|null>(null);

  const [error, setError] = useState<string|null>(null);

  const [progress, setProgress] = useState<number|null>(null);

  // Initial check for status and config
  useEffect(() => {
    checkStatus();
    checkAutoDownload();
    fetchFileSize();
  }, [resource]);

  // ... (checkStatus, checkAutoDownload, fetchFileSize remain same - omitted
  // for brevity if possible, but replace_file_content needs contiguity or full
  // replacement if chunks are complex. I'll replace the full body for safety or
  // target carefully) To avoid huge replacement, I will replace the state defs
  // and the download function separately if possible, or just the whole top and
  // bottom. Actually, I'll do a large chunk replacement to be safe.

  const checkStatus = async () => {
    try {
      const status = await invoke<boolean>('check_resource_status', {resource});
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
    } catch (error) {
      console.error('Failed to check auto-download config:', error);
    }
  };

  const fetchFileSize = async () => {
    // Simple check for YouTube URLs
    const isYoutube = resource.download_url.includes('youtube.com') ||
        resource.download_url.includes('youtu.be');
    if (isYoutube) return;  // Don't fetch size for YouTube links
    try {
      const sizeBytes =
          await invoke<number>('get_file_size', {url: resource.download_url});

      // Format bytes to human readable string
      const units = ['B', 'KB', 'MB', 'GB'];
      let size = sizeBytes;
      let unitIndex = 0;
      while (size >= 1024 && unitIndex < units.length - 1) {
        size /= 1024;
        unitIndex++;
      }
      setFileSize(`${size.toFixed(1)} ${units[unitIndex]}`);
    } catch (error) {
      console.error('Failed to fetch file size:', error);
      setFileSize(null);
    }
  };

  const download = async () => {
    if (isDownloading || isDownloaded) return;

    setIsDownloading(true);
    setError(null);
    setProgress(0);

    let unlisten: (() => void)|undefined;

    try {
      // Set up listener before starting download
      unlisten = await listen<{id: number, progress: number}>(
          'download-progress', (event) => {
            if (event.payload.id === resource.id) {
              setProgress(event.payload.progress);
            }
          });

      await invoke('download_resource', {resource});

      setIsDownloaded(true);
      await checkStatus();

    } catch (error: any) {
      console.error('Download failed:', error);
      setError(
          typeof error === 'string' ? error :
                                      error.message || 'Download failed');
    } finally {
      setIsDownloading(false);
      setProgress(null);
      if (unlisten) unlisten();
    }
  };

  const toggleAutoDownload = async () => {
    try {
      const config = await invoke<AppConfig>('get_config');
      let newCategories = [...config.auto_download_categories];

      if (isAutoDownloadEnabled) {
        newCategories = newCategories.filter(c => c !== resource.category);
      } else {
        if (!newCategories.includes(resource.category)) {
          newCategories.push(resource.category);
        }
      }

      const newConfig = {...config, auto_download_categories: newCategories};
      await invoke('set_config', {config: newConfig});
      setIsAutoDownloadEnabled(!isAutoDownloadEnabled);
    } catch (error) {
      console.error('Failed to toggle auto-download:', error);
    }
  };

  return {
    isDownloaded,
    isDownloading,
    isAutoDownloadEnabled,
    fileSize,
    error,
    progress,
    download,
    toggleAutoDownload
  };
}
