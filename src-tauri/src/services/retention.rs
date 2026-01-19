//! File retention service
//!
//! Handles archiving of old week files and retention policy enforcement.

use crate::error::FileError;
use crate::models::WeekIdentifier;
use chrono::{Duration, Utc};
use std::fs;
use std::path::{Path, PathBuf};

/// Archive directory name
const ARCHIVE_DIR: &str = ".archive";
/// Superseded files subdirectory within week archive
const SUPERSEDED_DIR: &str = ".superseded";

/// Service for managing file retention and archiving
pub struct FileRetentionService {
    work_dir: PathBuf,
}

impl FileRetentionService {
    /// Create a new FileRetentionService
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    /// Get the archive directory path
    pub fn archive_dir(&self) -> PathBuf {
        self.work_dir.join(ARCHIVE_DIR)
    }

    /// Get the archive path for a specific week
    pub fn week_archive_path(&self, week: &WeekIdentifier) -> PathBuf {
        self.archive_dir().join(week.as_dir_name())
    }

    /// Get the superseded files path for a specific week
    pub fn superseded_path(&self, week: &WeekIdentifier) -> PathBuf {
        self.week_archive_path(week).join(SUPERSEDED_DIR)
    }

    /// Archive a file for a previous week
    ///
    /// Moves the file from work_dir to .archive/{week}/
    pub fn archive_file(&self, file_path: &Path, week: &WeekIdentifier) -> Result<PathBuf, FileError> {
        let archive_path = self.week_archive_path(week);
        
        // Create archive directory if it doesn't exist
        fs::create_dir_all(&archive_path).map_err(|e| FileError::CreateDirectoryFailed {
            path: archive_path.clone(),
            source: e,
        })?;

        // Get filename and construct destination
        let file_name = file_path
            .file_name()
            .ok_or_else(|| FileError::MoveFileFailed {
                from: file_path.to_path_buf(),
                to: archive_path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid filename"),
            })?;
        let dest_path = archive_path.join(file_name);

        // Move the file
        fs::rename(file_path, &dest_path).map_err(|e| FileError::MoveFileFailed {
            from: file_path.to_path_buf(),
            to: dest_path.clone(),
            source: e,
        })?;

        Ok(dest_path)
    }

    /// Move a superseded file to the superseded directory
    ///
    /// Moves from work_dir to .archive/{week}/.superseded/
    pub fn archive_superseded(&self, file_path: &Path, week: &WeekIdentifier) -> Result<PathBuf, FileError> {
        let superseded_path = self.superseded_path(week);
        
        // Create superseded directory if it doesn't exist
        fs::create_dir_all(&superseded_path).map_err(|e| FileError::CreateDirectoryFailed {
            path: superseded_path.clone(),
            source: e,
        })?;

        let file_name = file_path
            .file_name()
            .ok_or_else(|| FileError::MoveFileFailed {
                from: file_path.to_path_buf(),
                to: superseded_path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid filename"),
            })?;
        let dest_path = superseded_path.join(file_name);

        fs::rename(file_path, &dest_path).map_err(|e| FileError::MoveFileFailed {
            from: file_path.to_path_buf(),
            to: dest_path.clone(),
            source: e,
        })?;

        Ok(dest_path)
    }

