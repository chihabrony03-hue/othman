//! /api/conversations — DM & group chat over PostgreSQL + MQTT.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::AuthedUser;
use crate::error::{AppError, AppResult};
use crate::models::{AttachmentOut, MessageOut, ReadEvent, SenderOut};
use crate::state::AppState;

fn is_member_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)"
}

async fn ensure_member(state: &AppState, conv: Uuid, user: Uuid) -> AppResult<()> {
    let ok: bool = sqlx::query_scalar(is_member_sql())
        .bind(conv)
        .bind(user)
        .fetch_one(&state.pool)
        .await?;
    if !ok {
        return Err(AppError::Forbidden("لست عضواً في هذه المحادثة.".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// List conversations
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ConversationOut {
    id: Uuid,
    is_group: bool,
    name: Option<String>,
    avatar_url: Option<String>,
    members: Vec<SenderOut>,
    last_message: Option<MessageOut>,
    unread_count: i64,
    last_read_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

pub async fn list_conversations(
    State(state): State<AppState>,
    user: AuthedUser,
) -> AppResult<Json<Value>> {
    let rows = sqlx::query(
        r#"
        SELECT c.id, c.is_group, c.name, c.avatar_url, c.last_message_at, c.updated_at,
               cm.last_read_at
        FROM conversations c
        JOIN conversation_members cm ON cm.conversation_id = c.id
        WHERE cm.user_id = $1
        ORDER BY COALESCE(c.last_message_at, c.updated_at) DESC
        "#,
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await?;

    let mut out = Vec::new();
    for row in rows {
        let conv_id: Uuid = row.get("id");
        let is_group: bool = row.get("is_group");
        let name: Option<String> = row.get("name");
        let avatar_url: Option<String> = row.get("avatar_url");
        let last_read_at: Option<DateTime<Utc>> = row.get("last_read_at");
        let last_message_at: Option<DateTime<Utc>> = row.get("last_message_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        let members = conversation_members(&state, conv_id).await?;
        let last_message = match last_message_at {
            Some(_) => last_message(&state, conv_id).await,
            None => None,
        };
        let unread = last_message
            .as_ref()
            .map(|m| {
                let read_ts = last_read_at.unwrap_or(DateTime::<Utc>::from_timestamp(0, 0).unwrap());
                if m.sent_at > read_ts && m.sender.id != user.user_id { 1 } else { 0 }
            })
            .unwrap_or(0);

        out.push(json!({
            "id": conv_id,
            "is_group": is_group,
            "name": name,
            "avatar_url": avatar_url,
            "members": members,
            "last_message": last_message,
            "unread_count": unread,
            "last_read_at": last_read_at,
            "updated_at": updated_at,
        }));
    }
    Ok(Json(json!({ "conversations": out })))
}

// ---------------------------------------------------------------------------
// Create DM / group
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateDmReq {
    user_id: Uuid,
}

pub async fn create_dm(State(state): State<AppState>, user: AuthedUser, Json(req): Json<CreateDmReq>) -> AppResult<Json<Value>> {
    if req.user_id == user.user_id {
        return Err(AppError::BadRequest("لا يمكنك مراسلة نفسك.".into()));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_active = true)")
        .bind(req.user_id)
        .fetch_one(&state.pool)
        .await?;
    if !exists {
        return Err(AppError::NotFound("المستخدم غير موجود.".into()));
    }

    // Find existing DM (is_group=false with exactly these two members).
    let conv_row = sqlx::query(
        r#"
        SELECT c.id
        FROM conversations c
        WHERE c.is_group = false
          AND EXISTS (SELECT 1 FROM conversation_members WHERE conversation_id = c.id AND user_id = $1)
          AND EXISTS (SELECT 1 FROM conversation_members WHERE conversation_id = c.id AND user_id = $2)
          AND (SELECT COUNT(*) FROM conversation_members WHERE conversation_id = c.id) = 2
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .bind(req.user_id)
    .fetch_optional(&state.pool)
    .await?;

    let conv_id: Uuid = match conv_row {
        Some(r) => r.get("id"),
        None => {
            let new_id = Uuid::new_v4();
            let mut tx = state.pool.begin().await?;
            sqlx::query("INSERT INTO conversations (id, is_group) VALUES ($1, false)")
                .bind(new_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO conversation_members (conversation_id, user_id) VALUES ($1, $2), ($1, $3)")
                .bind(new_id)
                .bind(user.user_id)
                .bind(req.user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            new_id
        }
    };

    Ok(Json(json!({ "conversation_id": conv_id })))
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupReq {
    name: Option<String>,
    member_ids: Vec<Uuid>,
}

pub async fn create_group(State(state): State<AppState>, user: AuthedUser, Json(req): Json<CreateGroupReq>) -> AppResult<Json<Value>> {
    if req.member_ids.is_empty() || req.member_ids.len() > 49 {
        return Err(AppError::BadRequest("المجموعة يجب أن تضم بين 1 و 49 عضواً غيرك.".into()));
    }
    let name = req
        .name
        .as_deref()
        .map(|s| s.trim().chars().filter(|c| !c.is_control()).take(60).collect::<String>())
        .filter(|s| !s.is_empty());

    let mut ids: Vec<Uuid> = req.member_ids.clone();
    ids.push(user.user_id);
    ids.sort();
    ids.dedup();
    if ids.len() < 3 {
        return Err(AppError::BadRequest("المجموعة تحتاج عضواً واحداً على الأقل غيرك.".into()));
    }
    // All members must exist & be active.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ANY($1) AND is_active = true")
        .bind(&ids)
        .fetch_one(&state.pool)
        .await?;
    if count != ids.len() as i64 {
        return Err(AppError::BadRequest("أحد الأعضاء غير موجود.".into()));
    }

    let conv_id = Uuid::new_v4();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO conversations (id, is_group, name, created_by) VALUES ($1, true, $2, $3)",
    )
    .bind(conv_id)
    .bind(&name)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    for uid in &ids {
        sqlx::query("INSERT INTO conversation_members (conversation_id, user_id) VALUES ($1, $2)")
            .bind(conv_id)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(Json(json!({ "conversation_id": conv_id })))
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MessagesParams {
    before: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

pub async fn list_messages(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(conv_id): Path<Uuid>,
    Query(p): Query<MessagesParams>,
) -> AppResult<Json<Value>> {
    ensure_member(&state, conv_id, user.user_id).await?;
    let limit = p.limit.unwrap_or(50).clamp(1, 100);
    let rows = if let Some(before) = p.before {
        sqlx::query(
            r#"
            SELECT m.id, m.conversation_id, m.content, m.sent_at,
                   u.id AS sender_id, u.username, u.display_name, u.avatar_url,
                   a.id AS att_id, a.original_name, a.mime_type, a.size, a.kind,
                   a.stored_rel, a.thumb_rel, a.width, a.height, a.duration_ms
            FROM messages m
            JOIN users u ON u.id = m.sender_id
            LEFT JOIN attachments a ON a.id = m.attachment_id
            WHERE m.conversation_id = $1 AND m.sent_at < $2
            ORDER BY m.sent_at DESC
            LIMIT $3
            "#,
        )
        .bind(conv_id)
        .bind(before)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT m.id, m.conversation_id, m.content, m.sent_at,
                   u.id AS sender_id, u.username, u.display_name, u.avatar_url,
                   a.id AS att_id, a.original_name, a.mime_type, a.size, a.kind,
                   a.stored_rel, a.thumb_rel, a.width, a.height, a.duration_ms
            FROM messages m
            JOIN users u ON u.id = m.sender_id
            LEFT JOIN attachments a ON a.id = m.attachment_id
            WHERE m.conversation_id = $1
            ORDER BY m.sent_at DESC
            LIMIT $2
            "#,
        )
        .bind(conv_id)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };

    let mut messages: Vec<MessageOut> = rows.into_iter().map(message_from_row).collect();
    messages.reverse();
    Ok(Json(json!({ "messages": messages })))
}

#[derive(Debug, Deserialize)]
pub struct SendMessageReq {
    content: Option<String>,
    attachment_id: Option<Uuid>,
}

pub async fn send_message(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(conv_id): Path<Uuid>,
    Json(req): Json<SendMessageReq>,
) -> AppResult<Json<MessageOut>> {
    ensure_member(&state, conv_id, user.user_id).await?;

    let content = req
        .content
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(state.cfg.max_message_len)
        .collect::<String>();

    let attachment_id = match req.attachment_id {
        Some(aid) => {
            if state.cfg.max_attachment_per_message < 1 {
                return Err(AppError::BadRequest("المرفقات معطلة.".into()));
            }
            let ok: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM attachments WHERE id = $1 AND owner_id = $2 AND message_id IS NULL)",
            )
            .bind(aid)
            .bind(user.user_id)
            .fetch_one(&state.pool)
            .await?;
            if !ok {
                return Err(AppError::BadRequest("المرفق غير موجود أو تم استخدامه من قبل.".into()));
            }
            Some(aid)
        }
        None => None,
    };

    if content.trim().is_empty() && attachment_id.is_none() {
        return Err(AppError::BadRequest("لا يمكن إرسال رسالة فارغة.".into()));
    }

    let msg_id = Uuid::new_v4();
    let sent_at = Utc::now();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO messages (id, conversation_id, sender_id, content, attachment_id, sent_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(msg_id)
    .bind(conv_id)
    .bind(user.user_id)
    .bind(&content)
    .bind(attachment_id)
    .bind(sent_at)
    .execute(&mut *tx)
    .await?;
    if let Some(aid) = attachment_id {
        sqlx::query("UPDATE attachments SET message_id = $1 WHERE id = $2")
            .bind(msg_id)
            .bind(aid)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "UPDATE conversations SET last_message_at = $2 WHERE id = $1",
    )
    .bind(conv_id)
    .bind(sent_at)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE conversation_members SET last_read_at = $2 WHERE conversation_id = $1 AND user_id = $3",
    )
    .bind(conv_id)
    .bind(sent_at)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let message = load_message(&state, msg_id).await?;

    // Deliver over MQTT (message bus) + local sockets. The MQTT bridge dedups
    // the broker echo because we mark the topic as locally-originated.
    let topic = format!("meev/{conv_id}/messages");
    state.mark_local_event(&topic).await;
    state.mqtt.publish_message(&message);
    let data = serde_json::to_value(&message).unwrap_or_default();
    state.hub.broadcast_room(conv_id, "message", data).await;

    Ok(Json(message))
}

pub async fn mark_read(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(conv_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    ensure_member(&state, conv_id, user.user_id).await?;
    let now = Utc::now();
    sqlx::query(
        "UPDATE conversation_members SET last_read_at = $3 WHERE conversation_id = $1 AND user_id = $2",
    )
    .bind(conv_id)
    .bind(user.user_id)
    .bind(now)
    .execute(&state.pool)
    .await?;

    let ev = ReadEvent { conversation_id: conv_id, user_id: user.user_id, read_at: now };
    state.mark_local_event(&format!("meev/{conv_id}/read")).await;
    state.hub.broadcast_room(conv_id, "read", serde_json::to_value(&ev).unwrap_or_default()).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn typing(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(conv_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    ensure_member(&state, conv_id, user.user_id).await?;
    let topic = format!("meev/{conv_id}/typing");
    state.mark_local_event(&topic).await;
    state.mqtt.publish_typing(conv_id, user.user_id);
    let ev = crate::models::TypingEvent { conversation_id: conv_id, user_id: user.user_id };
    state.hub.broadcast_room(conv_id, "typing", serde_json::to_value(&ev).unwrap_or_default()).await;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn conversation_members(state: &AppState, conv_id: Uuid) -> AppResult<Vec<SenderOut>> {
    let rows = sqlx::query_as::<_, SenderOut>(
        r#"
        SELECT u.id, u.username, u.display_name, u.avatar_url
        FROM conversation_members cm JOIN users u ON u.id = cm.user_id
        WHERE cm.conversation_id = $1 AND u.is_active = true
        ORDER BY u.display_name
        "#,
    )
    .bind(conv_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

async fn last_message(state: &AppState, conv_id: Uuid) -> Option<MessageOut> {
    let row = sqlx::query(
        r#"
        SELECT m.id, m.conversation_id, m.content, m.sent_at,
               u.id AS sender_id, u.username, u.display_name, u.avatar_url,
               a.id AS att_id, a.original_name, a.mime_type, a.size, a.kind,
               a.stored_rel, a.thumb_rel, a.width, a.height, a.duration_ms
        FROM messages m
        JOIN users u ON u.id = m.sender_id
        LEFT JOIN attachments a ON a.id = m.attachment_id
        WHERE m.conversation_id = $1
        ORDER BY m.sent_at DESC
        LIMIT 1
        "#,
    )
    .bind(conv_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()?;
    Some(message_from_row(row))
}

async fn load_message(state: &AppState, id: Uuid) -> AppResult<MessageOut> {
    let row = sqlx::query(
        r#"
        SELECT m.id, m.conversation_id, m.content, m.sent_at,
               u.id AS sender_id, u.username, u.display_name, u.avatar_url,
               a.id AS att_id, a.original_name, a.mime_type, a.size, a.kind,
               a.stored_rel, a.thumb_rel, a.width, a.height, a.duration_ms
        FROM messages m
        JOIN users u ON u.id = m.sender_id
        LEFT JOIN attachments a ON a.id = m.attachment_id
        WHERE m.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound("الرسالة غير موجودة.".into()));
    };
    Ok(message_from_row(row))
}

fn message_from_row(row: sqlx::postgres::PgRow) -> MessageOut {
    let sender = SenderOut {
        id: row.get("sender_id"),
        username: row.get("username"),
        display_name: row.get("display_name"),
        avatar_url: row.get("avatar_url"),
    };
    let attachment: Option<AttachmentOut> = row
        .get::<Option<Uuid>, _>("att_id")
        .map(|_| {
            let att_id: Uuid = row.get("att_id");
            AttachmentOut {
                id: att_id,
                kind: row.get("kind"),
                original_name: row.get("original_name"),
                mime_type: row.get("mime_type"),
                size: row.get("size"),
                width: row.get("width"),
                height: row.get("height"),
                duration_ms: row.get("duration_ms"),
                url: format!("/api/media/{att_id}/file"),
                thumb_url: row.get::<Option<String>, _>("thumb_rel").map(|_| format!("/api/media/{att_id}/thumb")),
            }
        });
    MessageOut {
        id: row.get("id"),
        conversation_id: row.get("conversation_id"),
        sender,
        content: row.get("content"),
        attachment,
        sent_at: row.get("sent_at"),
    }
}


