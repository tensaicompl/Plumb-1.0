#!/usr/bin/env bash
# E0.4: fails when production source contains placeholder markers (TODO, FIXME,
# unimplemented!, todo!, panic!("TODO")). Tests, generated files and docs are not scanned.
# Usage: scripts/check-placeholders.sh [ROOT]
set -euo pipefail
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$ROOT"
paths=()
[[ -d crates ]] && paths+=(crates)
[[ -d ui/src ]] && paths+=(ui/src)
hits="$(grep -rnE 'TODO|FIXME|unimplemented!|todo!\(|panic!\("TODO' "${paths[@]}" \
  --include='*.rs' --include='*.ts' --include='*.tsx' 2>/dev/null \
  | grep -vE '/tests/|\.test\.tsx?:|/test/|schema\.d\.ts' || true)"
if [[ -n "$hits" ]]; then
  echo "placeholder markers in production source:"
  echo "$hits"
  exit 1
fi
echo "no placeholder markers"
