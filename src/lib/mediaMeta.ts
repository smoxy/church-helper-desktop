import type {CSSProperties} from 'react';

import type {ResourceTag, Storyboard} from '../types';
import type {TKey, TVars} from './i18n';

/**
 * Pure presentation helpers for the resource metadata (duration, storyboard
 * sprite, tags). No domain rules here: values come from the backend and are
 * only formatted / mapped to pixels.
 */

/** "4:58" (m:ss), or "1:02:03" from one hour up. Null when unknown. */
export function formatDuration(seconds: number|null|undefined): string|null {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) ||
      seconds <= 0) {
    return null;
  }
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** Frame index (0-based) under a horizontal position `fraction` in [0, 1]. */
export function frameIndexAt(fraction: number, frames: number): number {
  if (!Number.isFinite(fraction) || frames <= 0) return 0;
  const clamped = Math.min(Math.max(fraction, 0), 1);
  return Math.min(frames - 1, Math.floor(clamped * frames));
}

/**
 * Frame selected by a key on the storyboard slider, or null when the key
 * does not move it. `current` is the frame on screen, null when nothing is
 * being scrubbed: the poster stands for frame 1 then, so ArrowRight goes to
 * frame 2 (index 1) and the announced value actually changes.
 */
export function nextKeyFrame(key: string, current: number | null, frames: number): number | null {
  if (frames <= 0) return null;
  const last = frames - 1;
  if (key === "ArrowRight") return current === null ? Math.min(1, last) : Math.min(last, current + 1);
  if (key === "ArrowLeft") return current === null ? last : Math.max(0, current - 1);
  if (key === "Home") return 0;
  if (key === "End") return last;
  return null;
}

/**
 * CSS that shows frame `index` of the sprite filling its element: the sprite
 * is scaled to cols x rows times the element and shifted so that one cell
 * covers it exactly (percent positions, so no pixel sizes are needed).
 */
export function spriteFrameStyle(
    storyboard: Storyboard, index: number): CSSProperties {
  const {cols, rows, url} = storyboard;
  const i = Math.min(Math.max(index, 0), storyboard.frames - 1);
  const col = i % cols;
  const row = Math.floor(i / cols);
  const x = cols > 1 ? (col / (cols - 1)) * 100 : 0;
  const y = rows > 1 ? (row / (rows - 1)) * 100 : 0;
  return {
    // Only the characters that could break out of the CSS string are
    // escaped; the URL is otherwise used as sent (already URL-encoded).
    backgroundImage: `url("${
        url.replace(/["\\\n\r]/g, c => encodeURIComponent(c))}")`,
    backgroundSize: `${cols * 100}% ${rows * 100}%`,
    backgroundPosition: `${x}% ${y}%`,
    backgroundRepeat: 'no-repeat',
  };
}

/** A tag ready to show: its facet label key and display text. */
export interface TagChip {
  facet: 'numero'|'trimestre'|'durata';
  labelKey: TKey;
  textKey: TKey;
  vars: TVars;
}

// Order of the chips; the card shows the first two available.
const CHIP_ORDER: readonly TagChip['facet'][] = ['numero', 'trimestre', 'durata'];

function toChip(tag: ResourceTag): TagChip|null {
  const value = tag.value.trim();
  switch (tag.facet) {
    case 'numero': {
      if (!/^\d{1,4}$/.test(value)) return null;
      return {
        facet: 'numero',
        labelKey: 'tags.facet.numero',
        textKey: 'tags.value.numero',
        vars: {n: Number(value)},
      };
    }
    case 'trimestre': {
      const m = /^(\d{4})-Q([1-4])$/.exec(value);
      if (!m) return null;
      return {
        facet: 'trimestre',
        labelKey: 'tags.facet.trimestre',
        textKey: 'tags.value.trimestre',
        vars: {q: Number(m[2]), year: m[1]},
      };
    }
    case 'durata': {
      const duration = formatDuration(Number(value));
      if (!/^\d+(\.\d+)?$/.test(value) || !duration) return null;
      return {
        facet: 'durata',
        labelKey: 'tags.facet.durata',
        textKey: 'tags.value.durata',
        vars: {duration},
      };
    }
    // categoria and data are already shown elsewhere (chip, date); unknown
    // facets are ignored.
    default:
      return null;
  }
}

/**
 * The tag chips to show, in a stable order (numero, trimestre, durata), one
 * per facet; malformed values are skipped. `limit` caps the count (cards).
 */
export function tagChips(
    tags: ResourceTag[]|null|undefined, limit?: number): TagChip[] {
  const byFacet = new Map<TagChip['facet'], TagChip>();
  for (const tag of tags ?? []) {
    const chip = toChip(tag);
    if (chip && !byFacet.has(chip.facet)) byFacet.set(chip.facet, chip);
  }
  const chips = CHIP_ORDER.flatMap(f => {
    const chip = byFacet.get(f);
    return chip ? [chip] : [];
  });
  return limit === undefined ? chips : chips.slice(0, limit);
}
