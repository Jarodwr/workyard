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
- Config: `app/src-tauri/tauri.conf.json` (`externalBin`, `resources`, `icon`, `targets: app, dmg`, identifier `dev.workyard.app`). Icons were generated from a 128px placeholder; replace `app/src-tauri/icons/icon.png` with a 1024px source and run `npx tauri icon src-tauri/icons/icon.png`.

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

Orchestrator workers load the plugin with `--claude-arg=--plugin-dir --claude-arg=<path to plugin>`. The `SessionStart` hook (`plugin/hooks/actor.sh`) puts `$WORKYARD_BIN` on `PATH` when `workyard` is not installed, and sets a stable actor unless `WORKYARD_ACTOR` is already set. The orchestrator already prepends its own executable's directory to the worker's `PATH`.

## Code the desktop still needs (not done here: `app/src-tauri/src/main.rs` belongs to another stream)

The app must use the bundled CLI and plugin instead of whatever is on `PATH`. Wherever it spawns `workyard` (start orchestrator, open planner, open in terminal), resolve:

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

Fall back to `workyard` on `PATH` and `plugin/` in a checkout in dev. Export `WORKYARD_BIN=<dir of the CLI>` to spawned terminals/workers so the plugin hook finds it. Optionally add `bundled_cli()`'s directory to the user's PATH via "Install CLI" (symlink to `/usr/local/bin/workyard`).

## Signing and notarization (document only, not run)

Tauri signs and notarizes during `tauri build` when these are set:

- `APPLE_SIGNING_IDENTITY` (for example `Developer ID Application: Name (TEAMID)`), or `APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD` for CI keychain import.
- Notarization: `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`; or `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`.
- The sidecar is signed with the app by Tauri. Check with `codesign --verify --deep --strict Workyard.app` and `spctl -a -vv Workyard.app`.

Unsigned builds run locally; others need right-click Open (or `xattr -dr com.apple.quarantine Workyard.app`).

## CLI changes the skill should follow

Stream A adds `orch session pause --for-integration`, `plan board-review` and `orch run --permissions`. `plugin/skills/workyard/SKILL.md` documents pause with `--waiting-question` and `--for-review` only; once those land, mention `--for-integration` in step 5 and keep the rule that workers use Write/Edit instead of heredocs (added now).
