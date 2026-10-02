#!/usr/bin/env bash
# Build on Linux ARM64 (for example, GitHub's ubuntu-24.04-arm runner).
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $(uname -m) != aarch64 || $(uname -s) != Linux ]]; then
  echo 'Preview bundles must be built on Linux ARM64.' >&2
  exit 1
fi
cargo build --locked --release -p hookrunner-server
./scripts/build-web.sh
# Avoid including an earlier bundle in a manually repeated build.
rm -f dist/preview.tar.gz
package_dir=$(mktemp -d "${TMPDIR:-/tmp}/hookrunner-preview.XXXXXX")
trap 'rm -rf -- "$package_dir"' EXIT
server="${CARGO_TARGET_DIR:-target}/release/hookrunner-server"
cp "$server" "$package_dir/"
"$server" --print-build > "$package_dir/simulation-build.txt"
cp -R dist "$package_dir/dist"
tar -C "$package_dir" -czf dist/preview.tar.gz hookrunner-server simulation-build.txt dist
echo 'Preview bundle: dist/preview.tar.gz'
