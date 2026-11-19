#!/usr/bin/env bash
# End-to-end smoke test: real window, real IPC, real local MySQL + PostgreSQL, and a SQLite file.
#
#   e2e/run.sh            run the test
#   SHOTS=1 e2e/run.sh    also save a screenshot of the window at each stage (macOS)
#   KEEP_DB=1 e2e/run.sh  leave the `puricat_e2e` demo databases in place afterwards
#   SMOKE=<file> e2e/run.sh   inject another script (a part of smoke.js, while working on it);
#                             the checks of the full run's files then do not apply
#
# Needs local servers that accept:  mysql -u$MYSQL_USER -h127.0.0.1   /   psql -U$PG_USER -h127.0.0.1
# It creates ONE database on each server, `puricat_e2e`, and drops it again. Nothing else is touched,
# and the app runs against a throwaway settings directory with an in-memory secret store.
# The AI CLIs are stand-ins (e2e/fixtures/bin): the agent chat is tested without any account,
# network or quota, and what the app hands to the CLI is checked afterwards.
# The release server is a stand-in too (e2e/fixtures/update-server.py): the app never asks
# GitHub, and what it sent when it looked for an update is checked afterwards.
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
# The "new database" stage creates this one through the app; it must not exist beforehand.
drop_made() {
  "$MYSQL" -u"$MYSQL_USER" -h127.0.0.1 -e 'DROP DATABASE IF EXISTS puricat_e2e_made' 2>/dev/null || true
  "$PSQL" -qX -h127.0.0.1 -U"$PG_USER" -d postgres -c 'DROP DATABASE IF EXISTS puricat_e2e_made WITH (FORCE)' >/dev/null 2>&1 || true
  "$PSQL" -qX -h127.0.0.1 -U"$PG_USER" -d postgres -c 'DROP DATABASE IF EXISTS puricat_e2e_renamed WITH (FORCE)' >/dev/null 2>&1 || true
}
drop_made

# SQLite needs no server: a throwaway file, made with the sqlite3 that ships with macOS.
SQLITE3=${SQLITE3:-sqlite3}
"$SQLITE3" "$WORK/demo.sqlite" <"$ROOT/e2e/fixtures/sqlite.sql"

sed -e "s/__MYSQL_USER__/$MYSQL_USER/" -e "s/__PG_USER__/$PG_USER/" -e "s|__WORK__|$WORK|g" "${SMOKE:-$ROOT/e2e/smoke.js}" >"$WORK/smoke.js"
# A script that does not parse would leave the app waiting for a test that never starts.
if command -v node >/dev/null 2>&1; then
  node --check "$WORK/smoke.js" || { echo "✗ e2e/smoke.js has a syntax error"; exit 1; }
fi

