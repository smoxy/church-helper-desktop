//! Error types for Church Helper Desktop

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during file operations
#[derive(Debug, Error)]
pub enum FileError {
    #[error("Work directory not configured")]
    WorkDirectoryNotSet,

    #[error("Work directory does not exist: {0}")]
    WorkDirectoryNotFound(PathBuf),

    #[error("Failed to create directory: {path}: {source}")]
    CreateDirectoryFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to move file from {from} to {to}: {source}")]
    MoveFileFailed {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to delete file {path}: {source}")]
    DeleteFileFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to move to trash: {path}: {source}")]
    TrashFailed {
        path: PathBuf,
        #[source]
        source: trash::Error,
    },
}

/// Errors that can occur during downloads
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Failed to write file: {path}: {source}")]
    WriteError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to create URL shortcut: {0}")]
    ShortcutCreationFailed(std::io::Error),

    #[error("Download cancelled")]
    Cancelled,
}

/// Errors that can occur during polling
#[derive(Debug, Error)]
pub enum PollingError {
    #[error("API request failed: {0}")]
    ApiError(#[from] reqwest::Error),

    #[error("Failed to parse API response: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Polling is not enabled")]
    PollingDisabled,
}

/// Errors from configuration operations
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load configuration: {0}")]
    LoadFailed(String),

    #[error("Failed to save configuration: {0}")]
    SaveFailed(String),

    #[error("Invalid configuration: {0}")]
    ValidationFailed(String),
}

/// Unified error type for Tauri commands
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    File(#[from] FileError),

    #[error(transparent)]
    Download(#[from] DownloadError),

    #[error(transparent)]
    Polling(#[from] PollingError),

    #[error(transparent)]
    Config(#[from] ConfigError),
}

// Implement Serialize for AppError to work with Tauri commands
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_error_display() {
        let err = FileError::WorkDirectoryNotSet;
        assert_eq!(err.to_string(), "Work directory not configured");
    }

    #[test]
    fn test_download_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = DownloadError::WriteError {
            path: PathBuf::from("/test/file.zip"),
            source: io_err,
        };
        assert!(err.to_string().contains("/test/file.zip"));
    }

    #[test]
    fn test_app_error_serialization() {
        let err = AppError::File(FileError::WorkDirectoryNotSet);
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("Work directory not configured"));
    }
}
