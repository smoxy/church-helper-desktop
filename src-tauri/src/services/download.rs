//! Download service
//!
//! Handles downloading resources, creating URL shortcuts, and calculating integrity hashes.

use crate::error::DownloadError;
use crate::models::Resource;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tauri::AppHandle;

// Download status constants
pub const STATUS_RUNNING: u8 = 0;
pub const STATUS_PAUSED: u8 = 1;
pub const STATUS_CANCELLED: u8 = 2;

/// Service for downloading resources
pub struct DownloadService {
    client: reqwest::Client,
}

impl DownloadService {
    /// Create a new DownloadService with default client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Create a new DownloadService with custom client
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Check if a resource file already exists
    pub fn check_file_exists(resource: &Resource, work_dir: &Path) -> bool {
        let week_dir = resource.week().as_dir_name();
        let dest_dir = work_dir.join(week_dir);
        
        let filename = extract_filename_from_url(&resource.download_url)
            .unwrap_or_else(|| sanitize_filename(&resource.title));
            
        let dest_path = dest_dir.join(filename);
        dest_path.exists()
    }

    /// Download a resource to the destination directory
    ///
    /// Returns the path to the downloaded file and its SHA-256 hash.
    /// For YouTube URLs, creates a shortcut and returns a placeholder hash.
    pub async fn download_resource(
        &self,
        resource: &Resource,
        dest_dir: &Path,
        app: Option<&AppHandle>,
        signal: Option<Arc<AtomicU8>>,
    ) -> Result<(PathBuf, String), DownloadError> {
        if resource.is_youtube() {
            let path = self.create_youtube_shortcut(resource, dest_dir)?;
            Ok((path, "youtube-shortcut".to_string()))
        } else {
            self.download_file(resource, dest_dir, app, signal).await
        }
    }

    /// Download a regular file with resume capability and hash calculation
    async fn download_file(
        &self,
        resource: &Resource,
        dest_dir: &Path,
        app: Option<&AppHandle>,
        signal: Option<Arc<AtomicU8>>,
    ) -> Result<(PathBuf, String), DownloadError> {
        use futures_util::StreamExt;
        use tauri::Emitter;

        tracing::debug!("Starting download_file for resource: {} ({})", resource.title, resource.download_url);

        // Extract filename
        let filename = extract_filename_from_url(&resource.download_url)
            .unwrap_or_else(|| sanitize_filename(&resource.title));

        let dest_path = dest_dir.join(&filename);
        let part_path = dest_dir.join(format!("{}.part", filename));

        tracing::debug!("Destination path: {:?}", dest_path);

        // Check for existing partial download
        let mut resume_offset = 0;
        if part_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&part_path) {
                resume_offset = metadata.len();
            }
        }

        // Build request
        let mut request = self.client.get(&resource.download_url);
        if resume_offset > 0 {
            request = request.header("Range", format!("bytes={}-", resume_offset));
        }

        let response = request.send().await?;
        let status = response.status();
        tracing::debug!("Download response status: {} for {}", status, resource.title);

        // If server doesn't support range (returns 200 instead of 206), we start over
        let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
        if !is_partial && resume_offset > 0 {
            // Server ignored range, restart download
            resume_offset = 0;
            // Truncate file if it existed
            if let Ok(file) = std::fs::File::create(&part_path) {
                let _ = file.set_len(0);
            }
        }

        let content_length = response.content_length().map(|len| len + resume_offset);

        // Open file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resume_offset > 0 && is_partial)
            .truncate(resume_offset == 0 || !is_partial) // Truncate if new download
            .open(&part_path)
            .map_err(|e| DownloadError::WriteError {
                path: part_path.clone(),
                source: e,
            })?;

        let mut stream = response.bytes_stream();
        let mut downloaded = resume_offset;
        tracing::debug!("Starting download stream for {} (total size: {:?})", resource.title, content_length);

        while let Some(item) = stream.next().await {
            // Check cancellation signal
            if let Some(sig) = &signal {
                let status = sig.load(Ordering::Relaxed);
                if status != STATUS_RUNNING {
                    if status == STATUS_CANCELLED {
                        // Attempt to delete partial file
                        let _ = std::fs::remove_file(&part_path);
                    }
                    return Err(DownloadError::Cancelled);
                }
            }

            let chunk = item?;
            file.write_all(&chunk).map_err(|e| DownloadError::WriteError {
                path: part_path.clone(),
                source: e,
            })?;

            downloaded += chunk.len() as u64;

            if let Some(app) = app {
                if let Some(total) = content_length {
                    let progress = ((downloaded as f64 / total as f64) * 100.0) as u8;
                    let _ = app.emit(
                        "download-progress",
                        serde_json::json!({
                            "id": resource.id,
                            "progress": progress,
                            "current_bytes": downloaded,
                            "total_bytes": total
                        }),
                    );
                }
            }
        }

        tracing::debug!("Download stream complete for {}, renaming .part file", resource.title);

        // Rename .part file upon success
        std::fs::rename(&part_path, &dest_path).map_err(|e| DownloadError::WriteError {
            path: dest_path.clone(),
            source: e,
        })?;

        // Calculate hash of the completed file
        let hash = calculate_file_hash(&dest_path).map_err(|e| DownloadError::WriteError {
            path: dest_path.clone(),
            source: e,
        })?;

        Ok((dest_path, hash))
    }

    /// Create a platform-specific URL shortcut for YouTube links
    fn create_youtube_shortcut(
        &self,
        resource: &Resource,
        dest_dir: &Path,
    ) -> Result<PathBuf, DownloadError> {
        let safe_name = sanitize_filename(&resource.title);

        #[cfg(target_os = "windows")]
        let (filename, content) = create_windows_url_shortcut(&safe_name, &resource.download_url);

        #[cfg(target_os = "macos")]
        let (filename, content) = create_macos_webloc_shortcut(&safe_name, &resource.download_url);

        #[cfg(target_os = "linux")]
        let (filename, content) =
            create_linux_desktop_shortcut(&safe_name, &resource.download_url);

        let dest_path = dest_dir.join(&filename);

        let mut file =
            std::fs::File::create(&dest_path).map_err(DownloadError::ShortcutCreationFailed)?;

        file.write_all(content.as_bytes())
            .map_err(DownloadError::ShortcutCreationFailed)?;

        Ok(dest_path)
    }
}

