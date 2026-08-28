//! Media pipeline: uploaded files are compressed with ffmpeg.
//!  * Images  -> WebP (quality 80, max 1920px) + 320px thumbnail
//!  * Videos  -> H.264 MP4 (CRF 26, max 1280px, faststart) + WebP poster frame
//!  * Audio   -> AAC container (m4a, 96 kbps)
//!  * Other   -> stored as-is
//!
//! The ffmpeg / ffprobe binaries are configured in the `.env` file
//! (`FFMPEG_PATH`, `FFPROBE_PATH`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct ProcessedMedia {
    pub kind: String, // image | video | audio | file
    pub mime_type: String,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i64>,
    pub stored_rel: String,
    pub thumb_rel: Option<String>,
    pub hash: String,
}

/// Read the first file field of a multipart request into a temp file.
/// Returns (temp_path, original_name, mime_type). Size is bounded by the
/// request body limit configured in the .env (MAX_UPLOAD_MB).
pub async fn save_first_file(
    multipart: &mut axum::extract::Multipart,
) -> AppResult<(PathBuf, String, String)> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("ملف غير صالح: {e}")))?
    {
        if field.file_name().is_none() {
            continue;
        }
        let name = sanitize_file_name(field.file_name().unwrap_or("file"));
        let mime = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("فشل قراءة الملف: {e}")))?;
        if data.is_empty() {
            return Err(AppError::BadRequest("الملف المرفوع فارغ.".into()));
        }
        let tmp = std::env::temp_dir().join(format!("meev-upload-{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, &data).map_err(|e| AppError::Internal(format!("tmp write: {e}")))?;
        return Ok((tmp, name, mime));
    }
    Err(AppError::BadRequest("لم يتم إرسال أي ملف.".into()))
}

/// Keep a file name safe for display only: strip control characters,
/// path separators and quotes. The real path on disk is always a UUID.
pub fn sanitize_file_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_control() || c == '/' || c == '\\' || c == '"' || c == '\'' || c == ':' {
                '_'
            } else {
                c
            }
        })
        .take(160)
        .collect();
    if out.trim().is_empty() {
        out = "file".to_string();
    }
    out
}

pub fn file_kind_from_mime(mime: &str) -> &'static str {
    match mime {
        m if m.starts_with("image/") => "image",
        m if m.starts_with("video/") => "video",
        m if m.starts_with("audio/") => "audio",
        _ => "file",
    }
}

