#!/bin/sh
# Build the `workyard` CLI and stage it where Tauri expects an external binary:
# app/src-tauri/binaries/workyard-<target-triple>. Tauri refuses to build the app crate
# (even in dev) when that file is missing, so run this once after cloning.
#
#   scripts/stage-sidecar.sh            release build for the host triple (use for packaging)
#   scripts/stage-sidecar.sh --debug    debug build (fast; use for development)
#   scripts/stage-sidecar.sh --target aarch64-apple-darwin   cross target (rustup target add first)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
profile=release
target=""
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug ;;
    --target) shift; target="$1" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
host=$(rustc -vV | sed -n 's/^host: //p')
triple=${target:-$host}
flags=""
[ "$profile" = release ] && flags="--release"
[ -n "$target" ] && flags="$flags --target $target"
# shellcheck disable=SC2086
(cd "$root" && cargo build -p workyard-cli $flags)
if [ -n "$target" ]; then src="$root/target/$target/$profile/workyard"; else src="$root/target/$profile/workyard"; fi
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac
mkdir -p "$root/app/src-tauri/binaries"
cp "$src$ext" "$root/app/src-tauri/binaries/workyard-$triple$ext"
echo "staged app/src-tauri/binaries/workyard-$triple$ext ($profile)"
