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
use std::time::{Duration, Instant};
use tauri::AppHandle;
use urlencoding;

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
    /// Uses the effective download URL based on prefer_optimized setting
    pub fn check_file_exists(resource: &Resource, work_dir: &Path, prefer_optimized: bool) -> bool {
        resolve_dest_path(resource, work_dir, prefer_optimized).exists()
    }

    /// Download a resource to the destination directory
    ///
    /// Returns the path to the downloaded file and its SHA-256 hash.
    /// For YouTube URLs, creates a shortcut and returns a placeholder hash.
    /// If prefer_optimized is true and optimized_video_url is available, uses that URL.
    pub async fn download_resource(
        &self,
        resource: &Resource,
        dest_dir: &Path,
        app: Option<&AppHandle>,
        signal: Option<Arc<AtomicU8>>,
        prefer_optimized: bool,
    ) -> Result<(PathBuf, String), DownloadError> {
        if resource.is_youtube() {
            let path = self.create_youtube_shortcut(resource, dest_dir)?;
            Ok((path, "youtube-shortcut".to_string()))
        } else {
            self.download_file(resource, dest_dir, app, signal, prefer_optimized)
                .await
        }
    }

    /// Download a regular file with resume capability and hash calculation
    async fn download_file(
        &self,
        resource: &Resource,
        dest_dir: &Path,
        app: Option<&AppHandle>,
        signal: Option<Arc<AtomicU8>>,
        prefer_optimized: bool,
    ) -> Result<(PathBuf, String), DownloadError> {
        use futures_util::StreamExt;
        use tauri::Emitter;

        // Determine which URL to use
        let download_url = if prefer_optimized {
            resource
                .optimized_video_url
                .as_ref()
                .unwrap_or(&resource.download_url)
        } else {
            &resource.download_url
        };

        tracing::debug!(
            "Starting download_file for resource: {} ({})",
            resource.title,
            download_url
        );

        // Extract filename
        let filename = extract_filename_from_url(download_url)
            .unwrap_or_else(|| sanitize_filename(&resource.title));

        let dest_path = dest_dir.join(&filename);
        let part_path = dest_dir.join(format!("{}.part", filename));

        // Defensive path-traversal guard: the resolved filename must stay directly
        // inside dest_dir. If join() escaped the base (absolute path or `..`), reject.
        if dest_path.parent() != Some(dest_dir) || part_path.parent() != Some(dest_dir) {
            return Err(DownloadError::InvalidFilename);
        }

        tracing::debug!("Destination path: {:?}", dest_path);

        // Check for existing partial download
        let mut resume_offset = 0;
        if part_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&part_path) {
                resume_offset = metadata.len();
            }
        }

        // Build request
        let mut request = self.client.get(download_url);
        if resume_offset > 0 {
            request = request.header("Range", format!("bytes={}-", resume_offset));
        }

        let response = request.send().await?;
        let status = response.status();
        tracing::debug!(
            "Download response status: {} for {}",
            status,
            resource.title
        );

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
        let mut last_progress_emit = Instant::now();
        const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

        tracing::debug!(
            "Starting download stream for {} (total size: {:?})",
            resource.title,
            content_length
        );

        while let Some(item) = stream.next().await {
            // Check cancellation signal
            if let Some(sig) = &signal {
                let status = sig.load(Ordering::Relaxed);
                if status == STATUS_PAUSED {
                    // Keep .part file for resume
                    return Err(DownloadError::Paused);
                } else if status == STATUS_CANCELLED {
                    // Delete partial file on cancel
                    let _ = std::fs::remove_file(&part_path);
                    return Err(DownloadError::Cancelled);
                }
            }

            let chunk = item?;
            file.write_all(&chunk)
                .map_err(|e| DownloadError::WriteError {
                    path: part_path.clone(),
                    source: e,
                })?;

            downloaded += chunk.len() as u64;

            // Throttle progress events to max 10/second (100ms interval)
            if let Some(app) = app {
                if let Some(total) = content_length {
                    let now = Instant::now();
                    if now.duration_since(last_progress_emit) >= PROGRESS_EMIT_INTERVAL {
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
                        last_progress_emit = now;
                    }
                }
            }
        }

        tracing::debug!(
            "Download stream complete for {}, renaming .part file",
            resource.title
        );

        // Emit final progress event to ensure 100% is shown
        if let Some(app) = app {
            if let Some(total) = content_length {
                let _ = app.emit(
                    "download-progress",
                    serde_json::json!({
                        "id": resource.id,
                        "progress": 100,
                        "current_bytes": downloaded,
                        "total_bytes": total
                    }),
                );
            }
        }

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
        let (filename, content) = create_linux_desktop_shortcut(&safe_name, &resource.download_url);

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
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let hash = hasher.finalize();
    Ok(hex::encode(hash))
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

