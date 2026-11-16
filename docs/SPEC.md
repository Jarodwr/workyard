# Workyard — Build Spec

Status: agreed design, 2026-11-15. This document supersedes the initial draft and incorporates the decisions made during review.

## 1. Product and scope

A local-first desktop kanban dashboard for work done with AI agents. All persistent data lives in SQLite on the user's machine. The desktop is a human review surface: it displays agent work, accepts answers to questions, and lets the human append notes to cards. Agents create and manage work through named CLI commands. An open board reacts to each committed event, including actions made by an agent outside the app.

### Which document is authoritative

This file describes the core model, ownership, events, and UI. Where another document covers a topic, that document wins:

| Topic | Authority |
|---|---|
| Projects, repositories, stakeholders, start/finish dependencies, review gates | [PROJECTS.md](PROJECTS.md) |
| Phases and `next` scheduling | [ADR 0001](adr/0001-board-phases-and-scheduling.md) |
| Cancelling work | [ADR 0002](adr/0002-cancelling-work.md) |
| Phase dependency graph | [ADR 0003](adr/0003-phase-dependency-graph.md) |
| Orchestrator events, sessions, and delivery | [ORCHESTRATOR.md](ORCHESTRATOR.md) |
| Build order and acceptance checks | [IMPLEMENTATION.md](IMPLEMENTATION.md) |

The local train runner and hosted review observation remain open. Legacy data (`priority`, `assignee`, Review and custom lanes) is retained for compatibility only and is ignored by the CLI, MCP, and scheduling.

### Goals

- Multiple boards with three fixed lanes (Backlog, In Progress, Done), ordered cards, dependency impact, blocked state, and engineering and business questions with answer options.
- Smooth movement between lanes for external CLI actions.
- Progress percentage and a short status note visible on every card.
- Agent-friendly reads and writes through named CLI commands.
- Exclusive ownership of a claimed card by one agent, with optimistic version checks on edits.
- Local operation, with no account or network service required.

### Non-goals for v1

- Multi-user sync, authentication, cloud storage, or security against another process running as the same local user.
- Polished Linux and Windows packaging. Keep the design portable, but verify macOS first.
- Free-form SQL, rich text, attachments, and time tracking.
- A built-in agent orchestrator or messaging transport. The app provides an event contract for a running orchestrator to consume.

## 2. Stack and repository

| Layer | Choice |
|---|---|
| Desktop | Tauri 2 |
| Frontend | React, TypeScript, Vite, Motion (`motion/react`); dnd-kit if drag is included |
| Shared core | Rust `workyard-core`, `rusqlite` with bundled SQLite, `serde`, `thiserror` |
| CLI | Rust `workyard` binary using `clap` and `workyard-core` |
| Agent access | The `workyard` CLI with `--json`, plus a Claude Code plugin (`plugin/`: skill and actor hook). There is no MCP server; the app is local and agents have a shell |
| Database | SQLite with WAL, foreign keys, and busy timeout |

Rust owns the schema, rules, transactions, event construction, CLI, and Tauri command wrappers. React and TypeScript own presentation, interaction, and animation. Do not add a Node sidecar.

```text
workyard/
  README.md
  Cargo.toml
  crates/
    workyard-core/
    workyard-cli/
  app/
    src-tauri/
    src/
  docs/
    SPEC.md
    IMPLEMENTATION.md
```

The default DB is in the platform app-data directory (for example `~/Library/Application Support/workyard/workyard.db` on macOS). `WORKYARD_DB` and `--db` override it. Embedded migrations run at startup and maintain `schema_version`. Each connection enables foreign keys, sets a busy timeout, and uses WAL. Write commands use `BEGIN IMMEDIATE`.

## 3. Identity, time, and data model

Every board, lane, card, question, and dependency has its own UUID primary key. UUIDs are internal, stable references. Boards, cards, and questions also have unique, human-friendly display keys such as `B-1`, `C-12`, and `Q-3`, assigned by transactional per-type counters. The CLI accepts display keys and return both keys and UUIDs. Lane references accept a UUID or an unambiguous name within a board. Events use an integer, monotonic cursor ID. Timestamps are UTC ISO-8601 text.

