#!/usr/bin/env bash
# Builds the local-install package: dist/plumb-local/ and dist/plumb-local-<os>-<arch>.tar.gz.
#
#   scripts/package-local.sh
#
# The package holds the release CLI built with the Anthropic provider (bin/plumb), the built UI
# (ui/), the start script, an example configuration and requirements, and the README.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/dist/plumb-local"
os="$(uname -s | tr '[:upper:]' '[:lower:]')"
arch="$(uname -m)"

cargo build --release --manifest-path "$root/Cargo.toml" -p plumb-cli --features llm
(cd "$root/ui" && npm ci && npm run build)

rm -rf "$out"
mkdir -p "$out/bin" "$out/ui" "$out/examples"
cp "$root/target/release/plumb-cli" "$out/bin/plumb"
cp -R "$root/ui/dist/." "$out/ui/"
cp "$root/packaging/start.sh" "$out/start.sh"
cp "$root/packaging/plumb.sh" "$out/plumb.sh"
cp "$root/packaging/README.md" "$out/README.md"
cp "$root/packaging/examples/hr-pilot.json" "$out/examples/hr-pilot.json"
cp "$root/fixtures/hr-leave/requirements.md" "$out/examples/hr-leave-requirements.md"
chmod +x "$out/start.sh" "$out/plumb.sh" "$out/bin/plumb"

archive="$root/dist/plumb-local-$os-$arch.tar.gz"
tar -czf "$archive" -C "$root/dist" plumb-local
echo "packaged $out and $archive"
