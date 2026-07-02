//! Integration tests that deserialize REAL payloads captured from the local
//! api-stub (architecture/docs/contracts/resources-api.md, adr-0008), not
//! hand-written approximations. Regenerate the fixtures under
//! `tests/fixtures/` by running the stub (`node server.mjs` in the sibling
//! `api-stub/` repo — see this repo's README, "Puntare il desktop allo stub
//! API locale") and re-capturing `GET /api/resources/latest-week` after
//! `POST /stub/scenario/:name` for the relevant scenario.

use church_helper_desktop_lib::models::ResourceListResponse;
use church_helper_desktop_lib::services::FileRetentionService;

const MULTI_VIDEO_JSON: &str = include_str!("fixtures/stub_multi_video_latest_week.json");
const OLD_WEEKS_JSON: &str = include_str!("fixtures/stub_old_weeks_latest_week.json");
const NO_OPTIMIZED_JSON: &str = include_str!("fixtures/stub_no_optimized_latest_week.json");

/// adr-0008 "multi-video" scenario: resource 102 (a zip) offers 3 optimized
/// variants. Confirms the REAL stub payload parses and that the desktop
/// sees them ordered by size_bytes desc, with optimized_video_url pointing
/// at the first (largest) one, exactly as contract-resources-api specifies.
#[test]
fn test_stub_multi_video_scenario_parses() {
    let response: ResourceListResponse =
        serde_json::from_str(MULTI_VIDEO_JSON).expect("stub multi-video payload must deserialize");

    assert_eq!(response.count, 4);
    assert_eq!(response.resources.len(), 4);

    let missioni = response
        .resources
        .iter()
        .find(|r| r.id == 102)
        .expect("resource 102 (missioni zip) must be present");

    let videos = missioni
        .optimized_videos
        .as_ref()
        .expect("resource 102 must expose optimized_videos in this scenario");
    assert_eq!(videos.len(), 3);
    assert!(
        videos
            .windows(2)
            .all(|w| w[0].size_bytes >= w[1].size_bytes),
        "optimized_videos must be ordered by size_bytes desc: {:?}",
        videos.iter().map(|v| v.size_bytes).collect::<Vec<_>>()
    );
    assert_eq!(
        missioni.optimized_video_url.as_deref(),
        Some(videos[0].url.as_str()),
        "optimized_video_url must be the compat-default first element"
    );

    // Resources without a multi-video choice still deserialize fine with an
    // explicit `optimized_videos: null` (this scenario always emits the
    // field present, unlike "no-optimized" below where the keys are absent).
    let decime = response
        .resources
        .iter()
        .find(|r| r.id == 101)
        .expect("resource 101 must be present");
    assert_eq!(decime.optimized_videos, None);
}

/// adr-0008 additive-field tolerance, but against the REAL response shape a
/// pre-adr-0008 server would send: `optimized_video_url` / `optimized_videos`
/// are entirely ABSENT keys, not explicit `null`.
#[test]
fn test_stub_no_optimized_scenario_parses_with_missing_keys() {
    let response: ResourceListResponse = serde_json::from_str(NO_OPTIMIZED_JSON)
        .expect("stub no-optimized payload must deserialize");

    assert_eq!(response.count, 4);
    for resource in &response.resources {
        assert_eq!(resource.optimized_videos, None);
        assert_eq!(resource.optimized_video_url, None);
    }
}

/// contract-resources-api / adr-0003 (additive-only): `description` is a
/// tolerant field. Derives two degraded payloads from the REAL stub response
/// — one where a resource sends `description: null`, one where the key is
/// absent entirely — and confirms neither breaks the whole poll (both map to
/// `None`), while the untouched resources keep their descriptions.
#[test]
fn test_description_null_and_missing_do_not_break_real_payload() {
    let mut value: serde_json::Value =
        serde_json::from_str(NO_OPTIMIZED_JSON).expect("fixture must be valid JSON");
    let resources = value["resources"]
        .as_array_mut()
        .expect("payload must carry a resources array");

    // First resource: explicit null description.
    resources[0]
        .as_object_mut()
        .expect("resource must be a JSON object")
        .insert("description".to_string(), serde_json::Value::Null);
    // Second resource: description key removed entirely.
    resources[1]
        .as_object_mut()
        .expect("resource must be a JSON object")
        .remove("description");

    let degraded = serde_json::to_string(&value).expect("re-serialization must succeed");
    let response: ResourceListResponse = serde_json::from_str(&degraded)
        .expect("null/absent description must not fail the whole poll");

    assert_eq!(response.count, 4);
    assert_eq!(response.resources[0].description, None);
    assert_eq!(response.resources[1].description, None);
    // A resource that still carries a description keeps it as `Some(..)`.
    assert!(response.resources[2].description.is_some());
}

