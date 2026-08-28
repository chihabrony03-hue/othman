//! REST route handlers.

pub mod auth;
pub mod chat;
pub mod media;
pub mod suggestions;
pub mod users;
pub mod ws;

use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Input validation — every user-supplied value is validated and normalized
/// BEFORE it ever reaches the database. SQL strings are always passed as
/// bound parameters ($1, $2 ...), never concatenated.
pub fn normalize_username(username: &str) -> AppResult<String> {
    let u = username.trim().to_lowercase();
    if u.len() < 3 || u.len() > 24 {
        return Err(AppError::BadRequest("اسم المستخدم يجب أن يكون بين 3 و 24 حرفاً.".into()));
    }
    let mut chars = u.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err(AppError::BadRequest("اسم المستخدم يجب أن يبدأ بحرف.".into()));
    }
    if !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
        return Err(AppError::BadRequest("اسم المستخدم مسموح فيه فقط الأحرف اللاتينية والأرقام و _ و .".into()));
    }
    Ok(u)
}

pub fn validate_email(email: &str) -> AppResult<String> {
    let e = email.trim().to_lowercase();
    if e.len() > 254 {
        return Err(AppError::BadRequest("البريد الإلكتروني طويل جداً.".into()));
    }
    let at = e.find('@').ok_or_else(|| AppError::BadRequest("البريد الإلكتروني غير صالح.".into()))?;
    let local = &e[..at];
    let domain = &e[at + 1..];
    if local.is_empty() || domain.len() < 3 || !domain.contains('.') {
        return Err(AppError::BadRequest("البريد الإلكتروني غير صالح.".into()));
    }
    if !local.chars().all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+-/=?^_`{|}~".contains(c)) {
        return Err(AppError::BadRequest("البريد الإلكتروني غير صالح.".into()));
    }
    if !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err(AppError::BadRequest("البريد الإلكتروني غير صالح.".into()));
    }
    Ok(e)
}

pub fn validate_display_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::BadRequest("الاسم الظاهر لا يمكن أن يكون فارغاً.".into()));
    }
    if n.chars().count() > 40 {
        return Err(AppError::BadRequest("الاسم الظاهر طويل جداً (الحد 40 حرفاً).".into()));
    }
    let mut cleaned = String::new();
    for c in n.chars() {
        if c.is_control() {
            continue;
        }
        cleaned.push(c);
    }
    if cleaned.trim().is_empty() {
        return Err(AppError::BadRequest("الاسم الظاهر غير صالح.".into()));
    }
    Ok(cleaned.trim().to_string())
}

pub fn validate_bio(bio: &str) -> AppResult<String> {
    let b = bio.trim();
    if b.chars().count() > 300 {
        return Err(AppError::BadRequest("السيرة الذاتية طويلة جداً (الحد 300 حرفاً).".into()));
    }
    Ok(b.chars().filter(|c| !c.is_control() || *c == '\n').collect())
}

pub fn validate_interests(list: &[String]) -> AppResult<Vec<String>> {
    if list.len() > 20 {
        return Err(AppError::BadRequest("الحد الأقصى 20 اهتماماً.".into()));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in list {
        let t = raw.trim().to_lowercase();
        if t.is_empty() || t.chars().count() > 32 {
            return Err(AppError::BadRequest("كل اهتمام يجب أن يكون بين 1 و 32 حرفاً.".into()));
        }
        if t.chars().any(|c| c.is_control()) {
            return Err(AppError::BadRequest("قيمة اهتمام غير صالحة.".into()));
        }
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    Ok(out)
}

pub fn validate_lat_lng(lat: f64, lng: f64) -> AppResult<()> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return Err(AppError::BadRequest("إحداثيات الموقع غير صالحة.".into()));
    }
    Ok(())
}

pub fn uuid_from_str(s: &str) -> AppResult<Uuid> {
    Uuid::parse_str(s).map_err(|_| AppError::BadRequest("معرّف غير صالح.".into()))
}
