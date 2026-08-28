#!/usr/bin/env bash
# MEEV release launcher.
# 1. Creates .env from meev.env.example if missing (with random secrets).
# 2. (Optional) creates the PostgreSQL role/database if psql is available.
# 3. Starts the backend (migrations run automatically when AUTO_MIGRATE=true).
set -euo pipefail

cd "$(dirname "$0")"

BIN="./meev-backend"
STATIC="./static"
UPLOADS="./uploads"
ENV="./.env"

if [ ! -x "$BIN" ]; then
  echo "✖ meev-backend not found or not executable on this platform." >&2
  echo "  → Available builds: release zip (Linux x86_64, glibc)." >&2
  echo "  → Build from source with: cargo build --release (see README)." >&2
  exit 1
fi

if [ ! -f "$ENV" ]; then
  echo "✔ Creating .env with random secrets..."
  bash ./gen-env.sh "$ENV"
  echo "  → Open $ENV and set the PostgreSQL password if you created the DB manually."
fi

# Extract DATABASE_URL so we can create the DB if needed.
DB_URL="$(grep -E '^DATABASE_URL=' "$ENV" | head -1 | cut -d= -f2-)"
DB_NAME="$(echo "$DB_URL" | sed -E 's|.*/([^?]+)(\?.*)?$|\1|')"
DB_USER="$(echo "$DB_URL" | sed -E 's|.*://([^:]+):.*|\1|')"
DB_PASS="$(echo "$DB_URL" | sed -E 's|.*://[^:]+:([^@]+)@.*|\1|')"

if command -v psql >/dev/null 2>&1; then
  echo "✔ Ensuring PostgreSQL database '$DB_NAME'..."
  sudo -n -u postgres psql -tc "SELECT 1 FROM pg_roles WHERE rolname='$DB_USER'" | grep -q 1 || \
    sudo -n -u postgres psql -c "CREATE ROLE $DB_USER LOGIN PASSWORD '$DB_PASS' CREATEDB;" || true
  sudo -n -u postgres psql -tc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1 || \
    sudo -n -u postgres psql -c "CREATE DATABASE $DB_NAME OWNER $DB_USER;" || true
  echo "✔ PostgreSQL ready."
else
  echo "⚠ psql not found — create the database yourself and verify DATABASE_URL."
fi

mkdir -p "$UPLOADS" "$STATIC"
echo "✔ Starting MEEV backend..."
exec "$BIN"
