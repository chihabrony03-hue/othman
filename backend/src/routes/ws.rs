//! /ws — realtime WebSocket endpoint.
//!
//! Client protocol (JSON messages):
//!   -> {"type":"init","rooms":["<conversation-id>"]}
//!   -> {"type":"join","room":"..."}   {"type":"leave","room":"..."}
//!   -> {"type":"ping"}
//!   <- {"type":"pong"} | {"type":"message"|"typing"|"presence"|"read"|"follow", "data":{...}}

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthedUser;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    user: AuthedUser,
    Query(_q): Query<WsQuery>,
) -> Result<Response, AppError> {
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, user)))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "init")]
    Init { rooms: Vec<Uuid> },
    #[serde(rename = "join")]
    Join { room: Uuid },
    #[serde(rename = "leave")]
    Leave { room: Uuid },
    #[serde(rename = "ping")]
    Ping,
}

async fn handle_socket(mut socket: WebSocket, state: AppState, user: AuthedUser) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let conn_id = state.hub.connect(user.user_id, tx).await;
    state.mark_local_event(&format!("meev/presence/{}", user.user_id)).await;
    state.mqtt.publish_presence(user.user_id, true);
    let _ = sqlx::query(
        "UPDATE users SET is_online = true, last_seen = now() WHERE id = $1",
    )
    .bind(user.user_id)
    .execute(&state.pool)
    .await;

    // Push initial online state to client.
    let _ = socket
        .send(Message::Text(json!({"type":"welcome","user_id": user.user_id}).to_string().into()))
        .await;

    let mut joined: Vec<Uuid> = Vec::new();

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let txt = text.as_str().to_string();
                        if let Ok(parsed) = serde_json::from_str::<ClientMsg>(&txt) {
                            match parsed {
                                ClientMsg::Init { rooms } => {
                                    for r in rooms {
                                        if is_member(&state, r, user.user_id).await {
                                            state.hub.join_room(user.user_id, r).await;
                                            joined.push(r);
                                        }
                                    }
                                }
                                ClientMsg::Join { room } => {
                                    if is_member(&state, room, user.user_id).await {
                                        state.hub.join_room(user.user_id, room).await;
                                        if !joined.contains(&room) { joined.push(room); }
                                    }
                                }
                                ClientMsg::Leave { room } => {
                                    state.hub.leave_room(user.user_id, room).await;
                                    joined.retain(|r| *r != room);
                                }
                                ClientMsg::Ping => {
                                    let _ = socket.send(Message::Text(json!({"type":"pong"}).to_string().into())).await;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(_))) => {
                        let _ = socket.send(Message::Pong(axum::body::Bytes::new())).await;
                    }
                    _ => {}
                }
            }
            outgoing = rx.recv() => {
                match outgoing {
                    Some(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    state.hub.disconnect(user.user_id, conn_id).await;
    for room in &joined {
        state.hub.leave_room(user.user_id, *room).await;
    }
    let online = state.hub.is_online(user.user_id).await;
    if !online {
        state.mark_local_event(&format!("meev/presence/{}", user.user_id)).await;
        state.mqtt.publish_presence(user.user_id, false);
        let _ = sqlx::query(
            "UPDATE users SET is_online = false, last_seen = now() WHERE id = $1",
        )
        .bind(user.user_id)
        .execute(&state.pool)
        .await;
    }
}

async fn is_member(state: &AppState, conv: Uuid, uid: Uuid) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)",
    )
    .bind(conv)
    .bind(uid)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(false)
}


