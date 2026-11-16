#!/bin/sh
# Run by the Workyard orchestrator once, from the root of a newly created worker worktree.
# A worktree is a bare checkout, so gitignored files are missing; restore what workers need.
# Env: WORKYARD_MAIN_CHECKOUT (the main checkout), WORKYARD_CARD (the card key).
set -eu
main=${WORKYARD_MAIN_CHECKOUT:-}

# app/node_modules: share the main checkout's, else install.
if [ ! -e app/node_modules ] && [ -f app/package.json ]; then
  if [ -n "$main" ] && [ -d "$main/app/node_modules" ]; then
    ln -s "$main/app/node_modules" app/node_modules
    echo "linked app/node_modules"
  elif [ -f app/package-lock.json ]; then
    (cd app && npm ci)
  fi
fi

# Staged sidecar binaries: Tauri refuses to build without them. Copy the staged files from the
# main checkout (not symlinks: stage-sidecar.sh's cp would write through into the main checkout);
# with none there, build.rs writes a placeholder on the first cargo build.
if [ -n "$main" ] && [ -d "$main/app/src-tauri/binaries" ]; then
  mkdir -p app/src-tauri/binaries
  for f in "$main"/app/src-tauri/binaries/workyard-*; do
    [ -f "$f" ] || continue
    dest=app/src-tauri/binaries/$(basename "$f")
    [ -e "$dest" ] || cp "$f" "$dest"
  done
fi
