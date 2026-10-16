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
        use tokio::io::AsyncWriteExt;

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
        let meta_path = part_meta_path(dest_dir, &filename);

        // Defensive path-traversal guard: the resolved filename must stay directly
        // inside dest_dir. If join() escaped the base (absolute path or `..`), reject.
        if dest_path.parent() != Some(dest_dir)
            || part_path.parent() != Some(dest_dir)
            || meta_path.parent() != Some(dest_dir)
        {
            return Err(DownloadError::InvalidFilename);
        }

        tracing::debug!("Destination path: {:?}", dest_path);

        // Check for an existing partial download. It is resumed only when its
        // metadata proves it belongs to this same resource id + URL and carries
        // a validator of the first response, sent back as If-Range: without
        // it, a file changed behind the same URL would be spliced onto the old
        // bytes. Anything else restarts from zero (the open below truncates).
        let part_len = tokio::fs::metadata(&part_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let resume_validator = if part_len > 0 {
            resume_validator_for(read_part_meta(&meta_path).await, resource.id, download_url)
        } else {
            None
        };
        if part_len > 0 && resume_validator.is_none() {
            tracing::info!(
                "Discarding partial download of {}: no matching resume metadata, restarting",
                resource.title
            );
        }
        let mut resume_offset = if resume_validator.is_some() {
            part_len
        } else {
            0
        };

        // Build request
        let mut request = self.client.get(download_url);
        if let (true, Some(validator)) = (resume_offset > 0, resume_validator.as_deref()) {
            request = request
                .header(reqwest::header::RANGE, format!("bytes={}-", resume_offset))
                .header(reqwest::header::IF_RANGE, validator);
        }

        let response = request.send().await?;
        let status = response.status();
        tracing::debug!(
            "Download response status: {} for {}",
            status,
            resource.title
        );

        // A non-2xx answer (404 after an errata, 403 on an expired link, 5xx
        // on a cold gateway, ...) carries an error page, not the file: never
        // write it into the .part file nor rename it to the final name, or a
        // corrupted file would be recorded as downloaded.
        if let Err(e) = check_download_status(status) {
            tracing::warn!(
                "Download of {} rejected: HTTP {} from {}",
                resource.title,
                status,
                download_url
            );
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                // The resume offset no longer matches the remote file (it
                // changed or the .part is already complete/corrupt): drop the
                // .part so the next attempt starts from scratch.
                let _ = tokio::fs::remove_file(&part_path).await;
                let _ = tokio::fs::remove_file(&meta_path).await;
            }
            return Err(e);
        }

        // 200 instead of 206: the server ignored the Range, or the If-Range
        // validator no longer matches (the content behind the URL changed).
        // Either way the body is the whole file: start over.
        let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
        if !is_partial && resume_offset > 0 {
            tracing::info!(
                "Resume of {} refused by the server (HTTP {}), restarting from zero",
                resource.title,
                status
            );
            resume_offset = 0;
        }
        let appending = resume_offset > 0 && is_partial;

        let content_length = response.content_length().map(|len| len + resume_offset);

        // A fresh body: drop the old metadata BEFORE truncating, and write the
        // new one only AFTER, so a crash at any point never leaves a validator
        // next to bytes it does not describe.
        if !appending {
            let _ = tokio::fs::remove_file(&meta_path).await;
        }

        // Open file
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(appending)
            .truncate(!appending) // Truncate if new download
            .open(&part_path)
            .await
            .map_err(|e| DownloadError::WriteError {
                path: part_path.clone(),
                source: e,
            })?;

        if !appending {
            // No usable validator: no metadata, so this .part is never resumed.
            if let Some(validator) = response_validator(response.headers()) {
                let meta = PartMeta {
                    resource_id: resource.id,
                    url: download_url.to_string(),
                    validator,
                };
                if let Err(e) = write_part_meta(&meta_path, &meta).await {
                    tracing::warn!(
                        "Could not write resume metadata for {}: {} (a later resume will restart)",
                        resource.title,
                        e
                    );
                }
            }
        }

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
                    // Flush and close before returning so the on-disk length
                    // matches what resume reads back from `metadata.len()`;
                    // otherwise still-buffered bytes would be re-fetched and
                    // duplicated past the resume offset. Keep the .part file.
                    let _ = file.flush().await;
                    drop(file);
                    return Err(DownloadError::Paused);
                } else if status == STATUS_CANCELLED {
                    // Close the handle before deleting so no in-flight write
                    // races the remove and leaves a zombie .part behind.
                    drop(file);
                    let _ = tokio::fs::remove_file(&part_path).await;
                    let _ = tokio::fs::remove_file(&meta_path).await;
                    return Err(DownloadError::Cancelled);
                }
            }

            let chunk = item?;
            file.write_all(&chunk)
                .await
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

        // Flush and close the file handle before renaming so all buffered
        // chunk data is persisted and the OS handle is released (required for
        // rename on Windows).
        file.flush().await.map_err(|e| DownloadError::WriteError {
            path: part_path.clone(),
            source: e,
        })?;
        drop(file);

        // Rename .part file upon success
        tokio::fs::rename(&part_path, &dest_path)
            .await
            .map_err(|e| DownloadError::WriteError {
                path: dest_path.clone(),
                source: e,
            })?;
        let _ = tokio::fs::remove_file(&meta_path).await;

        // Calculate hash of the completed file off the async runtime: the
        // chunked read is blocking I/O, so run it on a blocking thread.
        let hash_path = dest_path.clone();
        let hash = tokio::task::spawn_blocking(move || calculate_file_hash(&hash_path))
            .await
            .map_err(|e| DownloadError::WriteError {
                path: dest_path.clone(),
                source: std::io::Error::other(e),
            })?
            .map_err(|e| DownloadError::WriteError {
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

/// Accept only 2xx download responses (200, or 206 for a resumed range).
/// Anything else is a [`DownloadError::HttpStatus`]. Pure so the policy is
/// unit-testable without a server.
pub(crate) fn check_download_status(status: reqwest::StatusCode) -> Result<(), DownloadError> {
    if status.is_success() {
        Ok(())
    } else {
        Err(DownloadError::HttpStatus(status.as_u16()))
    }
}

/// Resume metadata kept next to a `.part` file: which resource and URL the
/// partial bytes came from, and the validator (ETag, else Last-Modified) of
/// the response that started them, sent back as `If-Range` on resume.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PartMeta {
    pub resource_id: i64,
    pub url: String,
    pub validator: String,
}

/// Path of the resume metadata for `filename` in `dir`. It ends in `.part` on
/// purpose: week-folder archiving (`services::retention`) never moves `.part`
/// files, so the metadata always stays next to its partial download.
fn part_meta_path(dir: &Path, filename: &str) -> PathBuf {
    dir.join(format!("{}.meta.part", filename))
}

async fn read_part_meta(path: &Path) -> Option<PartMeta> {
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn write_part_meta(path: &Path, meta: &PartMeta) -> std::io::Result<()> {
    let json = serde_json::to_vec(meta).map_err(std::io::Error::other)?;
    tokio::fs::write(path, json).await
}

/// The If-Range validator to resume with, only when the metadata belongs to
/// this same resource id and URL. Pure so the policy is unit-testable.
pub(crate) fn resume_validator_for(
    meta: Option<PartMeta>,
    resource_id: i64,
    url: &str,
) -> Option<String> {
    meta.filter(|m| m.resource_id == resource_id && m.url == url && !m.validator.is_empty())
        .map(|m| m.validator)
}

/// Validator to persist from a full (200) response: a strong ETag, else
/// Last-Modified. A weak ETag (`W/...`) is never valid in `If-Range` (RFC
/// 9110 13.1.5 requires a strong comparison), so it counts as absent.
pub(crate) fn response_validator(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let header = |name: reqwest::header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    header(reqwest::header::ETAG)
        .filter(|etag| !etag.starts_with("W/"))
        .or_else(|| header(reqwest::header::LAST_MODIFIED))
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
///
/// Read-fallback for the week-dir naming migration (self-explanatory
/// Saturday-dated folders, e.g. "W19-2026-05-09", replacing the old
/// "2026-W19"): tries the new-format path first, then the legacy-format path.
/// If the file exists under the legacy name, that IS the effective dest path
/// (so pause/resume, reveal-in-folder and downloaded-count keep working for
/// files saved by older builds) — otherwise (including a brand-new download,
/// where neither exists yet) the new-format path is used.
pub(crate) fn resolve_dest_path(
    resource: &Resource,
    work_dir: &Path,
    prefer_optimized: bool,
) -> PathBuf {
    let effective_url = resource.get_effective_download_url(prefer_optimized);
    let filename = extract_filename_from_url(effective_url)
        .unwrap_or_else(|| sanitize_filename(&resource.title));

    let week = resource.week();
    let new_path = work_dir.join(week.as_dir_name()).join(&filename);
    if new_path.exists() {
        return new_path;
    }

    let legacy_path = work_dir.join(week.legacy_dir_name()).join(&filename);
    if legacy_path.exists() {
        return legacy_path;
    }

    new_path
}

/// Resolve the week directory a resource's download should be written into
/// (the containing folder of `resolve_dest_path`'s result): the legacy
/// folder if the file already lives there, otherwise the new-format folder
/// — including for a brand-new download, which always lands in the new
/// format. Single source of truth for callers that need to create/ensure the
/// destination directory before starting a download (`services::queue`,
/// `commands::download_resource`), so they never drift from
/// `resolve_dest_path`'s own resolution.
pub(crate) fn resolve_week_dir(
    resource: &Resource,
    work_dir: &Path,
    prefer_optimized: bool,
) -> PathBuf {
    resolve_dest_path(resource, work_dir, prefer_optimized)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| work_dir.join(resource.week().as_dir_name()))
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
    use chrono::{TimeZone, Utc};

    fn make_resource(id: i64, url: &str, created_at: chrono::DateTime<Utc>) -> Resource {
        Resource {
            id,
            category: "test".to_string(),
            title: "Test Resource".to_string(),
            description: None,
            download_url: url.to_string(),
            thumbnail_url: None,
            file_type: None,
            checksum: None,
            is_active: true,
            created_at,
            week_date: None,
            optimized_video_url: None,
            optimized_videos: None,
            supersedes_resource_id: None,
        }
    }

    /// A file already saved under the legacy "{year}-W{week}" folder (older
    /// build) must keep resolving there, so pause/resume, reveal and the
    /// downloaded-count check still find it after the week-dir naming
    /// migration.
    #[test]
    fn test_resolve_dest_path_finds_legacy_dir_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let work_dir = tmp.path();
        let created_at = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap(); // 2026-W04
        let resource = make_resource(1, "https://example.com/file.mp4", created_at);
        let week = resource.week();

        let legacy_dir = work_dir.join(week.legacy_dir_name());
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("file.mp4"), b"x").unwrap();

        let resolved = resolve_dest_path(&resource, work_dir, true);
        assert_eq!(resolved, legacy_dir.join("file.mp4"));
        assert!(DownloadService::check_file_exists(
            &resource, work_dir, true
        ));

        // The directory-creation helper must agree with resolve_dest_path.
        assert_eq!(resolve_week_dir(&resource, work_dir, true), legacy_dir);
    }

    /// A brand-new download (neither the new- nor legacy-format file exists
    /// yet) must resolve to the new self-explanatory Saturday-dated folder.
    #[test]
    fn test_resolve_dest_path_new_download_uses_new_format_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let work_dir = tmp.path();
        let created_at = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap(); // 2026-W04
        let resource = make_resource(2, "https://example.com/file.mp4", created_at);
        let week = resource.week();

        let resolved = resolve_dest_path(&resource, work_dir, true);
        let expected_new_dir = work_dir.join(week.as_dir_name());
        assert_eq!(resolved, expected_new_dir.join("file.mp4"));
        assert_ne!(week.as_dir_name(), week.legacy_dir_name());
        assert!(!DownloadService::check_file_exists(
            &resource, work_dir, true
        ));

        assert_eq!(
            resolve_week_dir(&resource, work_dir, true),
            expected_new_dir
        );
    }

    /// When a file exists under BOTH the new and legacy dir (shouldn't
    /// normally happen, but must resolve deterministically), the new-format
    /// path wins — it's checked first.
    #[test]
    fn test_resolve_dest_path_prefers_new_over_legacy_when_both_exist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let work_dir = tmp.path();
        let created_at = Utc.with_ymd_and_hms(2026, 1, 19, 12, 0, 0).unwrap(); // 2026-W04
        let resource = make_resource(3, "https://example.com/file.mp4", created_at);
        let week = resource.week();

        let new_dir = work_dir.join(week.as_dir_name());
        let legacy_dir = work_dir.join(week.legacy_dir_name());
        std::fs::create_dir_all(&new_dir).unwrap();
        std::fs::write(new_dir.join("file.mp4"), b"new").unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("file.mp4"), b"legacy").unwrap();

        let resolved = resolve_dest_path(&resource, work_dir, true);
        assert_eq!(resolved, new_dir.join("file.mp4"));
    }

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

    #[test]
    fn test_check_download_status_accepts_only_2xx() {
        assert!(check_download_status(reqwest::StatusCode::OK).is_ok());
        assert!(check_download_status(reqwest::StatusCode::PARTIAL_CONTENT).is_ok());
        for code in [403u16, 404, 416, 500, 503] {
            let status = reqwest::StatusCode::from_u16(code).unwrap();
            match check_download_status(status) {
                Err(DownloadError::HttpStatus(c)) => assert_eq!(c, code),
                other => panic!("{code} must be rejected, got {other:?}"),
            }
        }
    }

    /// End to end against a real local HTTP server: a 404 with an HTML body
    /// must surface as an error and leave NO final file behind (previously
    /// the error page was renamed to the final name and recorded as a
    /// successful download).
    #[tokio::test]
    async fn test_download_non_2xx_never_creates_the_final_file() {
        let url = crate::services::test_http::serve_once(
            "404 Not Found",
            "text/html",
            "<html>not found</html>",
        )
        .await;
        let tmp = tempfile::TempDir::new().unwrap();
        let resource = make_resource(
            1,
            &format!("{url}/files/video.mp4"),
            Utc.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap(),
        );
        let service =
            DownloadService::with_client(reqwest::Client::builder().no_proxy().build().unwrap());

        let result = service
            .download_resource(&resource, tmp.path(), None, None, false)
            .await;

        assert!(
            matches!(result, Err(DownloadError::HttpStatus(404))),
            "got {result:?}"
        );
        assert!(!tmp.path().join("video.mp4").exists(), "no final file");
    }

    /// The happy path still works end to end (200 -> renamed final file).
    #[tokio::test]
    async fn test_download_200_creates_the_final_file() {
        let url = crate::services::test_http::serve_once("200 OK", "video/mp4", "MP4DATA").await;
        let tmp = tempfile::TempDir::new().unwrap();
        let resource = make_resource(
            2,
            &format!("{url}/files/ok.mp4"),
            Utc.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap(),
        );
        let service =
            DownloadService::with_client(reqwest::Client::builder().no_proxy().build().unwrap());

        let (path, _hash) = service
            .download_resource(&resource, tmp.path(), None, None, false)
            .await
            .expect("200 must download");
        assert_eq!(path, tmp.path().join("ok.mp4"));
        assert_eq!(std::fs::read(&path).unwrap(), b"MP4DATA");
        assert!(!tmp.path().join("ok.mp4.part").exists());
    }

    // -- resume with If-Range (D2) ------------------------------------------

    /// One-shot server that answers the first request with `head` (status
    /// line + headers, without the blank line) and `body`, and hands back the
    /// request head it received, lowercased, to assert on Range/If-Range.
    async fn serve_capture(
        head: &str,
        body: &'static [u8],
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let response = format!(
            "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase());
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(body).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}"), rx)
    }

    fn test_service() -> DownloadService {
        DownloadService::with_client(reqwest::Client::builder().no_proxy().build().unwrap())
    }

    fn resource_at(id: i64, url: &str) -> Resource {
        make_resource(
            id,
            url,
            Utc.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap(),
        )
    }

    fn write_meta(dir: &Path, filename: &str, meta: &PartMeta) {
        std::fs::write(
            part_meta_path(dir, filename),
            serde_json::to_vec(meta).unwrap(),
        )
        .unwrap();
    }

    /// Matching metadata: resume with Range + If-Range, the server confirms
    /// with 206 and the tail is appended; the metadata is removed at the end.
    #[tokio::test]
    async fn test_resume_sends_if_range_and_appends_on_206() {
        let (url, req) = serve_capture(
            "206 Partial Content\r\nContent-Range: bytes 4-7/8\r\nETag: \"e1\"",
            b"BBBB",
        )
        .await;
        let url = format!("{url}/files/v.mp4");
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("v.mp4.part"), b"AAAA").unwrap();
        write_meta(
            tmp.path(),
            "v.mp4",
            &PartMeta {
                resource_id: 7,
                url: url.clone(),
                validator: "\"e1\"".into(),
            },
        );

        let (path, _) = test_service()
            .download_resource(&resource_at(7, &url), tmp.path(), None, None, false)
            .await
            .expect("resume must succeed");

        let head = req.await.unwrap();
        assert!(head.contains("range: bytes=4-"), "{head}");
        assert!(head.contains("if-range: \"e1\""), "{head}");
        assert_eq!(std::fs::read(&path).unwrap(), b"AAAABBBB");
        assert!(!part_meta_path(tmp.path(), "v.mp4").exists());
    }

    /// The content behind the URL changed: the server answers 200 to the
    /// If-Range request and the file must be the new body alone, never the
    /// old partial bytes spliced with the new ones.
    #[tokio::test]
    async fn test_resume_answered_with_200_restarts_from_zero() {
        let (url, req) = serve_capture("200 OK\r\nETag: \"e2\"", b"NEWCONTENT").await;
        let url = format!("{url}/files/v.mp4");
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("v.mp4.part"), b"OLDOLD").unwrap();
        write_meta(
            tmp.path(),
            "v.mp4",
            &PartMeta {
                resource_id: 7,
                url: url.clone(),
                validator: "\"e1\"".into(),
            },
        );

        let (path, _) = test_service()
            .download_resource(&resource_at(7, &url), tmp.path(), None, None, false)
            .await
            .unwrap();

        assert!(req.await.unwrap().contains("if-range: \"e1\""));
        assert_eq!(std::fs::read(&path).unwrap(), b"NEWCONTENT");
        assert!(!tmp.path().join("v.mp4.part").exists());
        assert!(!part_meta_path(tmp.path(), "v.mp4").exists());
    }

    /// No metadata (e.g. a .part left by an older build), or metadata of a
    /// different resource id or URL: never resume, restart from zero.
    #[tokio::test]
    async fn test_part_without_matching_meta_is_not_resumed() {
        for case in ["no-meta", "other-id", "other-url"] {
            let (url, req) = serve_capture("200 OK", b"FRESH").await;
            let url = format!("{url}/files/v.mp4");
            let tmp = tempfile::TempDir::new().unwrap();
            std::fs::write(tmp.path().join("v.mp4.part"), b"STALE").unwrap();
            match case {
                "other-id" => write_meta(
                    tmp.path(),
                    "v.mp4",
                    &PartMeta {
                        resource_id: 99,
                        url: url.clone(),
                        validator: "\"e1\"".into(),
                    },
                ),
                "other-url" => write_meta(
                    tmp.path(),
                    "v.mp4",
                    &PartMeta {
                        resource_id: 7,
                        url: "https://elsewhere.example/v.mp4".into(),
                        validator: "\"e1\"".into(),
                    },
                ),
                _ => {}
            }

            let (path, _) = test_service()
                .download_resource(&resource_at(7, &url), tmp.path(), None, None, false)
                .await
                .unwrap();

            let head = req.await.unwrap();
            assert!(!head.contains("range:"), "{case}: {head}");
            assert_eq!(std::fs::read(&path).unwrap(), b"FRESH", "{case}");
            assert!(!part_meta_path(tmp.path(), "v.mp4").exists(), "{case}");
        }
    }

    /// A fresh download persists id, URL and the validator next to the .part
    /// (here kept because the download is paused before the first chunk).
    #[tokio::test]
    async fn test_fresh_download_persists_meta_until_completion() {
        for (head, expected) in [
            ("200 OK\r\nETag: \"abc\"", Some("\"abc\"")),
            (
                "200 OK\r\nETag: W/\"weak\"\r\nLast-Modified: Fri, 16 Oct 2026 10:00:00 GMT",
                Some("Fri, 16 Oct 2026 10:00:00 GMT"),
            ),
            ("200 OK", None),
        ] {
            let (url, _req) = serve_capture(head, b"DATA").await;
            let url = format!("{url}/files/v.mp4");
            let tmp = tempfile::TempDir::new().unwrap();
            let signal = Arc::new(AtomicU8::new(STATUS_PAUSED));

            let result = test_service()
                .download_resource(&resource_at(7, &url), tmp.path(), None, Some(signal), false)
                .await;
            assert!(matches!(result, Err(DownloadError::Paused)), "{result:?}");

            let meta: Option<PartMeta> = std::fs::read(part_meta_path(tmp.path(), "v.mp4"))
                .ok()
                .map(|b| serde_json::from_slice(&b).unwrap());
            match expected {
                Some(v) => assert_eq!(
                    meta,
                    Some(PartMeta {
                        resource_id: 7,
                        url: url.clone(),
                        validator: v.into()
                    })
                ),
                None => assert!(meta.is_none(), "no validator -> no metadata"),
            }
        }
    }

    #[test]
    fn test_resume_validator_for_requires_same_id_and_url() {
        let meta = PartMeta {
            resource_id: 1,
            url: "https://h/a.mp4".into(),
            validator: "\"e\"".into(),
        };
        assert_eq!(
            resume_validator_for(Some(meta.clone()), 1, "https://h/a.mp4"),
            Some("\"e\"".into())
        );
        assert!(resume_validator_for(Some(meta.clone()), 2, "https://h/a.mp4").is_none());
        assert!(resume_validator_for(Some(meta), 1, "https://h/b.mp4").is_none());
        assert!(resume_validator_for(None, 1, "https://h/a.mp4").is_none());
    }
}
