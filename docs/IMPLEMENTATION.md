# Implementation plan

The build order puts the highest-risk behavior first: an external move must look continuous while UI state survives. Each milestone should leave a runnable, reviewable slice.

Current progress: workspace, migrations, core command surface, CLI, Tauri bridge, and a React board prototype are in source. Core/CLI tests, a Tauri binary build, and the frontend bundle pass. Ordered board phases, opportunistic next-phase scheduling, cancellation, and a phase-grouped dependency graph are implemented. Human writes are limited to answers and card notes. packaging also remains open. The external move animation check has been accepted.

Review workflow follow-up: the agreed agent-led cycle is recorded in `SPEC.md` and `ORCHESTRATOR.md`. New boards now have three lanes; the showcase Review lane was emptied through CLI moves and archived. Other existing boards may still have legacy Review lanes and need owner-authorized moves before archival. Wire the external orchestrator to deliver each linked question answer and explanation to the current ticket owner. Ticket moves and resubmissions remain agent actions.

1. **Skeleton and animation proof.** Create the Cargo workspace, Tauri/React app, and a two-lane fixture. Move one card through a CLI stub while the app is open. Prove cross-lane animation, external highlight, reduced-motion behavior, and persistence of an open card detail draft. Record the chosen rendering approach before building the full board.
2. **Core.** Implement migrations, UUID/display-key generation, typed commands/errors, ownership, versions, dependencies, derived state, atomic one-event writes, and tests. Use temp DBs for tests.
3. **CLI.** Implement all commands, strict JSON stdout, exit codes, and integration tests. Add an optional `--seed-demo` command for screenshots and local development; it must use normal commands so events are valid.
4. **Agent plugin.** The CLI is grouped into `task`, `plan`, `orch`, and `admin`; `plugin/` holds the Claude Code skill and actor hook. Try it with a real agent session.
5. **Desktop board.** Connect Tauri reads to core, expose only question answers and card notes as human writes, and build project-first navigation with project context above nested board views, plus lanes and a card drawer.
6. **Event-driven UI.** Implement cursor-based polling, event effect cues, refreshes, progress animation, catch-up behavior, and reconnect handling. Run the external CLI acceptance scenario.
7. **Dependency view and polish.** Add a sequence and chokepoint graph, dark mode, empty states, keyboard/accessibility checks, macOS packaging, and final acceptance pass.

## Key design checks before broad implementation

- Agree on the JSON shape of an event and its `effects` array, then use one fixture across core, UI, and orchestrator tests.
- Define stable actor names for each agent/client. Ownership is enforced against those names, not authenticated users.
- Define how the orchestrator reads events and delivers messages in its actual runtime. The dashboard exposes the event feed; message transport belongs to the orchestrator.
- Install the `workyard` CLI on the agent's PATH; the Claude Code plugin in `plugin/` assumes it.

## Example acceptance walkthrough

1. Create a board through the CLI, then start the app and confirm it appears.
2. From an agent session with the plugin, create two cards and a question blocking one card.
3. Claim the other card, move it to In Progress, and update progress with a note.
4. Open its detail drawer and type an unsent note, then move it from the terminal with the owning actor and current version. Confirm the note draft remains, the card moves smoothly, and the external highlight appears.
5. Resolve the question; confirm one question event includes the newly ready card in `effects`, and the card pulses once.
6. Make an existing prerequisite of a claimed active card unresolved. Confirm the board shows it blocked; with an orchestrator running, confirm its owner receives one message and only that owner can edit the card.
7. Turn on reduced motion and repeat a move. Confirm state updates promptly without a tween.
