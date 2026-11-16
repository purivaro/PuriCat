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
# The AI CLIs are stand-ins (e2e/fixtures/bin): the agent chat is tested without any account,
# network or quota, and what the app hands to the CLI is checked afterwards.
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

sed -e "s/__MYSQL_USER__/$MYSQL_USER/" -e "s/__PG_USER__/$PG_USER/" -e "s|__WORK__|$WORK|g" "$ROOT/e2e/smoke.js" >"$WORK/smoke.js"

cleanup() {
  rm -rf "$WORK"
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
export PATH="$ROOT/e2e/fixtures/bin:$PATH"
export PURICAT_STAND_IN_DIR="$WORK"
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
  # The files the app exported must hold exactly what the grid showed.
  check() { grep -q "$2" "$1" || { echo "✗ export check failed: $1 lacks $2"; exit 1; }; }
  check "$WORK/export.csv" '^1,สมชาย ใจดี,'
  check "$WORK/export.json" '"name":"สมชาย ใจดี"'
  unzip -p "$WORK/export.xlsx" xl/sharedStrings.xml | grep -q 'สมชาย ใจดี' || { echo "✗ export check failed: xlsx"; exit 1; }
  head -c 3 "$WORK/export.csv" | xxd -p | grep -q '^efbbbf$' || { echo "✗ export check failed: CSV has no byte-order mark"; exit 1; }
  echo "  · exported CSV, JSON and XLSX verified"

  # What the agent chat gave the CLI: the schema and the conversation on stdin, nothing that
  # identifies the server, no tools, and an empty working directory of the app's own.
  check "$WORK/claude.stdin" '^Engine: PostgreSQL'
  check "$WORK/claude.stdin" '^Database: puricat_e2e$'
  check "$WORK/claude.stdin" 'take your time'
  check "$WORK/codex.stdin" '^You are the database assistant inside PuriCat'
  if grep -qE "127\.0\.0\.1|Demo PostgreSQL|Demo MySQL" "$WORK/claude.stdin" "$WORK/codex.stdin"; then
    echo "✗ agent check failed: the prompt names the server or the connection"; exit 1
  fi
  if grep -qE "orders|puricat_e2e" "$WORK/claude.args" "$WORK/codex.args"; then
    echo "✗ agent check failed: schema or conversation text in the CLI's arguments"; exit 1
  fi
  check "$WORK/claude.args" '^--strict-mcp-config$'
  check "$WORK/claude.args" '^--no-session-persistence$'
  check "$WORK/codex.args" '^read-only$'
  grep -A1 -x -- '--tools' "$WORK/claude.args" | tail -1 | grep -qx '' || { echo "✗ agent check failed: built-in tools were not disabled"; exit 1; }
  check "$WORK/claude.cwd" '/data/agent-scratch$'
  [ -z "$(ls -A "$WORK/data/agent-scratch")" ] || { echo "✗ agent check failed: the scratch directory is not empty"; exit 1; }
  echo "  · agent chat: prompt, arguments and working directory verified"
fi
if [ "$last" = done ]; then
  echo "✓ end-to-end smoke test passed — report: target/e2e-report.json"
else
  echo "✗ end-to-end smoke test failed at: ${last:-startup}"
  [ -f "$REPORT" ] && cat "$REPORT"
  tail -30 "$WORK/app.log"
  exit 1
fi
