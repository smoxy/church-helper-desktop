import { create } from 'zustand';

import { computeSavings } from '../lib/savings';

export interface Celebration {
  id: string;
  resourceId: number;
  title: string;
  originalBytes: number|null;
  optimizedBytes: number|null;
  // Snapshotted via computeSavings at add-time so the panel and the session
  // total always agree, even if resourceStatuses changes afterwards.
  savedBytes: number|null;
  percent: number|null;
}

interface CelebrationState {
  celebrations: Celebration[];
  // Session-only cumulative total (in memory, resets on app restart). No
  // Rust/config persistence: that would need a new backend-owned config
  // field + set_config plumbing, disproportionate for a delight feature.
  sessionSavedBytes: number;
  addCelebration: (c: {
    resourceId: number,
    title: string,
    originalBytes: number|null,
    optimizedBytes: number|null,
  }) => void;
  removeCelebration: (id: string) => void;
  clearCelebrations: () => void;
}

// Limits memory/DOM churn when many auto-downloads complete back to back.
const MAX_KEPT = 12;

export const useCelebrationStore = create<CelebrationState>((set) => ({
  celebrations: [],
  sessionSavedBytes: 0,
  addCelebration: ({resourceId, title, originalBytes, optimizedBytes}) =>
      set((state) => {
        const s = computeSavings(originalBytes, optimizedBytes);
        const entry: Celebration = {
          id: Math.random().toString(36).slice(2, 9),
          resourceId,
          title,
          originalBytes,
          optimizedBytes,
          savedBytes: s?.savedBytes ?? null,
          percent: s?.percent ?? null,
        };
        // Dedupe per resourceId: a retry/re-download never stacks duplicates.
        const deduped =
            state.celebrations.filter(c => c.resourceId !== resourceId);
        return {
          celebrations: [entry, ...deduped].slice(0, MAX_KEPT),
          // Accumulated at add-time only: never double-counted, and does not
          // decrease when a panel is dismissed.
          sessionSavedBytes: state.sessionSavedBytes + (s?.savedBytes ?? 0),
        };
      }),
  removeCelebration: (id) =>
      set((state) => (
          {celebrations: state.celebrations.filter(c => c.id !== id)})),
  clearCelebrations: () => set({celebrations: []}),
}));
