//! PostgreSQL connection pool + schema initialization.
//!
//! The schema is applied at startup with idempotent SQL
//! (CREATE TABLE IF NOT EXISTS ...), so no external migration tool is needed
//! and the app is truly ready-to-run from the release package.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::config::Config;

const SCHEMA_SQL: &str = include_str!("../migrations/0001_init.sql");

pub async fn connect(cfg: &Config) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.db_max_connections)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&cfg.database_url)
        .await?;

    if cfg.auto_migrate {
        apply_schema(&pool).await?;
        tracing::info!("database schema is up to date");
    }

    Ok(pool)
}

/// Execute the idempotent schema. Statements are split on ';' — the schema
/// contains no functions/triggers, so simple splitting is safe.
pub async fn apply_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in SCHEMA_SQL.split(';') {
        let stmt = statement.trim();
        if stmt.is_empty() || stmt.starts_with("--") {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }
    Ok(())
}
