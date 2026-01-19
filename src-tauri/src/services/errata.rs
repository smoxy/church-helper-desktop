//! Errata Corrige detection logic
//!
//! This module handles detection of updated resources within the same week.

use crate::models::{DownloadedFile, ErrataChange, Resource};

/// Detect errata corrige changes by comparing local files with remote resources.
///
/// An errata corrige is detected when:
/// - A resource with the same ID exists locally
/// - The remote resource's created_at is newer than local downloaded_at
/// - Both are in the same week
pub fn detect_errata_changes(
    local_files: &[DownloadedFile],
    remote_resources: &[Resource],
) -> Vec<ErrataChange> {
    remote_resources
        .iter()
        .filter_map(|remote| {
            // Find local file with same resource ID in the same week
            local_files
                .iter()
                .find(|local| {
                    local.resource_id == remote.id
                        && local.week == remote.week()
                        && !local.is_superseded
                })
                .filter(|local| local.downloaded_at < remote.created_at)
                .map(|local| ErrataChange {
                    resource_id: remote.id,
                    old_file: local.clone(),
                    new_resource: remote.clone(),
                })
        })
        .collect()
}

/// Find resources that are new (not yet downloaded)
pub fn find_new_resources(
    local_files: &[DownloadedFile],
    remote_resources: &[Resource],
) -> Vec<Resource> {
    remote_resources
        .iter()
        .filter(|remote| {
            !local_files
                .iter()
                .any(|local| local.resource_id == remote.id && !local.is_superseded)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WeekIdentifier;
    use chrono::{TimeZone, Utc};
    use std::path::PathBuf;

    fn create_resource(id: i64, created_at: chrono::DateTime<Utc>) -> Resource {
        Resource {
            id,
            category: "test".to_string(),
            title: format!("Resource {}", id),
            description: "Test resource".to_string(),
            download_url: format!("https://example.com/file_{}.zip", id),
            thumbnail_url: None,
            file_type: None,
            is_active: true,
            created_at,
        }
    }

    fn create_downloaded_file(
        resource_id: i64,
        week: WeekIdentifier,
        downloaded_at: chrono::DateTime<Utc>,
    ) -> DownloadedFile {
        DownloadedFile {
            resource_id,
            week,
            local_path: PathBuf::from(format!("/downloads/file_{}.zip", resource_id)),
            downloaded_at,
            source_url: format!("https://example.com/file_{}.zip", resource_id),
            is_superseded: false,
        }
    }

    #[test]
    fn test_no_errata_when_no_local_files() {
        let local: Vec<DownloadedFile> = vec![];
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let remote = vec![create_resource(1, dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_no_errata_when_same_timestamp() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        let local = vec![create_downloaded_file(1, week, dt)];
        let remote = vec![create_resource(1, dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_errata_detected_when_remote_is_newer() {
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let local = vec![create_downloaded_file(1, week.clone(), original_dt)];
        let remote = vec![create_resource(1, updated_dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource_id, 1);
        assert_eq!(changes[0].old_file.downloaded_at, original_dt);
        assert_eq!(changes[0].new_resource.created_at, updated_dt);
    }

    #[test]
    fn test_no_errata_when_local_is_newer() {
        // This shouldn't happen in practice, but test defensive behavior
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let remote_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let local = vec![create_downloaded_file(1, week, original_dt)];
        let remote = vec![create_resource(1, remote_dt)];

        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_multiple_errata_detected() {
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let local = vec![
            create_downloaded_file(1, week.clone(), original_dt),
            create_downloaded_file(2, week.clone(), original_dt),
            create_downloaded_file(3, week.clone(), original_dt),
        ];
        let remote = vec![
            create_resource(1, updated_dt),         // Updated
            create_resource(2, original_dt),        // Not updated
            create_resource(3, updated_dt),         // Updated
        ];

        let changes = detect_errata_changes(&local, &remote);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| c.resource_id == 1));
        assert!(changes.iter().any(|c| c.resource_id == 3));
        assert!(!changes.iter().any(|c| c.resource_id == 2));
    }

    #[test]
    fn test_superseded_files_are_ignored() {
        let original_dt = Utc.with_ymd_and_hms(2026, 1, 19, 10, 0, 0).unwrap();
        let updated_dt = Utc.with_ymd_and_hms(2026, 1, 19, 14, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(original_dt);

        let mut superseded_file = create_downloaded_file(1, week, original_dt);
        superseded_file.is_superseded = true;
        let local = vec![superseded_file];
        let remote = vec![create_resource(1, updated_dt)];

        // Should NOT detect errata for already superseded files
        let changes = detect_errata_changes(&local, &remote);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_find_new_resources_empty_local() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let remote = vec![
            create_resource(1, dt),
            create_resource(2, dt),
        ];

        let new = find_new_resources(&[], &remote);
        assert_eq!(new.len(), 2);
    }

    #[test]
    fn test_find_new_resources_some_existing() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        let local = vec![create_downloaded_file(1, week, dt)];
        let remote = vec![
            create_resource(1, dt),
            create_resource(2, dt),
            create_resource(3, dt),
        ];

        let new = find_new_resources(&local, &remote);
        assert_eq!(new.len(), 2);
        assert!(new.iter().any(|r| r.id == 2));
        assert!(new.iter().any(|r| r.id == 3));
        assert!(!new.iter().any(|r| r.id == 1));
    }

    #[test]
    fn test_find_new_resources_ignores_superseded() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap();
        let week = WeekIdentifier::from_datetime(dt);

        let mut superseded = create_downloaded_file(1, week, dt);
        superseded.is_superseded = true;
        let local = vec![superseded];
        let remote = vec![create_resource(1, dt)];

        // Resource 1 should be considered new since existing one is superseded
        let new = find_new_resources(&local, &remote);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].id, 1);
    }
}