The Rust core exposes typed `Card` and `Question` snapshots, distinct board/card/question/lane ID types, enums for lane kind, question kind/status, and legacy priority, plus validated `Progress`, `Version`, and `QuestionOptions` values. Dependencies resolve to a `Card` or `Question` target; question resolution uses an `Answered` or `Dropped` outcome. The core serializes these types to the existing JSON shape for the CLI, Tauri, and events. Existing databases may contain non-UUID historical IDs; their typed ID wrappers preserve those strings, while all newly generated IDs are UUIDs.

### Tables

| Table | Fields and constraints |
|---|---|
| `boards` | `id` UUID, `key`, `name`, `description`, `created_at`, `archived` boolean, `version` |
| `lanes` | `id` UUID, `board_id` FK, `name`, `position`, `kind` (`backlog`, `active`, `done`; legacy `review`/`custom` values remain readable for migration), `archived` boolean, `version`; unique active name within a board |
| `cards` | `id` UUID, `key`, `board_id` FK, `lane_id` FK, `position`, `title`, `description`, legacy `priority` (`P0`–`P3`, retained for database and core API compatibility; ignored by the CLI and scheduling), `progress` integer 0–100, `status_note`, `agent_type` free text, `owner_actor` nullable, `created_at`, `updated_at`, `version` |
| `card_reports` | `card_id` PK/FK, `board_id` FK, `body` markdown (max 256 KB), `actor`, `version` (starts at 1, +1 per replacement), `updated_at`; one report per card, replaced whole |
| `card_notes` | `id` UUID, `board_id` FK, `card_id` FK, `actor`, `body`, `created_at`; append-only comments separate from agent-owned card fields |
| `task_sessions` | `id` UUID, `card_id` FK, owner actor, runner, external session ID, attach/pause/detach times, optional waiting question FK, and last delivered event ID; at most one attached session per task |
| `questions` | `id` UUID, `key`, `board_id` FK, `kind` (`engineering`, `business`), `title`, `body`, `options_json` (2–10 distinct substantive choices plus `Other`), `recommended_option` (one of the choices), `recommendation_reason` (plain-language, at most 280 characters), `status` (`open`, `answered`, `dropped`), selected `answer`, optional `answer_note`, `owner`, `created_at`, `answered_at`, `version` |
| `dependencies` | `id` UUID, `board_id` FK, `from_card_id` FK, `to_card_id` nullable FK, `to_question_id` nullable FK, `created_at`; exactly one target column must be non-null; unique per source/target |
| `events` | `id` autoincrement integer, `ts`, `actor`, `type`, `board_id`, `entity_type`, `entity_id`, `before` JSON, `after` JSON, `effects` JSON array, `note` |
| `id_counters` | type and next human-readable number; increments in the same transaction as creation |
| `projects`, `project_repositories`, `project_documents`, `project_stakeholders` | Added in migration 7. A board has an optional `project_id`; a question has an optional `stakeholder_id`. See [PROJECTS.md](PROJECTS.md) |
| `card_changes` | Added in migration 7. Linked repository, working and target branch, `commit_oid`, `review_required`, `approved_commit_oid`, `reviewer`, `version`. A new commit clears approval |
| `phases` | Added in migrations 8 and 9. Ordered per board with a unique name, description, and `version`. `phases.closed_at` and `boards.current_phase_id` are no longer read or written (the columns stay for old databases); phase status is derived, see ADR 0001 addendum; `cards` and `questions` carry `phase_id`. See ADR 0001 |
| `orchestrator_sessions` | Added in migrations 11 and 12. One row per board: `desired_state` (`running`, `paused`, `draining`), event `cursor`, `pool_size`, the lease (`holder`, `host`, `pid`, `epoch`, `heartbeat_at`, `lease_expires_at`), and `backoff_until`/`backoff_reason`. See [ORCHESTRATOR.md](ORCHESTRATOR.md) |
| `cards.workspace`, `task_sessions.held_by` | Added in migration 13. `workspace` is `worktree` (default) or `none`; `held_by` names a human who has taken a session over |
| `dependencies.scope` | Added in migration 7: `start` (default) or `finish` |
| `cards.cancelled_at`, `cancellation_reason`, `cancelled_by` | Added in migration 8. A terminal marker, not a lane. See ADR 0002 |

