//! Real-network TLS smoke test, kept `#[ignore]` so it never runs in CI or the
//! default `cargo test` (it needs outbound HTTPS to the production API).
//!
//! Its purpose is to guard the reqwest 0.13 upgrade, which switched the default
//! TLS backend from native-tls to rustls: it performs a genuine HTTPS GET with a
//! `reqwest::Client` built exactly like the app's `shared_http_client`
//! (`reqwest::Client::new()`) and asserts the handshake succeeds, the status is
//! 200, and the body deserializes into `ResourceListResponse`.
//!
//! Run explicitly with: `cargo test --test real_api_tls_smoke -- --ignored`

use church_helper_desktop_lib::models::ResourceListResponse;

#[tokio::test]
#[ignore = "hits the real production API over HTTPS; run with --ignored"]
async fn real_api_tls_smoke() {
    let url = "https://api.adventistyouth.it/api/resources/latest-week";

    // Same client configuration the app uses for its shared HTTP client.
    let client = reqwest::Client::new();

    let response = client
        .get(url)
        .send()
        .await
        .expect("HTTPS request failed (TLS handshake or network) — inspect the error to tell a rustls/TLS failure apart from a sandboxed/offline environment");

    assert_eq!(
        response.status().as_u16(),
        200,
        "expected HTTP 200 from {url}, got {}",
        response.status()
    );

    let parsed: ResourceListResponse = response
        .json()
        .await
        .expect("response body did not deserialize into ResourceListResponse");

    // Sanity: count field must agree with the number of resources returned.
    assert_eq!(parsed.count as usize, parsed.resources.len());
}