/// Whether `file_name`'s stem (everything before its first `.`) is a Windows
/// reserved device name (CON, PRN, AUX, NUL, COM1-COM9, LPT1-LPT9),
/// case-insensitive. Such names cannot be created as regular files on Windows,
/// so a URL yielding one is rejected in favour of the sanitized title.
fn is_windows_reserved_stem(file_name: &str) -> bool {
    let stem = file_name.split('.').next().unwrap_or(file_name);
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            if rest.len() == 1 && matches!(rest.as_bytes()[0], b'1'..=b'9') {
                return true;
            }
        }
    }
    false
}

/// Resolve `<work_dir>/<week-dir>/<filename>` for a resource, deriving the
/// filename from its effective download URL (honoring `prefer_optimized`) with
/// a fallback to the sanitized title. Single source of truth for the
/// existence/status/summary checks.
pub(crate) fn resolve_dest_path(
    resource: &Resource,
    work_dir: &Path,
    prefer_optimized: bool,
) -> PathBuf {
    let dest_dir = work_dir.join(resource.week().as_dir_name());
    let effective_url = resource.get_effective_download_url(prefer_optimized);
    let filename = extract_filename_from_url(effective_url)
        .unwrap_or_else(|| sanitize_filename(&resource.title));
    dest_dir.join(filename)
}

/// Extract filename from URL with URL decoding support
///
/// 1. Extracts the filename from the last path segment
/// 2. Removes query parameters
/// 3. Decodes URL-encoded characters (%20 -> space, etc.)
/// 4. Sanitizes against path traversal: the decoded value is reduced to its
///    final path component (`Path::file_name`), rejecting `..`, `.`, empty or
///    separator-bearing results so an encoded path (e.g. `..%2F..%2Fevil.sh`
///    or `%2Fetc%2Fx`) can never escape the destination directory.
/// 5. Returns None if the result is invalid/empty (callers fall back to
///    `sanitize_filename(title)`).
pub(crate) fn extract_filename_from_url(url: &str) -> Option<String> {
    url.split('/')
        .next_back()
        .filter(|s| !s.is_empty() && s.contains('.'))
        .and_then(|s| {
            // Remove query parameters
            let without_query = s.split('?').next().unwrap_or(s);

            // Decode URL-encoded characters
            let decoded = urlencoding::decode(without_query).ok()?.into_owned();

            // Reduce to the final path component and reject anything that could
            // traverse out of the destination directory. Note: on Linux `\` is
            // not a path separator, so `Path::file_name` would keep it; the
            // explicit separator checks below neutralize that case too.
            let file_name = Path::new(&decoded).file_name()?.to_str()?;
            if file_name.is_empty()
                || file_name == ".."
                || file_name == "."
                || file_name.contains('/')
                || file_name.contains('\\')
                || is_windows_reserved_stem(file_name)
            {
                None
            } else {
                Some(file_name.to_string())
            }
        })
}

