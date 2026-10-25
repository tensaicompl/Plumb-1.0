#!/usr/bin/env bash
# Runs one Plumb command for a project folder, filling in its database, configuration and the
# Anthropic key, so only the command itself is typed:
#
#   ./plumb.sh <project-folder> import <file.md|file.docx|file.txt>
#   ./plumb.sh <project-folder> extract        (any plumb command; ./plumb.sh <folder> help)
#
# Uses the same defaults as start.sh (PLUMB_ANTHROPIC_KEY_FILE, <folder>/plumb.json).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
project="${1:?usage: ./plumb.sh <project-folder> <command> [arguments]}"
shift
project="$(cd "$project" && pwd)"
config="${PLUMB_CONFIG:-$project/plumb.json}"
key="${PLUMB_ANTHROPIC_KEY_FILE:-$HOME/.config/plumb/anthropic-key}"

if [[ "${1:-}" == "import" && $# -ge 2 && "$*" != *--media-type* ]]; then
  file="$2"
  case "$file" in
    *.md) media=text/markdown ;;
    *.txt) media=text/plain ;;
    *.docx) media=application/vnd.openxmlformats-officedocument.wordprocessingml.document ;;
    *) echo "unsupported file type: $file (use .md, .txt or .docx)" >&2; exit 1 ;;
  esac
  set -- import "$file" --media-type "$media" --name "$(basename "$file")" "${@:3}"
fi

provider=()
[[ -f "$key" ]] && provider=(--provider anthropic --anthropic-api-key-file "$key")
exec "$here/bin/plumb" --db "$project/plumb.sqlite" --config "$config" \
  ${provider[@]+"${provider[@]}"} "$@"
