# Workyard

A local-first desktop kanban dashboard for work shared with AI agents. Implementation is underway.

- [Build specification](docs/SPEC.md)
- [Implementation plan and acceptance checks](docs/IMPLEMENTATION.md)
- [Orchestrator event and notification contract](docs/ORCHESTRATOR.md)
- [Project design notes](docs/PROJECTS.md)
- [Board phases and scheduling](docs/adr/0001-board-phases-and-scheduling.md)
- [Cancelling work](docs/adr/0002-cancelling-work.md)
- [Phase dependency graph](docs/adr/0003-phase-dependency-graph.md)

## Agreed architecture

```text
React UI → Tauri commands ─┐
                           ├→ workyard-core (Rust) → SQLite
MCP server → workyard CLI ────┘
human terminal → workyard CLI ┘
```

The CLI and Tauri backend share the same Rust command layer. The MCP server invokes the CLI with structured JSON output. SQLite stays on the user's machine. The event log is the source for live UI updates and orchestrator notifications.

## Desktop UI

The frontend uses Tailwind CSS 4 with local shadcn/ui components in `app/src/components/ui`. The desktop is for watching agent work, answering questions, and leaving notes on cards. Agents create and manage boards, tasks, progress, and dependencies through the CLI/MCP. Run `mise run install`, then `mise run dev`. The frontend uses Yarn 3.6.1 only; do not run `npm install`.

## Current state

The Rust core, CLI, MCP adapter, Tauri bridge, and React board have initial implementations. A separate demo script can simulate external card moves. The SQLite migrations, core tests, a CLI workflow test, a Tauri binary build, and the frontend production build pass. The desktop animation acceptance check has been reviewed and accepted.

The current UI starts with projects. Selecting one shows Overview, then separate Stakeholders and Boards sections in the sidebar. Each stakeholder row shows their pending question count. Repository details live on Overview. Opening a board shows its phase graph; clicking a phase title opens a Kanban scoped to that phase, with a back button. Completed phases are hidden from the graph by default. Agents can describe, complete, and advance phases, assign work to phases, and cancel obsolete cards through CLI/MCP commands. Final packaging remains to be built.

Claimed In Progress tasks can have an attached agent session. The CLI/MCP can pause it for a question, read a change briefing, acknowledge delivered events, and resume it. `workyard next` queries paused, actionable sessions before new Backlog work; there is no stored queue. Agent transport and worker execution still belong to an external orchestrator.

## Development

```sh
cd app
yarn install
yarn dev                   # browser demo
yarn tauri dev             # desktop app, after Rust dependencies are installed
```

Use `workyard --db /path/to/test.db` or `WORKYARD_DB` to select a database. The CLI also accepts `--actor` and `--json`.
