#!/usr/bin/env bash
# The in-app updater against real packages (macOS). Needs no database server and no network:
# the release server is a stand-in on 127.0.0.1 (e2e/fixtures/update-server.py).
#
#   e2e/update.sh
#
# It builds the app twice as a bundled debug build: once as it is (the "installed" version)
# and once as version 99.0.0 with its signed update package. Then, in a copy of the installed
# app that lives in a temporary directory:
#
#   1. a package whose bytes were changed after signing must be refused;
#   2. a genuine package offered under another version number must be refused;
#   3. a package signed with another key must be refused;
#   4. the genuine package must install, and after "Restart now" the app must be 99.0.0 and
#      find nothing newer.
#
# After each refusal the app on disk must be byte for byte what it was.
#
# The packages are signed with a key made for this run, and both builds are given its public
# half. With TAURI_SIGNING_PRIVATE_KEY (and its password) in the environment the packages are
# signed with that key instead and the builds keep the public key of src-tauri/tauri.conf.json:
# that is the check that the release key and the app's public key belong together.
#
# The app /Applications/PuriCat.app and the user's settings are not touched. A debug build is
# used because only a debug build accepts a stand-in server and the test's script.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
# The real path: the updater refuses an app that was started through a symbolic link, and
# the temporary directory of macOS is behind one (/var).
WORK=$(cd "$(mktemp -d)" && pwd -P)
OLD=$(node -p "require('./src-tauri/tauri.conf.json').version")
NEW=99.0.0
BUNDLE="$ROOT/target/debug/bundle/macos"

