#!/usr/bin/env bash
# End-to-end smoke test: real window, real IPC, real local MySQL + PostgreSQL.
#
#   e2e/run.sh            run the test
#   SHOTS=1 e2e/run.sh    also save a screenshot of the window at each stage (macOS)
#   KEEP_DB=1 e2e/run.sh  leave the `puricat_e2e` demo databases in place afterwards
#
# Needs local servers that accept:  mysql -u$MYSQL_USER -h127.0.0.1   /   psql -U$PG_USER -h127.0.0.1
# It creates ONE database on each server, `puricat_e2e`, and drops it again. Nothing else is touched,
# and the app runs against a throwaway settings directory with an in-memory secret store.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MYSQL_USER=${MYSQL_USER:-root}
PG_USER=${PG_USER:-$USER}
MYSQL=${MYSQL:-mysql}
PSQL=${PSQL:-psql}
WORK=$(mktemp -d)
REPORT="$WORK/report.json"

echo "› fixtures"
"$MYSQL" -u"$MYSQL_USER" -h127.0.0.1 -e 'DROP DATABASE IF EXISTS puricat_e2e; CREATE DATABASE puricat_e2e CHARACTER SET utf8mb4'
"$MYSQL" -u"$MYSQL_USER" -h127.0.0.1 puricat_e2e <"$ROOT/e2e/fixtures/mysql.sql"
"$PSQL" -qX -h127.0.0.1 -U"$PG_USER" -d postgres -c 'DROP DATABASE IF EXISTS puricat_e2e WITH (FORCE)' -c 'CREATE DATABASE puricat_e2e'
"$PSQL" -qX -h127.0.0.1 -U"$PG_USER" -d puricat_e2e -v ON_ERROR_STOP=1 -f "$ROOT/e2e/fixtures/postgres.sql" >/dev/null

sed -e "s/__MYSQL_USER__/$MYSQL_USER/" -e "s/__PG_USER__/$PG_USER/" "$ROOT/e2e/smoke.js" >"$WORK/smoke.js"

cleanup() {
  if [ "${KEEP_DB:-0}" != 1 ]; then
    "$MYSQL" -u"$MYSQL_USER" -h127.0.0.1 -e 'DROP DATABASE IF EXISTS puricat_e2e' 2>/dev/null || true
    "$PSQL" -qX -h127.0.0.1 -U"$PG_USER" -d postgres -c 'DROP DATABASE IF EXISTS puricat_e2e WITH (FORCE)' 2>/dev/null || true
  fi
  if [ -n "${APP_PID:-}" ]; then
    kill "$APP_PID" 2>/dev/null || true
  else
    pkill -f "$ROOT/target/debug/puricat" 2>/dev/null || true
    lsof -ti tcp:1420 2>/dev/null | xargs kill 2>/dev/null || true
  fi
}
trap cleanup EXIT

if [ -z "${APP:-}" ] && lsof -ti tcp:1420 >/dev/null 2>&1; then
  echo "✗ port 1420 is already in use; stop the other dev server first" >&2
  exit 1
fi

# APP=<binary> tests a bundled debug build (`pnpm tauri build --debug`): embedded assets and the
# production Content-Security-Policy instead of the Vite dev server.
cd "$ROOT"
if [ -n "${APP:-}" ]; then
  echo "› launching $APP"
  PURICAT_DATA_DIR="$WORK/data" PURICAT_SECRET_STORE=memory \
    PURICAT_E2E_SCRIPT="$WORK/smoke.js" PURICAT_E2E_REPORT="$REPORT" \
    "$APP" >"$WORK/app.log" 2>&1 &
  APP_PID=$!
else
  echo "› launching PuriCat (pnpm tauri dev)"
  PURICAT_DATA_DIR="$WORK/data" PURICAT_SECRET_STORE=memory \
    PURICAT_E2E_SCRIPT="$WORK/smoke.js" PURICAT_E2E_REPORT="$REPORT" \
    pnpm tauri dev >"$WORK/app.log" 2>&1 &
fi

window_id() {
  swift -e 'import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerName as String] as? String)?.lowercased() == "puricat" && (w[kCGWindowLayer as String] as? Int) == 0 {
  print(w[kCGWindowNumber as String] as! Int); break
}' 2>/dev/null
}

last=""
shot=0
for _ in $(seq 1 1200); do
  sleep 0.25
  [ -f "$REPORT" ] || continue
  stage=$(sed -n 's/^ "stage": "\(.*\)",\{0,1\}$/\1/p' "$REPORT" | head -1)
  [ "$stage" = "$last" ] && continue
  last=$stage
  echo "  · $stage"
  if [ "${SHOTS:-0}" = 1 ] && [ "$stage" != done ] && [ "$stage" != FAILED ]; then
    mkdir -p "$ROOT/screenshots/e2e"
    sleep 0.5
    id=$(window_id) && [ -n "$id" ] && screencapture -x -o -l "$id" "$ROOT/screenshots/e2e/$(printf '%02d' $shot)-$stage.png" 2>/dev/null || true
    shot=$((shot + 1))
  fi
  if [ "$stage" = done ] || [ "$stage" = FAILED ]; then break; fi
done

cp "$REPORT" "$ROOT/target/e2e-report.json" 2>/dev/null || true
if [ "$last" = done ]; then
  echo "✓ end-to-end smoke test passed — report: target/e2e-report.json"
else
  echo "✗ end-to-end smoke test failed at: ${last:-startup}"
  [ -f "$REPORT" ] && cat "$REPORT"
  tail -30 "$WORK/app.log"
  exit 1
fi
