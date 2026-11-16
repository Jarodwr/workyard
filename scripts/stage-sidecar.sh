#!/bin/sh
# Build the `workyard` CLI and stage it where Tauri expects an external binary:
# app/src-tauri/binaries/workyard-<target-triple>. Tauri refuses to build the app crate
# (even in dev) when that file is missing, so run this once after cloning.
#
#   scripts/stage-sidecar.sh            release build for the host triple (use for packaging)
#   scripts/stage-sidecar.sh --debug    debug build (fast; use for development)
#   scripts/stage-sidecar.sh --target aarch64-apple-darwin   cross target (rustup target add first)
#   scripts/stage-sidecar.sh --check    build nothing; fail if the staged binary is missing, a
#                                       placeholder, or older than anything in crates/ (for cargo test / CI)
#
# Tauri copies the staged file over target/<profile>/workyard (the CLI's own build output) on every
# desktop build or `tauri dev`, so a stale staged copy silently replaces a fresh CLI. tauri.conf.json
# runs this script from beforeDevCommand / beforeBuildCommand to keep it current.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
profile=release
target=""
check=0
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug ;;
    --check) check=1 ;;
    --target) shift; target="$1" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
host=$(rustc -vV | sed -n 's/^host: //p')
triple=${target:-$host}
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac
staged="$root/app/src-tauri/binaries/workyard-$triple$ext"
if [ "$check" = 1 ]; then
  rel="app/src-tauri/binaries/workyard-$triple$ext"
  if [ ! -s "$staged" ] || grep -q 'workyard-placeholder-sidecar' "$staged" 2>/dev/null; then
    echo "error: $rel is missing or a placeholder; run scripts/stage-sidecar.sh" >&2
    exit 1
  fi
  newer=$(cd "$root" && find crates Cargo.toml Cargo.lock -type f -newer "$staged" ! -path '*/target/*' 2>/dev/null | head -5)
  if [ -n "$newer" ]; then
    echo "error: staged sidecar $rel is older than the sources (for example:" >&2
    echo "$newer" | sed 's/^/  /' >&2
    echo "); run scripts/stage-sidecar.sh to rebuild and re-stage it. A stale copy would overwrite target/<profile>/workyard." >&2
    exit 1
  fi
  echo "staged sidecar is up to date"
  exit 0
fi
flags=""
[ "$profile" = release ] && flags="--release"
[ -n "$target" ] && flags="$flags --target $target"
# shellcheck disable=SC2086
(cd "$root" && cargo build -p workyard-cli $flags)
if [ -n "$target" ]; then src="$root/target/$target/$profile/workyard"; else src="$root/target/$profile/workyard"; fi
mkdir -p "$root/app/src-tauri/binaries"
cp "$src$ext" "$staged"
echo "staged app/src-tauri/binaries/workyard-$triple$ext ($profile)"