cleanup() {
  [ -z "${APP_PID:-}" ] || kill "$APP_PID" 2>/dev/null || true
  pkill -f "$WORK/installed/PuriCat.app" 2>/dev/null || true
  if [ -n "${SERVER_PID:-}" ]; then kill "$SERVER_PID" 2>/dev/null || true; wait "$SERVER_PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# ---- keys --------------------------------------------------------------------------------
PUBKEY=()
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "› signing with the key from the environment; the builds keep the app's own public key"
else
  echo "› a signing key for this run"
  pnpm -s tauri signer generate -p test -w "$WORK/key" --ci >/dev/null 2>&1
  TAURI_SIGNING_PRIVATE_KEY=$(cat "$WORK/key")
  export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD=test
  PUBKEY=(--config "{\"plugins\":{\"updater\":{\"pubkey\":\"$(cat "$WORK/key.pub")\"}}}")
fi
# A second key, which the app does not know.
pnpm -s tauri signer generate -p other -w "$WORK/other" --ci >/dev/null 2>&1

# ---- the two builds ----------------------------------------------------------------------
echo "› building the installed version ($OLD)"
pnpm -s tauri build --debug --bundles app ${PUBKEY[@]+"${PUBKEY[@]}"} >"$WORK/build-old.log" 2>&1 || { tail -30 "$WORK/build-old.log"; exit 1; }
cp -R "$BUNDLE/PuriCat.app" "$WORK/old.app"

echo "› building the update ($NEW) and its signed package"
pnpm -s tauri build --debug --bundles app --config src-tauri/tauri.updater.conf.json --config "{\"version\":\"$NEW\"}" \
  ${PUBKEY[@]+"${PUBKEY[@]}"} >"$WORK/build-new.log" 2>&1 || { tail -30 "$WORK/build-new.log"; exit 1; }
mkdir -p "$WORK/good" "$WORK/tampered" "$WORK/relabelled" "$WORK/otherkey"
cp "$BUNDLE/PuriCat.app.tar.gz" "$BUNDLE/PuriCat.app.tar.gz.sig" "$WORK/good/"

# 1. One byte of the package changed; the signature is the genuine one.
python3 - "$WORK/good/PuriCat.app.tar.gz" "$WORK/tampered/PuriCat.app.tar.gz" <<'PY'
import sys
data = bytearray(open(sys.argv[1], "rb").read())
data[len(data) // 2] ^= 0x01
open(sys.argv[2], "wb").write(data)
PY
cp "$WORK/good/PuriCat.app.tar.gz.sig" "$WORK/tampered/"
# 2. The genuine package and signature, announced as a version it was not signed for.
cp "$WORK/good/"* "$WORK/relabelled/"
# 3. The genuine package, signed again with a key the app does not know.
cp "$WORK/good/PuriCat.app.tar.gz" "$WORK/otherkey/"
TAURI_SIGNING_PRIVATE_KEY=$(cat "$WORK/other") TAURI_SIGNING_PRIVATE_KEY_PASSWORD=other \
  pnpm -s tauri signer sign --app-version "$NEW" "$WORK/otherkey/PuriCat.app.tar.gz" >/dev/null

version_on_disk() { /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$WORK/installed/PuriCat.app/Contents/Info.plist"; }

# run_case <directory> <announced version> <install|refuse>
run_case() {
  local dir=$1 announced=$2 expect=$3 report="$WORK/report-$1.json"
  rm -rf "$WORK/installed" && mkdir -p "$WORK/installed" && cp -R "$WORK/old.app" "$WORK/installed/PuriCat.app"
  rm -f "$WORK/port"
  python3 "$ROOT/e2e/fixtures/update-server.py" "$WORK/$dir" "$WORK/port" "$WORK/requests-$dir.jsonl" &
  SERVER_PID=$!
  for _ in $(seq 1 50); do [ -s "$WORK/port" ] && break; sleep 0.1; done
  local url
  url="http://127.0.0.1:$(cat "$WORK/port")"
  node -e '
    const fs = require("fs");
    const [dir, version, url] = process.argv.slice(1);
    const entry = { signature: fs.readFileSync(`${dir}/PuriCat.app.tar.gz.sig`, "utf8"), url: `${url}/PuriCat.app.tar.gz` };
    const manifest = { version, pub_date: new Date().toISOString(), platforms: { "darwin-aarch64": entry, "darwin-x86_64": entry } };
    fs.writeFileSync(`${dir}/latest.json`, JSON.stringify(manifest, null, 2) + "\n");
  ' "$WORK/$dir" "$announced" "$url"
  sed -e "s/__OLD__/$OLD/g" -e "s/__NEW__/$NEW/g" -e "s/__ANNOUNCED__/\"v$announced\"/g" -e "s/__EXPECT__/$expect/g" \
    "$ROOT/e2e/update.js" >"$WORK/update.js"
  node --check "$WORK/update.js"

  PURICAT_DATA_DIR="$WORK/data-$dir" PURICAT_SECRET_STORE=memory PURICAT_UPDATE_ENDPOINT="$url/latest.json" \
    PURICAT_E2E_SCRIPT="$WORK/update.js" PURICAT_E2E_REPORT="$report" \
    "$WORK/installed/PuriCat.app/Contents/MacOS/puricat" >"$WORK/app-$dir.log" 2>&1 &
  APP_PID=$!

  local stage="" last="" checked_disk=0
  for _ in $(seq 1 1200); do
    sleep 0.25
    [ -f "$report" ] || continue
    stage=$(sed -n 's/^ "stage": "\(.*\)",\{0,1\}$/\1/p' "$report" | head -1)
    [ "$stage" = "$last" ] && continue
    last=$stage
    if [ "$stage" = installed ] && [ "$checked_disk" = 0 ]; then
      checked_disk=1
      # The old version is still the one running; the new one is already on disk.
      [ "$(version_on_disk)" = "$NEW" ] || { echo "✗ $dir: after the install the app on disk is $(version_on_disk)"; exit 1; }
    fi
    if [ "$stage" = done ] || [ "$stage" = FAILED ]; then break; fi
  done
  cp "$report" "$ROOT/target/e2e-update-$dir.json" 2>/dev/null || true
  cp "$WORK/requests-$dir.jsonl" "$ROOT/target/e2e-update-$dir-requests.jsonl" 2>/dev/null || true
  # After a restart the app is another process than the one started here.
  kill "$APP_PID" 2>/dev/null || true
  wait "$APP_PID" 2>/dev/null || true
  pkill -f "$WORK/installed/PuriCat.app" 2>/dev/null || true
  APP_PID=""
  kill "$SERVER_PID" 2>/dev/null || true; wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
  if [ "$last" != done ]; then
    echo "✗ $dir: stopped at ${last:-startup}"
    [ -f "$report" ] && cat "$report"
    echo "--- the app's output"; tail -20 "$WORK/app-$dir.log"
    echo "--- requests the server saw"; cat "$WORK/requests-$dir.jsonl" 2>/dev/null || echo "(none)"
    echo "--- the manifest"; cat "$WORK/$dir/latest.json"
    exit 1
  fi

  if [ "$expect" = refuse ]; then
    [ "$(version_on_disk)" = "$OLD" ] || { echo "✗ $dir: a refused package changed the app to $(version_on_disk)"; exit 1; }
    diff -r "$WORK/old.app" "$WORK/installed/PuriCat.app" >/dev/null || { echo "✗ $dir: a refused package changed files of the app"; exit 1; }
    local error
    error=$(node -p "JSON.parse(require('fs').readFileSync('$report', 'utf8')).error")
    case "$error" in
      *"did not pass the signature check"*) ;;
      *) echo "✗ $dir: refused, but not for its signature: $error"; exit 1 ;;
    esac
    echo "  ✓ $dir: refused, the app is unchanged — “$error”"
  else
    [ "$(version_on_disk)" = "$NEW" ] || { echo "✗ $dir: the app on disk is $(version_on_disk), expected $NEW"; exit 1; }
    [ "$checked_disk" = 1 ] || { echo "✗ $dir: the test never saw the installed state"; exit 1; }
    node -e '
      const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
      if (r.version !== process.argv[2]) { console.error(`the app that answered last is ${r.version}`); process.exit(1); }
    ' "$report" "$NEW" || { echo "✗ $dir: the restarted app is not $NEW"; exit 1; }
    echo "  ✓ $dir: installed, restarted as $NEW, and it finds nothing newer"
  fi
  # Looking for the update and fetching it sent nothing but two plain GETs.
  python3 - "$WORK/requests-$dir.jsonl" <<'PY' || { echo "✗ $dir: unexpected requests"; exit 1; }
import json, sys
for line in open(sys.argv[1], encoding="utf-8"):
    r = json.loads(line)
    assert r["method"] == "GET" and r["path"] in ("/latest.json", "/PuriCat.app.tar.gz"), r
    headers = {k.lower() for k in r["headers"]}
    assert headers <= {"accept", "accept-encoding", "host", "user-agent"}, sorted(headers)
PY
}

echo "› cases"
run_case tampered "$NEW" refuse
run_case relabelled 99.5.0 refuse
run_case otherkey "$NEW" refuse
run_case good "$NEW" install
echo "✓ updater test passed — reports: target/e2e-update-*.json"
