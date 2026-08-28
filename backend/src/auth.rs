//! Authentication: Argon2id password hashing + HS512 JWT access tokens
//! + revocable refresh tokens stored (hashed) in PostgreSQL.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Password hashing (Argon2id, OWASP recommended parameters)
// ---------------------------------------------------------------------------

pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("hashing failed: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Strong password policy: 10-128 chars, must contain lower, upper, digit.
pub fn validate_password(pw: &str) -> AppResult<()> {
    if pw.len() < 10 {
        return Err(AppError::BadRequest("كلمة المرور يجب أن تكون 10 أحرف على الأقل.".into()));
    }
    if pw.len() > 128 {
        return Err(AppError::BadRequest("كلمة المرور طويلة جداً (الحد 128 حرفاً).".into()));
    }
    let has_upper = pw.chars().any(|c| c.is_uppercase());
    let has_lower = pw.chars().any(|c| c.is_lowercase());
    let has_digit = pw.chars().any(|c| c.is_ascii_digit());
    if !(has_upper && has_lower && has_digit) {
        return Err(AppError::BadRequest(
            "كلمة المرور يجب أن تحتوي على حرف كبير وحرف صغير ورقم على الأقل.".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// JWT
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub iat: usize,
    pub exp: usize,
    /// refresh token id (only present in refresh tokens)
    pub jti: Option<String>,
}

pub fn create_access_token(cfg: &crate::config::Config, user_id: Uuid, username: &str) -> AppResult<String> {
    let now = Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        iat: now,
        exp: now + cfg.jwt_access_ttl_secs as usize,
        jti: None,
    };
    encode(&Header::new(Algorithm::HS512), &claims, &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()))
        .map_err(|e| AppError::Internal(format!("jwt: {e}")))
}

pub async fn create_refresh_token(state: &AppState, user_id: Uuid) -> AppResult<String> {
    let now = Utc::now().timestamp() as usize;
    let jti = Uuid::new_v4().to_string();
    let claims = Claims {
        sub: user_id.to_string(),
        username: String::new(),
        iat: now,
        exp: now + state.cfg.jwt_refresh_ttl_secs as usize,
        jti: Some(jti.clone()),
    };
    let token = encode(
        &Header::new(Algorithm::HS512),
        &claims,
        &EncodingKey::from_secret(state.cfg.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("jwt: {e}")))?;

    // Store only a SHA-256 hash of the token in the DB (revocable, non-leakable).
    let token_hash = refresh_hash(&token);
    let expires = Utc::now() + chrono::Duration::seconds(state.cfg.jwt_refresh_ttl_secs);
    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&jti)
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires)
    .execute(&state.pool)
    .await?;

    Ok(token)
}

pub async fn rotate_refresh_token(state: &AppState, token: &str) -> AppResult<(Uuid, String)> {
    let mut validation = Validation::new(Algorithm::HS512);
    validation.leeway = 10;
    let data = decode::<Claims>(token, &DecodingKey::from_secret(state.cfg.jwt_secret.as_bytes()), &validation)
        .map_err(|_| AppError::Unauthorized("الجلسة منتهية أو غير صالحة. سجل الدخول مجدداً.".into()))?;
    let claims = data.claims;
    let jti = claims.jti.clone().ok_or_else(|| AppError::Unauthorized("رمز غير صالح.".into()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized("رمز غير صالح.".into()))?;
    let token_hash = refresh_hash(token);

    // One-time rotation: the stored hash must match, then it is deleted.
    let result = sqlx::query(
        "DELETE FROM refresh_tokens WHERE id = $1 AND user_id = $2 AND token_hash = $3 AND expires_at > now()",
    )
    .bind(&jti)
    .bind(user_id)
    .bind(&token_hash)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() != 1 {
        return Err(AppError::Unauthorized("رمز التحديث تم استخدامه سابقاً أو انتهت صلاحيته.".into()));
    }

    let new_token = create_refresh_token(state, user_id).await?;
    Ok((user_id, new_token))
}

pub async fn revoke_refresh_token(state: &AppState, token: &str) {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.cfg.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS512),
    );
    if let Ok(data) = data {
        if let Some(jti) = data.claims.jti {
            let _ = sqlx::query("DELETE FROM refresh_tokens WHERE id = $1")
                .bind(jti)
                .execute(&state.pool)
                .await;
        }
    }
}

fn refresh_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn verify_access_token(cfg: &crate::config::Config, token: &str) -> AppResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS512);
    validation.leeway = 30;
    let data = decode::<Claims>(token, &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()), &validation)
        .map_err(|_| AppError::Unauthorized("جلسة غير صالحة أو منتهية. سجل الدخول مجدداً.".into()))?;
    Ok(data.claims)
}

// ---------------------------------------------------------------------------
// Extractor for protected routes
// ---------------------------------------------------------------------------

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub user_id: Uuid,
    pub username: String,
}

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?
            .or_else(|| query_token(parts))
            .ok_or_else(|| AppError::Unauthorized("يجب تسجيل الدخول أولاً.".into()))?;
        let claims = verify_access_token(&state.cfg, &token)?;
        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized("رمز غير صالح.".into()))?;
        Ok(AuthedUser { user_id, username: claims.username })
    }
}

/// Extract bearer token without full verification (used to key rate limiting by user).
pub fn bearer_token(parts: &Parts) -> Option<String> {
    let header = parts.headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    header.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

fn query_token(parts: &Parts) -> Option<String> {
    let uri = parts.uri.query()?;
    for pair in uri.split('&') {
        if let Some(v) = pair.strip_prefix("token=") {
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
