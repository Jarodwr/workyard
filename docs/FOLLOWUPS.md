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
3. **Resume when the condition is met, not when a question is answered.** **Done.** `--for-integration` is a wait kind
   beside a question and `--for-review`; a session resumes only when every condition recorded on its pause holds. A pause
   that waits only for a question still resumes as soon as that question is answered, by design.
4. **Fast-failure guard counts only failure exits.** **Done.**
5. **No questions for mechanical blockers.** **Partial.** In the worker prompt and plugin skill only (nothing enforces
   it); the same text now also asks workers to ask, with a recommendation, whenever a card leaves a real choice open.
   The second run raised none about mechanics.
6. **Merge trains.** **Partial.** The prompt tells workers to follow the repository's documented train; no train was
   tested.
7. **Recovery of detached cards.** **Done**: reclaimed up to 2 times per card per run, then a human is notified.
   Proven by accident in the second run.

## Permissions and safety

8. **Dangerous-auto workers.** **Done** (`orch run --permissions full|safe`, default `full`; shown in the toolbar).
9. **Unblock a stalled worker in safe mode.** **Done.** In `--permissions safe` workers run with
   `--permission-prompts host --permission-prompt-tool stdio`, so Claude Code asks the orchestrator before a tool
   that needs approval and waits (verified in the C-6 investigation: `--permission-prompts host` alone is not enough).
   The orchestrator stores each ask as a `permission_requests` row and a `permission_requested` event, notifies, and
   the agents dock shows **Allow once / Allow for this run / Deny** on the card. The decision is a
   `permission_decided` event; the orchestrator answers the waiting worker on its stdin, so there is no restart.
   Allow for this run grants a rule (a Bash command's first word, `Bash(touch *)`; any other tool by name) with
   destination `session`: it lives in that worker process and is never written to a settings file. Grants are kept in
   the database and passed as `--allowedTools` if the worker is respawned or resumed. Requests whose worker is gone
   (exit, stop, orchestrator restart) become `expired`; the supervisor's own asks are denied at once. CLI:
   `workyard orch perms list <board> [--status pending]` and `orch perms decide <id> allow-once|allow-run|deny
   [--rule R] [--reason T]`; the supervisor and workers are refused. User and project allow rules in Claude settings
   still bypass the host entirely. Not verified: rule shapes for non-Bash tools, and what Claude does if the host
   never answers (it blocks).
10. **Write/Edit, not Bash heredocs.** **Done** (worker prompt and plugin skill).

## Review

11. **Review off by default**, opt-in per board (`plan board-review`, settings popover). **Done.**

## Visibility

12. **Live phase graph and running animation.** **Done**: the graph and the kanban cards both animate running work.
13. **Agents dock.** **Done**, with a gap in checking: seen open with no agents; not yet seen with live agents, the log
    tail, or the settings popover.
14. **Progress reporting.** **Partial.** Each card shows its last action live and the graph shows a progress bar. A
    running card still at 0% shows an estimated bar, but the real percentage is only what the agent reports (or 100 on
    Done). Left: checkpoint progress at set points in the prompt, or a better estimate from the stream.
15. **Wire routing in the graph.** **Done** (corridors, lanes, rounded corners, one arrowhead per merge, zoom steps
    50-150%, corridor tracks capped). Not checked on a very large board.

## Orchestrator agent

16. **Supervisor agent beside the script.** **Done, opt-in** (`orch run --supervisor`). The script keeps the lease and
    the processes; the supervisor unsticks the workflow with its shell, git and the CLI (see the rules below). Its
    limits are enforced by the core (`forbidden_for_supervisor`), not only by its prompt. The desktop has a Supervisor
    setting next to Start. Gap: never seen handling a real stuck worker.

    Rules for the supervisor:
    - It repairs and re-launches work (mechanics); it never decides what the work should be (judgement), and it is not a
      substitute for human intervention. When in doubt it escalates.
    - It investigates first (worker log, `git status`, `git log`, the card), fixes the cause, verifies the fix, then
      `orch reclaim`s the card or notes the live worker, and records `Supervisor did: ...` on the card.
    - Before any destructive step: a `backup/<card>-<timestamp>` branch at HEAD and a `git stash`. It never discards
      commits or uncommitted work, never force-pushes and never rewrites history on the target branch.
    - It touches the main checkout only to abort a stuck merge or rebase, to move aside a blocking change (with a
      backup) or to fast-forward a tested branch. It resolves only mechanical conflicts, then re-runs the tests.
    - Anything needing judgement gets a `NEEDS HUMAN:` card note (problem and options; one desktop notification) and no
      further action on that card.
    - The core refuses it: answering or dropping questions, approving, moving cards between lanes, cancelling,
      force-releasing, transferring, and changing a card's description, scope or dependencies. It may read, add notes,
      hold and release sessions, resume, and `orch reclaim`.

## Delivery

17. **Packaging.** **Partial.** Done: CLI sidecar, plugin marketplace manifest, bundled plugin, icon set from an SVG
    source, release `.app` and dmg built and checked (unsigned, ad-hoc signature), `--plugin-dir` autodetection (also
    `share/workyard/plugin` for Homebrew), the desktop starting the bundled CLI and exporting `WORKYARD_BIN` to
    terminals, `workyard --version` and `doctor`, one-place versioning (`scripts/set-version.sh`), `scripts/release.sh`
    (tarball and checksums) and Homebrew formula/cask templates (`docs/RELEASING.md`, `docs/HOMEBREW.md`). Open:
    signing and notarization (needs Apple credentials), publishing a release and tap (nothing has been tagged, pushed or
    installed into a real Homebrew prefix), a `LICENSE` file, an "Install CLI" button.
18. **Verify the desktop visually.** **Partial.** Graph, toolbar, running chips and the opened dock were seen in the
    real app on macOS. Not seen: dock with live agents, log tail, settings popover, graph at other zoom levels, the
    progress bar, the card detail tabs, the permission buttons.

## New, found in the second run

19. **Planner guidance.** **Done.** The planner skill tells planners to split cards by file or behaviour so they do
    not overlap. Not enforced.
20. **A rebuilt `workyard` binary mid-run breaks workers.** **Done.** Workers run from a stable copy in `<db>.bin`;
    `schema_too_new` names both versions and which side is older; a migration is refused under a live older
    orchestrator (`--allow-migrate` overrides); `orch restart <board>` moves a board to a new binary.

## Found while dogfooding the planner

21. **Idle workers while ready cards waited in later phases.** **Done.** The dispatcher offered only the current and the
    next phase, so ready cards in the third phase sat idle (C-12 and C-15). It now dispatches any ready card whatever
    its phase (dependencies order the work); the board setting `phase_window` (`plan board-phase-window <board>
    <n|all>`, default `all`) narrows that, and `orch status` shows `held_back_by_phase` and `held_back_cards`, so
    idle workers have a visible reason. Not shown in the desktop yet.

## What changed since the second run

- Workers: stable binary copy, main-checkout check before dispatch, worktree and branch removed once Done and merged
  (`--keep-worktrees`), a worktree setup script, `--worker-env` and a board default (`plan board-worker-env`), a claim
  prompt carrying the repository id and the exact `link-change` line, a card's owner linking changes without a version.
- Merging: `plan board-integration merge|rebase`; `--for-integration` pauses; Done gate unchanged (`not_integrated`).
- Orchestrator: `orch restart`, `orch reclaim`, `orch perms`, safe-mode permission prompts answered from the desktop,
  `phase_window`, a pool of 3 workers by default, the supervisor limited by the core.
- CLI: `status`, `wait`, `doctor`, `--version`, `task report` (markdown report on a card, shown in a Report tab).
- Planning: new project from a repository and a one-click project planner; phase status derived from cards and
  questions; a cancelled prerequisite no longer blocks its dependents and stays visible as cancelled.
- Desktop: tabbed card detail with attention badges, markdown in descriptions and notes, remembered project, board,
  view, phase, zoom and dock, running animation on kanban cards, the Supervisor setting, Allow/Deny buttons.
- Packaging: see item 17.

