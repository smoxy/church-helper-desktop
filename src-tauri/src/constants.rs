//! Shared constants for Church Helper Desktop

/// Base URL of the resources API (mail-parser-service), consumed by the
/// polling service and by the manual "force poll" command.
pub const API_BASE_URL: &str = "https://api.adventistyouth.it";

/// Environment variable read by [`api_base_url`] to override [`API_BASE_URL`]
/// at runtime. Dev-only: see the README section on pointing the desktop at
/// the local `api-stub` for how to use it.
#[cfg(any(debug_assertions, test))]
const API_BASE_URL_ENV_VAR: &str = "CHURCH_HELPER_API_BASE";

/// Single point of resolution for the resources API base URL. Both
/// `services/polling.rs` and `commands.rs` MUST call this instead of reading
/// `API_BASE_URL` directly, so the two never drift apart again (the base URL
/// used to be duplicated between them).
///
/// Precedence, highest first:
/// 1. Runtime debug override — the `CHURCH_HELPER_API_BASE` env var read at
///    process start, only compiled into debug builds (`cargo tauri dev`, plain
///    `cargo build`/`cargo test`), never into release builds shipped to users,
///    so a stray env var in a production environment can never redirect a
///    released app's API traffic.
/// 2. Build-time default — the same-named env var captured by `option_env!`
///    when the binary is compiled (set in CI via the Actions variable), used
///    when non-empty after trimming.
/// 3. The compiled-in [`API_BASE_URL`] constant.
pub fn api_base_url() -> String {
    #[cfg(debug_assertions)]
    {
        if let Some(value) = runtime_api_base_override() {
            return value;
        }
    }
    build_time_api_base_url().to_string()
}

/// Runtime, debug-only override (precedence step 1 in [`api_base_url`]):
/// `Some` only when `CHURCH_HELPER_API_BASE` is set to a value that is
/// non-empty after trimming.
#[cfg(debug_assertions)]
fn runtime_api_base_override() -> Option<String> {
    let value = std::env::var(API_BASE_URL_ENV_VAR).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Build-time default (precedence step 2 in [`api_base_url`]):
/// `CHURCH_HELPER_API_BASE` as seen by `option_env!` at compile time, falling
/// back to [`API_BASE_URL`] when absent or empty. CI passes the Actions
/// variable, which is the empty string when unset — treated as absent.
fn build_time_api_base_url() -> &'static str {
    // option_env! only accepts a string literal, so the env var name is
    // spelled out here rather than reusing API_BASE_URL_ENV_VAR.
    match option_env!("CHURCH_HELPER_API_BASE") {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => API_BASE_URL,
    }
}

/// True when the debug-only runtime `CHURCH_HELPER_API_BASE` override is
/// active, i.e. this run is a local test session against a stub backend. Used
/// to skip side effects that must not leave the machine during tests (e.g. the
/// GitHub update check). Always false in release builds, like the override —
/// this tracks the runtime override only, not the build-time default, so a
/// CI-configured build-time base URL does not count as "overridden".
pub fn is_api_base_overridden() -> bool {
    #[cfg(debug_assertions)]
    {
        runtime_api_base_override().is_some()
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // All scenarios live in one test (rather than separate #[test] fns)
    // because they mutate the shared process environment variable: Rust runs
    // tests in parallel by default, so separate tests touching the same env
    // var would race each other. Sequential steps within a single test avoid
    // that entirely.
    #[test]
    fn test_api_base_url_env_override_precedence() {
        // Expected value when the runtime override is inactive: the build-time
        // default. That is API_BASE_URL unless this binary happened to be
        // compiled with a non-empty CHURCH_HELPER_API_BASE, so comparing
        // against it (rather than API_BASE_URL directly) keeps the test correct
        // regardless of the compile-time configuration.
        let fallback = build_time_api_base_url();

        // Unset (typical case): falls back to the build-time default.
        std::env::remove_var(API_BASE_URL_ENV_VAR);
        assert_eq!(api_base_url(), fallback);
        assert!(!is_api_base_overridden());

        // Present and non-empty: wins over the default (debug builds only,
        // which is exactly what `cargo test` compiles by default).
        std::env::set_var(API_BASE_URL_ENV_VAR, "http://127.0.0.1:8787");
        assert_eq!(api_base_url(), "http://127.0.0.1:8787");
        assert!(is_api_base_overridden());

        // Whitespace is trimmed.
        std::env::set_var(API_BASE_URL_ENV_VAR, "  http://localhost:8787  ");
        assert_eq!(api_base_url(), "http://localhost:8787");

        // Present but empty/whitespace-only: treated as "not set".
        std::env::set_var(API_BASE_URL_ENV_VAR, "");
        assert_eq!(api_base_url(), fallback);
        assert!(!is_api_base_overridden());
        std::env::set_var(API_BASE_URL_ENV_VAR, "   ");
        assert_eq!(api_base_url(), fallback);
        assert!(!is_api_base_overridden());

        // Clean up so this test never leaks state into others.
        std::env::remove_var(API_BASE_URL_ENV_VAR);
    }

    #[test]
    fn test_build_time_api_base_url_is_non_empty_and_trimmed() {
        // Whatever the compile-time configuration, the build-time default is
        // always a non-empty, already-trimmed URL: an absent or empty
        // option_env! falls back to the constant.
        let value = build_time_api_base_url();
        assert!(!value.is_empty());
        assert_eq!(value, value.trim());
    }
}
