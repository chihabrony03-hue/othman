#!/usr/bin/env bash
# ============================================================
#  MEEV — packaging script
#  Builds the two final deliverables:
#    1) MEEV-source.zip      — complete source code
#    2) MEEV-release-*.zip   — built, runnable release
#  Usage:
#    bash scripts/make-zips.sh            # both
#    bash scripts/make-zips.sh source     # source only
#    bash scripts/make-zips.sh release    # release only
# ============================================================
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT_DIR="${OUT_DIR:-$ROOT/release}"
mkdir -p "$OUT_DIR"

MODE="${1:-both}"

# ------------------------------------------------------------
# 1) SOURCE ARCHIVE
# ------------------------------------------------------------
if [ "$MODE" = "both" ] || [ "$MODE" = "source" ]; then
  echo "==> Building MEEV-source.zip ..."
  rm -rf /tmp/meev-source && mkdir -p /tmp/meev-source/MEEV-source
  # everything tracked by git, minus .git; plus generated-lock files
  git archive --format=tar HEAD | tar -x -C /tmp/meev-source/MEEV-source
  # restore CI workflow (it is part of the deliverable even while it is
  # temporarily absent from the pushed branch)
  mkdir -p /tmp/meev-source/MEEV-source/.github/workflows
  git show 35ecf26:.github/workflows/build.yml \
    > /tmp/meev-source/MEEV-source/.github/workflows/build.yml 2>/dev/null || true
  # keep the resolved dependency lockfile in the archive
  cp backend/Cargo.lock /tmp/meev-source/MEEV-source/backend/Cargo.lock
  rm -rf /tmp/meev-source/MEEV-source/frontend/node_modules \
         /tmp/meev-source/MEEV-source/frontend/dist
  (cd /tmp/meev-source && zip -qr "$OUT_DIR/MEEV-source.zip" MEEV-source)
  echo "    -> $OUT_DIR/MEEV-source.zip"
fi

# ------------------------------------------------------------
# 2) RELEASE ARCHIVE (requires release/meev-linux-x86_64/meev-backend)
# ------------------------------------------------------------
if [ "$MODE" = "both" ] || [ "$MODE" = "release" ]; then
  echo "==> Building MEEV-release zip ..."
  STAGE="$ROOT/release/meev-linux-x86_64"
  # locate the built binary: $MEEV_BINARY > backend/target/release > downloaded artifact
  SRC_BIN="${MEEV_BINARY:-}"
  if [ -z "$SRC_BIN" ] && [ -x "$ROOT/backend/target/release/meev-backend" ]; then
    SRC_BIN="$ROOT/backend/target/release/meev-backend"
  fi
  if [ -z "$SRC_BIN" ] && [ -f "$ROOT/release/meev-backend-linux-x86_64" ]; then
    SRC_BIN="$ROOT/release/meev-backend-linux-x86_64"
  fi
  if [ -z "$SRC_BIN" ]; then
    echo "    meev-backend binary not found" >&2
    echo "    Drop the CI artifact binary at release/meev-backend-linux-x86_64," >&2
    echo "    or build locally: cd backend && cargo build --release" >&2
    exit 1
  fi
  rm -rf "$STAGE" && mkdir -p "$STAGE"
  cp "$SRC_BIN" "$STAGE/meev-backend"
  # frontend dist must exist (npm run build)
  if [ -d "$ROOT/frontend/dist" ]; then
    cp -r "$ROOT/frontend/dist" "$STAGE/static"
  else
    echo "    frontend/dist missing — run: cd frontend && npm run build" >&2
    exit 1
  fi
  cp "$ROOT/backend/.env.example" "$STAGE/meev.env.example"
  cp "$ROOT/scripts/start.sh" "$ROOT/scripts/gen-env.sh" "$STAGE/"
  cp "$ROOT/README.md" "$STAGE/README.md"
  chmod +x "$STAGE/start.sh" "$STAGE/gen-env.sh" "$STAGE/meev-backend"
  (cd "$ROOT/release" && zip -qr "MEEV-release-linux-x86_64.zip" meev-linux-x86_64)
  echo "    -> $OUT_DIR/MEEV-release-linux-x86_64.zip"
fi

echo "==> Done."
