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
  // Session savings, DERIVED (not accumulated) from the authoritative backend
  // total: max(0, totalSavedBytes - sessionBaselineBytes). Recomputed every
  // time totalSavedBytes moves (see applyTotal below), so it can never drift
  // from the backend and is naturally idempotent under duplicate/replayed
  // events — no per-resource ledger needed.
  sessionSavedBytes: number;
  // The value of totalSavedBytes right before this session's first download
  // landed — the "session t=0" mark sessionSavedBytes is measured against.
  // `null` until the first candidate arrives (see applyBaselineCandidate for
  // why a plain Math.min of candidates is the correct, race-proof way to
  // pick it).
  sessionBaselineBytes: number|null;
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

// Folds a new "pre-session-total" candidate into the running baseline.
//
// Non-obvious constraint this encodes: the baseline should be the backend's
// totalSavedBytes at the instant this session started, but nothing delivers
// that value directly. Two independent, racy sources approximate it: (a) the
// get_savings_stats seed fetched at startup, and (b) a download payload's own
// `total - saved`, i.e. the total right before *that* download's
// contribution. Either can arrive first:
//   - If the seed wins the race, it already equals the true baseline.
//   - If a download-complete/savings-resolved event beats the seed (listener
//     registration and the initial fetch run concurrently), the seed would
//     already include that download's saving, overstating the baseline and
//     erasing it from the session count. That event's own `total - saved`
//     candidate is the true baseline instead.
// Math.min is the safe combinator for both orders: the true baseline is the
// smallest of every candidate ever observed, because totalSavedBytes only
// grows, so every candidate computed after the first download can only be >=
// the true baseline. Taking the min self-corrects the race above and is a
// no-op once the baseline has already settled on the correct value.
function applyBaselineCandidate(
    current: number|null, candidate: number): number {
  return current === null ? candidate : Math.min(current, candidate);
}

// The baseline candidate implied by a download-complete/savings-resolved
// payload: the authoritative total *before* this particular event's own
// saving was folded in.
function payloadBaselineCandidate(
    totalSavedBytes: number, savedBytes: number|null): number {
  return totalSavedBytes - (savedBytes ?? 0);
}

// Single place session accounting happens: bumps totalSavedBytes (monotonic
// max — guards against an older snapshot arriving after a newer one, e.g.
// parallel downloads or a savings-resolved racing a later download-complete),
// folds `baselineCandidate` into sessionBaselineBytes, and re-derives
// sessionSavedBytes as the difference. All three write sites
// (setTotalSavedBytes / addCelebration / resolveSavings) go through this so
// the session figure is always a pure function of the two, never a separate
// accumulator.
function applyTotal(
    state: {totalSavedBytes: number, sessionBaselineBytes: number|null},
    totalSavedBytes: number,
    baselineCandidate: number,
): {
  totalSavedBytes: number,
  sessionBaselineBytes: number,
  sessionSavedBytes: number,
} {
  const total = Math.max(state.totalSavedBytes, totalSavedBytes);
  const baseline =
      applyBaselineCandidate(state.sessionBaselineBytes, baselineCandidate);
  return {
    totalSavedBytes: total,
    sessionBaselineBytes: baseline,
    sessionSavedBytes: Math.max(0, total - baseline),
  };
}

export const useCelebrationStore = create<CelebrationState>((set) => ({
  celebrations: [],
  sessionSavedBytes: 0,
  sessionBaselineBytes: null,
  totalSavedBytes: 0,
  // Startup seed from get_savings_stats: also the first (and usually only)
  // baseline candidate — see applyBaselineCandidate.
  setTotalSavedBytes: (bytes) =>
      set((state) => applyTotal(state, bytes, bytes)),
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
          ...applyTotal(
              state, totalSavedBytes,
              payloadBaselineCandidate(totalSavedBytes, savedBytes)),
        };
      }),
  resolveSavings:
      ({id: resourceId, saved_bytes: savedBytes, original_bytes: originalBytes, total_saved_bytes: totalSavedBytes}) =>
      set((state) => {
        const idx = state.celebrations.findIndex(c => c.resourceId === resourceId);
        const counters = applyTotal(
            state, totalSavedBytes,
            payloadBaselineCandidate(totalSavedBytes, savedBytes));
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
