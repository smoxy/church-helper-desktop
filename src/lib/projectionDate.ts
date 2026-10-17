import type {Resource} from '../types';
import type {Lang} from './i18n';

/**
 * The projection Saturday as a UTC-midnight Date, parsed from the backend's
 * `projection_date` ("YYYY-MM-DD"). The rule that picks the Saturday
 * (`week_date`, else the Saturday of `created_at`'s ISO week) lives only in
 * Rust (`Resource::projection_date`, src-tauri/src/models.rs); this module
 * only parses and formats it. Null when the field is absent or malformed.
 */
function projectionSaturday(resource: Resource): Date|null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(resource.projection_date ?? '');
  if (!m) return null;
  const date =
      new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  return Number.isNaN(date.getTime()) ? null : date;
}

/**
 * The projection Saturday as an ISO calendar date ("2026-10-17"), for the
 * `dateTime` attribute of `<time>`.
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
