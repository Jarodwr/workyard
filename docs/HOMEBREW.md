# Adding Workyard to the Homebrew tap

Everything for `Jarodwr/homebrew-tap` is generated from `packaging/homebrew/`. Nothing here pushes, tags, uploads or touches the tap: you copy the output across yourself. The tap's conventions come from `Formula/fennel-ls-rs.rb` and `docs/helix-steel.md`.

| Template | Becomes | Purpose |
|---|---|---|
| `packaging/homebrew/workyard.rb.tmpl` | `Formula/workyard.rb` | Builds the CLI from the GitHub source archive, installs the plugin to `share/workyard/plugin`, `head`, caveats, `test do` |
| `packaging/homebrew/workyard-app.rb.tmpl` | `Casks/workyard.rb` | Installs `Workyard.app` from the dmg and links the bundled CLI |
| `packaging/homebrew/workyard.md.tmpl` | `docs/workyard.md` | The doc page the tap keeps per formula |

Source and homepage are `https://github.com/Jarodwr/workyard`, licence MIT (board decisions). The repository has no `LICENSE` file yet: add one before the first release, so the tarball and the GitHub archive carry it.

## 1. Release first

Follow `docs/RELEASING.md` steps 1 to 3: bump the version, `scripts/release.sh <version> --app`, tag, push and `gh release create` with the tarball, `SHA256SUMS` and the dmg (`Workyard-<version>-aarch64.dmg`). The formula needs the tag `v<version>` to exist on GitHub.

## 2. Render

```sh
curl -sL https://github.com/Jarodwr/workyard/archive/refs/tags/v0.2.0.tar.gz | shasum -a 256   # source sha256
grep dmg dist/SHA256SUMS                                                                      # dmg sha256
scripts/render-homebrew.sh 0.2.0 <source-sha256> <dmg-sha256>
```

This writes `dist/tap/Formula/workyard.rb`, `dist/tap/Casks/workyard.rb` and `dist/tap/docs/workyard.md` (`dist/` is gitignored). Without the dmg checksum the cask gets `sha256 :no_check` and a TODO; do not publish it that way. The bottle block is deliberately left out.

## 3. Check it in a throwaway tap

Never install into your real prefix to test. Use a scratch tap, then remove it. `brew uninstall` also autoremoves formulas nothing else needs, so set `HOMEBREW_NO_AUTOREMOVE=1` for that step.

```sh
brew tap-new --no-git local/scratch
cp dist/tap/Formula/workyard.rb "$(brew --repository)"/Library/Taps/local/homebrew-scratch/Formula/
brew install --build-from-source local/scratch/workyard
brew test local/scratch/workyard
brew audit --strict local/scratch/workyard
brew style local/scratch/workyard
HOMEBREW_NO_AUTOREMOVE=1 brew uninstall workyard && brew untap local/scratch
```

For the cask, copy it to `Casks/` in the scratch tap and run `brew trust local/scratch`, `brew style --cask local/scratch/workyard` and `brew audit --cask --strict local/scratch/workyard`. To try the formula before a tag exists, point it at a local archive: `git archive --format=tar.gz --prefix=workyard-<version>/ -o /tmp/x/workyard-<version>.tar.gz HEAD`, then render with `WORKYARD_SOURCE_URL=file:///tmp/x/workyard-<version>.tar.gz`.

Checked on 2026-11-17 with the 0.1.0 sources (`file://` archive of this repository, Homebrew 7.0.13, Apple silicon): `brew install --build-from-source` (37 s), `brew test`, `brew audit --strict` and `brew style` pass for the formula; `brew style --cask` and `brew audit --cask --strict` pass for the cask (the cask was not installed, since no dmg exists yet).

## 4. Add to the tap

In a clone of `Jarodwr/homebrew-tap`, on a branch:

```sh
cp dist/tap/Formula/workyard.rb Formula/
mkdir -p Casks && cp dist/tap/Casks/workyard.rb Casks/
cp dist/tap/docs/workyard.md docs/
```

Add `workyard` to the formula table in the tap's `README.md`, commit, push the branch and open a PR.

## 5. Bottles

As for the other formulas: do not paste a `bottle do` block by hand. The PR runs **brew test-bot** (`.github/workflows/tests.yml`), which builds and tests the bottle on macOS; once green, run **brew pr-pull** with the PR number. It uploads the bottle to a release named `workyard-<version>` and commits the block, which looks like:

```ruby
bottle do
  root_url "https://github.com/Jarodwr/homebrew-tap/releases/download/workyard-0.2.0"
  sha256 cellar: :any_skip_relocation, arm64_tahoe: "<sha>"
end
```

To do it by hand: `brew install --build-bottle jarodwr/tap/workyard`, `brew bottle --json --root-url=<the root_url> jarodwr/tap/workyard`, upload the `.tar.gz` to that release, and paste the block. A formula change that keeps the version (for instance a caveats edit that changes the build) needs `revision 1` so a new bottle is built. Casks are not bottled.

## 6. Signing and notarization for the cask

`scripts/release.sh --app` produces an ad-hoc signed dmg unless the `APPLE_*` variables from `docs/PACKAGING.md` are set. Gatekeeper rejects a downloaded ad-hoc build, and Homebrew has been phasing out casks that need quarantine workarounds (`homebrew/cask` no longer accepts unsigned apps; third-party taps still can). For the tap you have two choices:

- **Unsigned (what the rendered cask assumes):** it carries a `caveats` block telling users to run `xattr -dr com.apple.quarantine /Applications/Workyard.app` or right-click Open. Fine for your own tap.
- **Signed and notarized (recommended before wide use):** needs a paid Apple Developer account, a *Developer ID Application* certificate, and either an app-specific password or an App Store Connect API key. Build the dmg with `APPLE_SIGNING_IDENTITY` and `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` (or `APPLE_API_*`) set; Tauri signs, notarizes and staples. Verify with `codesign --verify --deep --strict Workyard.app` and `spctl -a -vv Workyard.app`, then remove the `caveats` block from `workyard-app.rb.tmpl`.

The cask links the bundled CLI with `binary`, the same name as the formula's: users install one or the other. The cask is Apple silicon only; an Intel dmg needs its own build and the `arch` / `sha256 arm:, intel:` form.
