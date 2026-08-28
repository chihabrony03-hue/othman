//! In-process WebSocket hub.
//! Keeps connected sockets per user and per conversation room so that
//! realtime events can be pushed instantly. MQTT is used as the message
//! bus between backend instances; the hub delivers locally.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub type Sender = mpsc::UnboundedSender<String>;

#[derive(Default)]
pub struct Hub {
    // user_id -> (connection_id -> sender)
    users: RwLock<HashMap<Uuid, HashMap<Uuid, Sender>>>,
    // conversation_id -> set of user ids
    rooms: RwLock<HashMap<Uuid, HashSet<Uuid>>>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn connect(&self, user_id: Uuid, tx: Sender) -> Uuid {
        let conn = Uuid::new_v4();
        let mut users = self.users.write().await;
        users.entry(user_id).or_default().insert(conn, tx);
        conn
    }

    pub async fn disconnect(&self, user_id: Uuid, conn: Uuid) {
        let mut users = self.users.write().await;
        let mut was_last = false;
        if let Some(map) = users.get_mut(&user_id) {
            map.remove(&conn);
            if map.is_empty() {
                users.remove(&user_id);
                was_last = true;
            }
        }
        drop(users);
        if was_last {
            let mut rooms = self.rooms.write().await;
            for members in rooms.values_mut() {
                members.remove(&user_id);
            }
        }
    }

    pub async fn is_online(&self, user_id: Uuid) -> bool {
        self.users.read().await.contains_key(&user_id)
    }

    pub async fn join_room(&self, user_id: Uuid, room: Uuid) {
        let mut rooms = self.rooms.write().await;
        rooms.entry(room).or_default().insert(user_id);
    }

    pub async fn leave_room(&self, user_id: Uuid, room: Uuid) {
        let mut rooms = self.rooms.write().await;
        if let Some(members) = rooms.get_mut(&room) {
            members.remove(&user_id);
        }
    }

    pub async fn broadcast_room(&self, room: Uuid, kind: &str, data: Value) {
        let payload = json!({ "type": kind, "data": data });
        let users = self.users.read().await;
        let rooms = self.rooms.read().await;
        let Some(members) = rooms.get(&room) else { return };
        for member in members {
            if let Some(sockets) = users.get(member) {
                for tx in sockets.values() {
                    let _ = tx.send(payload.to_string());
                }
            }
        }
    }

    pub async fn broadcast_user(&self, user_id: Uuid, kind: &str, data: Value) {
        let payload = json!({ "type": kind, "data": data });
        let users = self.users.read().await;
        if let Some(sockets) = users.get(&user_id) {
            for tx in sockets.values() {
                let _ = tx.send(payload.to_string());
            }
        }
    }

    /// Broadcast to every connected client (used for presence).
    pub async fn broadcast_all(&self, kind: &str, data: Value) {
        let payload = json!({ "type": kind, "data": data });
        let users = self.users.read().await;
        for sockets in users.values() {
            for tx in sockets.values() {
                let _ = tx.send(payload.to_string());
            }
        }
    }

    pub async fn online_count(&self) -> usize {
        self.users.read().await.len()
    }
}

/// Small TTL deduplication table used by the MQTT bridge to avoid a
/// locally-broadcast event being delivered twice by the broker echo.
pub struct Dedup {
    map: RwLock<HashMap<String, std::time::Instant>>,
}

impl Dedup {
    pub fn new() -> Self {
        Self { map: RwLock::new(HashMap::new()) }
    }

    /// Returns true if the key was NOT seen recently (i.e. caller should process it)
    /// and records it.
    pub async fn see(&self, key: &str) -> bool {
        let now = std::time::Instant::now();
        let mut map = self.map.write().await;
        // prune occasionally
        if map.len() > 50_000 {
            map.retain(|_, t| now.duration_since(*t) < std::time::Duration::from_secs(90));
        }
        if let Some(t) = map.get(key) {
            if now.duration_since(*t) < std::time::Duration::from_secs(90) {
                return false;
            }
        }
        map.insert(key.to_string(), now);
        true
    }
}

impl Default for Dedup {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedDedup = Arc<Dedup>;