All FKs have explicit delete rules. Prefer preventing deletion of referenced records in v1; archive rather than delete boards and lanes. Enforce enum values, progress bounds, UUID/display-key uniqueness, exactly-one-target dependencies, and no self-dependency in the database where possible. The command layer checks cross-board references and card-to-card cycles inside the write transaction. Question edges cannot form a cycle because questions have no outgoing edges.

Migration 3 adds `agent_type`. Older databases retain their legacy `assignee` column for data preservation, but application commands and reads no longer use it. Existing cards have an unspecified agent type until an agent updates them.

Migration 4 adds answer options and explanation notes. Old questions receive `Yes`, `No`, and `Other` as a compatibility fallback. Agents can replace those choices on open questions with `update_question_options`; new questions must provide meaningful choices explicitly. `Other` requires an explanatory note when selected.

Migration 5 adds agent recommendations. Older questions have no recommendation until an agent updates them; the UI does not invent one. New questions must recommend exactly one supplied option and give a short, simple reason. The human can select any option regardless of the recommendation.

Migration 6 ensures existing questions have exactly one canonical `Other` choice at the end, preserving their substantive choices, selected answers, recommendations, versions, and event history. For every new or revised question, the core appends `Other` automatically as the last option. Agents must supply 2–10 distinct substantive choices and must not include `Other`; an explicit `Other` is rejected. Choosing `Other` requires an explanation.

`board_id` and `lane_id` on a card must refer to the same board; enforce this in the command layer and, where practical, with a composite database constraint. A dependency's source and target must belong to its `board_id`.

### Derived state

- A card is **blocked** if any dependency targets a card outside a `done` lane that is not cancelled or an `open` question.
- A **cancelled** prerequisite never finishes, so it blocks neither start nor finish scope. Cancelling it recomputes its dependents in the cancelling transaction and emits the usual `card_became_ready` / `card_unblocked` and `card_can_finish` effects, plus `prerequisite_cancelled`, so owners and the orchestrator notice. The link stays: the card snapshot lists it under `cancelled_prerequisites` (not in `blockers`), card detail shows it struck through, and the graph greys and strikes the node. A cancelled card is terminal and cannot be reopened, so the link never re-blocks; if a way to reopen is ever added, recomputation re-blocks it. A cancelled prerequisite no longer holds a phase open, and `blocking_impact` ignores it. Adding a new dependency on cancelled work is still rejected.
- A card is **ready** if it is not blocked and is outside a `done` lane.
- **Blockers** are the unresolved target IDs and titles. Blocked state is computed on reads and never stored on cards.
- An active card can become blocked when an existing prerequisite moves out of `done`. It stays in its lane and gains a blocked badge.
- A question's `dropped` status resolves it for dependency purposes, just as `answered` does.
- **Blocking impact** is the number of distinct downstream cards reachable through unresolved dependency links. It is computed on reads, changes as work is completed or questions are resolved, and is the only scheduling rank in v1. Cards with equal impact retain backlog lane order. A card with no downstream dependents has impact zero; this does not mean its work has no value.

## 4. Command and ownership contract

`workyard-core` is the only application write path. Each successful write uses one transaction and appends **exactly one event for that command**. A failed write rolls back all data changes and appends no event. Every command receives an `actor` string. Errors return `{ code, message, hint, details? }`; relevant errors include the current entity/version or a list of blockers.

### Ownership

`claim_card(card, expected_version)` sets `owner_actor` to the caller and increments the card version. A claimed card's fields, lane, progress, and outgoing dependency links may be changed or released only by that exact owner actor. `agent_type` names the capability needed for a task, such as Frontend or Documentation; `owner_actor` identifies the agent currently holding the work lock. An unclaimed card can be edited by any local actor. Claiming an already claimed card fails with `ownership_conflict` and identifies the owner.

`release_card` requires the owner and expected version. `transfer_card` requires the current owner and expected version. `force_release_card` is an explicit local recovery command for a dead agent; it records actor, reason, and previous owner in the event. It is available through the CLI. Since actors are local strings rather than authenticated principals, this is cooperative process ownership, not protection from a malicious same-user process.

