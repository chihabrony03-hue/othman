//! /api/users — profiles, customization, search, followers/following.

use axum::extract::{Multipart, Path, Query, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::{self, AuthedUser};
use crate::error::{AppError, AppResult};
use crate::routes::auth::user_json;
use crate::routes::{validate_bio, validate_display_name, validate_interests, validate_lat_lng, uuid_from_str};
use crate::state::AppState;
use crate::{media, models::AttachmentOut};

// ---------------------------------------------------------------------------
// Public user profile
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PublicProfile {
    #[serde(flatten)]
    user: Value,
    followers_count: i64,
    following_count: i64,
    is_following: bool,
    is_followed_by: bool,
    pending_follow: bool,
    blocked: bool,
    online: bool,
}

pub async fn get_profile(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(username): Path<String>,
) -> AppResult<Json<PublicProfile>> {
    let username = username.trim().to_lowercase();
    let row = sqlx::query(
        "SELECT id, username, display_name, avatar_url, banner_url, bio, interests, location_name,
                location_lat, location_lng, is_private, is_active, last_seen, created_at
         FROM users WHERE LOWER(username) = $1 AND is_active = true",
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound("المستخدم غير موجود.".into()));
    };
    let target_id: Uuid = row.get("id");

    let followers_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM follows WHERE followee_id = $1 AND status = 'accepted'")
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?;
    let following_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM follows WHERE follower_id = $1 AND status = 'accepted'")
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?;

    let rel = sqlx::query(
        "SELECT
            EXISTS(SELECT 1 FROM follows WHERE follower_id = $1 AND followee_id = $2 AND status = 'accepted') AS is_following,
            EXISTS(SELECT 1 FROM follows WHERE follower_id = $2 AND followee_id = $1 AND status = 'accepted') AS is_followed_by,
            EXISTS(SELECT 1 FROM follows WHERE follower_id = $1 AND followee_id = $2 AND status = 'pending') AS pending_follow,
            EXISTS(SELECT 1 FROM blocks WHERE blocker_id = $1 AND blocked_id = $2) AS blocked",
    )
    .bind(user.user_id)
    .bind(target_id)
    .fetch_one(&state.pool)
    .await?;

    let online = state.hub.is_online(target_id).await;
    Ok(Json(PublicProfile {
        user: user_json(&row),
        followers_count,
        following_count,
        is_following: rel.get("is_following"),
        is_followed_by: rel.get("is_followed_by"),
        pending_follow: rel.get("pending_follow"),
        blocked: rel.get("blocked"),
        online,
    }))
}

// ---------------------------------------------------------------------------
// Profile customization
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct UpdateProfileReq {
    display_name: Option<String>,
    bio: Option<String>,
    is_private: Option<bool>,
    location_name: Option<String>,
}

pub async fn update_profile(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(req): Json<UpdateProfileReq>,
) -> AppResult<Json<Value>> {
    let display_name = match req.display_name {
        Some(n) => Some(validate_display_name(&n)?),
        None => None,
    };
    let bio = match req.bio {
        Some(b) => Some(validate_bio(&b)?),
        None => None,
    };
    let location_name = match req.location_name {
        Some(l) => Some(l.trim().chars().filter(|c| !c.is_control()).take(120).collect::<String>()),
        None => None,
    };

    let row = sqlx::query(
        r#"
        UPDATE users SET
            display_name = COALESCE($2, display_name),
            bio = COALESCE($3, bio),
            is_private = COALESCE($4, is_private),
            location_name = COALESCE($5, location_name),
            updated_at = now()
        WHERE id = $1
        RETURNING id, username, display_name, avatar_url, banner_url, bio, interests, location_name,
                  location_lat, location_lng, is_private, is_active, created_at, updated_at, last_seen
        "#,
    )
    .bind(user.user_id)
    .bind(display_name)
    .bind(bio)
    .bind(req.is_private)
    .bind(location_name)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(user_json(&row)))
}

#[derive(Debug, Deserialize)]
pub struct LocationReq {
    lat: f64,
    lng: f64,
    name: Option<String>,
    country: Option<String>,
}

