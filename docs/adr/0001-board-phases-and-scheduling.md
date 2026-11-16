# ADR 0001: Board phases and work scheduling

Status: Implemented in core, CLI, MCP adapter, and desktop UI

## Context

A dependency graph orders work by prerequisites, not by when it was discovered. A new unblocked task found during later work can therefore appear beside the board's earliest tasks. A board already represents an epic, so adding milestones solely to organize this work would duplicate that concept.

The orchestrator should normally finish the current phase before taking work from the next one. It may use otherwise idle capacity on independent work in the immediately following phase.

## Decision

- Each board has an ordered list of phases and exactly one current phase. Earlier phases are closed; later phases are planned. Cards and questions each belong to exactly one phase of their board. Phase membership is independent of dependency edges. Phases are not milestones, lanes, or implicit dependencies.
- A new board starts with one current phase. Migrate existing cards and questions into one current phase per existing board, preserving their lanes, IDs, and event history.
- New cards and questions default to the current phase. A command may explicitly choose the current phase or a future one. Work discovered from an older card is still created in an open phase; its origin can be explained in the new item's description or notes. Nothing may be created in or reassigned into a closed phase.
- A normal `next_ready_task` call returns a claimable, start-unblocked Backlog card from the current phase. When **no claimable, start-unblocked Backlog card remains anywhere in the current phase**, it may return one from the **immediately next phase** if that card has no unresolved start dependency. Apply the existing impact and position ranking within the chosen phase. Agent type filtering selects among eligible cards but must not make a phase appear exhausted merely because one agent type has no match.
- Current-phase cards already claimed or In Progress do not count as **new Backlog work**. A later task-session extension makes eligible paused In Progress cards resumable work selected before new Backlog work; the query does not store a separate queue. Thus the orchestrator may start independent next-phase work while current-phase work or questions are still underway. If new current-phase work appears, subsequent new selections prefer it. An already owned next-phase card may continue; no automatic preemption or ownership transfer occurs.
- The core enforces the same eligibility when a card is claimed or moved into In Progress, including direct CLI/MCP calls. A future-phase card may be planned and edited but cannot be started except through the immediately-next-phase allowance. A card cannot move to Done from a planned phase unless it has first been started under this rule; this prevents bypassing scheduling by a direct Done move.
- For every card or question dependency, the prerequisite's phase must be the same as or earlier than the dependent card's phase. This applies to both start and finish scopes, including question blockers. Reject a dependency on future-phase work. Revalidate all adjacent dependencies when an item changes phase. Cross-board dependencies remain unsupported.
- `phase advance` moves the current pointer forward by exactly one phase and completes the departing phase in one transaction. `phase complete` closes the current phase without moving the pointer, so the final phase can be archived too. Both commands require all cards in the current phase to be Done or cancelled, all its questions to be answered or dropped, and no cancelled prerequisite in that phase to block a live card. An empty current phase may complete. No phase can be skipped or reopened; a later phase can be added and advanced into after final-phase completion.
- Once closed, a phase's cards, questions, memberships, dependencies, and review requirements are structurally immutable. Historical reads and human notes remain available. Later remediation is new work in the current or a future phase, with a provenance link to the old item where useful.
- Phase creation appends to the sequence. Reordering and deletion are deferred to keep the command surface small; no command may change the current or closed sequence.

## CLI and concurrency

Keep the command surface small: `phase add <board> <name> [--description ...]`, `phase describe <board> <phase> <description> --expected-version N`, `phase complete <board>`, `phase advance <board>`, and `phase assign <card-or-question> <phase>`. Completion is explicit, including for the final phase. Advancing also completes the departing phase. Existing creation commands gain optional `--phase`; `board` and the dependency view gain optional `--phase` filters. Existing `next`, `claim`, and `move` commands gain phase checks without new variants. Phase names are unique within a board; stable phase IDs remain the stored references.

All creation, assignment, claim, move, dependency, and advance checks run inside their write transactions. If work is added before advancement, advancement sees it and may fail. If advancement commits first, creation into the now-closed phase fails. Each successful command records one event with derived changes as effects; failed commands record none. An advance event identifies the old and new phase so the UI and orchestrator can refresh.

## Consequences

The current phase expresses the board's intended sequence without fabricating card dependencies. The next-phase allowance improves utilization, but it is intentionally limited to one phase ahead and only when the current phase has no claimable ready Backlog work. Scheduling eligibility is distinct from a card's dependency-derived blocked state.
