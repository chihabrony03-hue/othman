//! Shared data models used across routes and the MQTT bridge.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SenderOut {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AttachmentOut {
    pub id: Uuid,
    pub kind: String,
    pub original_name: String,
    pub mime_type: String,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i64>,
    pub url: String,
    pub thumb_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageOut {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub sender: SenderOut,
    pub content: String,
    pub attachment: Option<AttachmentOut>,
    pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingEvent {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceEvent {
    pub user_id: Uuid,
    pub online: bool,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadEvent {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub read_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishEvent {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub event: String,
    pub data: serde_json::Value,
}
