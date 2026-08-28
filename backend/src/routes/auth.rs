//! /api/auth — register, login, refresh, logout, me.

use axum::extract::State;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::{self, AuthedUser};
use crate::error::{AppError, AppResult};
use crate::routes::{validate_display_name, validate_email, normalize_username};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterReq {
    username: String,
    email: String,
    password: String,
    display_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct AuthResp {
    access_token: String,
    refresh_token: String,
    user: serde_json::Value,
}

pub async fn register(State(state): State<AppState>, Json(req): Json<RegisterReq>) -> AppResult<Json<AuthResp>> {
    let username = normalize_username(&req.username)?;
    let email = validate_email(&req.email)?;
    auth::validate_password(&req.password)?;
    let display_name = match req.display_name.as_deref() {
        Some(n) => validate_display_name(n)?,
        None => username.clone(),
    };

    let password_hash = auth::hash_password(&req.password)?;

    let row = sqlx::query(
        r#"
        INSERT INTO users (id, username, email, password_hash, display_name)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, username, email, password_hash, display_name, avatar_url, banner_url, bio, interests,
                  location_name, location_lat, location_lng, is_private, is_active, is_online, last_seen,
                  created_at, updated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&username)
    .bind(&email)
    .bind(&password_hash)
    .bind(&display_name)
    .fetch_one(&state.pool)
    .await?;

    let user_id: Uuid = row.get("id");
    let access_token = auth::create_access_token(&state.cfg, user_id, &username)?;
    let refresh_token = auth::create_refresh_token(&state, user_id).await?;
    let user = user_json(&row);

    Ok(Json(AuthResp { access_token, refresh_token, user }))
}

#[derive(Debug, Deserialize)]
pub struct LoginReq {
    identifier: String, // username OR email
    password: String,
}

pub async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> AppResult<Json<AuthResp>> {
    let identifier = req.identifier.trim().to_lowercase();
    if identifier.is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest("أدخل اسم المستخدم/البريد وكلمة المرور.".into()));
    }

    let row = sqlx::query(
        r#"
SELECT id, username, email, password_hash, display_name, avatar_url, banner_url, bio, interests,
               location_name, location_lat, location_lng, is_private, is_active, is_online, last_seen,
               created_at, updated_at
        FROM users
        WHERE (LOWER(username) = $1 OR LOWER(email) = $1) AND is_active = true
        "#,
    )
    .bind(&identifier)
    .fetch_optional(&state.pool)
    .await?;

    let Some(row) = row else {
        // Same error for unknown user and wrong password (no account enumeration).
        return Err(AppError::Unauthorized("بيانات الدخول غير صحيحة.".into()));
    };

    let password_hash: String = row.get("password_hash");
    if !auth::verify_password(&req.password, &password_hash) {
        return Err(AppError::Unauthorized("بيانات الدخول غير صحيحة.".into()));
    }

    let user_id: Uuid = row.get("id");
    let username: String = row.get("username");
    let access_token = auth::create_access_token(&state.cfg, user_id, &username)?;
    let refresh_token = auth::create_refresh_token(&state, user_id).await?;
    let user = user_json(&row);

    Ok(Json(AuthResp { access_token, refresh_token, user }))
}

#[derive(Debug, Deserialize)]
pub struct RefreshReq {
    refresh_token: String,
}

pub async fn refresh(State(state): State<AppState>, Json(req): Json<RefreshReq>) -> AppResult<Json<AuthResp>> {
    let (user_id, refresh_token) = auth::rotate_refresh_token(&state, &req.refresh_token).await?;

    let row = sqlx::query(
        "SELECT id, username, display_name, avatar_url, banner_url, bio, interests, location_name,
                location_lat, location_lng, is_private, is_active, is_online, last_seen, created_at, updated_at
         FROM users WHERE id = $1 AND is_active = true",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let username: String = row.get("username");
    let access_token = auth::create_access_token(&state.cfg, user_id, &username)?;
    let user = user_json(&row);
    Ok(Json(AuthResp { access_token, refresh_token, user }))
}

#[derive(Debug, Deserialize)]
pub struct LogoutReq {
    refresh_token: Option<String>,
}

pub async fn logout(State(state): State<AppState>, user: AuthedUser, Json(req): Json<LogoutReq>) -> AppResult<Json<serde_json::Value>> {
    if let Some(token) = req.refresh_token.as_deref() {
        auth::revoke_refresh_token(&state, token).await;
    }
    let _ = sqlx::query("UPDATE users SET is_online = false WHERE id = $1")
        .bind(user.user_id)
        .execute(&state.pool)
        .await;
    state.mark_local_event(&format!("meev/presence/{}", user.user_id)).await;
    state.mqtt.publish_presence(user.user_id, false);
    Ok(Json(json!({ "ok": true })))
}

pub async fn me(State(state): State<AppState>, user: AuthedUser) -> AppResult<Json<serde_json::Value>> {
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

use sqlx::Row;

pub(crate) fn user_json(row: &sqlx::postgres::PgRow) -> serde_json::Value {
    json!({
        "id": row.get::<Uuid, _>("id"),
        "username": row.get::<String, _>("username"),
        "display_name": row.get::<String, _>("display_name"),
        "avatar_url": row.get::<Option<String>, _>("avatar_url"),
        "bio": row.get::<Option<String>, _>("bio"),
        "interests": row.get::<Option<Vec<String>>, _>("interests").unwrap_or_default(),
        "location_name": row.get::<Option<String>, _>("location_name"),
        "is_private": row.get::<bool, _>("is_private"),
        "is_active": row.get::<bool, _>("is_active"),
        "last_seen": row.get::<Option<chrono::DateTime<Utc>>, _>("last_seen").unwrap_or_else(Utc::now),
    })
}
