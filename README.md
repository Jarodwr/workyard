# Workyard

A local-first desktop kanban dashboard for work shared with AI agents: humans plan and supervise, Claude Code workers run cards through an orchestrator.

- [Build specification](docs/SPEC.md)
- [Implementation plan and acceptance checks](docs/IMPLEMENTATION.md)
- [Operating Workyard (human workflow)](docs/WORKFLOW.md)
- [Orchestrator event and notification contract](docs/ORCHESTRATOR.md)
- [Packaging](docs/PACKAGING.md), [releasing](docs/RELEASING.md) and [Homebrew](docs/HOMEBREW.md)
- [Follow-ups: what is built and what is open](docs/FOLLOWUPS.md)
- [Project design notes](docs/PROJECTS.md)
- [Board phases and scheduling](docs/adr/0001-board-phases-and-scheduling.md)
- [Cancelling work](docs/adr/0002-cancelling-work.md)
- [Phase dependency graph](docs/adr/0003-phase-dependency-graph.md)

## Agreed architecture

```text
React UI → Tauri commands ─┐
                           ├→ workyard-core (Rust) → SQLite
agents (Claude Code plugin) → workyard CLI ┘
human terminal → workyard CLI ┘
```

The CLI and Tauri backend share the same Rust command layer. Agents call the CLI with `--json`; a Claude Code plugin in `plugin/` supplies the skill and actor setup. SQLite stays on the user's machine. The event log is the source for live UI updates and orchestrator notifications.

## Desktop UI

The frontend uses Tailwind CSS 4 with local shadcn/ui components in `app/src/components/ui`. The desktop is for watching agent work, answering questions, leaving notes on cards, approving reviewed commits, and pausing or resuming a board's orchestrator. Agents create and manage boards, tasks, progress, and dependencies through the CLI. Run `mise run install`, then `mise run dev`. The frontend uses Yarn 3.6.1 only; do not run `npm install`.

## Current state

The Rust core, CLI, orchestrator, Tauri bridge and React desktop work end to end: Claude workers have built a sample app from a planned board, each in its own git worktree, merging its own work into the main branch. A debug and an unsigned release `.app` build; signing, notarization and publishing a release are not done. [docs/FOLLOWUPS.md](docs/FOLLOWUPS.md) lists what is done, partial and open. A separate demo script can simulate external card moves.

The current UI starts with projects. Selecting one shows Overview, then separate Stakeholders and Boards sections in the sidebar. Each stakeholder row shows their pending question count. Repository details live on Overview. Opening a board shows its phase graph; clicking a phase title opens a Kanban scoped to that phase, with a back button. Completed phases are hidden from the graph by default. Agents can describe, complete, and advance phases, assign work to phases, and cancel obsolete cards through CLI commands. See [docs/PACKAGING.md](docs/PACKAGING.md) for building the app bundle and installing the Claude plugin.

Claimed In Progress tasks can have an attached agent session. The CLI can pause it for a question, read a change briefing, acknowledge delivered events, and resume it. `workyard next` queries paused, actionable sessions before new Backlog work; there is no stored queue. `workyard orch run <board>` is the orchestrator that runs workers (`claude -p`) from that queue; see [docs/WORKFLOW.md](docs/WORKFLOW.md). `workyard status` and `workyard wait` summarize a board for people and scripts, and `workyard doctor` checks the installation.

## Development

```sh
cd app
yarn install
yarn dev                   # browser demo
yarn tauri dev             # desktop app, after Rust dependencies are installed
```

Use `workyard --db /path/to/test.db` or `WORKYARD_DB` to select a database. The CLI also accepts `--actor` and `--json`.

## Packaging

```sh
scripts/stage-sidecar.sh            # build the CLI and stage it as the app's sidecar (once per clone)
cd app && npx tauri build           # Workyard.app and a dmg
claude plugin marketplace add .     # install the Claude plugin from this repo
claude plugin install workyard@workyard
```

Details, signing and the desktop-side code changes are in [docs/PACKAGING.md](docs/PACKAGING.md).
