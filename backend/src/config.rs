//! MEEV configuration loader.
//!
//! All settings come from a secure `.env` file (never committed).
//! The loader validates every value and fails fast with a clear message.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_access_ttl_secs: i64,
    pub jwt_refresh_ttl_secs: i64,
    pub db_max_connections: u32,
    pub rate_limit_requests: u32,
    pub rate_limit_window_secs: u64,
    pub mqtt_enabled: bool,
    pub mqtt_url: String,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub ffmpeg_path: PathBuf,
    pub ffprobe_path: PathBuf,
    pub upload_dir: PathBuf,
    pub static_dir: PathBuf,
    pub max_upload_bytes: usize,
    pub cors_origins: Vec<String>,
    pub trust_proxy: bool,
    pub log_level: String,
    pub auto_migrate: bool,
    pub max_message_len: usize,
    pub max_attachment_per_message: usize,
    pub media_timeout_secs: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        // Load `.env` if present (does not override real environment variables).
        if let Err(e) = dotenvy::dotenv() {
            if !e.not_found() {
                println!("[config] dotenv: {e}");
            }
        }

        let get = |k: &str| -> Result<String, String> {
            env::var(k).map_err(|_| format!("Missing required environment variable `{k}` in the .env file"))
        };
        let get_opt = |k: &str| env::var(k).ok().filter(|v| !v.trim().is_empty());
        let get_u64 = |k: &str, min: u64| -> Result<u64, String> {
            let v: u64 = get(k)?.parse().map_err(|_| format!("`{k}` must be an unsigned integer"))?;
            if v < min {
                return Err(format!("`{k}` must be >= {min}"));
            }
            Ok(v)
        };
        let get_u64_opt = |k: &str, min: u64, default: u64| -> Result<u64, String> {
            match get_opt(k) {
                Some(raw) => {
                    let v: u64 = raw.parse().map_err(|_| format!("`{k}` must be an unsigned integer"))?;
                    if v < min {
                        return Err(format!("`{k}` must be >= {min}"));
                    }
                    Ok(v)
                }
                None => Ok(default),
            }
        };

        let host = get_opt("APP_HOST").unwrap_or_else(|| "0.0.0.0".into());
        let port = get_u64_opt("APP_PORT", 1, 8080)? as u16;

        let database_url = get("DATABASE_URL")?;
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err("`DATABASE_URL` must start with postgres:// or postgresql://".into());
        }

        let jwt_secret = get_opt("JWT_SECRET").unwrap_or_default();
        if jwt_secret.len() < 32 {
            return Err("`JWT_SECRET` is too weak: it must be at least 32 characters (use the generated secret from .env.example)".into());
        }

        let ffmpeg_path = PathBuf::from(get_opt("FFMPEG_PATH").unwrap_or_else(|| "/usr/bin/ffmpeg".into()));
        let ffprobe_path =
            PathBuf::from(get_opt("FFPROBE_PATH").unwrap_or_else(|| "/usr/bin/ffprobe".into()));

        let upload_dir = PathBuf::from(get_opt("UPLOAD_DIR").unwrap_or_else(|| "./uploads".into()));
        let static_dir = PathBuf::from(get_opt("STATIC_DIR").unwrap_or_else(|| "./static".into()));

        let max_upload_mb = get_u64_opt("MAX_UPLOAD_MB", 1, 50)?;
        let cors = get_opt("CORS_ORIGINS").unwrap_or_else(|| "http://localhost:5173,http://localhost:4173".into());
        let cors_origins: Vec<String> = cors.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

        let mqtt_enabled = get_opt("MQTT_ENABLED").unwrap_or_else(|| "true".into()) == "true";

        Ok(Config {
            host,
            port,
            database_url,
            jwt_secret,
            jwt_access_ttl_secs: get_u64_opt("JWT_ACCESS_TTL_SECONDS", 60, 900)? as i64,
            jwt_refresh_ttl_secs: get_u64_opt("JWT_REFRESH_TTL_SECONDS", 3600, 2592000)? as i64,
            db_max_connections: get_u64_opt("DB_MAX_CONNECTIONS", 1, 10)? as u32,
            rate_limit_requests: get_u64_opt("RATE_LIMIT_REQUESTS", 1, 120)? as u32,
            rate_limit_window_secs: get_u64_opt("RATE_LIMIT_WINDOW_SECONDS", 1, 60)?,
            mqtt_enabled,
            mqtt_url: get_opt("MQTT_URL").unwrap_or_else(|| "tcp://localhost:1883".into()),
            mqtt_username: get_opt("MQTT_USERNAME"),
            mqtt_password: get_opt("MQTT_PASSWORD"),
            ffmpeg_path,
            ffprobe_path,
            upload_dir,
            static_dir,
            max_upload_bytes: (max_upload_mb * 1024 * 1024) as usize,
            cors_origins,
            trust_proxy: get_opt("TRUST_PROXY").unwrap_or_else(|| "false".into()) == "true",
            log_level: get_opt("LOG_LEVEL").unwrap_or_else(|| "info".into()),
            auto_migrate: get_opt("AUTO_MIGRATE").unwrap_or_else(|| "true".into()) == "true",
            max_message_len: get_u64_opt("MAX_MESSAGE_LENGTH", 1, 4000)? as usize,
            max_attachment_per_message: get_u64_opt("MAX_ATTACHMENTS_PER_MESSAGE", 0, 1)? as usize,
            media_timeout_secs: get_u64_opt("MEDIA_TIMEOUT_SECONDS", 5, 120)?,
        })
    }

    pub fn bind_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port).parse().expect("invalid bind address")
    }

    pub fn media_timeout(&self) -> Duration {
        Duration::from_secs(self.media_timeout_secs)
    }
}