pub async fn update_location(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(req): Json<LocationReq>,
) -> AppResult<Json<Value>> {
    validate_lat_lng(req.lat, req.lng)?;
    let name = req
        .name
        .as_deref()
        .map(|s| s.trim().chars().filter(|c| !c.is_control()).take(120).collect::<String>())
        .filter(|s| !s.is_empty());
    let country = req
        .country
        .as_deref()
        .map(|s| s.trim().chars().filter(|c| !c.is_control()).take(80).collect::<String>())
        .filter(|s| !s.is_empty());

    let row = sqlx::query(
        "UPDATE users SET location_lat = $2, location_lng = $3, location_name = $4, country = $5, updated_at = now()
         WHERE id = $1
         RETURNING id, username, display_name, avatar_url, banner_url, bio, interests, location_name,
                   location_lat, location_lng, is_private, is_active, created_at, updated_at, last_seen",
    )
    .bind(user.user_id)
    .bind(req.lat)
    .bind(req.lng)
    .bind(name)
    .bind(country)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(user_json(&row)))
}

#[derive(Debug, Deserialize)]
pub struct InterestsReq {
    interests: Vec<String>,
}

pub async fn set_interests(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(req): Json<InterestsReq>,
) -> AppResult<Json<Value>> {
    let interests = validate_interests(&req.interests)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM user_interests WHERE user_id = $1")
        .bind(user.user_id)
        .execute(&mut *tx)
        .await?;
    for interest in &interests {
        sqlx::query(
            "INSERT INTO interests (name) VALUES ($1) ON CONFLICT (name) DO NOTHING",
        )
        .bind(interest)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO user_interests (user_id, name) VALUES ($1, $2)")
            .bind(user.user_id)
            .bind(interest)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    let row = sqlx::query(
        "SELECT id, username, display_name, avatar_url, banner_url, bio, interests, location_name,
                location_lat, location_lng, is_private, is_active, created_at, updated_at, last_seen
         FROM users WHERE id = $1",
    )
    .bind(user.user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(user_json(&row)))
}

#[derive(Debug, Deserialize)]
pub struct PasswordReq {
    current_password: String,
    new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(req): Json<PasswordReq>,
) -> AppResult<Json<Value>> {
    auth::validate_password(&req.new_password)?;
    let row = sqlx::query("SELECT password_hash FROM users WHERE id = $1")
        .bind(user.user_id)
        .fetch_one(&state.pool)
        .await?;
    let hash: String = row.get("password_hash");
    if !auth::verify_password(&req.current_password, &hash) {
        return Err(AppError::BadRequest("كلمة المرور الحالية غير صحيحة.".into()));
    }
    let new_hash = auth::hash_password(&req.new_password)?;
    sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
        .bind(user.user_id)
        .bind(&new_hash)
        .execute(&state.pool)
        .await?;
    // Revoke all refresh tokens of this user (force re-login everywhere).
    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
        .bind(user.user_id)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Avatar / banner upload (compressed via ffmpeg pipeline)
// ---------------------------------------------------------------------------

pub async fn upload_avatar(
    State(state): State<AppState>,
    user: AuthedUser,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let stored = save_profile_image(&state, user.user_id, &mut multipart).await?;
    sqlx::query("UPDATE users SET avatar_url = $2, updated_at = now() WHERE id = $1")
        .bind(user.user_id)
        .bind(&stored.url)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "avatar_url": stored.url })))
}

pub async fn upload_banner(
    State(state): State<AppState>,
    user: AuthedUser,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let stored = save_profile_image(&state, user.user_id, &mut multipart).await?;
    sqlx::query("UPDATE users SET banner_url = $2, updated_at = now() WHERE id = $1")
        .bind(user.user_id)
        .bind(&stored.url)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "banner_url": stored.url })))
}

