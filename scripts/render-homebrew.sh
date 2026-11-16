#!/bin/sh
# Render the Homebrew formula, cask and doc page for a release into dist/tap/.
#
#   scripts/render-homebrew.sh <version> <source-sha256> [<dmg-sha256>]
#
# <source-sha256> is the checksum of the GitHub source archive for v<version>
# (https://github.com/Jarodwr/workyard/archive/refs/tags/v<version>.tar.gz).
# Without <dmg-sha256> the cask is rendered with the sha256 :no_check placeholder.
# Environment overrides (for testing): WORKYARD_SOURCE_URL, WORKYARD_HOMEPAGE.
# Writes dist/tap/{Formula/workyard.rb,Casks/workyard.rb,docs/workyard.md}; the
# bottle block is left out (docs/HOMEBREW.md says how it is added). Pushes nothing.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

usage() { echo "usage: scripts/render-homebrew.sh <version> <source-sha256> [<dmg-sha256>]" >&2; exit 2; }
[ $# -ge 2 ] && [ $# -le 3 ] || usage
version=${1#v}
src_sha=$2
dmg_sha=${3:-}

for s in "$src_sha" $dmg_sha; do
  printf '%s' "$s" | grep -Eq '^[0-9a-f]{64}$' || { echo "not a sha256: $s" >&2; exit 2; }
done

homepage=${WORKYARD_HOMEPAGE:-https://github.com/Jarodwr/workyard}
source_url=${WORKYARD_SOURCE_URL:-$homepage/archive/refs/tags/v$version.tar.gz}
license=MIT

current=$(sed -nE '/^\[workspace\.package\]/,/^\[/ s/^version = "(.*)"/\1/p' Cargo.toml | head -n 1)
[ "$version" = "$current" ] || echo "warning: Cargo.toml is at $current, rendering for $version" >&2

if [ -n "$dmg_sha" ]; then
  dmg_line="  sha256 \"$dmg_sha\""
  cask_note="The dmg is not notarized unless it was built with the \`APPLE_*\` variables; see docs/HOMEBREW.md in the Workyard repository."
else
  dmg_line="  sha256 :no_check # TODO: replace with the dmg checksum before publishing"
  cask_note="The cask has no dmg checksum yet (\`sha256 :no_check\`); render again with the dmg checksum before publishing."
fi
unsigned_note='  caveats <<~EOS
    Workyard.app is not signed with an Apple Developer ID or notarized, so
    macOS blocks it on first launch. Allow it once with
      xattr -dr com.apple.quarantine /Applications/Workyard.app
    or right-click the app and choose Open.
  EOS'

out=dist/tap
rm -rf "$out"
mkdir -p "$out/Formula" "$out/Casks" "$out/docs"

# Values contain `&`, `|` and `\` rarely; escape them for sed anyway.
esc() { printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'; }
render() {
  sed \
    -e "s|@VERSION@|$(esc "$version")|g" \
    -e "s|@SOURCE_URL@|$(esc "$source_url")|g" \
    -e "s|@SOURCE_SHA256@|$src_sha|g" \
    -e "s|@HOMEPAGE@|$(esc "$homepage")|g" \
    -e "s|@LICENSE@|$license|g" \
    "$1"
}

render packaging/homebrew/workyard.rb.tmpl > "$out/Formula/workyard.rb"
render packaging/homebrew/workyard-app.rb.tmpl |
  awk -v dmg="$dmg_line" -v note="$unsigned_note" '
    /@DMG_SHA256@/ { print dmg; next }
    /^@UNSIGNED_NOTE@$/ { print note; next }
    { print }' > "$out/Casks/workyard.rb"
render packaging/homebrew/workyard.md.tmpl |
  awk -v note="$cask_note" '/^@CASK_NOTE@$/ { print note; next } { print }' > "$out/docs/workyard.md"

if grep -rn '@[A-Z_]*@' "$out"; then echo "unfilled placeholder in $out" >&2; exit 1; fi

echo "rendered $out/ for workyard $version:"
find "$out" -type f | sort
