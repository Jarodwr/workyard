# Follow-ups

Changes found or requested during the first real sample runs (a todo app built by Claude workers through
`workyard orch run`). Status as of 2026-11-17: **done** = built and tested; **partial** = built, with a stated gap;
**open** = not built.

## Coordination and merging

1. **Workyard-managed worktrees instead of `claude --worktree`.** **Done.** Worktrees live in
   `<repo>/.workyard/worktrees/<card>`; workers merge their own work into the target branch. Proven by the second
   todo run (all 8 cards merged by their workers). A dirty main checkout still makes the fast-forward fail; the worker
   is told to stop and ask.
2. **Done gate checks the commit is on the target branch.** **Done** (`not_integrated`).
3. **Resume when the condition is met, not when a question is answered.** **Partial.** `--for-integration` is a new wait
   kind that resumes only when the branch is on the target; review already worked this way. A plain question wait still
   resumes as soon as the question is answered, whatever else is unresolved.
4. **Fast-failure guard counts only failure exits.** **Done.**
5. **No questions for mechanical blockers.** **Partial.** In the worker prompt only (nothing enforces it). The second
   run raised none.
6. **Merge trains.** **Partial.** The prompt tells workers to follow the repository's documented train; no train was
   tested.
7. **Recovery of detached cards.** **Done**: reclaimed up to 2 times per card per run, then a human is notified.
   Proven by accident in the second run.

## Permissions and safety

8. **Dangerous-auto workers.** **Done** (`orch run --permissions full|safe`, default `full`; shown in the toolbar).
9. **Unblock a stalled worker in safe mode.** **Partial.** Refused tools are detected from the stream, notified once
   and shown as `blocked: ...` on the card; `npm` and friends are on the safe allowlist. Not built: one-click Allow
   once / for this run / Deny. `--permission-prompts host` is still unverified.
10. **Write/Edit, not Bash heredocs.** **Done** (worker prompt and plugin skill).

## Review

11. **Review off by default**, opt-in per board (`plan board-review`, settings popover). **Done.**

## Visibility

12. **Live phase graph and running animation.** **Done** for the graph. Kanban cards have no running animation.
13. **Agents dock.** **Done**, with a gap in checking: seen open with no agents; not yet seen with live agents, the log
    tail, or the settings popover.
14. **Progress reporting.** **Partial.** Each card now shows its last action live and the graph shows a progress bar,
    but the percentage is still only what the agent reports (or 100 on Done), so bars sit at 0% while a card runs.
    Idea: estimate from the stream, or have the prompt checkpoint progress at set points.
15. **Wire routing in the graph.** **Done** (corridors, lanes, rounded corners, arrowheads mid-line). Where several
    wires merge into one card their arrowheads overlap; not checked on a large board or at other zoom levels.

## Orchestrator agent

16. **Supervisor agent beside the script.** **Done, opt-in** (`orch run --supervisor`). The script keeps the lease and
    the processes; the supervisor is a delegate that acts through the CLI (notes, engineering answers with a
    recommended option). It behaved well when the CLI was broken. Gaps: no toggle in the desktop; its limits are
    enforced by its prompt, not by the core; never seen handling a real stuck worker.

## Delivery

17. **Packaging.** **Partial.** Done: CLI sidecar, plugin marketplace manifest, bundled plugin, icons config, debug
    `.app` build, `--plugin-dir` autodetection. Open: release build and dmg, signing and notarization (needs Apple
    credentials), a real 1024px icon, `WORKYARD_BIN` exported to terminals opened from the desktop.
18. **Verify the desktop visually.** **Partial.** Graph, toolbar, running chips and the opened dock were seen in the
    real app on macOS. Not seen: dock with live agents, log tail, settings popover, graph at other zoom levels, the new
    progress bar.

## New, found in the second run

19. **Planner guidance.** Cards that overlap in scope (C-4 built what C-6 was meant to). Tell planners to split cards
    by file or behaviour so they do not overlap.
20. **A rebuilt `workyard` binary mid-run breaks workers.** Workers use the binary in the orchestrator's directory; a
    rebuild during the run made every command fail with `schema_too_new` for a while. Copy the binary at start, or
    point workers at a stable copy, and make the error say which side is older.
