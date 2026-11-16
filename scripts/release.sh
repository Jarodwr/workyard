#!/bin/sh
# Build the release artifacts into dist/ and print the commands that publish them.
#
#   scripts/release.sh <version>             CLI tarball + SHA256SUMS
#   scripts/release.sh <version> --app       also the desktop dmg (slow: full Tauri build)
#   scripts/release.sh <version> --dry-run   check everything and print the plan; build nothing
#
# Refuses a dirty tree or a version that differs from Cargo.toml (scripts/check-version.sh).
# Never tags, pushes or creates a release: it prints the `git tag` and `gh release create`
# commands for you to run. See docs/RELEASING.md.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

usage() { echo "usage: scripts/release.sh <version> [--app] [--dry-run]" >&2; exit 2; }

version=""
app=0
dry=0
for arg in "$@"; do
  case "$arg" in
    --app) app=1 ;;
    --dry-run) dry=1 ;;
    -*) usage ;;
    *) [ -z "$version" ] || usage; version="$arg" ;;
  esac
done
[ -n "$version" ] || usage
version=${version#v}

[ "$(uname -s)" = Darwin ] || { echo "release.sh builds macOS artifacts; run it on a Mac" >&2; exit 1; }
case "$(uname -m)" in
  arm64|aarch64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac

if [ -n "$(git status --porcelain)" ]; then
  echo "refusing to release from a dirty tree; commit or stash first:" >&2
  git status --short >&2
  exit 1
fi

# check-version.sh compares every manifest with Cargo.toml; then ours must match too.
sh scripts/check-version.sh >/dev/null || { sh scripts/check-version.sh >&2 || true; exit 1; }
current=$(sed -nE '/^\[workspace\.package\]/,/^\[/ s/^version = "(.*)"/\1/p' Cargo.toml | head -n 1)
if [ "$version" != "$current" ]; then
  echo "version mismatch: asked for $version but the repository is at $current" >&2
  echo "bump it with scripts/set-version.sh $version, commit, and run this again" >&2
  exit 1
fi

tag="v$version"
tarball="workyard-$version-$arch-apple-darwin.tar.gz"
dmg="Workyard-$version-$arch.dmg"
tdir=${CARGO_TARGET_DIR:-$root/target}
dist="$root/dist"

if [ -n "$(git tag --list "$tag")" ]; then
  echo "note: tag $tag already exists" >&2
fi
[ -f LICENSE ] || echo "warning: no LICENSE file in the repository; the tarball will not contain one" >&2

if [ $dry -eq 1 ]; then
  echo "dry run: would build"
  echo "  cargo build --release -p workyard-cli --locked"
  echo "  dist/$tarball (bin/workyard, share/workyard/plugin, LICENSE)"
  [ $app -eq 1 ] && echo "  (cd app && yarn install --frozen-lockfile && npx tauri build) -> dist/$dmg"
  echo "  dist/SHA256SUMS"
else
  cargo build --release -p workyard-cli --locked

  rm -rf "$dist"
  mkdir -p "$dist"
  stage=$(mktemp -d)
  trap 'rm -rf "$stage"' EXIT
  root_dir="$stage/workyard-$version-$arch-apple-darwin"
  mkdir -p "$root_dir/bin" "$root_dir/share/workyard"
  cp "$tdir/release/workyard" "$root_dir/bin/workyard"
  cp -R plugin "$root_dir/share/workyard/plugin"
  [ -f LICENSE ] && cp LICENSE "$root_dir/LICENSE"
  # The plugin directory is a checkout; keep the archive free of macOS metadata.
  find "$root_dir" -name .DS_Store -delete
  COPYFILE_DISABLE=1 tar -czf "$dist/$tarball" -C "$stage" "workyard-$version-$arch-apple-darwin"

  # The binary must report the version being released.
  reported=$("$root_dir/bin/workyard" --version)
  case "$reported" in
    *"$version"*) ;;
    *) echo "built CLI reports '$reported', expected $version" >&2; exit 1 ;;
  esac

  if [ $app -eq 1 ]; then
    (cd app && yarn install --frozen-lockfile && npx tauri build --bundles dmg)
    built=$(ls "$tdir"/release/bundle/dmg/Workyard_"$version"_*.dmg 2>/dev/null | head -n 1)
    [ -n "$built" ] || { echo "no dmg for $version under $tdir/release/bundle/dmg" >&2; exit 1; }
    cp "$built" "$dist/$dmg"
  fi

  (cd "$dist" && shasum -a 256 $(ls | grep -E "\.(tar\.gz|dmg)$") > SHA256SUMS)
  echo "built:"
  (cd "$dist" && ls -l)
  echo
  cat "$dist/SHA256SUMS"
fi

assets="dist/$tarball"
[ $app -eq 1 ] && assets="$assets dist/$dmg"
assets="$assets dist/SHA256SUMS"
cat <<EOF

Nothing has been tagged, pushed or published. To publish, run:

  git tag -a $tag -m "Workyard $version"
  git push origin $tag
  gh release create $tag $assets --title "Workyard $version" --generate-notes

Then update the Homebrew tap (docs/RELEASING.md). The source archive that the formula needs:

  https://github.com/Jarodwr/workyard/archive/refs/tags/$tag.tar.gz
EOF
