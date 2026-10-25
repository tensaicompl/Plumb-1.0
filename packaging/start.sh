#!/usr/bin/env bash
# Starts Plumb for one project folder and opens it in the browser.
#
#   ./start.sh <project-folder> [config.json]
#
# The project folder holds the project database (plumb.sqlite) and, unless a configuration is
# given, its configuration (plumb.json, copied from examples/hr-pilot.json on first start).
# The Anthropic API key is read from PLUMB_ANTHROPIC_KEY_FILE, default
# ~/.config/plumb/anthropic-key; without it Plumb runs, but inference stages cannot.
# PLUMB_PORT sets the local port (default 8080); PLUMB_NO_BROWSER=1 skips opening the browser.
# Plumb only listens on 127.0.0.1.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
project="${1:?usage: ./start.sh <project-folder> [config.json]}"
mkdir -p "$project"
project="$(cd "$project" && pwd)"
config="${2:-$project/plumb.json}"
key="${PLUMB_ANTHROPIC_KEY_FILE:-$HOME/.config/plumb/anthropic-key}"
port="${PLUMB_PORT:-8080}"
db="$project/plumb.sqlite"
plumb="$here/bin/plumb"

if [[ ! -f "$config" ]]; then
  cp "$here/examples/hr-pilot.json" "$config"
  echo "Created $config from the HR pilot example; edit its directory for your own project."
fi
if [[ ! -f "$db" ]]; then
  name="$(basename "$project" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9._-' '-')"
  "$plumb" --db "$db" --config "$config" init --project "project:${name%-}"
fi

provider=()
if [[ -f "$key" ]]; then
  provider=(--provider anthropic --anthropic-api-key-file "$key")
else
  echo "No API key at $key: inference stages are unavailable (set PLUMB_ANTHROPIC_KEY_FILE)."
fi

url="http://127.0.0.1:$port"
echo "Plumb is starting at $url (Ctrl+C stops it)."
if [[ -z "${PLUMB_NO_BROWSER:-}" ]]; then
  (sleep 2; open "$url" 2>/dev/null || xdg-open "$url" 2>/dev/null || true) &
fi
exec "$plumb" --db "$db" --config "$config" ${provider[@]+"${provider[@]}"} \
  serve --listen "127.0.0.1:$port" --ui-dir "$here/ui"