fn shasum(path: &Path) -> AppResult<String> {
    let bytes = std::fs::read(path).map_err(|e| AppError::BadRequest(format!("غير قادر على قراءة الملف: {e}")))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn month_rel() -> String {
    let now = chrono::Utc::now();
    format!("{}/{:02}", now.format("%Y"), now.format("%m"))
}

pub async fn process_upload(
    cfg: &Config,
    tmp_path: &Path,
    original_name: &str,
    mime: &str,
    id: uuid::Uuid,
) -> AppResult<ProcessedMedia> {
    let kind = file_kind_from_mime(mime).to_string();
    let out_dir = cfg.upload_dir.join(month_rel());
    std::fs::create_dir_all(&out_dir).map_err(|e| AppError::Internal(format!("upload dir: {e}")))?;

    let ffmpeg = cfg.ffmpeg_path.to_string_lossy().to_string();
    let ffprobe = cfg.ffprobe_path.to_string_lossy().to_string();
    let timeout = cfg.media_timeout();

    let (stored_rel, mime_type, size, w, h, dur, thumb_rel, hash) = match kind.as_str() {
        "image" => {
            let out = out_dir.join(format!("{id}.webp"));
            let args = vec![
                "-y", "-i",
                tmp_path.to_str().unwrap_or_default(),
                "-vf", "scale='min(1920\\,iw)':-2",
                "-c:v", "libwebp",
                "-quality", "80",
                "-preset", "photo",
                out.to_str().unwrap_or_default(),
            ];
            let ok = run_ffmpeg(&ffmpeg, &args, timeout).await;
            let final_path = if ok {
                out
            } else {
                // Fallback to JPEG when libwebp is unavailable or webp fails.
                let jpg = out_dir.join(format!("{id}.jpg"));
                let jpg_ok = run_ffmpeg(
                    &ffmpeg,
                    &[
                        "-y", "-i",
                        tmp_path.to_str().unwrap_or_default(),
                        "-vf", "scale='min(1920\\,iw)':-2",
                        "-q:v", "2",
                        jpg.to_str().unwrap_or_default(),
                    ],
                    timeout,
                )
                .await;
                if !jpg_ok {
                    return Err(AppError::UnsupportedMedia("فشل ضغط الصورة بواسطة ffmpeg.".into()));
                }
                jpg
            };
            let thumb = out_dir.join(format!("{id}_t.webp"));
            let thumb_ok = run_ffmpeg(
                &ffmpeg,
                &[
                    "-y", "-i",
                    tmp_path.to_str().unwrap_or_default(),
                    "-vf", "scale='min(320\\,iw)':-2",
                    "-c:v", "libwebp",
                    "-quality", "75",
                    thumb.to_str().unwrap_or_default(),
                ],
                timeout,
            )
            .await;
            let thumb_rel = if thumb_ok { Some(rel_name(&thumb, &cfg.upload_dir)) } else { None };

            let meta = probe(&ffprobe, &final_path, timeout).await;
            let ext = final_path.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_string();
            let stored_rel = rel_name(&final_path, &cfg.upload_dir);
            (
                stored_rel,
                if ext == "webp" { "image/webp".to_string() } else { "image/jpeg".to_string() },
                file_size(&final_path)?,
                meta.0,
                meta.1,
                meta.2,
                thumb_rel,
                shasum(&final_path)?,
            )
        }
        "video" => {
            let out = out_dir.join(format!("{id}.mp4"));
            let ok = run_ffmpeg(
                &ffmpeg,
                &[
                    "-y", "-i",
                    tmp_path.to_str().unwrap_or_default(),
                    "-vf", "scale='min(1280\\,iw)':-2",
                    "-c:v", "libx264",
                    "-preset", "veryfast",
                    "-crf", "26",
                    "-pix_fmt", "yuv420p",
                    "-c:a", "aac",
                    "-b:a", "96k",
                    "-movflags", "+faststart",
                    out.to_str().unwrap_or_default(),
                ],
                timeout,
            )
            .await;
            if !ok {
                return Err(AppError::UnsupportedMedia("فشل ضغط الفيديو بواسطة ffmpeg.".into()));
            }
            let thumb = out_dir.join(format!("{id}_t.webp"));
            let thumb_ok = run_ffmpeg(
                &ffmpeg,
                &[
                    "-y", "-ss", "1", "-i",
                    out.to_str().unwrap_or_default(),
                    "-frames:v", "1",
                    "-vf", "scale='min(320\\,iw)':-2",
                    "-c:v", "libwebp",
                    "-quality", "75",
                    thumb.to_str().unwrap_or_default(),
                ],
                timeout,
            )
            .await;
            let meta = probe(&ffprobe, &out, timeout).await;
            (
                rel_name(&out, &cfg.upload_dir),
                "video/mp4".to_string(),
                file_size(&out)?,
                meta.0,
                meta.1,
                meta.2,
                if thumb_ok { Some(rel_name(&thumb, &cfg.upload_dir)) } else { None },
                shasum(&out)?,
            )
        }
        "audio" => {
            let out = out_dir.join(format!("{id}.m4a"));
            let ok = run_ffmpeg(
                &ffmpeg,
                &[
                    "-y", "-i",
                    tmp_path.to_str().unwrap_or_default(),
                    "-c:a", "aac",
                    "-b:a", "96k",
                    out.to_str().unwrap_or_default(),
                ],
                timeout,
            )
            .await;
            if !ok {
                return Err(AppError::UnsupportedMedia("فشل ضغط الصوت بواسطة ffmpeg.".into()));
            }
            let meta = probe(&ffprobe, &out, timeout).await;
            (
                rel_name(&out, &cfg.upload_dir),
                "audio/mp4".to_string(),
                file_size(&out)?,
                meta.0,
                meta.1,
                meta.2,
                None,
                shasum(&out)?,
            )
        }
        _ => {
            // Store as-is (documents, archives...).
            let ext = Path::new(original_name)
                .extension()
                .and_then(|e| e.to_str())
                .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()) && e.len() <= 10)
                .unwrap_or("bin");
            let out = out_dir.join(format!("{id}.{ext}"));
            std::fs::copy(tmp_path, &out).map_err(|e| AppError::Internal(format!("copy: {e}")))?;
            let mime = if mime.is_empty() { "application/octet-stream".to_string() } else { mime.to_string() };
            let size = file_size(&out)?;
            let hash = shasum(&out)?;
            (rel_name(&out, &cfg.upload_dir), mime, size, None, None, None, None, hash)
        }
    };

    Ok(ProcessedMedia {
        kind,
        mime_type,
        size,
        width: w,
        height: h,
        duration_ms: dur,
        stored_rel,
        thumb_rel,
        hash,
    })
}

async fn run_ffmpeg(bin: &str, args: &[&str], timeout: Duration) -> bool {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(out)) => out.status.success(),
        _ => false,
    }
}

async fn probe(ffprobe: &str, path: &Path, timeout: Duration) -> (Option<i32>, Option<i32>, Option<i64>) {
    let Ok(Ok(out)) = tokio::time::timeout(
        timeout,
        tokio::process::Command::new(ffprobe)
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_streams",
                "-show_format",
                path.to_str().unwrap_or_default(),
            ])
            .output(),
    )
    .await
    else {
        return (None, None, None);
    };
    if !out.status.success() {
        return (None, None, None);
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return (None, None, None);
    };
    let width = v["streams"][0]["width"].as_i64().map(|w| w as i32);
    let height = v["streams"][0]["height"].as_i64().map(|h| h as i32);
    let duration_s = v["format"]["duration"].as_f64().or_else(|| v["streams"][0]["duration"].as_f64());
    let duration_ms = duration_s.map(|d| (d * 1000.0) as i64);
    (width, height, duration_ms)
}

fn file_size(p: &Path) -> AppResult<i64> {
    std::fs::metadata(p)
        .map(|m| m.len() as i64)
        .map_err(|e| AppError::Internal(format!("stat: {e}")))
}

fn rel_name(path: &Path, root: &Path) -> String {
    let abs_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    abs.strip_prefix(&abs_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
}

/// Resolve a stored relative path safely so no traversal escapes UPLOAD_DIR.
pub fn safe_join(root: &Path, rel: &str) -> AppResult<PathBuf> {
    let root_abs = std::fs::canonicalize(root).map_err(|e| AppError::Internal(format!("upload dir: {e}")))?;
    let candidate = root_abs.join(rel);
    let candidate_abs = std::fs::canonicalize(&candidate).map_err(|_| AppError::NotFound("الملف غير موجود.".into()))?;
    if !candidate_abs.starts_with(&root_abs) {
        return Err(AppError::Forbidden("مسار غير مسموح.".into()));
    }
    Ok(candidate_abs)
}