async fn save_profile_image(
    state: &AppState,
    owner_id: Uuid,
    multipart: &mut Multipart,
) -> AppResult<AttachmentOut> {
    let (tmp_path, name, mime) = crate::media::save_first_file(multipart).await?;
    let id = Uuid::new_v4();
    let processed = media::process_upload(&state.cfg, &tmp_path, &name, &mime, id).await?;
    let _ = std::fs::remove_file(&tmp_path);

    if processed.kind != "image" {
        return Err(AppError::BadRequest("يجب أن تكون الصورة من نوع صورة (PNG/JPEG/WebP...).".into()));
    }

    let row = sqlx::query(
        "INSERT INTO attachments (id, owner_id, original_name, mime_type, size, kind, stored_rel, thumb_rel,
                                  width, height, duration_ms, hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING id, original_name, mime_type, size, kind, stored_rel, thumb_rel, width, height, duration_ms",
    )
    .bind(id)
    .bind(owner_id)
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

    Ok(AttachmentOut {
        id: row.get("id"),
        kind: row.get("kind"),
        original_name: row.get("original_name"),
        mime_type: row.get("mime_type"),
        size: row.get("size"),
        width: row.get("width"),
        height: row.get("height"),
        duration_ms: row.get("duration_ms"),
        url: format!("/api/media/{}/file", row.get::<Uuid, _>("id")),
        thumb_url: row
            .get::<Option<String>, _>("thumb_rel")
            .map(|_| format!("/api/media/{}/thumb", row.get::<Uuid, _>("id"))),
    })
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: String,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Serialize)]
pub struct SearchResult {
    users: Vec<Value>,
    total: i64,
}