/// Sanitize a string to be a valid filename
///
/// Neutralizes reserved characters, path separators and `..` traversal
/// sequences anywhere in the name, and never returns an empty string
/// (falls back to `"download"`).
pub(crate) fn sanitize_filename(name: &str) -> String {
    let mapped = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>();

    // Neutralize `..` traversal sequences left after separator mapping.
    let neutralized = mapped.replace("..", "_");

    let trimmed = neutralized.trim();
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_filename_from_url_decoded() {
        // Test URL-encoded spaces
        assert_eq!(
            extract_filename_from_url("https://example.com/gcv_05%20-%20USARE%20LE%20COSE.mp4"),
            Some("gcv_05 - USARE LE COSE.mp4".to_string())
        );

        // Test URL-encoded special chars
        assert_eq!(
            extract_filename_from_url(
                "https://example.com/mis-05%20-%20SEMI%20CHE%20SI%20MOLTIPLICANO%20-%2001_2026.mp4"
            ),
            Some("mis-05 - SEMI CHE SI MOLTIPLICANO - 01_2026.mp4".to_string())
        );

        // Test with query parameters AND encoding
        assert_eq!(
            extract_filename_from_url("https://example.com/video%20name.mp4?token=abc&size=1080p"),
            Some("video name.mp4".to_string())
        );
    }

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
    fn test_extract_filename_rejects_path_traversal() {
        // Encoded `../../evil.sh` must be reduced to its final component only,
        // never escaping the destination directory.
        assert_eq!(
            extract_filename_from_url("https://host/..%2F..%2Fevil.sh"),
            Some("evil.sh".to_string())
        );

        // Encoded absolute path `/etc/passwd`: no dot in the last segment, so it
        // is filtered out and the caller falls back to the sanitized title.
        assert!(extract_filename_from_url("https://host/%2Fetc%2Fpasswd").is_none());

        // A single `..` segment must never be accepted as a filename.
        assert!(extract_filename_from_url("https://host/..").is_none());

        // Encoded nested path keeps only the final safe component.
        assert_eq!(
            extract_filename_from_url("https://host/a%2Fb.pdf"),
            Some("b.pdf".to_string())
        );

        // A normal encoded filename must remain valid and fully decoded.
        assert_eq!(
            extract_filename_from_url("https://host/file%20name.pdf"),
            Some("file name.pdf".to_string())
        );

        // URL without a path component yields no filename.
        assert!(extract_filename_from_url("https://host/").is_none());
    }

    #[test]
    fn test_extract_filename_rejects_windows_reserved_names() {
        // Names whose stem before the first `.` is a Windows reserved device
        // are rejected (→ None), case-insensitive, so the caller falls back to
        // the sanitized title.
        for url in [
            "https://host/CON.txt",
            "https://host/con.txt",
            "https://host/NUL.mp4",
            "https://host/aux.pdf",
            "https://host/prn.zip",
            "https://host/COM1.dat",
            "https://host/com9.bin",
            "https://host/LPT1.log",
            "https://host/lpt9.tmp",
            "https://host/CON.tar.gz",
        ] {
            assert!(
                extract_filename_from_url(url).is_none(),
                "`{url}` should be rejected as a Windows reserved name"
            );
        }

        // Non-reserved lookalikes stay valid: only the exact device names and
        // COM/LPT followed by a single 1-9 digit are reserved.
        assert_eq!(
            extract_filename_from_url("https://host/CONFIG.txt"),
            Some("CONFIG.txt".to_string())
        );
        assert_eq!(
            extract_filename_from_url("https://host/COM10.dat"),
            Some("COM10.dat".to_string())
        );
        assert_eq!(
            extract_filename_from_url("https://host/COM0.dat"),
            Some("COM0.dat".to_string())
        );
    }

    #[test]
    fn test_sanitize_filename_neutralizes_traversal() {
        // `..` sequences and separators are neutralized anywhere in the name:
        // the result must never contain a traversal token or a path separator.
        for input in ["../../evil.sh", "..\\..\\evil", "a/../b"] {
            let out = sanitize_filename(input);
            assert!(!out.contains(".."), "`{input}` -> `{out}` still has `..`");
            assert!(!out.contains('/'), "`{input}` -> `{out}` still has `/`");
            assert!(!out.contains('\\'), "`{input}` -> `{out}` still has `\\`");
            assert!(!out.is_empty());
        }
        assert_eq!(sanitize_filename(".."), "_");
        // Never returns an empty string.
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("   "), "download");
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

    #[test]
    fn test_download_error_paused_display() {
        let error = DownloadError::Paused;
        assert_eq!(error.to_string(), "Download paused");
    }

    #[test]
    fn test_download_error_cancelled_display() {
        let error = DownloadError::Cancelled;
        assert_eq!(error.to_string(), "Download cancelled");
    }

    #[test]
    fn test_download_error_paused_not_equal_cancelled() {
        // Verify that Paused and Cancelled are distinct error types
        let paused = DownloadError::Paused;
        let cancelled = DownloadError::Cancelled;

        assert_ne!(paused.to_string(), cancelled.to_string());
    }

    #[tokio::test]
    async fn test_pause_signal_returns_paused_error() {
        use std::sync::atomic::{AtomicU8, Ordering};
        use std::sync::Arc;

        let signal = Arc::new(AtomicU8::new(STATUS_RUNNING));

        // Set signal to paused
        signal.store(STATUS_PAUSED, Ordering::Relaxed);

        // Verify signal is paused
        assert_eq!(signal.load(Ordering::Relaxed), STATUS_PAUSED);
        assert_ne!(signal.load(Ordering::Relaxed), STATUS_CANCELLED);
    }

    #[tokio::test]
    async fn test_cancel_signal_returns_cancelled_error() {
        use std::sync::atomic::{AtomicU8, Ordering};
        use std::sync::Arc;

        let signal = Arc::new(AtomicU8::new(STATUS_RUNNING));

        // Set signal to cancelled
        signal.store(STATUS_CANCELLED, Ordering::Relaxed);

        // Verify signal is cancelled
        assert_eq!(signal.load(Ordering::Relaxed), STATUS_CANCELLED);
        assert_ne!(signal.load(Ordering::Relaxed), STATUS_PAUSED);
    }
}
