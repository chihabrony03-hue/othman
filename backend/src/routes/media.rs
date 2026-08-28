//! /api/media — upload, metadata and authenticated file serving.

use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::AuthedUser;
use crate::error::{AppError, AppResult};
use crate::models::AttachmentOut;
use crate::state::AppState;
use crate::media;

/// Upload a file (image/video/audio/other). Compression happens via ffmpeg.
pub async fn upload(
    State(state): State<AppState>,
    user: AuthedUser,
    mut multipart: Multipart,
) -> AppResult<Json<AttachmentOut>> {
    let (tmp_path, name, mime) = media::save_first_file(&mut multipart).await?;
    let id = Uuid::new_v4();
    let processed = media::process_upload(&state.cfg, &tmp_path, &name, &mime, id).await?;
    let _ = std::fs::remove_file(&tmp_path);

    let row = sqlx::query(
        "INSERT INTO attachments (id, owner_id, original_name, mime_type, size, kind, stored_rel, thumb_rel,
                                  width, height, duration_ms, hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING id, original_name, mime_type, size, kind, stored_rel, thumb_rel, width, height, duration_ms",
    )
    .bind(id)
    .bind(user.user_id)
    .bind(media::sanitize_file_name(&name))
    .bind(&processed.mime_type)
    .bind(processed.size)
    .bind(&processed.kind)
    .bind(&processed.stored_rel)
    .bind(&processed.thumb_rel)
    .bind(processed.width)
    .bind(processed.height)
    .bind(processed.duration_ms)
    .bind(&processed.hash)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(attachment_from_row(&row)))
}

pub fn attachment_from_row(row: &sqlx::postgres::PgRow) -> AttachmentOut {
    let id: Uuid = row.get("id");
    AttachmentOut {
        id,
        kind: row.get("kind"),
        original_name: row.get("original_name"),
        mime_type: row.get("mime_type"),
        size: row.get("size"),
        width: row.get("width"),
        height: row.get("height"),
        duration_ms: row.get("duration_ms"),
        url: format!("/api/media/{id}/file"),
        thumb_url: row
            .get::<Option<String>, _>("thumb_rel")
            .map(|_| format!("/api/media/{id}/thumb")),
    }
}

pub async fn metadata(
    State(state): State<AppState>,
    _user: AuthedUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<AttachmentOut>> {
    let row = sqlx::query(
        "SELECT id, original_name, mime_type, size, kind, stored_rel, thumb_rel, width, height, duration_ms
         FROM attachments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound("الملف غير موجود.".into()));
    };
    Ok(Json(attachment_from_row(&row)))
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    token: Option<String>,
}

fn file_response(path: &std::path::Path, mime: &str, download_name: Option<&str>) -> AppResult<Response> {
    let bytes = std::fs::read(path).map_err(|_| AppError::NotFound("الملف غير موجود.".into()))?;
    let mime = if mime.is_empty() { "application/octet-stream" } else { mime };
    let mut resp = Response::new(Body::from(bytes));
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Some(name) = download_name {
        let safe = media::sanitize_file_name(name);
        let dispos = format!("attachment; filename=\"{safe}\"");
        if let Ok(v) = HeaderValue::from_str(&dispos) {
            headers.insert(header::CONTENT_DISPOSITION, v);
        }
    }
    Ok(resp)
}

/// Serve the original file (authenticated; `?token=` works for <img>/<video>).
pub async fn file(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(id): Path<Uuid>,
    Query(q): Query<FileQuery>,
) -> AppResult<Response> {
    let _ = q;
    // Access control: the owner may always fetch; otherwise the attachment must
    // already be attached to a message in a conversation the requester belongs to.
    let row = sqlx::query(
        r#"
        SELECT a.stored_rel, a.mime_type, a.original_name
        FROM attachments a
        WHERE a.id = $1
          AND (
            a.owner_id = $2
            OR (
              a.message_id IS NOT NULL
              AND EXISTS (
                SELECT 1 FROM messages m
                JOIN conversation_members cm ON cm.conversation_id = m.conversation_id
                WHERE m.id = a.message_id AND cm.user_id = $2
              )
            )
          )
        "#,
    )
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound("الملف غير موجود.".into()));
    };
    let rel: String = row.get("stored_rel");
    let mime: String = row.get("mime_type");
    let name: String = row.get("original_name");
    let path = media::safe_join(&state.cfg.upload_dir, &rel)?;
    file_response(&path, &mime, Some(&name))
}

/// Serve the webp thumbnail (public enough for messages; authenticated).
pub async fn thumb(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    let row = sqlx::query(
        r#"
        SELECT a.stored_rel FROM attachments a
        WHERE a.id = $1 AND a.thumb_rel IS NOT NULL
          AND (
            a.owner_id = $2
            OR (
              a.message_id IS NOT NULL
              AND EXISTS (
                SELECT 1 FROM messages m
                JOIN conversation_members cm ON cm.conversation_id = m.conversation_id
                WHERE m.id = a.message_id AND cm.user_id = $2
              )
            )
          )
        "#,
    )
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound("الصورة المصغرة غير موجودة.".into()));
    };
    let rel: String = row.get("thumb_rel");
    let path = media::safe_join(&state.cfg.upload_dir, &rel)?;
    file_response(&path, "image/webp", None)
}