cleanup() {
  if [ -n "${UPDATE_SERVER_PID:-}" ]; then kill "$UPDATE_SERVER_PID" 2>/dev/null || true; wait "$UPDATE_SERVER_PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
  drop_made
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
# Files for the native Open dialogs. A native dialog cannot be driven, so a debug build answers
# each request with the next of these instead, in the order the stages ask: a SQL file for
# "Open SQL file…", then for Smart Paste the CSV and the workbook the test exports itself, a
# Thai CSV that is not UTF-8, and a CSV with more rows than one run takes.
printf 'SELECT 42 AS answer; -- เปิดจากไฟล์\n' >"$WORK/open.sql"
printf 'id,name,city\n9001,ทดสอบภาษาไทย,ลำปาง\n' | iconv -f UTF-8 -t CP874 >"$WORK/thai874.csv"
{ echo "id,name,city"; seq 1 10001 | sed 's/$/,x,y/'; } >"$WORK/big.csv"
export PURICAT_E2E_PICK="$WORK/open.sql:$WORK/export.csv:$WORK/export.xlsx:$WORK/thai874.csv:$WORK/big.csv"
# A release that does not exist, on a server of the test's own: the app must find it, show it,
# and fail to install it without harm (the package is not there).
mkdir -p "$WORK/updates"
python3 "$ROOT/e2e/fixtures/update-server.py" "$WORK/updates" "$WORK/update.port" "$WORK/update.log" &
UPDATE_SERVER_PID=$!
for _ in $(seq 1 50); do [ -s "$WORK/update.port" ] && break; sleep 0.1; done
[ -s "$WORK/update.port" ] || { echo "✗ the stand-in update server did not start"; exit 1; }
UPDATE_URL="http://127.0.0.1:$(cat "$WORK/update.port")"
cat >"$WORK/updates/latest.json" <<JSON
{
  "version": "99.0.0",
  "notes": "A release that exists only in the test.",
  "pub_date": "2026-12-01T09:00:00Z",
  "platforms": {
    "darwin-aarch64": { "signature": "bm90IGEgc2lnbmF0dXJl", "url": "$UPDATE_URL/PuriCat.app.tar.gz" },
    "darwin-x86_64": { "signature": "bm90IGEgc2lnbmF0dXJl", "url": "$UPDATE_URL/PuriCat.app.tar.gz" }
  }
}
JSON
export PURICAT_UPDATE_ENDPOINT="$UPDATE_URL/latest.json"
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

# The test's own main window: the largest one owned by the test's process. Not only "on
# screen" windows at layer 0: the test keeps its window on top (another layer), and a window
# on another Space is not listed as on screen but can still be captured by its number.
# A PuriCat the user has open for real work is another process: its window must never be
# captured (it shows their databases), so windows are matched by process id, not by name.
window_id() {
  local pids
  if [ -n "${APP_PID:-}" ]; then pids=$APP_PID; else pids=$(pgrep -f "$ROOT/target/debug/puricat" | tr '\n' ' '); fi
  [ -n "$pids" ] || return 0
  PURICAT_E2E_PIDS="$pids" swift -e 'import CoreGraphics
import Foundation
let pids = Set((ProcessInfo.processInfo.environment["PURICAT_E2E_PIDS"] ?? "").split(separator: " ").compactMap { Int($0) })
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
var best = (id: 0, area: 0.0)
for w in list where pids.contains(w[kCGWindowOwnerPID as String] as? Int ?? -1) {
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
cp "$WORK/update.log" "$ROOT/target/e2e-update-requests.jsonl" 2>/dev/null || true
if [ "$last" = done ] && [ -n "${SMOKE:-}" ]; then
  # A part of the script does not export the files or make the changes checked below.
  echo "✓ partial script passed (the checks of a full run were not made) — report: target/e2e-report.json"
  exit 0
fi
if [ "$last" = done ]; then
  # The files the app exported must hold exactly what the grid showed.
  check() { grep -q "$2" "$1" || { echo "✗ export check failed: $1 lacks $2"; exit 1; }; }
  check "$WORK/export.csv" '^1,สมชาย ใจดี,'
  check "$WORK/export.json" '"name":"สมชาย ใจดี"'
  unzip -p "$WORK/export.xlsx" xl/sharedStrings.xml | grep -q 'สมชาย ใจดี' || { echo "✗ export check failed: xlsx"; exit 1; }
  head -c 3 "$WORK/export.csv" | xxd -p | grep -q '^efbbbf$' || { echo "✗ export check failed: CSV has no byte-order mark"; exit 1; }
  echo "  · exported CSV, JSON and XLSX verified"

  # SQLite: the edit saved through the grid is in the file, the refused one is not, and
  # testing a path that does not exist did not create a database there.
  title=$("$SQLITE3" "$WORK/demo.sqlite" "SELECT title || '|' || pages || '|' || typeof(pages) FROM books WHERE id = 1")
  [ "$title" = "ธรรมบท ภาค ๑ (ตรวจแล้ว)|248|integer" ] || { echo "✗ sqlite check failed: the file holds $title"; exit 1; }
  [ ! -e "$WORK/nothing-here.sqlite" ] || { echo "✗ sqlite check failed: a database was created at a mistyped path"; exit 1; }
  echo "  · SQLite: the saved edit is in the file"

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
  # What the app said it did to databases is what the servers have: MySQL's was created and
  # then given another collation; PostgreSQL's was created, renamed and dropped.
  made=$("$MYSQL" -u"$MYSQL_USER" -h127.0.0.1 -Nse "SELECT CONCAT(DEFAULT_CHARACTER_SET_NAME, ' ', DEFAULT_COLLATION_NAME) FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = 'puricat_e2e_made'")
  [ "$made" = "utf8mb4 utf8mb4_unicode_ci" ] || { echo "✗ database check failed: MySQL has '$made', expected utf8mb4 utf8mb4_unicode_ci"; exit 1; }
  made=$("$PSQL" -qXAt -h127.0.0.1 -U"$PG_USER" -d postgres -c "SELECT count(*) FROM pg_database WHERE datname IN ('puricat_e2e_made', 'puricat_e2e_renamed')")
  [ "$made" = 0 ] || { echo "✗ database check failed: PostgreSQL still has $made of the created and renamed databases"; exit 1; }
  echo "  · databases verified on both servers: created, altered, renamed, dropped"
fi
if [ "$last" = done ]; then
  # What looking for an update sent: a plain GET of the manifest, with nothing that says who
  # asks or what they are connected to. (The package is asked for only by a bundled build.)
  python3 - "$WORK/update.log" "$USER" <<'PY' || { echo "✗ update check failed"; exit 1; }
import json, sys
seen = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8")]
assert any(r["path"] == "/latest.json" for r in seen), "the app never asked for the manifest"
for r in seen:
    assert r["method"] == "GET" and r["path"] in ("/latest.json", "/PuriCat.app.tar.gz"), r
    headers = {k.lower(): v for k, v in r["headers"].items()}
    assert set(headers) <= {"accept", "accept-encoding", "host", "user-agent"}, sorted(headers)
    assert headers["user-agent"].startswith("tauri-plugin-updater/"), headers["user-agent"]
    assert not any(word in json.dumps(r) for word in (sys.argv[2], "Demo", "puricat_e2e")), r
PY
  echo "  · update check: $(grep -c '"/latest.json"' "$WORK/update.log") request(s) for the manifest, nothing else sent"
fi
if [ "$last" = done ]; then
  echo "✓ end-to-end smoke test passed — report: target/e2e-report.json"
else
  echo "✗ end-to-end smoke test failed at: ${last:-startup}"
  [ -f "$REPORT" ] && cat "$REPORT"
  tail -30 "$WORK/app.log"
  exit 1
fi
