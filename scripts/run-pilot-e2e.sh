#!/usr/bin/env bash
# E0.3 pilot runner: builds the HR pilot database with the CLI simulation (product operations
# only: init, import, replayed stages, accept-set, confirm, findings, answers), serves it with
# the real backend, starts the UI against it the supported way (npm run dev, see
# docs/pilot/RUNNING-THE-UI.md) and runs every Playwright spec in ui/e2e/.
#
# Five databases, all built only by product operations, each served in its own phase:
# - the finished pilot (every untagged test);
# - the Inbox database: the HR document imported and the Requirements step replayed, so its 32
#   proposals wait for review (tests tagged @inbox-db);
# - the triage database, a copy of the Inbox database for the flows that change it, served with
#   the replay provider so a stale proposal's Refresh can run the step again (tagged @triage-db);
# - the questions database: the pilot simulation stopped where the expert's questions are open,
#   served with every execution the simulation recorded (tagged @questions-db);
# - an empty project served with the replay provider over the recorded Requirements step, so the
#   UI can run that step without an AI key (tagged @empty-db).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
API_PORT="${PLUMB_E2E_API_PORT:-8790}"
UI_PORT="${PLUMB_E2E_UI_PORT:-5173}"
export PLUMB_E2E_UI_PORT="$UI_PORT"
export PLUMB_E2E_API_PORT="$API_PORT"
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

# Serves the database $1 (with the pilot configuration, and the global options $2…) on the API
# port and waits for it.
serve() {
  local db="$1"
  shift
  "$ROOT/target/debug/plumb-cli" --db "$db" --config "$WORK/sim/plumb.json" "$@" \
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
TRIAGE_DB="$WORK/triage/plumb.sqlite"
mkdir -p "$WORK/triage"
cp "$INBOX_DB" "$TRIAGE_DB"

# The empty project: initialized, nothing imported.
EMPTY_DB="$WORK/empty/plumb.sqlite"
mkdir -p "$WORK/empty"
"$ROOT/target/debug/plumb-cli" --db "$EMPTY_DB" --config "$WORK/sim/plumb.json" init --project project:hr >/dev/null

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
npx playwright test --grep-invert "@inbox-db|@triage-db|@questions-db|@empty-db" --workers=1 --reporter=list

# Each further phase switches the backend to its database and runs the tests tagged for it.
phase() {
  local tag="$1"
  shift
  kill "$API_PID" 2>/dev/null || true
  wait "$API_PID" 2>/dev/null || true
  serve "$@"
  npx playwright test --grep "$tag" --workers=1 --reporter=list
}
phase @inbox-db "$INBOX_DB"
phase @triage-db "$TRIAGE_DB" --provider replay --replay-fixtures "$WORK/sim/replay-extract.json"
phase @questions-db "$WORK/sim/questions.sqlite" --provider replay --replay-fixtures "$WORK/sim/replay-all.json"
phase @empty-db "$EMPTY_DB" --provider replay --replay-fixtures "$WORK/sim/replay-extract.json"
