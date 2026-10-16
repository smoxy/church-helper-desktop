import type {Resource} from '../types';
import type {Lang} from './i18n';

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * The Saturday the resource is projected on, as a UTC-midnight Date.
 *
 * Owner decision: the date shown for a resource is its `week_date` (the
 * projection Saturday sent by the server), never the ingest date
 * (`created_at`). When `week_date` is null (older servers), fall back to the
 * Saturday of the ISO week of `created_at` in UTC — the same week the backend
 * derives in `Resource::week()` (src-tauri/src/models.rs), so the date shown
 * always belongs to the week the file is filed under. Returns null only for
 * an unparsable `created_at`.
 */
export function projectionSaturday(resource: Resource): Date|null {
  if (resource.week_date) {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(resource.week_date);
    if (m) {
      return new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
    }
  }
  const created = new Date(resource.created_at);
  if (Number.isNaN(created.getTime())) return null;
  const day = Date.UTC(
      created.getUTCFullYear(), created.getUTCMonth(), created.getUTCDate());
  // ISO weekday with Monday = 0 … Sunday = 6; Saturday is index 5.
  const isoWeekday = (created.getUTCDay() + 6) % 7;
  return new Date(day + (5 - isoWeekday) * DAY_MS);
}

/**
 * The projection Saturday as an ISO calendar date ("2026-10-17"), for the
 * `dateTime` attribute of `<time>`; also covers the created_at fallback.
 */
export function projectionIsoDate(resource: Resource): string|null {
  const date = projectionSaturday(resource);
  return date ? date.toISOString().slice(0, 10) : null;
}

/**
 * Formats the projection Saturday: `long` → "Sabato 17 ottobre 2026" /
 * "Saturday, October 17, 2026", `short` → "17/10/2026" / "10/17/2026".
 * Formatted in UTC because the Date is a calendar day, not an instant.
 */
export function formatProjectionDate(
    resource: Resource, lang: Lang, style: 'short'|'long'): string|null {
  const date = projectionSaturday(resource);
  if (!date) return null;
  const options: Intl.DateTimeFormatOptions = style === 'long' ?
      {weekday: 'long', year: 'numeric', month: 'long', day: 'numeric'} :
      {year: 'numeric', month: '2-digit', day: '2-digit'};
  const text = date.toLocaleDateString(
      lang === 'it' ? 'it-IT' : 'en-US', {...options, timeZone: 'UTC'});
  // it-IT yields a lowercase weekday ("sabato …"); shown as "Sabato …".
  return text.charAt(0).toUpperCase() + text.slice(1);
}
