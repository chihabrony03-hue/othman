#!/usr/bin/env bash
# Generates a secure .env with strong random secrets for MEEV.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/.env}"

secret() { openssl rand -base64 48 | tr -d '\n'; }
dbpass() { openssl rand -base64 32 | tr -d '\n'; }

echo "✔ Generating $OUT ..."
cat > "$OUT" <<EOF
# MEEV environment (auto-generated $(date -u +%FT%TZ))
APP_HOST=0.0.0.0
APP_PORT=8080
CORS_ORIGINS=http://localhost:5173,http://localhost:4173
TRUST_PROXY=false
DATABASE_URL=postgres://meev:$(dbpass)@localhost:5432/meev
DB_MAX_CONNECTIONS=10
AUTO_MIGRATE=true
JWT_SECRET=$(secret)
JWT_ACCESS_TTL_SECONDS=900
JWT_REFRESH_TTL_SECONDS=2592000
RATE_LIMIT_REQUESTS=120
RATE_LIMIT_WINDOW_SECONDS=60
MQTT_ENABLED=true
MQTT_URL=tcp://localhost:1883
MQTT_USERNAME=
MQTT_PASSWORD=
FFMPEG_PATH=/usr/bin/ffmpeg
FFPROBE_PATH=/usr/bin/ffprobe
MAX_UPLOAD_MB=50
UPLOAD_DIR=./uploads
MEDIA_TIMEOUT_SECONDS=120
STATIC_DIR=./static
MAX_MESSAGE_LENGTH=4000
MAX_ATTACHMENTS_PER_MESSAGE=1
LOG_LEVEL=info
EOF
chmod 600 "$OUT"
echo "✔ Done. Edit $OUT (database password must match the one you created in PostgreSQL)."
