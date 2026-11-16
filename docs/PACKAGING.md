# Packaging

What ships: the desktop app (`Workyard.app`, plus a dmg), the `workyard` CLI inside it, and the Claude Code plugin.

## Build the app

```sh
scripts/stage-sidecar.sh            # release CLI -> app/src-tauri/binaries/workyard-<triple>
cd app && yarn install && npx tauri build            # app + dmg in target/release/bundle/
npx tauri build --debug --bundles app                # quick check (verified: builds Workyard.app)
```

- `scripts/stage-sidecar.sh --debug` stages a debug CLI for development. **Tauri refuses to build the app crate while `binaries/workyard-<triple>` is missing** (`bundle.externalBin`), including `cargo check -p workyard-desktop`. Run the script once after cloning; the staged file is gitignored.
- Cross target: `scripts/stage-sidecar.sh --target x86_64-apple-darwin` then `npx tauri build --target x86_64-apple-darwin`.
- Config: `app/src-tauri/tauri.conf.json` (`externalBin`, `resources`, `icon`, `targets: app, dmg`, identifier `dev.workyard.app`). Icons are generated from `app/src-tauri/icons/source/icon.svg` (export it to `icon-1024.png` next to it, e.g. `qlmanage -t -s 1024 -o /tmp icon.svg` on macOS), then `cd app && npx tauri icon src-tauri/icons/source/icon-1024.png`; delete the `android/` and `ios/` folders it also emits.

## What the bundle contains

```
Workyard.app/Contents/MacOS/workyard            the CLI (sidecar; Tauri strips the target triple)
Workyard.app/Contents/MacOS/workyard-desktop    the app
Workyard.app/Contents/Resources/marketplace/    a Claude plugin marketplace
    .claude-plugin/marketplace.json
    plugin/                                     the workyard plugin
```

## The Claude plugin

From a checkout: `claude plugin marketplace add <repo>` then `claude plugin install workyard@workyard` (verified in a throwaway `CLAUDE_CONFIG_DIR`; `claude plugin validate` passes for both manifests). From an installed app, add `…/Workyard.app/Contents/Resources/marketplace` instead (verified against the debug bundle). To publish, push the repo and use `claude plugin marketplace add <owner>/<repo>`.

Orchestrator workers load the plugin through `orch run --plugin-dir <path>` (found automatically when omitted). The `SessionStart` hook (`plugin/hooks/actor.sh`) puts `$WORKYARD_BIN` on `PATH` when `workyard` is not installed, and sets a stable actor unless `WORKYARD_ACTOR` is already set. The orchestrator already prepends its own executable's directory to the worker's `PATH`.

## How the desktop finds the CLI and plugin

Done in `app/src-tauri/src/main.rs`: the desktop starts the orchestrator and planner with the CLI next to its own executable (falling back to `workyard` on `PATH`), and exports `WORKYARD_BIN=<that directory>` in terminals it opens so the plugin hook finds it. The CLI locates the plugin itself (`--plugin-dir`, `WORKYARD_PLUGIN_DIR`, the app bundle, `<exe dir>/../share/workyard/plugin`, or a checkout). The sketch below shows the idea and is not code to add:

```rust
// The CLI is next to the app executable in a bundle (Contents/MacOS/workyard).
fn bundled_cli() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let cli = dir.join("workyard");
    cli.exists().then_some(cli)
}
// The plugin sits in the bundle's resources.
fn bundled_plugin(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let dir = app.path().resource_dir().ok()?.join("marketplace/plugin");
    dir.exists().then_some(dir)
}
```

Not built: an "Install CLI" action that links the bundled CLI into `/usr/local/bin`.

## Release build verification (2026-11-16, aarch64, unsigned)

`yarn install` then `npx tauri build` (from `app/`, with `beforeBuildCommand` staging the release CLI) produced `Workyard.app` and `Workyard_0.1.0_aarch64.dmg` under `$CARGO_TARGET_DIR/release/bundle/` (`target/` by default) with no config changes needed. Checked:

- The `.app` has `Contents/MacOS/workyard`, `Contents/MacOS/workyard-desktop`, `Contents/Resources/marketplace/plugin` and `marketplace/.claude-plugin/marketplace.json`.
- The dmg mounts (`hdiutil attach -nobrowse -readonly -mountpoint <dir> <dmg>`) and holds `Workyard.app` and an `Applications` link.
- `claude plugin validate` passes on `…/Resources/marketplace/plugin` and on `…/Resources/marketplace`.
- `Contents/MacOS/workyard-desktop` starts and stays running (checked with `WORKYARD_DB=<tmp>/t.db` so it does not touch the real database; it created the database).
- Smoke-test the bundled CLI with `workyard --version` or `workyard doctor`.

The build is only ad-hoc signed (`codesign -dv` shows `Signature=adhoc`, no TeamIdentifier), so Gatekeeper rejects it when downloaded.

Still needed for a distributable build: an Apple Developer ID Application certificate, then the `APPLE_*` variables below set during `tauri build` (signing plus notarization and stapling). Nothing in the repo needs to change for that; only the environment.

