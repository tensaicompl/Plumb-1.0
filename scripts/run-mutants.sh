#!/usr/bin/env bash
# E0.4 mutation bar: cargo-mutants over the four frozen scopes, each of which must reach
# >= 80% caught. Score = caught / (caught + missed + timeout); unviable mutants (that do not
# compile) are excluded. A timeout counts against the score, so the bar is conservative.
#
#   plumb-expr        the PlumbExpr evaluator with its type checker and parser (whole crate)
#   plumb-sim         the scenario interpreter (whole crate)
#   plumb-intake      intake analysis (whole crate)
#   plumb-validation  the NOW gate rule evaluators only (src/rules/*.rs: I0, F1, F2, F3, F4)
#
# Usage: scripts/run-mutants.sh [crate...]   (default: all four)
# Env: MUTANTS_JOBS (default 4), MUTANTS_OUT (default target/mutants), MUTANTS_ARGS (extra
# cargo-mutants arguments, e.g. "--in-diff changes.diff" to test only a change).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
THRESHOLD=80
JOBS="${MUTANTS_JOBS:-4}"
OUT="${MUTANTS_OUT:-$ROOT/target/mutants}"
read -r -a EXTRA <<<"${MUTANTS_ARGS:-}"

scope_args() {
  case "$1" in
    plumb-expr | plumb-sim | plumb-intake) ;;
    plumb-validation) echo "--file crates/plumb-validation/src/rules/*.rs" ;;
    *) echo "unknown mutation scope: $1" >&2; return 1 ;;
  esac
}

CRATES=("$@")
if [[ ${#CRATES[@]} -eq 0 ]]; then
  CRATES=(plumb-expr plumb-sim plumb-intake plumb-validation)
fi

mkdir -p "$OUT"
FAILED=0
SUMMARY=()
for crate in "${CRATES[@]}"; do
  read -r -a SCOPE <<<"$(scope_args "$crate")"
  echo "== cargo mutants: $crate ${SCOPE[*]:-}"
  status=0
  cargo mutants -p "$crate" --test-package "$crate" --jobs "$JOBS" --timeout-multiplier 3 \
    --output "$OUT/$crate" ${SCOPE[@]+"${SCOPE[@]}"} ${EXTRA[@]+"${EXTRA[@]}"} || status=$?
  # 0: all caught; 2: some missed; 3: some timed out. Anything else is a run failure.
  if [[ "$status" != 0 && "$status" != 2 && "$status" != 3 ]]; then
    echo "FAIL: cargo mutants for $crate exited $status"
    FAILED=1
    continue
  fi
  dir="$OUT/$crate/mutants.out"
  count() { if [[ -f "$dir/$1.txt" ]]; then grep -c . "$dir/$1.txt" || true; else echo 0; fi; }
  caught=$(count caught)
  missed=$(count missed)
  timeout=$(count timeout)
  unviable=$(count unviable)
  line=$(python3 -c "
c, m, t, u = $caught, $missed, $timeout, $unviable
viable = c + m + t
pct = 100.0 * c / viable if viable else 100.0
print(f'{pct:.1f} {c} caught, {m} missed, {t} timeout, {u} unviable')")
  pct="${line%% *}"
  SUMMARY+=("$crate: ${pct}% (${line#* })")
  if python3 -c "import sys; sys.exit(0 if $pct >= $THRESHOLD else 1)"; then :; else
    echo "FAIL: $crate is ${pct}% caught, below ${THRESHOLD}%"
    FAILED=1
  fi
done

echo
echo "== mutation scores (bar: ${THRESHOLD}% caught)"
printf '%s\n' "${SUMMARY[@]}"
exit "$FAILED"
