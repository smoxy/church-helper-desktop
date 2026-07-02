import {type ClassValue, clsx} from 'clsx'
import {twMerge} from 'tailwind-merge'

import {CommandError} from '../types'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// Type guard: does an unknown caught value look like a structured CommandError
// (the { code, message } object Tauri delivers when a Rust command returns
// Err(CommandError))? Kept narrow so callers never resort to `any`.
export function isCommandError(e: unknown): e is CommandError {
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e &&
      typeof (e as {code: unknown}).code === 'string' &&
      typeof (e as {message: unknown}).message === 'string';
}

// Extract a user-facing message from any caught value. Prefers the structured
// CommandError.message, then a native Error.message, and finally String(e).
// Use this instead of interpolating a raw catch value (`${e}` renders a
// structured error as "[object Object]").
export function errorMessage(e: unknown): string {
  if (isCommandError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** Format a byte count as a human-readable size (e.g. "8.1 MB", "1.2 GB"). */
export function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB'];
  let size = bytes;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }
  return `${size.toFixed(1)} ${units[unitIndex]}`;
}
