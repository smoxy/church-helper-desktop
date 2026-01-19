import {invoke} from '@tauri-apps/api/core';
import {useEffect, useState} from 'react';

import {useAppStore} from '../stores/appStore';
import {AppConfig, Resource} from '../types';

export function useResource(resource: Resource) {
  const [isDownloaded, setIsDownloaded] = useState<boolean>(false);
  const [isAutoDownloadEnabled, setIsAutoDownloadEnabled] =
      useState<boolean>(false);
  const [fileSize, setFileSize] = useState<string|null>(null);

  // Get download state from global store
  const activeDownloads = useAppStore(state => state.activeDownloads);
  const startDownload = useAppStore(state => state.startDownload);

  const downloadState = activeDownloads[resource.id];
  const isDownloading = downloadState?.status === 'downloading';
  const progress = downloadState?.progress ?? null;
  const error = downloadState?.error ?? null;

  // Initial check for status and config
  useEffect(
      () => {
        checkStatus();
        checkAutoDownload();
        fetchFileSize();
      },
      [
        resource, downloadState?.status
      ]);  // Re-check when download status completes

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
    await startDownload(resource);
    // Determine success via effect dependency on downloadState.status
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
