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
