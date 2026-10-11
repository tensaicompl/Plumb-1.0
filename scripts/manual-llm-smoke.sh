#!/usr/bin/env bash
# Manual live smoke test of the Anthropic InferenceProvider (plan X1.1, Hotfix 057).
#
# Usage:
#   ANTHROPIC_API_KEY=... ANTHROPIC_MODEL=... ./scripts/manual-llm-smoke.sh
# Optional: ANTHROPIC_WORKSPACE_ID=...
#
# Runs only the ignored live smoke test. It never prints the credential and is never part of CI.
set -euo pipefail

if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "error: ANTHROPIC_API_KEY is not set; export it to run the live smoke test" >&2
  exit 1
fi
if [[ -z "${ANTHROPIC_MODEL:-}" ]]; then
  echo "error: ANTHROPIC_MODEL is not set; export the model name to run the live smoke test" >&2
  exit 1
fi

cd "$(dirname "$0")/.."
exec cargo test -p plumb-inference --features anthropic --test provider_contract \
  -- --ignored --exact provider_contract::anthropic::live_anthropic_smoke
