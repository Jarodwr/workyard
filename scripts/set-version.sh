#!/bin/sh
# Set the Workyard version everywhere it is written down. The Rust crates inherit
# [workspace.package] version, so Cargo.toml is edited once; the JSON manifests are literal.
# Usage: scripts/set-version.sh 0.2.0
set -eu
[ $# -eq 1 ] || { echo "usage: $0 <version>" >&2; exit 2; }
version=$1
case $version in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "version must look like 1.2.3 (optionally 1.2.3-rc.1)" >&2; exit 2 ;;
esac
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# [workspace.package] version (first `version = "..."` after the section header).
sed -E "/^\[workspace\.package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

# Any crate that pins its own version instead of inheriting gets the same one.
for manifest in crates/*/Cargo.toml app/src-tauri/Cargo.toml; do
  sed -E "/^\[package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" "$manifest" > "$manifest.tmp"
  mv "$manifest.tmp" "$manifest"
done

# JSON manifests: the first top-level "version" key (marketplace.json nests it under the plugin).
for file in app/src-tauri/tauri.conf.json plugin/.claude-plugin/plugin.json .claude-plugin/marketplace.json; do
  sed -E "s/(\"version\": *)\"[^\"]*\"/\1\"$version\"/" "$file" > "$file.tmp"
  mv "$file.tmp" "$file"
done

# Keep Cargo.lock in step with the manifests (offline: only our own packages change).
cargo update --workspace --offline >/dev/null 2>&1 || true
"$root/scripts/check-version.sh"
