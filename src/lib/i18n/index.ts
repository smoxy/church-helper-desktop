import {useCallback} from 'react';

import {useAppStore} from '../../stores/appStore';
import type {LanguageSetting} from '../../types';
import {en, it, TKey} from './dictionaries';

export type {TKey} from './dictionaries';
export type Lang = 'it'|'en';

/** Values a t()/tGlobal() call may substitute into a `{placeholder}` token. */
export type TVars = Record<string, string|number>;

// Mirrors theme.ts's THEME_HINT_KEY/paint/initThemeFromCache pattern: the
// last language actually resolved from a real (defined) config setting is
// cached here, so the transient `undefined` window before config.language
// has loaded can fall back to it instead of guessing from the browser
// locale. Without this, a user with an explicit override that differs from
// their OS/browser language (e.g. config=English on an Italian-locale
// machine) saw every `t()` string flash in the wrong language until
// fetchInitialData resolved.
const LANGUAGE_HINT_KEY = 'language-hint';

function systemLang(): Lang {
  return navigator.language.toLowerCase().startsWith('it') ? 'it' : 'en';
}

function readLanguageHint(): Lang {
  const cached = localStorage.getItem(LANGUAGE_HINT_KEY);
  return cached === 'it' || cached === 'en' ? cached : systemLang();
}

/**
 * Resolve an `AppConfig.language` setting to the concrete dictionary to use.
 * `System` defers to the OS/browser locale; an explicit `Italian`/`English`
 * always wins. The backend config is always the source of truth once loaded:
 * every resolution against a defined `setting` updates the cached hint
 * (`readLanguageHint`) for next time. The transient `undefined` before
 * config has loaded is the only case that reads the hint instead of
 * resolving fresh, to avoid the pre-config flash described above.
 */
export function resolveLanguage(setting: LanguageSetting|undefined): Lang {
  if (setting === undefined) return readLanguageHint();

  const lang = setting === 'Italian' ? 'it' :
      setting === 'English'          ? 'en' :
                                        systemLang();
  localStorage.setItem(LANGUAGE_HINT_KEY, lang);
  return lang;
}

/**
 * Synchronously read the cached language hint before React renders (called
 * from main.tsx, mirroring `theme.ts`'s `initThemeFromCache`), so the
 * document's `lang` attribute already matches the user's last-known
 * resolved language from the very first paint instead of a default.
 */
export function initLanguageFromCache(): void {
  document.documentElement.lang = readLanguageHint();
}

// Replaces every `{name}` token in `template` with `vars.name`. Tokens with
// no matching var are left untouched (fails loud in dev via the missing text
// rather than silently swallowing the placeholder).
function interpolate(template: string, vars?: TVars): string {
  if (!vars) return template;
  return template.replace(
      /\{(\w+)\}/g,
      (match, name: string) =>
          Object.prototype.hasOwnProperty.call(vars, name) ?
          String(vars[name]) :
          match);
}

function translate(lang: Lang, key: TKey, vars?: TVars): string {
  const dict = lang === 'it' ? it : en;
  return interpolate(dict[key], vars);
}

/**
 * React hook: resolves the active language from the store (selector-only
 * subscription, so components only re-render when `config.language` itself
 * changes) and returns a `t()` translator bound to it plus the resolved
 * `lang` code. Changing the language in Settings calls `updateConfig`, which
 * updates the store and re-renders every subscribed component immediately.
 */
export function useI18n() {
  const language = useAppStore(state => state.config?.language);
  const lang = resolveLanguage(language);
  const t = useCallback(
      (key: TKey, vars?: TVars) => translate(lang, key, vars), [lang]);
  return {t, lang};
}

/**
 * Non-React escape hatch for code that emits user-facing strings outside a
 * component render (store actions, event listeners) where hooks cannot be
 * called. Reads the store's current state on demand instead of subscribing,
 * so it always reflects the language active at call time.
 */
export function tGlobal(key: TKey, vars?: TVars): string {
  const language = useAppStore.getState().config?.language;
  const lang = resolveLanguage(language);
  return translate(lang, key, vars);
}