/// bl-desktop-archiving-not-called fixture: three resources spread across
/// three distinct past weeks, useful for exercising archiving/retention
/// week-boundary logic against realistic data instead of only synthetic
/// WeekIdentifiers.
#[test]
fn test_stub_old_weeks_scenario_parses_and_spans_distinct_weeks() {
    let response: ResourceListResponse =
        serde_json::from_str(OLD_WEEKS_JSON).expect("stub old-weeks payload must deserialize");

    assert_eq!(response.count, 3);
    let weeks: std::collections::HashSet<_> = response.resources.iter().map(|r| r.week()).collect();
    assert_eq!(
        weeks.len(),
        3,
        "old-weeks fixture must span 3 distinct weeks, got {:?}",
        weeks
    );
}

/// bl-desktop-archiving-not-called end to end (as far as a `cargo test` can
/// go without a full Tauri app): builds a work directory with one folder per
/// week actually reported by the REAL stub payload, treats the most recent
/// of the three as "current", and checks `archive_previous_weeks` moves the
/// other two into `.archive/` while leaving the current one alone.
#[test]
fn test_old_weeks_fixture_drives_archive_previous_weeks() {
    let response: ResourceListResponse =
        serde_json::from_str(OLD_WEEKS_JSON).expect("stub old-weeks payload must deserialize");

    let mut weeks: Vec<_> = response.resources.iter().map(|r| r.week()).collect();
    weeks.sort_by_key(|w| (w.year, w.week_number));
    let current = weeks
        .last()
        .cloned()
        .expect("fixture has at least one week");

    let temp_dir = tempfile::TempDir::new().unwrap();
    for week in &weeks {
        let dir = temp_dir.path().join(week.as_dir_name());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("video.mp4"), b"fixture content").unwrap();
    }

    let service = FileRetentionService::new(temp_dir.path().to_path_buf());
    let archived = service
        .archive_previous_weeks(&current, &std::collections::HashSet::new())
        .unwrap();

    assert_eq!(archived, (weeks.len() - 1) as u32);
    assert!(
        temp_dir.path().join(current.as_dir_name()).exists(),
        "current week folder must stay at the top level"
    );
    for week in &weeks {
        if week != &current {
            assert!(
                !temp_dir.path().join(week.as_dir_name()).exists(),
                "past week {} should have been moved out of the top level",
                week
            );
            assert!(service.week_archive_path(week).join("video.mp4").exists());
        }
    }
}

/// week_date (mail-parser fix shipping in parallel, adr-0003): the REAL
/// stub payloads captured so far predate the field, so it's absent on every
/// resource — already exercised implicitly by every test above (they all
/// deserialize fine). This confirms the *forward-compatible* shape too:
/// takes a real captured payload and injects `week_date` the way the
/// upgraded mail-parser will, without touching the fixture files themselves
/// (adr-0003: additive, so a real future payload will look exactly like
/// this).
#[test]
fn test_week_date_injected_into_real_payload_drives_week_and_tolerates_garbage() {
    let mut value: serde_json::Value =
        serde_json::from_str(NO_OPTIMIZED_JSON).expect("fixture must be valid JSON");
    let resources = value["resources"]
        .as_array_mut()
        .expect("payload must carry a resources array");
    assert!(
        resources.len() >= 3,
        "fixture must have at least 3 resources for this test"
    );

    // Resource 0: a valid week_date that must win over created_at.
    resources[0]
        .as_object_mut()
        .expect("resource must be a JSON object")
        .insert(
            "week_date".to_string(),
            serde_json::Value::String("2026-05-09".to_string()),
        );
    // Resource 1: a malformed week_date that must not break the batch.
    resources[1]
        .as_object_mut()
        .expect("resource must be a JSON object")
        .insert(
            "week_date".to_string(),
            serde_json::Value::String("boh".to_string()),
        );
    // Resource 2: left untouched (key absent), must keep falling back to
    // created_at exactly as before this field existed.

    let augmented = serde_json::to_string(&value).expect("re-serialization must succeed");
    let response: ResourceListResponse = serde_json::from_str(&augmented)
        .expect("a mix of valid/malformed/absent week_date must not break the whole poll");

    let with_valid_week_date = &response.resources[0];
    assert_eq!(
        with_valid_week_date.week_date,
        chrono::NaiveDate::from_ymd_opt(2026, 5, 9)
    );
    assert_eq!(with_valid_week_date.week().week_number, 19);

    let with_garbage_week_date = &response.resources[1];
    assert_eq!(with_garbage_week_date.week_date, None);
    assert_eq!(
        with_garbage_week_date.week(),
        church_helper_desktop_lib::models::WeekIdentifier::from_datetime(
            with_garbage_week_date.created_at
        ),
        "malformed week_date must degrade to the created_at fallback"
    );

    let with_absent_week_date = &response.resources[2];
    assert_eq!(with_absent_week_date.week_date, None);
}
