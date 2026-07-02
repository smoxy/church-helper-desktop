//! Shared constants for Church Helper Desktop

/// Base URL of the resources API (mail-parser-service), consumed by the
/// polling service and by the manual "force poll" command.
pub const API_BASE_URL: &str = "https://api.adventistyouth.it";

/// Environment variable read by [`api_base_url`] to override [`API_BASE_URL`]
/// at runtime. Dev-only: see the README section on pointing the desktop at
/// the local `api-stub` for how to use it.
const API_BASE_URL_ENV_VAR: &str = "CHURCH_HELPER_API_BASE";

/// Single point of resolution for the resources API base URL. Both
/// `services/polling.rs` and `commands.rs` MUST call this instead of reading
/// `API_BASE_URL` directly, so the two never drift apart again (the base URL
/// used to be duplicated between them).
///
/// Dev-only override: only compiled into debug builds (`cargo tauri dev`,
/// plain `cargo build`/`cargo test`), never into release builds shipped to
/// users, so a stray `CHURCH_HELPER_API_BASE` in a production environment
/// can never redirect a released app's API traffic. When present and
/// non-empty (after trimming whitespace), it wins over `API_BASE_URL`;
/// otherwise (unset, or present but empty/whitespace-only) nothing changes.
pub fn api_base_url() -> String {
    #[cfg(debug_assertions)]
    {
        if let Ok(value) = std::env::var(API_BASE_URL_ENV_VAR) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    API_BASE_URL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // All three scenarios live in one test (rather than separate #[test]
    // fns) because they mutate the shared process environment variable:
    // Rust runs tests in parallel by default, so separate tests touching
    // the same env var would race each other. Sequential steps within a
    // single test avoid that entirely.
    #[test]
    fn test_api_base_url_env_override_precedence() {
        // Unset (typical case): falls back to the constant.
        std::env::remove_var(API_BASE_URL_ENV_VAR);
        assert_eq!(api_base_url(), API_BASE_URL);

        // Present and non-empty: wins over the constant (debug builds only,
        // which is exactly what `cargo test` compiles by default).
        std::env::set_var(API_BASE_URL_ENV_VAR, "http://127.0.0.1:8787");
        assert_eq!(api_base_url(), "http://127.0.0.1:8787");

        // Whitespace is trimmed.
        std::env::set_var(API_BASE_URL_ENV_VAR, "  http://localhost:8787  ");
        assert_eq!(api_base_url(), "http://localhost:8787");

        // Present but empty/whitespace-only: treated as "not set".
        std::env::set_var(API_BASE_URL_ENV_VAR, "");
        assert_eq!(api_base_url(), API_BASE_URL);
        std::env::set_var(API_BASE_URL_ENV_VAR, "   ");
        assert_eq!(api_base_url(), API_BASE_URL);

        // Clean up so this test never leaks state into others.
        std::env::remove_var(API_BASE_URL_ENV_VAR);
    }
}

/// True when the debug-only `CHURCH_HELPER_API_BASE` override is active,
/// i.e. this run is a local test session against a stub backend. Used to
/// skip side effects that must not leave the machine during tests (e.g. the
/// GitHub update check). Always false in release builds, like the override.
pub fn is_api_base_overridden() -> bool {
    api_base_url() != API_BASE_URL
}
