import {invoke} from '@tauri-apps/api/core';
import {useEffect, useState} from 'react';

import {AppConfig, Resource} from '../types';

export function useResource(resource: Resource) {
  const [isDownloaded, setIsDownloaded] = useState<boolean>(false);
  const [isDownloading, setIsDownloading] = useState<boolean>(false);
  const [isAutoDownloadEnabled, setIsAutoDownloadEnabled] =
      useState<boolean>(false);

  const [fileSize, setFileSize] = useState<string|null>(null);

  // Initial check for status and config
  useEffect(() => {
    checkStatus();
    checkAutoDownload();
    fetchFileSize();
  }, [resource]);

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
    try {
      await invoke('download_resource', {resource});

      // Listen for the download-complete event or just poll/check locally?
      // Since the command is async and returns path, we can assume success if
      // no error
      setIsDownloaded(true);
      await checkStatus();  // Verify

      // Also emit a global event so other components know (like the card vs
      // modal) For now, simpler: we rely on individual components checking
      // status or re-mounting

    } catch (error) {
      console.error('Download failed:', error);
    } finally {
      setIsDownloading(false);
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
    download,
    toggleAutoDownload
  };
}
