//! MQTT bridge.
//!
//! MEEV uses MQTT as its realtime message bus:
//!   * REST handlers persist messages in PostgreSQL, then publish on
//!     `meev/{conversation_id}/messages` (QoS 1).
//!   * This bridge subscribes to `meev/+/messages`, `meev/+/typing` and
//!     `meev/presence/+` and fans events out to connected WebSocket clients
//!     (with an in-memory dedup table so local clients are not double-fed).
//!   * The broker is fully optional: if it is unreachable the app still works
//!     via WebSocket delivery; MQTT is reconnected automatically.

use chrono::Utc;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;


use crate::models::{MessageOut, PresenceEvent, TypingEvent};
use crate::state::AppState;

pub const TOPIC_MESSAGES: &str = "meev/+/messages";
pub const TOPIC_TYPING: &str = "meev/+/typing";
pub const TOPIC_PRESENCE: &str = "meev/presence/+";

#[derive(Clone)]
pub struct MqttHandle {
    tx: Option<mpsc::UnboundedSender<MqttCmd>>,
    pub enabled: bool,
    pub connected: Arc<AtomicBool>,
}

enum MqttCmd {
    Publish { topic: String, payload: Vec<u8> },
}

impl MqttHandle {
    pub fn disabled() -> Self {
        Self { tx: None, enabled: false, connected: Arc::new(AtomicBool::new(false)) }
    }

    pub fn publish(&self, topic: String, payload: Vec<u8>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(MqttCmd::Publish { topic, payload });
        }
    }

    pub fn publish_message(&self, m: &MessageOut) {
        let payload = serde_json::to_vec(m).unwrap_or_default();
        self.publish(format!("meev/{}/messages", m.conversation_id), payload);
    }

    pub fn publish_typing(&self, conv: Uuid, user: Uuid) {
        let ev = TypingEvent { conversation_id: conv, user_id: user };
        let payload = serde_json::to_vec(&ev).unwrap_or_default();
        self.publish(format!("meev/{conv}/typing"), payload);
    }

    pub fn publish_presence(&self, user: Uuid, online: bool) {
        let ev = PresenceEvent { user_id: user, online, at: Utc::now() };
        let payload = serde_json::to_vec(&ev).unwrap_or_default();
        self.publish(format!("meev/presence/{user}"), payload);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

/// Spawn publisher + subscriber tasks.
pub fn start(state: AppState) -> MqttHandle {
    if !state.cfg.mqtt_enabled {
        tracing::info!("MQTT is disabled by configuration (MQTT_ENABLED=false)");
        return MqttHandle::disabled();
    }

    let client_id = format!("meev-backend-{}", Uuid::new_v4().simple());
    let mut opts = MqttOptions::new(client_id, url_host(&state.cfg.mqtt_url), url_port(&state.cfg.mqtt_url));
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    if let Some(u) = &state.cfg.mqtt_username {
        opts.set_credentials(u, state.cfg.mqtt_password.as_deref().unwrap_or(""));
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 10);
    let (pub_tx, mut pub_rx) = mpsc::unbounded_channel::<MqttCmd>();
    let handle = MqttHandle {
        tx: Some(pub_tx),
        enabled: true,
        connected: Arc::new(AtomicBool::new(false)),
    };

    tokio::spawn(async move {
        // Publisher task
        while let Some(cmd) = pub_rx.recv().await {
            match cmd {
                MqttCmd::Publish { topic, payload } => {
                    if let Err(e) = client.publish(&topic, QoS::AtLeastOnce, false, payload).await {
                        tracing::debug!("mqtt publish failed on {topic}: {e}");
                    }
                }
            }
        }
    });

    let h2 = handle.clone();
    tokio::spawn(async move {
        // Subscriber / event-loop task with automatic reconnect.
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    h2.connected.store(true, Ordering::Relaxed);
                    tracing::info!("MQTT connected to {}", state.cfg.mqtt_url);
                    let subs = vec![
                        (TOPIC_MESSAGES, QoS::AtLeastOnce),
                        (TOPIC_TYPING, QoS::AtLeastOnce),
                        (TOPIC_PRESENCE, QoS::AtLeastOnce),
                    ];
                    if let Err(e) = client.subscribe_many(subs).await {
                        tracing::warn!("mqtt subscribe failed: {e}");
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    let topic = p.topic.clone();
                    let payload = p.payload.to_vec();
                    let st = state.clone();
                    tokio::spawn(async move { handle_topic(&st, &topic, &payload).await });
                }
                Ok(Event::Incoming(other)) => {
                    tracing::trace!("mqtt incoming: {other:?}");
                }
                Ok(Event::Outgoing(_)) => {}
                Err(e) => {
                    if h2.connected.swap(false, Ordering::Relaxed) {
                        tracing::warn!("MQTT connection lost: {e} (retrying...)");
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    });

    handle
}

async fn handle_topic(state: &AppState, topic: &str, payload: &[u8]) {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() != 3 {
        return;
    }
    let kind = parts[1];
    let _id = parts[2];
    if kind != "messages" && kind != "typing" && kind != "presence" {
        return;
    }

    // Dedup: events we originate locally (REST handlers) are already delivered
    // to local sockets, so ignore the broker echo.
    if !state.dedup.see(topic).await {
        return;
    }

    match kind {
        "messages" => {
            let msg: MessageOut = match serde_json::from_slice(payload) {
                Ok(m) => m,
                Err(_) => return,
            };
            // Validate the sender is a member of the conversation before fanning out.
            let is_member: Option<bool> = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE conversation_id=$1 AND user_id=$2)",
            )
            .bind(msg.conversation_id)
            .bind(msg.sender.id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
            if is_member != Some(true) {
                tracing::warn!("MQTT ignored message from non-member on {}", msg.conversation_id);
                return;
            }
            let data = serde_json::to_value(&msg).unwrap_or_default();
            state.hub.broadcast_room(msg.conversation_id, "message", data).await;
        }
        "typing" => {
            let Ok(ev) = serde_json::from_slice::<TypingEvent>(payload) else { return };
            let data = serde_json::to_value(&ev).unwrap_or_default();
            state.hub.broadcast_room(ev.conversation_id, "typing", data).await;
        }
        "presence" => {
            let Ok(ev) = serde_json::from_slice::<PresenceEvent>(payload) else { return };
            let data = serde_json::to_value(&ev).unwrap_or_default();
            state.hub.broadcast_all("presence", data).await;
        }
        _ => {}
    }
}

fn url_host(url: &str) -> String {
    let rest = url.strip_prefix("mqtt://").or_else(|| url.strip_prefix("tcp://")).or_else(|| url.strip_prefix("ssl://"));
    let rest = rest.unwrap_or(url);
    let host = rest.split(':').next().unwrap_or("localhost");
    host.to_string()
}

fn url_port(url: &str) -> u16 {
    let rest = url.strip_prefix("mqtt://").or_else(|| url.strip_prefix("tcp://")).or_else(|| url.strip_prefix("ssl://"));
    let rest = rest.unwrap_or(url);
    if let Some(after) = rest.split(':').nth(1) {
        if let Some(port) = after.split('/').next().and_then(|p| p.parse::<u16>().ok()) {
            return port;
        }
    }
    if url.starts_with("ssl://") || url.starts_with("mqtts://") {
        8883
    } else {
        1883
    }
}