Card notes are separate append-only records. A human can add a note to an agent-owned card without editing the card's content, owner, status note, or version. Each note has its own UUID and causes one `card_note_added` event. Agents can read notes in `view_card` and may append notes through the CLI as well.

Every semantic mutation of a card, including claim/release, move, progress, and outgoing dependency changes, requires `expected_version` and increments `version`. Position renormalization of sibling cards is lane-order housekeeping: it may shift their stored position, but does not change their content or version. Version conflicts include the current card so the caller can reread and retry. A long-held mutex or lock is not used. A write to a prerequisite card or question can change a claimed card's **derived** blocked state without editing the claimed card. If a new dependency on an owned card is discovered, the owner adds it after being informed; another actor does not silently edit its links.

Board updates use the board's expected version. The three work lanes have fixed names and order; normal commands cannot create, rename, reorder, or archive them. The CLI retains `archive-lane` solely to clean up an empty legacy Review or custom lane. Answering or dropping a question requires its expected version and fails if it is no longer open. Creation commands and read commands need no expected version.

### Reads

| Command | Input | Result |
|---|---|---|
| `list_boards` | none | Boards and per-lane card counts |
| `view_board` | board key/UUID; optional lane, agent type, blocked, text filters | Ordered lanes/cards; each card includes version, owner, progress, note, blocked flag, blockers, open-question count, and blocking impact |
| `view_card` | card key/UUID | Full card, attached session if any, inbound/outbound dependencies, card notes, last 20 events |
| `list_questions` | board optional; kind/status optional | Questions and cards they block; absent board means all boards |
| `next_ready_task` | board; agent type optional | First an eligible paused In Progress task with `dispatch_action: resume`, otherwise a ready, unclaimed Backlog task with `dispatch_action: claim`. Current phase sorts first, then nearer phases, within each group; the board's `phase_window` (default unlimited; `1` offers the next phase only once the current phase has no ready work) limits how far ahead cards are offered, and `orchestrator_status` reports the cards held back as `held_back_by_phase`; blocking impact and lane position break ties. The queue is computed on each read. |
| `task_session_briefing` | card, scan limit, optional after cursor | Current task and session plus relevant events since that session's delivered cursor or supplied page cursor; returns a page cursor and `has_more` without acknowledging delivery |
| `view_history` | board or card; `since_event_id`, limit | Events in ascending ID order with effects |
| `poll_events` | `since_event_id`, limit | Next page of events, last returned cursor, `has_more` |

### Writes

| Command | Input and behavior |
|---|---|
| `create_board` | Name, description; create Backlog, In Progress, and Done lanes |
| `archive_board`, `unarchive_board` | Board, expected version |
| `archive_lane` (CLI recovery only) | Empty legacy Review or custom lane, expected version; fixed work lanes cannot be archived |
| `create_card` | Board, title, optional description/lane/agent type; default Backlog and progress 0 |
| `claim_card`, `release_card`, `transfer_card`, `force_release_card` | Ownership operations as above |
| `update_card` | Card, expected version, title/description/agent type patch |
| `move_card` | Card, destination lane, optional position, expected version |
| `update_progress` | Card, 0–100, optional note, expected version |
| `set_card_report` | Card, nonblank markdown body of at most 256 KB (`validation_error` beyond); replaces the card's report, bumps the report version, emits one `card_report_set` event (size, not body); no card version or ownership required |
| `add_card_note` | Card, nonblank body; no expected version or card ownership required |
| `attach_task_session`, `pause_task_session`, `resume_task_session`, `acknowledge_task_session`, `detach_task_session` | Manage an attached agent session and its delivered-event cursor without changing the card version. Pausing may name a question whose resolution wakes the agent even when other blockers remain. Only a claimed In Progress task can attach, pause, resume, or acknowledge a session. Completing, cancelling, releasing, transferring, or moving a task back to Backlog detaches its session. |
| `add_dependency` | From card, exactly one target card/question, expected source-card version |
| `remove_dependency` | Dependency ID or source/target pair, expected source-card version |
| `raise_question` | Board, kind, title, 2–10 distinct substantive choices (`Other` added automatically), recommended option, simple reason, optional body and `blocks` list; linked cards must be unclaimed or owned by caller; one atomic command |
| `update_question_options` | Open question, 2–10 replacement substantive choices (`Other` added automatically), recommended option and reason, expected version; agents can improve legacy or outdated choices |
| `answer_question` | Question, one exact option, optional explanation (required for `Other`), expected version |
| `drop_question` | Question, reason, expected version |

