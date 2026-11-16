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
# A script that does not parse would leave the app waiting for a test that never starts.
if command -v node >/dev/null 2>&1; then
  node --check "$WORK/smoke.js" || { echo "✗ e2e/smoke.js has a syntax error"; exit 1; }
fi

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

# The app's main window: the largest one it owns. Not only "on screen" windows at layer 0:
# the test keeps its window on top (another layer), and a window on another Space is not
# listed as on screen but can still be captured by its number.
window_id() {
  swift -e 'import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
var best = (id: 0, area: 0.0)
for w in list where (w[kCGWindowOwnerName as String] as? String)?.lowercased() == "puricat" {
  let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
  let area = (b["Width"] ?? 0) * (b["Height"] ?? 0)
  if area > best.area { best = (w[kCGWindowNumber as String] as! Int, area) }
}
if best.id != 0 { print(best.id) }' 2>/dev/null
}

# Stages are numbered as they come: screenshots of an earlier run would sit between the new ones.
[ "${SHOTS:-0}" = 1 ] && rm -f "${ROOT:?}"/screenshots/e2e/*.png
last=""
shot=0
window=""
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
    # Looked up once: the window keeps its number for as long as the app runs.
    [ -n "$window" ] || window=$(window_id || true)
    [ -n "$window" ] && screencapture -x -o -l "$window" "$ROOT/screenshots/e2e/$(printf '%02d' $shot)-$stage.png" 2>/dev/null || true
    shot=$((shot + 1))
  fi
  if [ "$stage" = done ] || [ "$stage" = FAILED ]; then break; fi
done
if [ "${SHOTS:-0}" = 1 ] && ! ls "$ROOT"/screenshots/e2e/*.png >/dev/null 2>&1; then
  echo "  ! no screenshot could be taken (is screen recording allowed for this terminal?)"
fi

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

  # With database tools on: the CLI got PuriCat's own bridge as its only MCP server, only
  # that server's tools were allowed, and what it wrote through the reading tool was refused.
  check "$WORK/claude-tools.args" '^--mcp-config$'
  check "$WORK/claude-tools.args" '^--strict-mcp-config$'
  check "$WORK/claude-tools.args" '^mcp__puricat$'
  check "$WORK/claude-tools.args" '"args":\["--mcp-bridge","'
  check "$WORK/claude-tools.args" "\"command\":\"${APP:-$ROOT/target/debug/puricat}\""
  grep -A1 -x -- '--tools' "$WORK/claude-tools.args" | tail -1 | grep -qx '' || { echo "✗ tools check failed: built-in tools were not disabled"; exit 1; }
  grep -A1 -x -- '--setting-sources' "$WORK/claude-tools.args" | tail -1 | grep -qx '' || { echo "✗ tools check failed: the user's settings were not kept out"; exit 1; }
  if grep -qE "127\.0\.0\.1|Demo PostgreSQL|$PG_USER@" "$WORK/claude-tools.args" "$WORK/claude-tools.stdin"; then
    echo "✗ tools check failed: the CLI was told about the server or the connection"; exit 1
  fi
  check "$WORK/claude-tools.cwd" '/data/agent-scratch$'
  python3 - "$WORK/claude-tools.json" "$WORK/claude-tools.args" <<'PY' || { echo "✗ tools check failed"; exit 1; }
import json, os, re, sys
seen = json.load(open(sys.argv[1]))
assert "run_select" in seen["tools"] and "propose_sql" in seen["tools"], seen["tools"]
by_name = [(r["name"], r["isError"]) for r in seen["results"]]
assert by_name[:3] == [("list_tables", False), ("describe_table", False), ("run_select", False)], by_name
assert by_name[3] == ("run_select", True), "a DELETE through the reading tool was not refused"
socket = re.search(r'"--mcp-bridge","([^"]+)"', open(sys.argv[2]).read()).group(1)
assert not os.path.exists(os.path.dirname(socket)), "the turn's socket outlived the turn"
PY
  left=$("$PSQL" -qXAt -h127.0.0.1 -U"$PG_USER" -d puricat_e2e -c 'SELECT COUNT(*) FROM shop.orders')
  [ "$left" = 1250 ] || { echo "✗ tools check failed: $left orders left, expected 1250"; exit 1; }
  echo "  · database tools: MCP hand-over, refusals and the data verified"
fi
if [ "$last" = done ]; then
  echo "✓ end-to-end smoke test passed — report: target/e2e-report.json"
else
  echo "✗ end-to-end smoke test failed at: ${last:-startup}"
  [ -f "$REPORT" ] && cat "$REPORT"
  tail -30 "$WORK/app.log"
  exit 1
fi
