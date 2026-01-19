import {invoke} from '@tauri-apps/api/core';
import {useEffect, useState} from 'react';

import {AppConfig, Resource} from '../types';

export function useResource(resource: Resource) {
  const [isDownloaded, setIsDownloaded] = useState<boolean>(false);
  const [isDownloading, setIsDownloading] = useState<boolean>(false);
  const [isAutoDownloadEnabled, setIsAutoDownloadEnabled] =
      useState<boolean>(false);

  // Initial check for status and config
  useEffect(() => {
    checkStatus();
    checkAutoDownload();
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

  const download = async () => {
    if (isDownloading || isDownloaded) return;

    setIsDownloading(true);
    try {
      await invoke('download_resource', {resource});
      setIsDownloaded(true);
      await checkStatus();  // Verify
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
    download,
    toggleAutoDownload
  };
}
