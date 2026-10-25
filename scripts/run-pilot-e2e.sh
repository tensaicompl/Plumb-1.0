#!/usr/bin/env bash
# E0.3 pilot runner: builds the HR pilot database with the CLI simulation (product operations
# only: init, import, replayed stages, accept-set, confirm, findings, answers), serves it with
# the real backend, starts the UI against it the supported way (npm run dev, see
# docs/pilot/RUNNING-THE-UI.md) and runs every Playwright spec in ui/e2e/.
#
# Two databases, both built only by product operations:
# - the finished pilot (every spec except tests tagged @inbox-db);
# - the Inbox database: the HR document imported and the Requirements step replayed, so its 32
#   proposals wait for review (tests tagged @inbox-db, run after the backend switches to it).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
API_PORT="${PLUMB_E2E_API_PORT:-8790}"
UI_PORT="${PLUMB_E2E_UI_PORT:-5173}"
export PLUMB_E2E_UI_PORT="$UI_PORT"
# Stops a process and its descendants (npm run dev starts Vite as a grandchild).
kill_tree() {
  local child
  for child in $(pgrep -P "$1" 2>/dev/null); do
    kill_tree "$child"
  done
  kill "$1" 2>/dev/null || true
}
cleanup() {
  [[ -n "${API_PID:-}" ]] && kill "$API_PID" 2>/dev/null || true
  [[ -n "${UI_PID:-}" ]] && kill_tree "$UI_PID"
  rm -rf "$WORK"
}
trap cleanup EXIT

# Serves the database $1 (with the pilot configuration) on the API port and waits for it.
serve() {
  "$ROOT/target/debug/plumb-cli" --db "$1" --config "$WORK/sim/plumb.json" \
    serve --listen "127.0.0.1:$API_PORT" >"$WORK/serve.log" 2>&1 &
  API_PID=$!
  for _ in $(seq 1 60); do
    if curl -sf "http://127.0.0.1:$API_PORT/api/me" >/dev/null; then
      return
    fi
    sleep 0.5
  done
}

cd "$ROOT"
SIM_DIR="$WORK/sim" cargo test -q -p plumb-cli --test sim_hr hr_pilot_simulation
cargo build -q -p plumb-cli

# The Inbox database: init, import and the recorded Requirements (extract) step, nothing accepted.
INBOX_DB="$WORK/inbox/plumb.sqlite"
mkdir -p "$WORK/inbox"
inbox_cli() { "$ROOT/target/debug/plumb-cli" --db "$INBOX_DB" --config "$WORK/sim/plumb.json" "$@" >/dev/null; }
inbox_cli init --project project:hr
inbox_cli import "$ROOT/fixtures/hr-leave/requirements.md" --media-type text/markdown --name requirements.md
inbox_cli --provider replay --replay-fixtures "$WORK/sim/replay-extract.json" extract

serve "$WORK/sim/plumb.sqlite"
cd "$ROOT/ui"
PLUMB_API_TARGET="http://127.0.0.1:$API_PORT" npm run dev -- --host 127.0.0.1 --port "$UI_PORT" --strictPort >"$WORK/vite.log" 2>&1 &
UI_PID=$!
for _ in $(seq 1 60); do
  if curl -sf "http://127.0.0.1:$UI_PORT/" >/dev/null; then
    break
  fi
  sleep 0.5
done
npx playwright test --grep-invert @inbox-db --reporter=list

kill "$API_PID" 2>/dev/null || true
wait "$API_PID" 2>/dev/null || true
serve "$INBOX_DB"
npx playwright test --grep @inbox-db --reporter=list
