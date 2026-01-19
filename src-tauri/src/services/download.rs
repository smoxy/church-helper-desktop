//! Download service
//!
//! Handles downloading resources and creating URL shortcuts for YouTube links.

use crate::error::DownloadError;
use crate::models::Resource;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

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

    /// Download a resource to the destination directory
    ///
    /// For YouTube URLs, creates a platform-specific shortcut file instead.
    pub async fn download_resource(
        &self,
        resource: &Resource,
        dest_dir: &Path,
        _app: Option<&AppHandle>,
    ) -> Result<PathBuf, DownloadError> {
        if resource.is_youtube() {
            self.create_youtube_shortcut(resource, dest_dir)
        } else {
            self.download_file(resource, dest_dir).await
        }
    }

    /// Download a regular file
    async fn download_file(&self, resource: &Resource, dest_dir: &Path) -> Result<PathBuf, DownloadError> {
        let response = self.client.get(&resource.download_url).send().await?;
        
        // Extract filename from URL or use resource title
        let filename = extract_filename_from_url(&resource.download_url)
            .unwrap_or_else(|| sanitize_filename(&resource.title));
        
        let dest_path = dest_dir.join(&filename);

        // Stream to file
        let bytes = response.bytes().await?;
        
        std::fs::write(&dest_path, &bytes).map_err(|e| DownloadError::WriteError {
            path: dest_path.clone(),
            source: e,
        })?;

        Ok(dest_path)
    }

    /// Create a platform-specific URL shortcut for YouTube links
    fn create_youtube_shortcut(&self, resource: &Resource, dest_dir: &Path) -> Result<PathBuf, DownloadError> {
        let safe_name = sanitize_filename(&resource.title);
        
        #[cfg(target_os = "windows")]
        let (filename, content) = create_windows_url_shortcut(&safe_name, &resource.download_url);
        
        #[cfg(target_os = "macos")]
        let (filename, content) = create_macos_webloc_shortcut(&safe_name, &resource.download_url);
        
        #[cfg(target_os = "linux")]
        let (filename, content) = create_linux_desktop_shortcut(&safe_name, &resource.download_url);

        let dest_path = dest_dir.join(&filename);
        
        let mut file = std::fs::File::create(&dest_path)
            .map_err(DownloadError::ShortcutCreationFailed)?;
        
        file.write_all(content.as_bytes())
            .map_err(DownloadError::ShortcutCreationFailed)?;

        Ok(dest_path)
    }
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
    let content = format!(
        "[InternetShortcut]\r\nURL={}\r\n",
        url
    );
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
        let (filename, content) = create_linux_desktop_shortcut(
            "Test Video",
            "https://youtube.com/watch?v=abc123"
        );
        
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