    /// Get all archived weeks
    pub fn get_archived_weeks(&self) -> Vec<WeekIdentifier> {
        let archive_dir = self.archive_dir();
        
        if !archive_dir.exists() {
            return Vec::new();
        }

        fs::read_dir(&archive_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .filter_map(|e| parse_week_dir_name(e.file_name().to_str()?))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Enforce retention policy
    ///
    /// - retention_days = None: Keep forever
    /// - retention_days = Some(0): Delete immediately (move to trash)
    /// - retention_days = Some(n): Move to trash after n days
    ///
    /// Returns the number of weeks moved to trash
    pub fn enforce_retention(&self, retention_days: Option<u32>) -> Result<u32, FileError> {
        let retention_days = match retention_days {
            None => return Ok(0), // Keep forever
            Some(days) => days,
        };

        let cutoff_date = Utc::now() - Duration::days(retention_days as i64);
        let archived_weeks = self.get_archived_weeks();
        let mut deleted_count = 0;

        for week in archived_weeks {
            let week_path = self.week_archive_path(&week);
            
            // Check if the week is old enough to delete
            if let Ok(metadata) = fs::metadata(&week_path) {
                if let Ok(modified) = metadata.modified() {
                    let modified_datetime: chrono::DateTime<Utc> = modified.into();
                    
                    if modified_datetime < cutoff_date {
                        // Move to system trash
                        trash::delete(&week_path).map_err(|e| FileError::TrashFailed {
                            path: week_path.clone(),
                            source: e,
                        })?;
                        deleted_count += 1;
                    }
                }
            }
        }

        Ok(deleted_count)
    }

    /// Check if there are superseded files for a given week
    pub fn has_superseded_files(&self, week: &WeekIdentifier) -> bool {
        let path = self.superseded_path(week);
        path.exists() && fs::read_dir(&path).map(|rd| rd.count() > 0).unwrap_or(false)
    }

    /// Get list of superseded files for a week
    pub fn get_superseded_files(&self, week: &WeekIdentifier) -> Vec<PathBuf> {
        let path = self.superseded_path(week);
        
        if !path.exists() {
            return Vec::new();
        }

        fs::read_dir(&path)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Parse a directory name in format "YYYY-WNN" to WeekIdentifier
fn parse_week_dir_name(name: &str) -> Option<WeekIdentifier> {
    let parts: Vec<&str> = name.split("-W").collect();
    if parts.len() != 2 {
        return None;
    }

    let year: i32 = parts[0].parse().ok()?;
    let week: u32 = parts[1].parse().ok()?;

    if week >= 1 && week <= 53 {
        Some(WeekIdentifier::new(year, week))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> (TempDir, FileRetentionService) {
        let temp_dir = TempDir::new().unwrap();
        let service = FileRetentionService::new(temp_dir.path().to_path_buf());
        (temp_dir, service)
    }

    #[test]
    fn test_archive_dir_path() {
        let (temp_dir, service) = setup_test_dir();
        let expected = temp_dir.path().join(".archive");
        assert_eq!(service.archive_dir(), expected);
    }

    #[test]
    fn test_week_archive_path() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);
        let expected = temp_dir.path().join(".archive").join("2026-W04");
        assert_eq!(service.week_archive_path(&week), expected);
    }

    #[test]
    fn test_superseded_path() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);
        let expected = temp_dir.path().join(".archive").join("2026-W04").join(".superseded");
        assert_eq!(service.superseded_path(&week), expected);
    }

    #[test]
    fn test_parse_week_dir_name_valid() {
        assert_eq!(
            parse_week_dir_name("2026-W04"),
            Some(WeekIdentifier::new(2026, 4))
        );
        assert_eq!(
            parse_week_dir_name("2025-W52"),
            Some(WeekIdentifier::new(2025, 52))
        );
        assert_eq!(
            parse_week_dir_name("2024-W01"),
            Some(WeekIdentifier::new(2024, 1))
        );
    }

    #[test]
    fn test_parse_week_dir_name_invalid() {
        assert!(parse_week_dir_name("invalid").is_none());
        assert!(parse_week_dir_name("2026-04").is_none());
        assert!(parse_week_dir_name("2026-W00").is_none()); // Week 0 invalid
        assert!(parse_week_dir_name("2026-W54").is_none()); // Week 54 invalid
        assert!(parse_week_dir_name("abc-W04").is_none());
    }

    #[test]
    fn test_archive_file() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create a test file
        let test_file = temp_dir.path().join("test_file.zip");
        fs::write(&test_file, b"test content").unwrap();
        assert!(test_file.exists());

        // Archive it
        let archived_path = service.archive_file(&test_file, &week).unwrap();
        
        // Verify
        assert!(!test_file.exists()); // Original should be gone
        assert!(archived_path.exists()); // Archived should exist
        assert_eq!(
            archived_path,
            temp_dir.path().join(".archive/2026-W04/test_file.zip")
        );
    }

    #[test]
    fn test_archive_superseded() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create a test file
        let test_file = temp_dir.path().join("old_version.zip");
        fs::write(&test_file, b"old content").unwrap();

        // Archive as superseded
        let archived_path = service.archive_superseded(&test_file, &week).unwrap();

        assert!(!test_file.exists());
        assert!(archived_path.exists());
        assert_eq!(
            archived_path,
            temp_dir.path().join(".archive/2026-W04/.superseded/old_version.zip")
        );
    }

    #[test]
    fn test_get_archived_weeks_empty() {
        let (_temp_dir, service) = setup_test_dir();
        assert!(service.get_archived_weeks().is_empty());
    }

    #[test]
    fn test_get_archived_weeks() {
        let (temp_dir, service) = setup_test_dir();

        // Create some archive directories
        let archive = temp_dir.path().join(".archive");
        fs::create_dir_all(archive.join("2026-W03")).unwrap();
        fs::create_dir_all(archive.join("2026-W04")).unwrap();
        fs::create_dir_all(archive.join("2025-W52")).unwrap();
        // Invalid directory should be ignored
        fs::create_dir_all(archive.join("invalid-dir")).unwrap();

        let weeks = service.get_archived_weeks();
        assert_eq!(weeks.len(), 3);
        assert!(weeks.contains(&WeekIdentifier::new(2026, 3)));
        assert!(weeks.contains(&WeekIdentifier::new(2026, 4)));
        assert!(weeks.contains(&WeekIdentifier::new(2025, 52)));
    }

    #[test]
    fn test_has_superseded_files_false() {
        let (_temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);
        assert!(!service.has_superseded_files(&week));
    }

    #[test]
    fn test_has_superseded_files_true() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create superseded file
        let superseded_dir = temp_dir.path().join(".archive/2026-W04/.superseded");
        fs::create_dir_all(&superseded_dir).unwrap();
        fs::write(superseded_dir.join("old_file.zip"), b"old").unwrap();

        assert!(service.has_superseded_files(&week));
    }

    #[test]
    fn test_get_superseded_files() {
        let (temp_dir, service) = setup_test_dir();
        let week = WeekIdentifier::new(2026, 4);

        // Create superseded files
        let superseded_dir = temp_dir.path().join(".archive/2026-W04/.superseded");
        fs::create_dir_all(&superseded_dir).unwrap();
        fs::write(superseded_dir.join("old_v1.zip"), b"v1").unwrap();
        fs::write(superseded_dir.join("old_v2.zip"), b"v2").unwrap();

        let files = service.get_superseded_files(&week);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_retention_keep_forever() {
        let (_temp_dir, service) = setup_test_dir();
        let result = service.enforce_retention(None).unwrap();
        assert_eq!(result, 0);
    }
}