## Signing and notarization (document only, not run)

Tauri signs and notarizes during `tauri build` when these are set:

- `APPLE_SIGNING_IDENTITY` (for example `Developer ID Application: Name (TEAMID)`), or `APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD` for CI keychain import.
- Notarization: `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`; or `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`.
- The sidecar is signed with the app by Tauri. Check with `codesign --verify --deep --strict Workyard.app` and `spctl -a -vv Workyard.app`.

Unsigned builds run locally; others need right-click Open (or `xattr -dr com.apple.quarantine Workyard.app`).

## CLI changes the skill should follow

Done: `orch session pause --for-integration`, `plan board-review` and `orch run --permissions` landed, and `plugin/skills/workyard/SKILL.md` documents `--waiting-question`, `--for-review` and `--for-integration`, the Write/Edit rule, `task report`, and `status`/`wait`. When the CLI surface changes, update the skill in the same commit.

## Stale sidecar and the desktop build

Tauri copies the staged `app/src-tauri/binaries/workyard-<triple>` over `target/<profile>/workyard` on every desktop build and `tauri dev`. That is the path the CLI crate builds to, so a stale staged copy silently replaces a fresh CLI (we saw every `target/debug/workyard` command fail with `schema_too_new` afterwards). To prevent it:

- `beforeDevCommand` runs `scripts/stage-sidecar.sh --debug` and `beforeBuildCommand` runs `scripts/stage-sidecar.sh` (release): the CLI is built first and copied after, then the frontend step runs. `tauri build --debug` still stages a release CLI; that is fine, only freshness matters.
- `scripts/stage-sidecar.sh --check` builds nothing and exits non-zero with a message if the staged binary is missing, a placeholder, or older than any file in `crates/`, `Cargo.toml` or `Cargo.lock`. Run it in CI before packaging, or from a test.
- On a fresh checkout or new worktree `app/src-tauri/build.rs` writes a placeholder script into `binaries/` when nothing is staged, so `cargo check/test --workspace` work without staging. The placeholder only exits with an error; `--check` rejects it, and the real build commands above replace it.

## Hazard: `tauri dev` migrates a database that workers still use

`tauri dev` rebuilds and relaunches the desktop whenever `crates/workyard-core` changes. The new desktop opens the database and can migrate it, while a running orchestrator and its workers (an older binary) still use the same file. They then fail with `schema_too_new`, or worse, work against a schema they do not understand.

While an orchestrator run is in progress, run the desktop from a copied binary instead of `tauri dev`, so source edits cannot relaunch it:

```sh
scripts/stage-sidecar.sh --debug && (cd app && npx tauri build --debug --bundles app)
cp -R "${CARGO_TARGET_DIR:-target}"/debug/bundle/macos/Workyard.app /tmp/Workyard-run.app
open /tmp/Workyard-run.app
```

Keep the orchestrator and workers on a CLI built from the same sources (the copy contains it), and only rebuild or run `tauri dev` again once the run has finished.

## Version, `--version` and `workyard doctor`

The version lives in `[workspace.package]` of the root `Cargo.toml`; every crate (including `app/src-tauri`) inherits it with `version.workspace = true`. The JSON manifests (`app/src-tauri/tauri.conf.json`, `plugin/.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`) are literal.

- `scripts/set-version.sh 0.2.0` changes all of them together (and refreshes `Cargo.lock` offline).
- `scripts/check-version.sh` fails when they disagree. It runs from `cargo test -p workyard-cli` (`versions_are_consistent_everywhere`); run it in CI as well.
- `workyard --version` prints `workyard <version>`; with `--json` it prints `{"ok":true,"data":{"version":...,"schema_version":...}}`.
- `workyard doctor` prints one `OK`/`WARN`/`FAIL` line per check: version, schema, database path (and the orchestrator state of each board), `claude` and `git` (found on `PATH`, with versions), and the plugin directory. `claude` or `git` missing, or an unreadable database, is a FAIL and exits 1; a missing plugin or a database not created yet is a WARN. `--json` returns the same checks as `data.checks`.

## Homebrew layout

The CLI finds its plugin at `<exe dir>/../share/workyard/plugin` after resolving symlinks (Homebrew links `bin/workyard` into the prefix), besides `WORKYARD_PLUGIN_DIR`, the app bundle and a checkout. A formula therefore installs the plugin under `share/workyard`, in the tap's usual shape (see `Formula/fennel-ls-rs.rb` there):

```ruby
depends_on "rust" => :build

def install
  system "cargo", "install", *std_cargo_args(path: "crates/workyard-cli")
  (share/"workyard").install "plugin"
end

test do
  assert_match "workyard #{version}", shell_output("#{bin}/workyard --version")
end
```

Nothing here has been pushed, tagged or installed into a real Homebrew prefix. The rendered formula and cask templates, and the release script, are in [HOMEBREW.md](HOMEBREW.md) and [RELEASING.md](RELEASING.md); publishing them is the user's to do.
