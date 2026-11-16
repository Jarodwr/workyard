# Releasing

`scripts/release.sh` builds and checksums the artifacts. It never tags, pushes or publishes: it prints those commands for you to run.

## 1. Bump the version

```sh
scripts/set-version.sh 0.2.0      # Cargo.toml, tauri.conf.json, plugin and marketplace manifests
cargo build --locked              # refreshes Cargo.lock if a crate version changed; commit it
git commit -am "Release 0.2.0"
```

`scripts/check-version.sh` (also run by `cargo test`) fails if any manifest disagrees. `Cargo.lock` is committed and must stay current: `cargo build --locked` fails otherwise.

## 2. Build the artifacts

```sh
scripts/release.sh 0.2.0           # dist/workyard-0.2.0-<arch>-apple-darwin.tar.gz + dist/SHA256SUMS
scripts/release.sh 0.2.0 --app     # also dist/Workyard-0.2.0-<arch>.dmg (full Tauri build, several minutes)
scripts/release.sh 0.2.0 --dry-run # checks and prints the plan only
```

The script refuses a dirty tree, and a version that differs from `Cargo.toml` or from any manifest. The tarball holds `bin/workyard`, `share/workyard/plugin/` and `LICENSE` (the repository has no `LICENSE` yet: add one before the first public release; the script warns until then). `dist/` is gitignored. The dmg is unsigned unless the `APPLE_*` variables from `docs/PACKAGING.md` are set.

Check before publishing:

```sh
(cd dist && shasum -a 256 -c SHA256SUMS)
tar -tzf dist/workyard-0.2.0-*.tar.gz
```

## 3. Tag and publish

The script ends by printing these, with the right names:

```sh
git tag -a v0.2.0 -m "Workyard 0.2.0"
git push origin v0.2.0
gh release create v0.2.0 dist/workyard-0.2.0-*.tar.gz dist/SHA256SUMS --title "Workyard 0.2.0" --generate-notes
```

Add `dist/Workyard-0.2.0-*.dmg` to the `gh release create` line when you built with `--app`. A Mac with only an ad-hoc signature needs right-click Open (see `docs/PACKAGING.md`).

## 4. Update the Homebrew tap

The tap is `Jarodwr/homebrew-tap`; copy the conventions of `Formula/fennel-ls-rs.rb` and give the formula a doc in the tap's `docs/` folder. Formulas build from the GitHub source archive, not from the tarball above (the tarball is for people who do not want to compile).

1. Get the source archive checksum: `curl -L https://github.com/Jarodwr/workyard/archive/refs/tags/v0.2.0.tar.gz | shasum -a 256`. (Assumes the GitHub repository is `Jarodwr/workyard`; adjust if not.)
2. Create or update `Formula/workyard.rb` (a sketch: not yet built through brew, since no tag exists; step 3 is the first real test):

   ```ruby
   class Workyard < Formula
     desc "Local-first kanban for coordinating AI coding agents"
     homepage "https://github.com/Jarodwr/workyard"
     url "https://github.com/Jarodwr/workyard/archive/refs/tags/v0.2.0.tar.gz"
     sha256 "<checksum from step 1>"
     head "https://github.com/Jarodwr/workyard.git", branch: "main"

     depends_on "rust" => :build

     def install
       system "cargo", "install", "--locked", *std_cargo_args(path: "crates/workyard-cli")
       pkgshare.install "plugin"   # the Claude plugin, next to the binary's share dir
     end

     test do
       assert_match "workyard #{version}", shell_output("#{bin}/workyard --version")
     end
   end
   ```

   The CLI crate builds the `workyard` binary and `workyard --version` prints `workyard <version>`. The plugin ships as `#{pkgshare}/plugin` (the tarball uses `share/workyard/plugin`, the same place). Check the description against the README.
3. Build and test it from a throwaway tap, not your real prefix: `brew tap-new local/scratch`, copy the formula into it, `brew install --build-from-source local/scratch/workyard && brew test local/scratch/workyard`, then `brew uninstall workyard && brew untap local/scratch`.
4. Bottles: on the tap, build with `brew install --build-bottle`, `brew bottle`, upload the result to a tap release named `workyard-0.2.0`, and paste the `bottle do` block, as for the other formulas.
5. Write `docs/workyard.md` in the tap (pins and any differences), commit and push the tap.
