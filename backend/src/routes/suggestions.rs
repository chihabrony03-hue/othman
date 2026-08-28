//! /api/suggestions and /api/interests — friend discovery.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::AuthedUser;
use crate::error::AppResult;
use crate::state::AppState;
use crate::suggest;

#[derive(Debug, Deserialize)]
pub struct SuggestionsParams {
    limit: Option<usize>,
}

pub async fn get_suggestions(
    State(state): State<AppState>,
    user: AuthedUser,
    Query(p): Query<SuggestionsParams>,
) -> AppResult<Json<Value>> {
    let limit = p.limit.unwrap_or(24).clamp(6, 60);
    let items = suggest::suggestions(&state.pool, user.user_id, limit).await?;
    Ok(Json(json!({ "suggestions": items })))
}

#[derive(Debug, Deserialize)]
pub struct InterestsParams {
    q: Option<String>,
}

pub async fn suggest_interests(
    State(state): State<AppState>,
    _user: AuthedUser,
    Query(p): Query<InterestsParams>,
) -> AppResult<Json<Value>> {
    let q = p.q.unwrap_or_default().trim().to_lowercase();
    if q.is_empty() {
        let rows = sqlx::query("SELECT name, COUNT(*) AS uses FROM interests GROUP BY name ORDER BY uses DESC LIMIT 30")
            .fetch_all(&state.pool)
            .await?;
        let items: Vec<Value> = rows.iter().map(|r| json!({ "name": r.get::<String, _>("name") })).collect();
        return Ok(Json(json!({ "interests": items })));
    }
    let pattern = format!("{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let rows = sqlx::query(
        "SELECT name, COUNT(*) AS uses FROM interests WHERE name ILIKE $1 ESCAPE '\\' GROUP BY name ORDER BY uses DESC LIMIT 20",
    )
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<Value> = rows.iter().map(|r| json!({ "name": r.get::<String, _>("name") })).collect();
    Ok(Json(json!({ "interests": items })))
}