pub async fn search_users(
    State(state): State<AppState>,
    _user: AuthedUser,
    Query(params): Query<SearchParams>,
) -> AppResult<Json<SearchResult>> {
    let q = params.q.trim();
    if q.is_empty() {
        return Ok(Json(SearchResult { users: vec![], total: 0 }));
    }
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let offset = params.offset.unwrap_or(0).max(0);

    // Escape LIKE wildcards — parameterized query, no injection possible.
    let pattern = format!("%{}%", escape_like(q));

    let rows = sqlx::query(
        r#"
        SELECT id, username, display_name, avatar_url, bio, interests, location_name,
               is_private, is_active, last_seen
        FROM users
        WHERE is_active = true
          AND (username ILIKE $1 ESCAPE '\' OR display_name ILIKE $1 ESCAPE '\')
        ORDER BY
          CASE WHEN LOWER(username) = LOWER($2) THEN 0
               WHEN LOWER(display_name) = LOWER($2) THEN 1
               ELSE 2 END,
          last_seen DESC NULLS LAST
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(&pattern)
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let mut users = Vec::new();
    for row in rows {
        users.push(json!({
            "id": row.get::<Uuid, _>("id"),
            "username": row.get::<String, _>("username"),
            "display_name": row.get::<String, _>("display_name"),
            "avatar_url": row.get::<Option<String>, _>("avatar_url"),
            "bio": row.get::<Option<String>, _>("bio"),
            "interests": row.get::<Option<Vec<String>>, _>("interests").unwrap_or_default(),
            "location_name": row.get::<Option<String>, _>("location_name"),
            "is_private": row.get::<bool, _>("is_private"),
        }));
    }

    let total: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM users
           WHERE is_active = true AND (username ILIKE $1 ESCAPE '\' OR display_name ILIKE $1 ESCAPE '\')"#,
    )
    .bind(&pattern)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(SearchResult { users, total }))
}

fn escape_like(q: &str) -> String {
    q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

// ---------------------------------------------------------------------------
// Follow / unfollow
// ---------------------------------------------------------------------------

pub async fn follow_user(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(username): Path<String>,
) -> AppResult<Json<Value>> {
    let username = username.trim().to_lowercase();
    let target = sqlx::query("SELECT id, is_private, is_active FROM users WHERE LOWER(username) = $1")
        .bind(&username)
        .fetch_optional(&state.pool)
        .await?;
    let Some(target) = target else {
        return Err(AppError::NotFound("المستخدم غير موجود.".into()));
    };
    let target_id: Uuid = target.get("id");
    let target_private: bool = target.get("is_private");
    if target_id == user.user_id {
        return Err(AppError::BadRequest("لا يمكنك متابعة نفسك.".into()));
    }

    let status = if target_private { "pending" } else { "accepted" };
    sqlx::query(
        "INSERT INTO follows (follower_id, followee_id, status)
         VALUES ($1, $2, $3)
         ON CONFLICT (follower_id, followee_id)
         DO UPDATE SET status = EXCLUDED.status, created_at = now()",
    )
    .bind(user.user_id)
    .bind(target_id)
    .bind(status)
    .execute(&state.pool)
    .await?;

    // Notify the target via their sockets.
    state.hub.broadcast_user(target_id, "follow", json!({ "by": user.user_id, "status": status })).await;
    state.mqtt.publish_presence(user.user_id, true);
    Ok(Json(json!({ "status": status })))
}

pub async fn unfollow_user(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(username): Path<String>,
) -> AppResult<Json<Value>> {
    let username = username.trim().to_lowercase();
    sqlx::query("DELETE FROM follows WHERE follower_id = $1 AND followee_id = (SELECT id FROM users WHERE LOWER(username) = $2)")
        .bind(user.user_id)
        .bind(&username)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Followers / following lists
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListParams {
    limit: Option<i64>,
    offset: Option<i64>,
    username: Option<String>,
}

pub async fn followers(
    State(state): State<AppState>,
    user: AuthedUser,
    Query(p): Query<ListParams>,
) -> AppResult<Json<Value>> {
    let limit = p.limit.unwrap_or(30).clamp(1, 100);
    let offset = p.offset.unwrap_or(0).max(0);
    let target = match p.username {
        Some(u) => {
            let row = sqlx::query("SELECT id FROM users WHERE LOWER(username) = $1")
                .bind(u.trim().to_lowercase())
                .fetch_optional(&state.pool)
                .await?;
            match row {
                Some(r) => r.get::<Uuid, _>("id"),
                None => return Err(AppError::NotFound("المستخدم غير موجود.".into())),
            }
        }
        None => user.user_id,
    };
    let rows = sqlx::query(
        r#"
        SELECT u.id, u.username, u.display_name, u.avatar_url, u.bio, u.interests, u.location_name, f.status, f.created_at
        FROM follows f JOIN users u ON u.id = f.follower_id
        WHERE f.followee_id = $1 AND f.status = 'accepted'
        ORDER BY f.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(target)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({ "users": rows_to_user_list(rows) })))
}

pub async fn following(
    State(state): State<AppState>,
    user: AuthedUser,
    Query(p): Query<ListParams>,
) -> AppResult<Json<Value>> {
    let limit = p.limit.unwrap_or(30).clamp(1, 100);
    let offset = p.offset.unwrap_or(0).max(0);
    let target = match p.username {
        Some(u) => {
            let row = sqlx::query("SELECT id FROM users WHERE LOWER(username) = $1")
                .bind(u.trim().to_lowercase())
                .fetch_optional(&state.pool)
                .await?;
            match row {
                Some(r) => r.get::<Uuid, _>("id"),
                None => return Err(AppError::NotFound("المستخدم غير موجود.".into())),
            }
        }
        None => user.user_id,
    };
    let rows = sqlx::query(
        r#"
        SELECT u.id, u.username, u.display_name, u.avatar_url, u.bio, u.interests, u.location_name, f.status, f.created_at
        FROM follows f JOIN users u ON u.id = f.followee_id
        WHERE f.follower_id = $1 AND f.status = 'accepted'
        ORDER BY f.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(target)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({ "users": rows_to_user_list(rows) })))
}

fn rows_to_user_list(rows: Vec<sqlx::postgres::PgRow>) -> Vec<Value> {
    rows.into_iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "username": r.get::<String, _>("username"),
                "display_name": r.get::<String, _>("display_name"),
                "avatar_url": r.get::<Option<String>, _>("avatar_url"),
                "bio": r.get::<Option<String>, _>("bio"),
                "interests": r.get::<Option<Vec<String>>, _>("interests").unwrap_or_default(),
                "location_name": r.get::<Option<String>, _>("location_name"),
                "relation": r.get::<String, _>("status"),
            })
        })
        .collect()
}

pub fn uuid_param(s: &str) -> AppResult<Uuid> {
    uuid_from_str(s)
}