Do not add generic SQL or arbitrary file tools. If a new capability is needed, add a named command in core and surface it through each relevant adapter.

### Move and progress rules

- A blocked card cannot move from another lane into an `active` lane; errors list blockers. A card already active remains there if it becomes blocked later and can be reordered within that lane.
- Moving to `done` sets progress to 100 in the same command. Moving out of `done` keeps progress unchanged.
- Moving Backlog to active with 0% progress is allowed. Normalize positions in affected lanes within the transaction.
- Progress is an integer 0–100. Reaching 100 does not move the card.
- A status note is optional but encouraged. Decreases are allowed and visible in the event.
- Archived boards are read-only until restored. Card creation and moves target only one of the three fixed work lanes. A legacy lane can be emptied by owner-authorized moves and archived through the CLI cleanup command; it cannot be restored.

### Review workflow

Review is a decision on a ticket, not a separate lane. The default board has Backlog, In Progress, and Done. An agent keeps its owned ticket In Progress and raises a linked review question when it wants a human decision. The question presents concrete choices such as Approve and Request changes, plus the automatic Other choice. The human's answer and explanation are recorded in the question event; the external orchestrator delivers them to the current ticket owner.

The agent interprets the response and makes the next CLI action. On approval it may move the ticket to Done. On Request changes it revises the same ticket using the feedback, and raises a new review question when ready. For Other it follows the explanation or asks for clarification. It creates a separate task only when the feedback is genuinely separate work. Workyard does not automatically move a ticket, create rework, or ask a second “ready yet?” question. No new review-specific command is required for this cooperative workflow.

Existing boards may still have legacy Review lanes. Those lanes remain readable so their cards can be moved out through owner-authorized commands, then the empty lanes can be archived. New cards and moves cannot target them. Existing questions do not have a review-specific marker. Answering any question clears that question's dependency, regardless of the selected option. A separately configured review approval gate on a linked code change does prevent a Done move until the current commit is approved. The external orchestrator still needs to deliver answered questions and state changes to ticket owners.

## 5. One event per action, including effects

The event row represents the **cause**: one command, one event. `before` and `after` describe the primary entity. `effects` lists other observable changes caused by the transaction, including derived blocked/ready transitions. Each effect contains an entity ID/key, effect type, before/after state, and cause reference. The event also carries the actor and timestamp.

Example: answering a question that frees two cards emits one `question_answered` event:

```json
{
  "id": 104,
  "type": "question_answered",
  "actor": "agent:planner",
  "entity_type": "question",
  "entity_id": "<question-uuid>",
  "before": { "status": "open" },
  "after": { "status": "answered", "answer": "Use SQLite." },
  "effects": [
    { "type": "card_became_ready", "card_key": "C-2", "before": { "blocked": true }, "after": { "blocked": false }, "cause_key": "Q-3" },
    { "type": "card_became_ready", "card_key": "C-5", "before": { "blocked": true }, "after": { "blocked": false }, "cause_key": "Q-3" },
    { "type": "question_resolved_for_card", "card_id": "<C-2-uuid>", "question_key": "Q-3" },
    { "type": "question_resolved_for_card", "card_id": "<C-5-uuid>", "question_key": "Q-3" }
  ]
}
```

The same effect calculation applies to moves out of/into `done`, dependency changes, question resolution, and lane archive moves. Only include a blocked/ready transition when the card's aggregate derived state actually changes; resolving one of several blockers does not mark it ready. A separate `question_resolved_for_card` effect identifies every linked task even when it stays blocked, so its session briefing retains the answer. `view_history(card)` includes events where the card is primary or appears in `effects`. One board creation event may cover the three default lanes. One `raise_question` event may cover several linked dependencies.

