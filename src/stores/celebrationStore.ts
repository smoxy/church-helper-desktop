import { create } from 'zustand';

import { computeSavings } from '../lib/savings';
import type { SavingsResolvedPayload } from '../types';

export interface Celebration {
  id: string;
  resourceId: number;
  title: string;
  originalBytes: number|null;
  optimizedBytes: number|null;
  // Sourced directly from the `download-complete` payload (the backend is
  // the only reliable source: it knows the real downloaded/original sizes,
  // including for auto-downloads the frontend never fetches on its own). No
  // longer recomputed from resourceStatuses/cache, which could be stale or
  // simply not yet populated for a given resource.
  savedBytes: number|null;
  percent: number|null;
}

interface CelebrationState {
  celebrations: Celebration[];
  // Session-only cumulative total (in memory, resets on app restart): sum of
  // each celebration's savedBytes at add-time. Never decreases when a panel
  // is dismissed, never double-counted.
  sessionSavedBytes: number;
  // Persistent, cross-session running total mirrored from the backend
  // (SavingsStats.total_saved_bytes / DownloadCompletePayload.total_saved_bytes).
  // Seeded once via get_savings_stats on app startup (appStore.fetchInitialData)
  // and kept in sync thereafter by each download-complete payload — no
  // separate re-fetch needed.
  totalSavedBytes: number;
  setTotalSavedBytes: (bytes: number) => void;
  addCelebration: (c: {
    resourceId: number,
    title: string,
    originalBytes: number|null,
    optimizedBytes: number|null,
    savedBytes: number|null,
    totalSavedBytes: number,
  }) => void;
  // Upgrades the celebration matching `payload.id` (== resourceId) from its
  // generic "no savings info" copy to the full savings layout, once the
  // backend's detached background task resolves the original size that
  // wasn't cached yet at `download-complete` time (see
  // src-tauri/src/services/queue.rs). Still folds savedBytes/totalSavedBytes
  // into the session/total counters even if the panel itself was already
  // dismissed/cleared — the saving still happened.
  resolveSavings: (payload: SavingsResolvedPayload) => void;
  removeCelebration: (id: string) => void;
  clearCelebrations: () => void;
}

// Limits memory/DOM churn when many auto-downloads complete back to back.
const MAX_KEPT = 12;

export const useCelebrationStore = create<CelebrationState>((set) => ({
  celebrations: [],
  sessionSavedBytes: 0,
  totalSavedBytes: 0,
  // Monotonic guard: totalSavedBytes must never regress on screen because an
  // event carrying an older snapshot arrives after a newer one (parallel
  // downloads, or a savings-resolved racing a later download-complete).
  setTotalSavedBytes: (bytes) =>
      set((state) => ({totalSavedBytes: Math.max(state.totalSavedBytes, bytes)})),
  addCelebration:
      ({resourceId, title, originalBytes, optimizedBytes, savedBytes, totalSavedBytes}) =>
      set((state) => {
        // percent is still derived locally (same guard as computeSavings:
        // both sizes known and optimized actually smaller); savedBytes
        // itself always comes straight from the backend payload.
        const percent = computeSavings(originalBytes, optimizedBytes)?.percent ?? null;
        const entry: Celebration = {
          id: Math.random().toString(36).slice(2, 9),
          resourceId,
          title,
          originalBytes,
          optimizedBytes,
          savedBytes,
          percent,
        };
        // Dedupe per resourceId: a retry/re-download never stacks duplicates.
        const deduped =
            state.celebrations.filter(c => c.resourceId !== resourceId);
        return {
          celebrations: [entry, ...deduped].slice(0, MAX_KEPT),
          sessionSavedBytes: state.sessionSavedBytes + (savedBytes ?? 0),
          totalSavedBytes: Math.max(state.totalSavedBytes, totalSavedBytes),
        };
      }),
  resolveSavings:
      ({id: resourceId, saved_bytes: savedBytes, original_bytes: originalBytes, total_saved_bytes: totalSavedBytes}) =>
      set((state) => {
        const idx = state.celebrations.findIndex(c => c.resourceId === resourceId);
        // savedBytes was never counted at addCelebration time for this
        // resource (it was null then, which is exactly why this event
        // exists), so folding it in now — panel present or not — counts it
        // exactly once.
        const counters = {
          sessionSavedBytes: state.sessionSavedBytes + (savedBytes ?? 0),
          totalSavedBytes: Math.max(state.totalSavedBytes, totalSavedBytes),
        };
        if (idx === -1) return counters;

        const celebration = state.celebrations[idx];
        const percent =
            computeSavings(originalBytes, celebration.optimizedBytes)?.percent ?? null;
        const celebrations = [...state.celebrations];
        celebrations[idx] = {...celebration, originalBytes, savedBytes, percent};
        return {celebrations, ...counters};
      }),
  removeCelebration: (id) =>
      set((state) => (
          {celebrations: state.celebrations.filter(c => c.id !== id)})),
  clearCelebrations: () => set({celebrations: []}),
}));
