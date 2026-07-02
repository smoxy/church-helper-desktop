// Pure helper: keeps the "how much did the optimized variant save" math out
// of components (UI-dumb guard). Reused by the Intervento A inline toggle and
// the Intervento B celebration panel/store.
export interface Savings {
  savedBytes: number;
  percent: number;
}

// Returns null when the data is missing or there is no real saving
// (optimized >= original).
export function computeSavings(
    originalBytes: number|null|undefined,
    optimizedBytes: number|null|undefined,
    ): Savings|null {
  if (!originalBytes || !optimizedBytes) return null;
  if (optimizedBytes >= originalBytes) return null;
  const savedBytes = originalBytes - optimizedBytes;
  const percent = Math.round((savedBytes / originalBytes) * 100);
  return {savedBytes, percent};
}
