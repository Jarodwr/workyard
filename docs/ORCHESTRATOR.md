# Orchestrator event contract

Workyard records state changes but does not send messages to agents itself. A running orchestrator can use `workyard --json events --since <cursor>` or MCP `poll_events` to watch the ordered event stream. Store a separate cursor for each Workyard database.

## Task sessions and work selection

After an agent claims a task and moves it to In Progress, attach its runtime session with `session attach`. When the agent waits, use `session pause`, optionally naming the question it is waiting for. This releases an orchestrator worker slot; the task stays In Progress and claimed. The attached session and delivered-event cursor are persisted against the task without changing its content version.

Call `next` when a worker slot opens. Its `dispatch_action` is `resume` for an eligible paused In Progress task or `claim` for a new Backlog task. This is a live query, not a stored queue. Paused tasks take precedence; within each group, current-phase work precedes immediately-next-phase work. A general pause becomes eligible when start blockers clear. A pause for a named question becomes eligible when that question is answered or dropped, even if another blocker remains.

For `resume`, read every `session briefing` page first to capture the current task snapshot and relevant events since `last_delivered_event_id`. Use each returned cursor as `--after` while `has_more` is true; reading pages does not acknowledge them. Then call `session resume` as the atomic dispatch claim. Only the worker whose resume succeeds sends the captured briefing to the runtime. Acknowledge each page cursor only after the runtime accepts that page. If delivery fails, pause the session again for retry; keep unacknowledged pages. A replayed page must be safe to deliver again. The orchestrator owns the actual runtime transport and worker pool; Workyard does not execute or message agents.

## When owned work becomes blocked

Process each returned event in ID order. Inspect its `effects` array. For every effect with `type: "card_became_blocked"` and a non-null `owner_actor`, send that owner a message containing `card_key`, `cause_key`, and the board or card link available in the orchestrator. The message should ask the owner to review the blocker and update its own ticket or status note as appropriate. The orchestrator must not edit the owned card on the owner's behalf.

The effect is emitted only when the card's aggregate derived blocked state changes from false to true. Adding a second blocker to an already blocked card does not create a second blocked notification. The event also contains the primary action and actor that caused the change.

## When a linked question is answered

On each `question_answered` event, read the selected option and explanation from `after`. Its `question_resolved_for_card` effects identify every linked task, including tasks that remain blocked by something else. Check each task's current owner and session before delivery. A paused session receives the answer in its next briefing; a running owner may receive it immediately. Deduplicate by database identity, event ID, and card UUID. The orchestrator must not move or edit the owner's ticket.

For a review question, the owning agent decides whether the answer approves completion or requests changes. It moves an approved ticket to Done itself. If changes were requested, it revises the same ticket and raises another review question when ready. The current schema has no review-specific marker, so the agent/orchestrator must identify review questions by its workflow context. Message delivery is an integration task; the dashboard only records the event.

Example effect:

```json
{
  "type": "card_became_blocked",
  "card_id": "<card-uuid>",
  "card_key": "C-12",
  "owner_actor": "agent:builder",
  "before": { "blocked": false, "ready": true },
  "after": { "blocked": true, "ready": false },
  "cause_key": "Q-3"
}
```

## Cursor and delivery

- Poll with the last processed event ID; follow `has_more` immediately with the returned `cursor`. All events in a page must be inspected, even if none need a message.
- Use `(database identity, event ID, card UUID, effect type)` as a delivery deduplication key. A replay after a restart must not send the same user-visible message twice.
- Persist the global event-consumer cursor and delivery deduplication record in the orchestrator's own storage. The per-task session briefing cursor lives in Workyard's `task_sessions` table and advances only through `session ack` after successful delivery.
- If no orchestrator is running, the board still shows the card as blocked. Do not show a “message sent” state without a delivery acknowledgement from an integrated orchestrator.
- If the owner has released or transferred the card by the time the message is delivered, check the current card before sending and avoid notifying a stale owner.

`owner_actor` is a cooperative local identity string. An attached task session records its runtime and resume ID; the orchestrator still supplies the transport and handles tasks without an attached session.

## Orchestrator host (decided, not yet implemented)

- The orchestrator is a long-lived process started with `workyard orch run <board>` (also launched by the desktop). The `workyard-orchestrator` crate depends only on `workyard-core`'s public commands, never on `rusqlite`, so every action goes through the same validated, evented command layer as the CLI and UI. Its actor is `orch:<board>`.
- Runtimes are backends behind a `Runner` trait (`spawn`, `resume`, `send`, `is_alive`, `focus`): Claude Code first, then Codex, optionally a terminal-pane runner. Agents report work through board commands (progress, notes, questions, moves), which stay durable events. The runner's output stream and exit status supply only ephemeral liveness signals.
- At most one orchestrator runs per board. `orchestrator_sessions` holds one row per board: desired state (`running`, `paused`, `draining`), the event cursor, worker pool size, and the lease (holder, pid, `epoch`, `expires_at`). `orch_acquire` takes the lease in an immediate transaction and fails with `orchestrator_conflict` while a live lease exists. A takeover after expiry increments `epoch`.
- Every orchestrator-initiated write carries its epoch and is rejected if it is stale, so a suspended or zombie orchestrator cannot act after replacement. Lease expiry alone is not the guarantee.
- Dispatch is one command, `orch_dispatch(board, epoch)`: select via `next`, claim, move to In Progress, and attach the session in one transaction.
- `orch pause`, `orch resume`, and `orch drain` change the desired state. A paused board dispatches nothing; running workers continue unless draining. Planning edits made while paused are picked up on resume because `next` is a live query.
- On startup the orchestrator reconciles attached sessions with `is_alive`, re-adopting live ones and detaching dead ones.
