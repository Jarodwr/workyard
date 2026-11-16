#!/bin/sh
# Fail if the version is not the same everywhere. Run from CI and from `cargo test`.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# [workspace.package] version is the source of truth.
expected=$(sed -nE '/^\[workspace\.package\]/,/^\[/ s/^version = "(.*)"/\1/p' Cargo.toml | head -n 1)
[ -n "$expected" ] || { echo "no version in [workspace.package] of Cargo.toml" >&2; exit 1; }
status=0
check() { # file, found
  if [ "$2" != "$expected" ]; then
    echo "version mismatch: $1 has '$2', Cargo.toml [workspace.package] has '$expected'" >&2
    status=1
  fi
}

for manifest in crates/*/Cargo.toml app/src-tauri/Cargo.toml; do
  if grep -qE '^version\.workspace *= *true' "$manifest"; then continue; fi
  found=$(sed -nE '/^\[package\]/,/^\[/ s/^version = "(.*)"/\1/p' "$manifest" | head -n 1)
  check "$manifest" "$found"
done
for file in app/src-tauri/tauri.conf.json plugin/.claude-plugin/plugin.json .claude-plugin/marketplace.json; do
  found=$(sed -nE 's/^ *"version": *"([^"]*)".*/\1/p' "$file" | head -n 1)
  check "$file" "$found"
done

[ $status -eq 0 ] && echo "version $expected is consistent"
exit $status
