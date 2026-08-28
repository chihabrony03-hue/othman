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

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, OriginalUri, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;

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

// ---------------------------------------------------------------------------
// Middleware (small hand-rolled layers; no heavy HTTP stack)
// ---------------------------------------------------------------------------

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

fn apply_cors(cfg: &Config, origin: Option<&str>, resp: &mut Response) {
    let allowed = cfg.cors_origins.iter().any(|o| o == "*")
        || origin.is_some_and(|o| cfg.cors_origins.iter().any(|c| c == o));
    if let Some(o) = origin {
        if allowed {
            let h = resp.headers_mut();
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_str(o).unwrap_or_else(|_| HeaderValue::from_static("*")));
            h.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, HeaderValue::from_static("true"));
            h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("authorization, content-type, accept, origin"));
            h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"));
            h.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("content-type, content-length, cache-control"));
            h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
        }
    }
}

async fn cors_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|o| o.to_string());
    if req.method() == Method::OPTIONS {
        let mut resp = StatusCode::NO_CONTENT.into_response();
        apply_cors(&state.cfg, origin.as_deref(), &mut resp);
        return resp;
    }
    let mut resp = next.run(req).await;
    apply_cors(&state.cfg, origin.as_deref(), &mut resp);
    resp
}

async fn logging_middleware(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let started = std::time::Instant::now();
    let resp = next.run(req).await;
    tracing::info!("{method} {path} -> {} ({}ms)", resp.status(), started.elapsed().as_millis());
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
            let mut resp: Response = StatusCode::TOO_MANY_REQUESTS.into_response();
            resp.headers_mut().insert("retry-after", after.to_string().parse().unwrap_or("60".parse().unwrap()));
            resp
        }
    }
}

// ---------------------------------------------------------------------------
// Static frontend serving (SPA) — security-aware, cache-friendly.
// ---------------------------------------------------------------------------

async fn static_file(state: &AppState, rel: &str) -> Response {
    let root = &state.cfg.static_dir;
    let rel = rel.trim_start_matches('/');
    let path = if rel.is_empty() {
        root.join("index.html")
    } else {
        root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
    };
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let meta = tokio::fs::metadata(&path).await;
    let (file_path, is_asset) = match meta {
        Ok(m) if m.is_file() => {
            let is_asset = path
                .to_string_lossy()
                .to_lowercase()
                .contains("/assets/");
            (path, is_asset)
        }
        _ => {
            // SPA fallback: serve index.html for client-side routes.
            (root.join("index.html"), false)
        }
    };
    let Ok(bytes) = tokio::fs::read(&file_path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
    let mut resp = Response::new(Body::from(bytes));
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.essence_str()).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")));
    h.insert(
        header::CACHE_CONTROL,
        if is_asset {
            HeaderValue::from_static("public, max-age=31536000, immutable")
        } else {
            HeaderValue::from_static("no-cache")
        },
    );
    resp
}

pub async fn spa_fallback(State(state): State<AppState>, uri: OriginalUri) -> Response {
    static_file(&state, uri.path()).await
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

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

    let fallback = Router::new().fallback(get(spa_fallback)).with_state(state.clone());

    Router::new()
        .merge(api)
        .fallback_service(fallback)
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(logging_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), cors_middleware))
        .layer(middleware::from_fn_with_state(state, rate_limit_middleware))
}

pub async fn health(State(state): State<AppState>) -> axum::Json<serde_json::Value> {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    axum::Json(json!({
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
