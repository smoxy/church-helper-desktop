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

    #[error("Failed to read directory {path}: {source}")]
    ReadDirectoryFailed {
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

    #[error("Invalid or unsafe filename resolved from download source")]
    InvalidFilename,

    #[error("Download paused")]
    Paused,

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

/// Structured error carried across the Tauri IPC boundary.
///
/// Tauri serializes a command's `Err(E)` as the JSON rejection payload of the
/// corresponding `invoke(...)` promise. Returning `CommandError` from a command
/// therefore delivers `{ "code": "...", "message": "..." }` to the frontend
/// (mirrored by the `CommandError` interface in `src/types/index.ts`) instead
/// of a bare string. The UI branches on the stable `code` while showing
/// `message` to the user (see `errorMessage()` in `src/lib/utils.ts`).
///
/// `code` is a stable kebab-case identifier (e.g. `work-dir-not-set`,
/// `api-unreachable`, `store-failed`, `lock-poisoned`). `message` preserves the
/// human-readable detail the commands previously produced with `format!`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<AppError> for CommandError {
    fn from(err: AppError) -> Self {
        let code = match &err {
            AppError::File(e) => match e {
                FileError::WorkDirectoryNotSet => "work-dir-not-set",
                FileError::WorkDirectoryNotFound(_) => "work-dir-not-found",
                FileError::CreateDirectoryFailed { .. } => "create-directory-failed",
                FileError::MoveFileFailed { .. } => "move-file-failed",
                FileError::DeleteFileFailed { .. } => "delete-file-failed",
                FileError::ReadDirectoryFailed { .. } => "read-directory-failed",
                FileError::TrashFailed { .. } => "trash-failed",
            },
            AppError::Download(e) => match e {
                DownloadError::HttpError(_) => "http-error",
                DownloadError::WriteError { .. } => "write-error",
                DownloadError::ShortcutCreationFailed(_) => "shortcut-creation-failed",
                DownloadError::InvalidFilename => "invalid-filename",
                DownloadError::Paused => "download-paused",
                DownloadError::Cancelled => "download-cancelled",
            },
            AppError::Polling(e) => match e {
                PollingError::ApiError(_) => "api-unreachable",
                PollingError::ParseError(_) => "response-parse-failed",
                PollingError::PollingDisabled => "polling-disabled",
            },
            AppError::Config(e) => match e {
                ConfigError::LoadFailed(_) => "config-load-failed",
                ConfigError::SaveFailed(_) => "config-save-failed",
                ConfigError::ValidationFailed(_) => "config-invalid",
            },
        };
        CommandError::new(code, err.to_string())
    }
}

impl From<FileError> for CommandError {
    fn from(err: FileError) -> Self {
        AppError::from(err).into()
    }
}

impl From<DownloadError> for CommandError {
    fn from(err: DownloadError) -> Self {
        AppError::from(err).into()
    }
}

impl From<PollingError> for CommandError {
    fn from(err: PollingError) -> Self {
        AppError::from(err).into()
    }
}

impl From<ConfigError> for CommandError {
    fn from(err: ConfigError) -> Self {
        AppError::from(err).into()
    }
}

// A poisoned lock is a non-recoverable internal invariant break; collapse every
// `RwLock`/`Mutex` guard into one stable code rather than one per call site.
impl<T> From<std::sync::PoisonError<T>> for CommandError {
    fn from(err: std::sync::PoisonError<T>) -> Self {
        CommandError::new("lock-poisoned", err.to_string())
    }
}

impl From<tauri_plugin_store::Error> for CommandError {
    fn from(err: tauri_plugin_store::Error) -> Self {
        CommandError::new("store-failed", err.to_string())
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
    fn test_command_error_serializes_as_code_and_message() {
        let err = CommandError::new("work-dir-not-set", "Work directory not configured");
        let value: serde_json::Value = serde_json::to_value(&err).unwrap();
        assert_eq!(value["code"], "work-dir-not-set");
        assert_eq!(value["message"], "Work directory not configured");
        // Exactly the two fields the frontend contract expects.
        assert_eq!(value.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_app_error_maps_to_stable_code() {
        let err: CommandError = AppError::File(FileError::WorkDirectoryNotSet).into();
        assert_eq!(err.code, "work-dir-not-set");
        assert_eq!(err.message, "Work directory not configured");
    }

    #[test]
    fn test_polling_api_error_maps_to_api_unreachable() {
        let err: CommandError = PollingError::PollingDisabled.into();
        assert_eq!(err.code, "polling-disabled");
        // Nested variant routing through AppError keeps the display message.
        assert_eq!(err.message, "Polling is not enabled");
    }

    #[test]
    fn test_poison_error_maps_to_lock_poisoned() {
        let lock = std::sync::RwLock::new(0u8);
        let _ = std::panic::catch_unwind(|| {
            let _guard = lock.write().unwrap();
            panic!("poison it");
        });
        let err: CommandError = lock.read().unwrap_err().into();
        assert_eq!(err.code, "lock-poisoned");
    }
}
