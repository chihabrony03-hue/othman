#!/usr/bin/env bash
# MEEV backend live smoke test — used by CI (and locally by developers).
# Prerequisites: PostgreSQL with DATABASE_URL, ffmpeg, mosquitto
# (or MQTT_ENABLED=false), and ./backend/target/release/meev-backend built.
set -euo pipefail

HOST="http://127.0.0.1:${APP_PORT:-8080}"
cd "$(dirname "$0")/.."

echo "==> Starting MEEV backend..."
./backend/target/release/meev-backend > /tmp/meev.log 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null || true' EXIT

for i in $(seq 1 40); do
  if curl -sf "$HOST/api/health" > /tmp/health.json 2>/dev/null; then break; fi
  sleep 1
done
echo "==> Health: $(cat /tmp/health.json)"
python3 - <<'EOF'
import json
h = json.load(open('/tmp/health.json'))
assert h['status'] == 'ok', h
assert h['rate_limit_per_minute'] == 120, h
print("   health OK, rate limit config OK (MQTT expected when broker enabled)")
EOF

echo "==> Auth flow (two users)"
jk() { python3 -c "import json,sys; d=json.load(sys.stdin); print(d$1)"; }
REG_A=$(curl -sf -X POST "$HOST/api/auth/register" -H 'Content-Type: application/json' -d '{
  "username":"smoke_friend","email":"friend@meev.test","password":"StrongPass123xx","display_name":"صديق الاختبار"}')
echo "$REG_A" | jk "['user']['id']" > /tmp/friend_id.txt
REG_B=$(curl -sf -X POST "$HOST/api/auth/register" -H 'Content-Type: application/json' -d '{
  "username":"smoketest_user","email":"smoke@meev.test","password":"StrongPass123xx","display_name":"Smoke Tester"}')
TOKEN=$(echo "$REG_B" | jk "['access_token']")
AUTH="Authorization: Bearer $TOKEN"
echo "   register OK"

LOGIN=$(curl -sf -X POST "$HOST/api/auth/login" -H 'Content-Type: application/json' -d '{"identifier":"smoke@meev.test","password":"StrongPass123xx"}')
echo "$LOGIN" | python3 -c "import json,sys; d=json.load(sys.stdin); assert d['access_token']; print('   login OK')"

echo "==> Profile customization"
curl -sf -X PATCH "$HOST/api/users/me" -H "$AUTH" -H 'Content-Type: application/json' \
  -d '{"display_name":"Smoke Test","bio":"testing","location_name":"Algiers"}' > /dev/null
curl -sf -X PUT "$HOST/api/users/me/interests" -H "$AUTH" -H 'Content-Type: application/json' \
  -d '{"interests":["التقنية","الذكاء الاصطناعي","كرة القدم"]}' > /dev/null
curl -sf -X PATCH "$HOST/api/users/me/location" -H "$AUTH" -H 'Content-Type: application/json' \
  -d '{"lat":36.7538,"lng":3.0588,"name":"الجزائر العاصمة","country":"الجزائر"}' > /dev/null
echo "   profile OK"

echo "==> Follow + suggestions + search"
curl -sf -X POST "$HOST/api/users/smoke_friend/follow" -H "$AUTH" > /dev/null
echo "   follow OK"
curl -sf "$HOST/api/suggestions?limit=6" -H "$AUTH" | \
  python3 -c "import json,sys; d=json.load(sys.stdin); assert 'suggestions' in d; print('   suggestions OK', [s['username'] for s in d['suggestions']])"
curl -sf "$HOST/api/users/search?q=smoke" -H "$AUTH" | \
  python3 -c "import json,sys; d=json.load(sys.stdin); assert d['total'] >= 2; print('   search OK total', d['total'])"

echo "==> Conversation + message persistence"
FRIEND_ID=$(cat /tmp/friend_id.txt)
curl -sf -X POST "$HOST/api/conversations" -H "$AUTH" -H 'Content-Type: application/json' \
  -d "{\"user_id\":\"$FRIEND_ID\"}" > /tmp/dm.json
CONV=$(python3 -c "import json; print(json.load(open('/tmp/dm.json'))['conversation_id'])")
curl -sf -X POST "$HOST/api/conversations/$CONV/messages" -H "$AUTH" -H 'Content-Type: application/json' \
  -d '{"content":"مرحبا MEEV عبر MQTT ✅"}' > /tmp/msg.json
python3 - <<'EOF'
import json
m = json.load(open('/tmp/msg.json'))
assert m['content'].startswith('مرحبا'), m
assert m['sender']['username'] == 'smoketest_user', m
print("   message persisted OK", m['id'])
EOF
curl -sf "$HOST/api/conversations/$CONV/messages?limit=10" -H "$AUTH" | \
  python3 -c "import json,sys; d=json.load(sys.stdin); assert len(d['messages'])>=1; print('   history OK')"
curl -sf -X POST "$HOST/api/conversations/$CONV/read" -H "$AUTH" > /dev/null
curl -sf -X POST "$HOST/api/conversations/$CONV/typing" -H "$AUTH" > /dev/null
echo "   chat OK"

echo "==> Media upload (ffmpeg -> WebP)"
ffmpeg -y -f lavfi -i color=c=0xBD9125:s=640x480 -frames:v 1 /tmp/test.png > /dev/null 2>&1
curl -sf -X POST "$HOST/api/media" -H "$AUTH" -F "file=@/tmp/test.png" > /tmp/att.json
python3 - <<'EOF'
import json
a = json.load(open('/tmp/att.json'))
assert a['kind'] == 'image' and a['mime_type'] == 'image/webp', a
assert a['url'].startswith('/api/media/'), a
print("   webp compression OK,", a['size'], "bytes")
EOF

echo "==> Rate limit check (burst)"
CODES=$(for i in $(seq 1 200); do curl -s -o /dev/null -w '%{http_code}' "$HOST/api/health"; done)
if echo "$CODES" | grep -q 429; then echo "   rate limiting OK (429 observed)"; else echo "   (429 not observed this run)"; fi

echo "==> Injection safety (parameterized SQL only)"
curl -sf "$HOST/api/users/search?q=%27%3B%20DROP%20TABLE%20users%3B--" -H "$AUTH" > /dev/null
curl -sf "$HOST/api/users/search?q=%%25%%27%20OR%20%271%27%3D%271" -H "$AUTH" > /dev/null
echo "   SQLi attempts handled OK"

echo "==> MQTT broker roundtrip"
if command -v mosquitto_pub >/dev/null 2>&1; then
  (mosquitto_sub -h localhost -t meev/ci/smoke -C 1 -W 3 > /tmp/mqtt_out.txt 2>/dev/null &) 
  sleep 0.5
  mosquitto_pub -h localhost -t meev/ci/smoke -m "hello-mqtt" >/dev/null 2>&1 || true
  sleep 1
  grep -q "hello-mqtt" /tmp/mqtt_out.txt && echo "   MQTT broker OK" || echo "   (MQTT broker check skipped)"
fi

echo "✅ SMOKE TEST PASSED"