Each event's effects are computed from state before and after the command inside the same transaction. Consumers process event IDs in order and can always refresh authoritative board data rather than reconstructing it solely from JSON.

## 6. Live updates and orchestrator notifications

The Tauri backend exposes `poll_events(since_id, limit)`. The desktop wakes on the core's commit tick (and every two seconds regardless) instead of polling every 250 ms; the demo build, which has no backend, keeps the 250 ms poll. It requests `WHERE id > ? ORDER BY id ASC LIMIT ?`, processes **every returned event once**, advances its cursor only to the last event actually received, and immediately requests the next page while `has_more` is true. Polling does not use a separate `SELECT MAX(id)` gate. The UI refreshes affected boards after applying event cues; the DB remains authoritative. On reconnect or app resume, it catches up in pages.

For an owned card that becomes blocked, the event's effect identifies the card, owner, and cause. A **running external orchestrator** consumes events and sends the owner agent a message through its own messaging system. The owner then decides how to handle the blocker and updates its own card/status note. If no orchestrator is running, the UI still shows the blocked state immediately; the app does not claim a message was delivered. The orchestrator stores/maintains its own last processed event ID and must handle replay without duplicate user-visible messages.

An orchestrator attaches a resumable runtime session to a claimed In Progress task and pauses it when waiting. `next_ready_task` queries current task and session records instead of maintaining a separate queue: eligible paused tasks outrank new Backlog work. A question-specific pause becomes eligible when that question is answered or dropped, even if another blocker remains; a general pause becomes eligible when the task has no start blockers. The orchestrator reads all `task_session_briefing` pages using the returned `--after` cursors, atomically marks the session resumed to win dispatch, delivers the captured briefing, and acknowledges each cursor only after delivery. A failed delivery is paused again for retry. The external runtime supplies the message transport and actual process scheduling. Session writes create events but do not increment card versions.

The UI treats events as animation cues. A single event with multiple effects triggers the corresponding card pulses together. Initial load draws the current state without replaying historical animations. If more than about ten events accumulated, use one quick fade or restrained stagger rather than dozens of tweens.

## 7. CLI and agent plugin

Every CLI command accepts `--db`, `--actor` (or `WORKYARD_ACTOR`), and `--json`. JSON mode writes exactly one structured result to stdout, including typed errors; diagnostics go to stderr. Exit codes: 0 success, 1 usage, 2 validation/rule failure, 3 version/ownership conflict. Human-readable output is the default.

Commands are grouped by who calls them. Reads stay at the top level. Each mutating command on an existing entity takes `--expected-version`, with two exceptions. Notes never need one. `task progress`, `task move`, `task update`, and `task release` may omit it when the caller owns the card, since the claim already serializes the owner's writes; any other caller gets `version_required` (or `ownership_conflict` if someone else owns the card). Force-release always needs it. Every response carries the entity's new `version`.

