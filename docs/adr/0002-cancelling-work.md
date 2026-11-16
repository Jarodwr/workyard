# ADR 0002: Cancel work explicitly

Status: Implemented in core, CLI, MCP adapter, and desktop UI

## Context

An obsolete card must not hold a phase open forever. Treating cancellation as Done would misrepresent the result and could incorrectly release work that depended on its output.

## Decision

- Add `cancel_card` as a named core, CLI, and MCP command. It requires a nonblank reason and the card's expected version. The owner alone may cancel a claimed card; an unclaimed card may be cancelled by an authorized agent. Human UI access remains read-only for this operation.
- Cancellation is a terminal marker on the card, not a fourth lane or a delivery state. Preserve the card's last lane, progress, and history. Record the reason, actor, and time; exclude cancelled cards from `next`, claiming, active board counts, and phase work queues. Show them in history and as muted nodes when graph context requires them.
- A cancelled card **does not satisfy** a card dependency, whether it blocks start or finish. Dependents show a distinct “prerequisite cancelled” problem. The agent must remove or redirect the dependency, or cancel/replan the dependent card. Do not silently delete edges or mark dependent work ready.
- `phase advance` rejects a cancelled card that remains an unresolved prerequisite for a live card in the current or a future phase. This makes the replan explicit before the phase closes.
- Cancelling a Done card is rejected. A later defect in completed work becomes a new card in an open phase. A cancelled card is not implicitly reopened; create replacement work and link its provenance to the cancelled item when appropriate.
- Cancellation and its effects are one transaction and one event. The event includes the reason and affected dependents. A failed cancellation emits no event. Existing ownership and optimistic version checks apply.

## Consequences

The orchestrator can finish a phase containing abandoned work without claiming it was delivered. A cancelled prerequisite remains visible until the plan is repaired, so downstream work cannot quietly proceed on a missing result.
