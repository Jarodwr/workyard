# ADR 0002: Cancel work explicitly

Status: Implemented in core, CLI, MCP adapter, and desktop UI

## Context

An obsolete card must not hold a phase open forever. Treating cancellation as Done would misrepresent the result and could incorrectly release work that depended on its output.

## Decision

- Add `cancel_card` as a named core, CLI, and MCP command. It requires a nonblank reason and the card's expected version. The owner alone may cancel a claimed card; an unclaimed card may be cancelled by an authorized agent. Human UI access remains read-only for this operation.
- Cancellation is a terminal marker on the card, not a fourth lane or a delivery state. Preserve the card's last lane, progress, and history. Record the reason, actor, and time; exclude cancelled cards from `next`, claiming, active board counts, and phase work queues. Show them in history and as muted nodes when graph context requires them.
- A cancelled card **does not block** its dependents, in start or finish scope: it will never finish, so waiting on it would stall them forever. Cancelling recomputes dependents in the same transaction and emits `card_became_ready` / `card_unblocked` effects alongside `prerequisite_cancelled`. The dependency is kept and shown as cancelled (`cancelled_prerequisites`, struck through and grey in detail and graph). It is not treated as delivered work. (Supersedes the original rule that it blocked until repaired; amended by C-28.)
- `phase advance` is not held up by a cancelled prerequisite; only unfinished live cards and open questions are.
- Cancelling a Done card is rejected. A later defect in completed work becomes a new card in an open phase. A cancelled card is not implicitly reopened; create replacement work and link its provenance to the cancelled item when appropriate.
- Cancellation and its effects are one transaction and one event. The event includes the reason and affected dependents. A failed cancellation emits no event. Existing ownership and optimistic version checks apply.

## Consequences

The orchestrator can finish a phase containing abandoned work without claiming it was delivered. A cancelled prerequisite stays visible in history, and its dependents are released with an explicit effect so owners can check whether the missing result matters.
