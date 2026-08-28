//! MEEV backend — secure messenger with PostgreSQL + MQTT.
//!
//! Architecture:
//!   REST API (Axum) -> PostgreSQL (parameterized SQL only)
//!                  -> MQTT message bus  -> WebSocket fan-out to browsers.
//! All configuration comes from an untracked `.env` file.

mod auth;
mod config;
mod db;
mod error;
mod hub;
mod media;
mod models;
mod mqtt;
mod rate_limit;
mod routes;
mod state;
mod suggest;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::header;
use axum::http::{HeaderValue, Method};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::fs::{ServeDir, ServeFile};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::mqtt::MqttHandle;
use crate::rate_limit::{client_ip, RateLimiter};
use crate::state::AppState;

#[tokio::main]
async fn main() {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[MEEV] configuration error: {e}");
            eprintln!("[MEEV] copy meev.env.example to .env and fill the values, then start again.");
            std::process::exit(1);
        }
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.log_level)),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("MEEV backend starting (v{})", env!("CARGO_PKG_VERSION"));

    let pool = match db::connect(&config).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("cannot connect to PostgreSQL: {e}");
            std::process::exit(1);
        }
    };

    // Rate limiter: 120 requests / minute by default.
    let limiter = RateLimiter::new(config.rate_limit_requests, config.rate_limit_window_secs);

    // Create state first, then wire MQTT.
    let mut state = AppState::new(pool, config, limiter, MqttHandle::disabled());
    let mqtt = mqtt::start(state.clone());
    state.mqtt = mqtt;

    let addr = bind_addr_from(&state);
    let app = build_router(state);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("listening on http://{addr}");

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("server error: {e}");
    }
}

fn bind_addr_from(state: &AppState) -> std::net::SocketAddr {
    state.cfg.bind_addr()
}

/// Security headers + request logging, applied to every response.
async fn security_headers(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("x-xss-protection", HeaderValue::from_static("1; mode=block"));
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(self), microphone=(self), geolocation=(self)"),
    );
    resp
}

async fn rate_limit_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let ip = client_ip(&state.cfg, req.headers());
    let user_key = auth::bearer_token(req.parts())
        .and_then(|token| auth::verify_access_token(&state.cfg, &token).ok())
        .map(|c| c.sub);

    let key = match user_key {
        Some(uid) => format!("u:{uid}"),
        None => format!("ip:{ip}"),
    };
    match state.limiter.check(&key) {
        Ok(()) => next.run(req).await,
        Err(after) => {
            tracing::warn!("rate limited: {key} for {after}s");
            let mut resp = (axum::http::StatusCode::TOO_MANY_REQUESTS, "rate limit").into_response();
            resp.headers_mut().insert("retry-after", after.to_string().parse().unwrap_or("60".parse().unwrap()));
            resp
        }
    }
}

fn cors_layer(cfg: &Config) -> CorsLayer {
    let mut layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::ORIGIN,
        ])
        .expose_headers([header::CONTENT_TYPE, header::CONTENT_LENGTH, header::CACHE_CONTROL])
        .max_age(std::time::Duration::from_secs(86400));
    if cfg.cors_origins.iter().any(|o| o == "*") {
        layer = layer.allow_origin(Any);
    } else {
        let origins: Vec<HeaderValue> = cfg
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        layer = layer.allow_origin(origins);
    }
    layer
}

fn build_router(state: AppState) -> Router {
    let cfg = state.cfg.clone();
    let max_body = cfg.max_upload_bytes + 1024 * 1024;

    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/register", post(routes::auth::register))
        .route("/api/auth/login", post(routes::auth::login))
        .route("/api/auth/refresh", post(routes::auth::refresh))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me", get(routes::auth::me))
        .route("/api/users/search", get(routes::users::search_users))
        .route("/api/users/me", axum::routing::patch(routes::users::update_profile))
        .route("/api/users/me/interests", axum::routing::put(routes::users::set_interests))
        .route("/api/users/me/location", axum::routing::patch(routes::users::update_location))
        .route("/api/users/me/password", axum::routing::patch(routes::users::change_password))
        .route("/api/users/me/avatar", post(routes::users::upload_avatar))
        .route("/api/users/me/banner", post(routes::users::upload_banner))
        .route("/api/users/me/followers", get(routes::users::followers))
        .route("/api/users/me/following", get(routes::users::following))
        .route("/api/users/:username", get(routes::users::get_profile))
        .route("/api/users/:username/follow", post(routes::users::follow_user))
        .route("/api/users/:username/unfollow", axum::routing::delete(routes::users::unfollow_user))
        .route("/api/suggestions", get(routes::suggestions::get_suggestions))
        .route("/api/interests", get(routes::suggestions::suggest_interests))
        .route("/api/conversations", get(routes::chat::list_conversations))
        .route("/api/conversations", post(routes::chat::create_dm))
        .route("/api/conversations/group", post(routes::chat::create_group))
        .route("/api/conversations/:id/messages", get(routes::chat::list_messages))
        .route("/api/conversations/:id/messages", post(routes::chat::send_message))
        .route("/api/conversations/:id/read", post(routes::chat::mark_read))
        .route("/api/conversations/:id/typing", post(routes::chat::typing))
        .route("/api/media", post(routes::media::upload))
        .route("/api/media/:id", get(routes::media::metadata))
        .route("/api/media/:id/file", get(routes::media::file))
        .route("/api/media/:id/thumb", get(routes::media::thumb))
        .route("/ws", get(routes::ws::ws_handler))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state.clone());

    let serve_dir = ServeDir::new(&cfg.static_dir)
        .not_found_service(ServeFile::new(cfg.static_dir.join("index.html")));

    Router::new()
        .merge(api)
        .fallback_service(serve_dir)
        .layer(axum::middleware::from_fn(security_headers))
        .layer(axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
        .layer(CompressionLayer::new())
        .layer(SetRequestIdLayer::new(
            header::HeaderName::from_static("x-request-id"),
            MakeRequestUuid,
        ))
        .layer(PropagateRequestIdLayer::new(header::HeaderName::from_static("x-request-id")))
        .layer(CatchPanicLayer::new())
        .layer(cors_layer(&cfg))
        .layer(TraceLayer::new_for_http())
}

pub async fn health(State(state): State<AppState>) -> axum::Json<serde_json::Value> {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    axum::Json(serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "service": "meev-backend",
        "version": env!("CARGO_PKG_VERSION"),
        "mqtt_connected": state.mqtt.is_connected(),
        "mqtt_enabled": state.mqtt.enabled,
        "online_sockets": state.hub.online_count().await,
        "rate_limit_per_minute": state.limiter.requests_per_minute(),
        "time": chrono::Utc::now().to_rfc3339(),
    }))
}
