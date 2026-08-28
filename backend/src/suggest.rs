//! Friend suggestion algorithm.
//!
//! Each candidate user is scored with a multi-factor model:
//!   * interest overlap  (weight 0.45) — shared tags from the profile
//!   * geographical proximity (weight 0.30) — Haversine distance decay
//!   * mutual connections (weight 0.20) — shared followers/followees
//!   * recent activity   (weight 0.05) — active within the last 7 days
//! plus a small jitter so results are not frozen.
//!
//! All SQL is fully parameterized (`= ANY($1)`, `$2`...); nothing from the
//! user is ever concatenated into a query string.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub location_name: Option<String>,
    pub score: i32,
    pub reasons: Vec<String>,
    pub common_interests: Vec<String>,
    pub mutuals: i64,
    pub distance_km: Option<f64>,
}

#[derive(Debug, sqlx::FromRow)]
struct Candidate {
    id: Uuid,
    username: String,
    display_name: String,
    avatar_url: Option<String>,
    bio: Option<String>,
    location_name: Option<String>,
    location_lat: Option<f64>,
    location_lng: Option<f64>,
    last_seen: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct InterestRow {
    user_id: Uuid,
    name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct MutualRow {
    user_id: Uuid,
    cnt: i64,
}

pub async fn suggestions(pool: &PgPool, me: Uuid, limit: usize) -> AppResult<Vec<Suggestion>> {
    // My interests.
    let my_interests: Vec<String> = sqlx::query_scalar("SELECT name FROM user_interests WHERE user_id = $1")
        .bind(me)
        .fetch_all(pool)
        .await?;
    let my_set: HashSet<String> = my_interests.iter().map(|s| s.to_lowercase()).collect();

    // My location.
    let my_loc: Option<(f64, f64)> = sqlx::query_as::<_, (Option<f64>, Option<f64>)>(
        "SELECT location_lat, location_lng FROM users WHERE id = $1",
    )
    .bind(me)
    .fetch_optional(pool)
    .await?
    .and_then(|(lat, lng)| match (lat, lng) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    });

    // Candidate pool — excludes self, existing accepted follows, and blocks.
    let candidates: Vec<Candidate> = sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.display_name, u.avatar_url, u.bio, u.location_name,
               u.location_lat, u.location_lng, u.last_seen
        FROM users u
        WHERE u.id <> $1
          AND u.is_active = true
          AND NOT EXISTS (
              SELECT 1 FROM follows f
              WHERE f.status = 'accepted'
                AND ((f.follower_id = $1 AND f.followee_id = u.id)
                  OR (f.follower_id = u.id AND f.followee_id = $1))
          )
          AND NOT EXISTS (
              SELECT 1 FROM blocks b
              WHERE (b.blocker_id = $1 AND b.blocked_id = u.id)
                 OR (b.blocker_id = u.id AND b.blocked_id = $1)
          )
        ORDER BY u.last_seen DESC NULLS LAST
        LIMIT 400
        "#,
    )
    .bind(me)
    .fetch_all(pool)
    .await?;

    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<Uuid> = candidates.iter().map(|c| c.id).collect();

    // Interests of candidates (single batched query).
    let interest_rows: Vec<InterestRow> = sqlx::query_as("SELECT user_id, name FROM user_interests WHERE user_id = ANY($1)")
        .bind(&ids)
        .fetch_all(pool)
        .await?;
    let mut interest_map: HashMap<Uuid, Vec<String>> = HashMap::new();
    for r in interest_rows {
        interest_map.entry(r.user_id).or_default().push(r.name);
    }

    // Mutual connections: people who follow me and also follow the candidate,
    // plus people I follow who are followed by the candidate.
    let mut mutual_map: HashMap<Uuid, i64> = HashMap::new();
    let mutual_rows: Vec<MutualRow> = sqlx::query_as(
        r#"
        WITH my_followers AS (
            SELECT follower_id AS x FROM follows WHERE followee_id = $1 AND status = 'accepted'
        ),
        my_following AS (
            SELECT followee_id AS x FROM follows WHERE follower_id = $1 AND status = 'accepted'
        )
        SELECT u.id AS user_id,
               (SELECT COUNT(*) FROM follows f
                 WHERE f.status = 'accepted' AND f.followee_id = u.id
                   AND f.follower_id IN (SELECT x FROM my_followers))
             + (SELECT COUNT(*) FROM follows f
                 WHERE f.status = 'accepted' AND f.follower_id = u.id
                   AND f.followee_id IN (SELECT x FROM my_following)) AS cnt
        FROM users u
        WHERE u.id = ANY($2)
        "#,
    )
    .bind(me)
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    for r in mutual_rows {
        mutual_map.insert(r.user_id, r.cnt);
    }

    let now = Utc::now();
    let mut out: Vec<Suggestion> = Vec::new();

    for c in candidates {
        let cand_interests = interest_map.get(&c.id).cloned().unwrap_or_default();
        let cand_set: HashSet<String> = cand_interests.iter().map(|s| s.to_lowercase()).collect();
        let common: Vec<String> = cand_interests
            .iter()
            .filter(|s| my_set.contains(&s.to_lowercase()))
            .cloned()
            .collect();

        let interest_score = if my_set.is_empty() || cand_set.is_empty() {
            0.0
        } else {
            let inter = my_set.intersection(&cand_set).count() as f64;
            let union = my_set.union(&cand_set).count() as f64;
            inter / union.max(1.0)
        };

        let distance_km = match (my_loc, c.location_lat, c.location_lng) {
            (Some((lat1, lng1)), Some(lat2), Some(lng2)) => Some(haversine_km(lat1, lng1, lat2, lng2)),
            _ => None,
        };
        let loc_score = distance_km.map(|d| 1.0 / (1.0 + d / 50.0)).unwrap_or(0.0);

        let mutuals = mutual_map.get(&c.id).copied().unwrap_or(0);
        let mutual_score = (mutuals as f64 / 5.0).min(1.0);

        let active_score = match c.last_seen {
            Some(ts) if (now - ts).num_days() <= 7 => 1.0,
            _ => 0.0,
        };

        let jitter: f64 = rand::random::<f64>() * 0.05;
        let raw = 0.45 * interest_score + 0.30 * loc_score + 0.20 * mutual_score + 0.05 * active_score + jitter;
        let score = (raw * 100.0).round() as i32;

        let mut reasons = Vec::new();
        if !common.is_empty() {
            reasons.push(format!("اهتمامات مشتركة: {}", common.iter().take(3).cloned().collect::<Vec<_>>().join("، ")));
        }
        if let Some(d) = distance_km {
            if d < 30.0 {
                reasons.push(format!("قريب منك جغرافياً ({:.0} كم)", d));
            }
        }
        if mutuals > 0 {
            reasons.push(format!("أصدقاء/متابعات مشتركة: {mutuals}"));
        }
        if reasons.is_empty() {
            reasons.push("نشط في MEEV".to_string());
        }

        out.push(Suggestion {
            user_id: c.id,
            username: c.username,
            display_name: c.display_name,
            avatar_url: c.avatar_url,
            bio: c.bio,
            location_name: c.location_name,
            score,
            reasons,
            common_interests: common,
            mutuals,
            distance_km,
        });
    }

    out.sort_by(|a, b| b.score.cmp(&a.score));
    out.truncate(limit.max(6));
    Ok(out)
}

pub fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lng = (lng2 - lng1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lng / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().atan2((1.0 - a).sqrt())
}