```text
# reads (anyone)
workyard boards | board <B-1> [--phase P] [--lane X] [--blocked] | card <C-1>
workyard projects | project <P> | questions [B-1] [--open] [--kind K]
workyard next <B-1> [--agent-type T] | history <B-1|C-1> | events --since N

# task: a worker agent on a card
workyard task claim|release <C-1>          workyard task move <C-1> <lane>
workyard task progress <C-1> 40 [--note]   workyard task note <C-1> "text"   workyard task report <C-1> [--file report.md | stdin]
workyard task update <C-1> [--title|--desc|--agent-type]
workyard task add-card <B-1> "Title" [--phase P]
workyard task depend|undepend <C-1> --on <C-2|Q-3> [--scope start|finish]
workyard task ask <B-1> "Title" --kind K --option A --option B --recommend A --why "..." [--blocks C-1]
workyard task question-options <Q-3> ...
workyard task link-change <C-1> <repo> <branch> [--review-required]
workyard task update-change <change> <commit>
workyard task workspace <C-1> worktree|none      # also: task add-card --workspace none
workyard task require-review <change> --required true|false

# plan: project and board structure
workyard plan create-project|create-board|attach-board|add-repository|add-document|add-stakeholder
workyard plan assign-stakeholder|unassign-stakeholder <Q-3> ...
workyard plan phase list|add|describe|assign   (complete and advance are deprecated no-ops)
workyard plan cancel <C-1> --reason "..."

# orch: orchestrator and operator
workyard orch run <B-1> [--pool N] [--cwd REPO]  # foreground orchestrator; one per board
workyard orch status <B-1>                       # state, cursor, pool, lease
workyard orch pause|resume|drain <B-1>           # desired state; pause only stops new dispatches
workyard orch pool <B-1> <n>
workyard --actor orch:<B-1>#<epoch> orch backoff <B-1> [--until RFC3339] [--reason TEXT]   # stop dispatching until a time; omit --until to clear
workyard orch acquire <B-1> [--holder H] [--pid N] [--lease S] [--pool N]   # prints actor orch:<board>#<epoch>
workyard --actor orch:<B-1>#<epoch> orch heartbeat|release|cursor|dispatch <B-1> ...
workyard events --since N --follow               # one JSON line per event, wakes on commit
workyard orch session attach|pause|resume|ack|detach|briefing <C-1> ...
workyard orch session hold <C-1> [--by NAME] | release-hold <C-1>   # human takeover
workyard orch transfer <C-1> --to-actor A
workyard orch force-release <C-1> --reason "..."

# admin: humans and recovery
workyard admin archive-board|unarchive-board|archive-lane|scope-dependency|answer|drop ...
```

Review approval has no CLI command. A human approves the current commit of a linked change in the desktop, so an agent cannot approve its own work.

There is no MCP server. Agents run locally with a shell, so they call the CLI directly. The Claude Code plugin in `plugin/` provides a `workyard` skill describing the worker loop and a `SessionStart` hook that sets `WORKYARD_ACTOR` when the orchestrator has not. Actor strings must stay stable for a session, since ownership is checked against them. Do not expose raw SQL or arbitrary file access.

The Tauri backend calls `workyard-core` directly through thin `#[tauri::command]` wrappers with actor `ui`. It exposes reads and a small set of human writes: `answer_question`, `add_card_note`, stakeholder assignment, `approve_change` (the only way to approve a review gate), `set_orchestrator_state` (pause, resume, drain), `open_in_terminal` / `release_session_hold` (take a Claude session over in the default terminal), `open_planner` (a planner agent in the default terminal), and `start_orchestrator` / `stop_orchestrator` (a detached `orch run`, and a TERM to the pid in the lease). A background thread emits a `workyard-tick` event whenever the core touches `<db>.tick`, and every two seconds regardless. It opens its own connection and obeys the same command and ownership checks. Do not use a generic webview SQL plugin.

## 8. UI and animation

### Views

- Navigation: the project picker fills the window without a sidebar. Selecting a project opens a sidebar headed by its name and a back arrow, followed by Overview, then separate Stakeholders and Boards sections. Each stakeholder appears under Stakeholders with a pending question count; selecting one opens full answer cards for their assigned questions across the project's boards. The Boards section contains only the project's boards. Overview shows its one local repository. Unassigned boards stay accessible until attached to a project. Questions remain owned by their boards, but there is no separate board Questions page.
- Board navigation: clicking a board in the sidebar lands directly on its Phase graph, with no nested board links. Its phase titles show short descriptions and open a Kanban scoped to that phase. A back button at the top of the Kanban returns to the graph. Kanban lanes fill the available height and scroll independently; omit summary tiles and the search row.
- Task detail modal: a centered, scrollable dialog for the card description, owner and version, read-only progress and agent status note, blockers and downstream cards, append-only human notes, and event history. Match the question dialog's backdrop and rounded styling.
- Dependency view: a prerequisite-first directed graph, layered by dependency depth rather than kanban lane. Arrows point from a prerequisite to the work it enables. Mark unresolved and resolved links differently; highlight the longest chain by number of nodes and current chokepoints, measured by the number of distinct downstream cards held by unresolved links. Outline an open question in red when at least one unfinished task it blocks has unresolved question dependencies but no unresolved task dependencies. Evaluate this against the full board before phase filtering, so hidden work cannot cause a false highlight. These are dependency measures, not schedule estimates. Put the line-style legend beneath the scrollable graph; keep summary counts and controls above it. Selecting a node highlights its upstream and downstream chain; card links open details and open question nodes show an answer dialog with an optional stakeholder assignment control; answered questions are omitted. Completed phases are hidden by default and can be shown with a toggle. Show only nodes and links in the selected phase, with metrics scoped to that phase; the default graph shows current and future phases. Phase headings stay at the top of the scrolling graph and open phase Kanban views. Provide zoom and scrolling for larger graphs. Hide disconnected cards from the graph.
- No Gantt chart. The dependency graph communicates sequencing and chokepoints without inventing dates.
- Human actions in the desktop: optionally assign, reassign, or unassign a project stakeholder on a question, select an answer option, optionally explain it, append notes to cards, approve the current commit of a code change that needs review, and pause, resume, or drain a board's orchestrator. Board, lane, card, ownership, progress, dependency, question creation, and option editing controls are absent. Agents manage these through the CLI. Recovery uses an explicit CLI command.

