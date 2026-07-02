import {invoke} from '@tauri-apps/api/core';
import {useEffect, useState} from 'react';

import {useAppStore} from '../stores/appStore';
import {Resource} from '../types';

/**
 * Resolves the original and effective-optimized byte sizes for a resource,
 * used only by ResourceDetail. It seeds from the batched sizes already in the
 * store (get_resources_status) and only issues a HEAD request (get_file_size)
 * when a needed size is missing there, or when the user picked a non-default
 * optimized variant whose size was never batched. YouTube resources report no
 * size.
 */
export function useResourceFileSize(
    resource: Resource, selectedVideoUrl: string|null) {
  const statusEntry =
      useAppStore(state => state.resourceStatuses[resource.id]);

  const isYoutube = resource.download_url.includes('youtube.com') ||
      resource.download_url.includes('youtu.be');

  // Effective optimized URL: the user's pick if any, else the compat-default.
  const optimizedUrl =
      selectedVideoUrl ?? resource.optimized_video_url ?? null;
  // The batched optimized size only describes the compat-default variant, so
  // it is reusable only when no non-default variant is selected.
  const isDefaultVariant =
      !selectedVideoUrl || selectedVideoUrl === resource.optimized_video_url;

  const batchedOriginal = statusEntry?.file_size ?? null;
  const batchedOptimized =
      isDefaultVariant ? (statusEntry?.optimized_file_size ?? null) : null;

  const [originalSizeBytes, setOriginalSizeBytes] =
      useState<number|null>(batchedOriginal);
  const [optimizedSizeBytes, setOptimizedSizeBytes] =
      useState<number|null>(batchedOptimized);

  useEffect(() => {
    if (isYoutube) {
      setOriginalSizeBytes(null);
      setOptimizedSizeBytes(null);
      return;
    }

    let cancelled = false;

    if (batchedOriginal !== null) {
      setOriginalSizeBytes(batchedOriginal);
    } else {
      invoke<number>('get_file_size', {url: resource.download_url})
          .then(size => {
            if (!cancelled) setOriginalSizeBytes(size > 0 ? size : null);
          })
          .catch(() => {
            if (!cancelled) setOriginalSizeBytes(null);
          });
    }

    if (!optimizedUrl) {
      setOptimizedSizeBytes(null);
    } else if (batchedOptimized !== null) {
      setOptimizedSizeBytes(batchedOptimized);
    } else {
      invoke<number>('get_file_size', {url: optimizedUrl})
          .then(size => {
            if (!cancelled) setOptimizedSizeBytes(size > 0 ? size : null);
          })
          .catch(() => {
            if (!cancelled) setOptimizedSizeBytes(null);
          });
    }

    return () => {
      cancelled = true;
    };
  }, [
    resource.download_url, optimizedUrl, batchedOriginal, batchedOptimized,
    isYoutube
  ]);

  return {originalSizeBytes, optimizedSizeBytes};
}