/// Calculate SHA-256 hash of a file
fn calculate_file_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

impl Default for DownloadService {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a Windows .url shortcut
#[cfg(target_os = "windows")]
fn create_windows_url_shortcut(name: &str, url: &str) -> (String, String) {
    let filename = format!("{}.url", name);
    let content = format!("[InternetShortcut]\r\nURL={}\r\n", url);
    (filename, content)
}

/// Create a macOS .webloc shortcut
#[cfg(target_os = "macos")]
fn create_macos_webloc_shortcut(name: &str, url: &str) -> (String, String) {
    let filename = format!("{}.webloc", name);
    // Use a simpler format that macOS will accept
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>URL</key>
    <string>{}</string>
</dict>
</plist>"#,
        url
    );
    (filename, content)
}

/// Create a Linux .desktop shortcut
#[cfg(target_os = "linux")]
fn create_linux_desktop_shortcut(name: &str, url: &str) -> (String, String) {
    let filename = format!("{}.desktop", name);
    let content = format!(
        "[Desktop Entry]\nType=Link\nName={}\nURL={}\nIcon=video-x-generic\n",
        name, url
    );
    (filename, content)
}

// Fallback for other platforms (primarily for testing)
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn create_linux_desktop_shortcut(name: &str, url: &str) -> (String, String) {
    let filename = format!("{}.url", name);
    let content = format!("[InternetShortcut]\nURL={}\n", url);
    (filename, content)
}

/// Extract filename from URL
pub(crate) fn extract_filename_from_url(url: &str) -> Option<String> {
    url.split('/')
        .last()
        .filter(|s| !s.is_empty() && s.contains('.'))
        .map(|s| {
            // Remove query parameters
            s.split('?').next().unwrap_or(s).to_string()
        })
}

/// Sanitize a string to be a valid filename
pub(crate) fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_filename_from_url_valid() {
        assert_eq!(
            extract_filename_from_url("https://example.com/files/document.pdf"),
            Some("document.pdf".to_string())
        );
        assert_eq!(
            extract_filename_from_url("https://example.com/file.zip?token=abc"),
            Some("file.zip".to_string())
        );
    }

    #[test]
    fn test_extract_filename_from_url_invalid() {
        assert!(extract_filename_from_url("https://example.com/").is_none());
        assert!(extract_filename_from_url("https://example.com/folder").is_none());
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Normal Name"), "Normal Name");
        assert_eq!(sanitize_filename("File/Name"), "File_Name");
        assert_eq!(sanitize_filename("A:B:C"), "A_B_C");
        assert_eq!(sanitize_filename("Test<>Name"), "Test__Name");
        assert_eq!(sanitize_filename("File*?|Name"), "File___Name");
    }

    #[test]
    fn test_sanitize_filename_trims_whitespace() {
        assert_eq!(sanitize_filename("  Test  "), "Test");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_linux_desktop_shortcut_format() {
        let (filename, content) =
            create_linux_desktop_shortcut("Test Video", "https://youtube.com/watch?v=abc123");

        assert_eq!(filename, "Test Video.desktop");
        assert!(content.contains("[Desktop Entry]"));
        assert!(content.contains("Type=Link"));
        assert!(content.contains("Name=Test Video"));
        assert!(content.contains("URL=https://youtube.com/watch?v=abc123"));
    }

    #[test]
    fn test_download_service_default() {
        let service = DownloadService::default();
        // Just verify it creates without panicking
        assert!(std::mem::size_of_val(&service) > 0);
    }
}