### Card

Keep task cards dense: show a bold title, agent type, a compact progress row, and status note only when present. Show blocked state and question count inline. Show blocking impact as a small count when nonzero. Omit display keys from task and question cards; keep them in detail views and CLI output for reference. Keep owner identity, full description, dependencies, and notes in the task detail modal. Engineering and business question cards use identical shape, border, spacing, typography, and icon; only their colors differ (blue and amber). The tabs identify the kind, so question cards do not repeat it. Both show all selectable answer options and the agent's recommended choice with a simple reason when available. The mixed dependency graph labels each kind while preserving the same structure for both. A recommendation never selects an option for the human. Support light/dark modes and system fonts.

### Animation contract

Use Motion layout animation (`layout`/`layoutId`) and a board-level `LayoutGroup`. A move between lanes, including one made through the CLI, should tween from old to new position in about 350–450 ms. Siblings shift smoothly. An external move briefly highlights the card for about 1.2 s. Progress bar and number animate; note cross-fades. Blocked and ready badges animate, and newly ready cards pulse briefly. Respect `prefers-reduced-motion` with instant changes or simple fades. Filter changes animate card entrances/exits.

Card visual identity is keyed by stable UUID. Moving between lane parents may remount a React component even when `layoutId` creates a visual transition. Keep transient state that must survive (such as the selected detail and unsent note draft) above the lane-rendered component. Use shared layout transitions or a stable rendering layer as needed, and verify actual behavior in the prototype. For scrollable lanes, account for scroll position in layout measurement. Do not queue a separate animation for every event in a large catch-up batch.

## 9. Required checks

- Fresh DB starts with embedded migrations and default lanes on board creation.
- Every successful write produces one primary event with all effects; failures produce none. Event effects correctly report cards becoming blocked/ready only on real transitions.
- Rule tests cover progress bounds, blocked active move, cycle/self/cross-board dependency rejection, question resolution, archived targets, owner conflict, version conflict, and force release.
- A non-owner cannot change a claimed card's fields, lane, progress, or outgoing dependencies. The owner can; a stale owner version cannot.
- CLI integration tests use a temp DB and JSON mode. Plugin checks confirm the skill's commands exist in the grouped CLI.
- With the app open, an external CLI move appears within about half a second, animates between lanes, highlights, and preserves an open card detail draft. Test reduced motion separately.
- The desktop exposes no board, lane, or card management writes. A human can answer an open question and add a note to an agent-owned card; the note leaves the card version and owner unchanged.
- An agent using only the CLI and plugin can create a board/cards, claim a ready card, move it, update progress with notes, raise and answer a question, and release or transfer ownership.
- A sample orchestrator consumer or contract test replays an event where a claimed card becomes blocked and confirms it identifies the owner and cause once, with no edit to the owned card. Delivery through a specific orchestrator is tested when one is integrated.
- No adapter provides arbitrary SQL; blocked state is always derived.

## 10. References to verify at implementation time

- [SQLite foreign keys](https://www.sqlite.org/foreignkeys.html) and [PRAGMAs](https://www.sqlite.org/pragma.html): foreign keys and busy timeout are connection settings.
- [Motion layout animation](https://motion.dev/docs/react-layout-animations): shared layout transitions and grouped layout updates.
