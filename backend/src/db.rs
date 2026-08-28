//! PostgreSQL connection pool + automatic schema migrations.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::config::Config;

pub async fn connect(cfg: &Config) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.db_max_connections)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&cfg.database_url)
        .await?;

    if cfg.auto_migrate {
        sqlx::migrate!("./migrations").run(&pool).await?;
        tracing::info!("database schema is up to date");
    }

    Ok(pool)
}
