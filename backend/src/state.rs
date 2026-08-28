//! Shared application state.

use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;
use crate::hub::{Dedup, Hub, SharedDedup};
use crate::mqtt::MqttHandle;
use crate::rate_limit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cfg: Arc<Config>,
    pub hub: Arc<Hub>,
    pub limiter: Arc<RateLimiter>,
    pub mqtt: MqttHandle,
    pub dedup: SharedDedup,
}

impl AppState {
    pub fn new(pool: PgPool, cfg: Config, limiter: RateLimiter, mqtt: MqttHandle) -> Self {
        Self {
            pool,
            cfg: Arc::new(cfg),
            hub: Arc::new(Hub::new()),
            limiter: Arc::new(limiter),
            mqtt,
            dedup: Arc::new(Dedup::new()),
        }
    }

    /// Mark an event that we generated locally so the MQTT broker echo is not
    /// fanned out twice to local clients.
    pub async fn mark_local_event(&self, key: &str) {
        let _ = self.dedup.see(key).await;
    }
}
