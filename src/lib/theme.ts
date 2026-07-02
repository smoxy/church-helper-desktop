import type { ThemeSetting } from "../types";

const THEME_HINT_KEY = "theme-hint";
const DARK_QUERY = "(prefers-color-scheme: dark)";

// Resolve the effective dark/light of a setting: an explicit Light/Dark wins,
// System defers to the OS via matchMedia.
function resolveDark(setting: ThemeSetting): boolean {
    if (setting === "Dark") return true;
    if (setting === "Light") return false;
    return window.matchMedia(DARK_QUERY).matches;
}

function paint(isDark: boolean): void {
    document.documentElement.classList.toggle("dark", isDark);
    // Persist the resolved class so the next launch can paint before React
    // mounts (see initThemeFromCache) and avoid a light-to-dark flash.
    localStorage.setItem(THEME_HINT_KEY, isDark ? "dark" : "light");
}

/**
 * Apply `setting` to the document root and keep the cached paint hint in sync.
 * Returns a cleanup function: for `System` it registers an OS-change listener
 * that re-applies on the fly and the cleanup removes it; for `Light`/`Dark`
 * the cleanup is a no-op. Owns no store/IPC state.
 */
export function applyTheme(setting: ThemeSetting): () => void {
    paint(resolveDark(setting));

    if (setting !== "System") {
        return () => {};
    }

    const media = window.matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent) => paint(event.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
}

/**
 * Synchronously apply the cached paint hint before React renders, so the very
 * first paint already matches the user's last-known theme. Only touches the
 * root class from localStorage; the authoritative setting is applied later via
 * `applyTheme` once the config is loaded.
 */
export function initThemeFromCache(): void {
    document.documentElement.classList.toggle(
        "dark",
        localStorage.getItem(THEME_HINT_KEY) === "dark"
    );
}
