# Operating Workyard

How a person runs work through Workyard: prepare a board with a planner agent, start the orchestrator, then supervise from the desktop.

## One-time setup

1. Build and put `workyard` on your PATH: `cargo build --release -p workyard-cli` and link `target/release/workyard` (the desktop app bundles its own copy). Run `workyard doctor` to check the install.
2. Install the Claude Code plugin from `plugin/` so agents get the `workyard` and `workyard-planner` skills (`claude plugin marketplace add <repo>`, `claude plugin install workyard@workyard`; see PACKAGING.md). The orchestrator also loads it into workers itself.
3. Run the desktop: `mise run dev` (or `WORKYARD_DB=... mise run dev` for another database).
4. Make sure `ANTHROPIC_API_KEY` is **not** set if you want workers to use your Claude subscription; a key in the environment overrides the login.

## 1. Plan (once per piece of work)

Planning happens before any worker starts and is done by a separate agent, so it can be slow and careful. Start a Claude Code session in the repository and give it a prompt like:

> Use the workyard-planner skill. Goal: *<what you want built>*. The repository is *<path>*. Set up a project, a board with phases, small cards with clear done-criteria and dependencies, mark investigations with `--workspace none`, and raise a question for anything you cannot decide. Do not start any card. Tell me which board to run.

In the desktop, **New project** from a repository creates the project and a planner run does the rest in one click; the prompt above is the same thing by hand. Then open the desktop and review what it made: the phase graph (order, chokepoints, blocked work) and each card. Fix it by asking the planner to change things, or answer the questions it raised. Planning edits are safe while an orchestrator runs: a paused board starts nothing, and a running one picks up changes on its next pass.

## 2. Run

From the desktop, open the board and press **Start** in the toolbar. It starts `workyard orch run` detached in the project's repository, so it keeps running if you close the app; **Stop** (the square) ends it the same way Ctrl-C does. Output goes to `<db>.logs/orchestrator-<board>.log`, and if it exits straight away (another orchestrator holds the board, `claude` not found) the reason is shown. It runs through a login shell so it finds `claude` on your PATH; if you set PATH only in `.zshrc`, start it from a terminal instead:

```sh
workyard orch run B-1 --cwd /path/to/repo --pool 2
```

- From a terminal it stays in the foreground; Ctrl-C stops the workers, pauses their sessions, and releases the board.
- Each worker runs `claude -p` in its own git worktree (`<repo>/.workyard/worktrees/<card>`, branch `workyard/<card>`) and finishes by merging its own work into the main branch. A new worktree first runs `scripts/worktree-setup.sh` (see ORCHESTRATOR.md) so `app/node_modules` and the staged sidecar are present. It is not sandboxed to the worktree, so it can do that merge. Workers run with permission checks off by default (`--permissions full`); pass `--permissions safe` for edits plus an allowlist of commands. Investigations (`--workspace none`) run in the repository without a worktree.
- Only one orchestrator can hold a board; a second `orch run` is refused and names the holder. A board starts with a pool of 3 workers (`--pool`, or `workyard orch pool`).
- Useful flags: `--supervisor` (a supervisor agent that unsticks the workflow; the desktop has a Supervisor setting), `--worker-env KEY=VALUE` (also `plan board-worker-env` for a board default), `--keep-worktrees` (worktrees and branches are removed once a card is Done and merged), `--allow 'Bash(make:*)'` with `--permissions safe`.
- Ready cards are dispatched whatever their phase. `plan board-phase-window <board> <n|all>` limits that to `n` phases past the current one; `orch status` shows `held_back_by_phase` when the window leaves workers idle.
- After you rebuild `workyard`, `workyard orch restart B-1` drains the board and relaunches the orchestrator on the new binary; workers run from a stable copy in `<db>.bin`, so a rebuild does not break a run in progress.
- macOS notifications tell you when a question needs an answer, a commit needs review, a card needs a human, or the board is waiting on a usage limit. Add `--no-notify` to silence them.

## 3. Supervise

| You want to | Do this |
|---|---|
| Answer a question | Open it in the phase graph or the stakeholder view and pick an option. The worker resumes with your answer. |
| Approve code | Card → Code changes → **Approve this commit**. A card with a review gate cannot reach Done without it. Review is off by default; `workyard plan board-review <board> on` turns it on for new changes. |
| Choose merge or rebase | Board settings → **Integration**, or `workyard plan board-integration <board> merge\|rebase`. `merge` (default) has workers merge the target into their branch, leaving a merge commit per card; `rebase` has them rebase onto the target and fast-forward, so history stays linear. Either way the Done gate is the same: the branch tip must be on the target. |
| Tell a worker something | Add a note on its card. It reaches the running worker within moments (at its next tool step), or its next briefing if it is paused. |
| Pause new work | **Pause** in the board's orchestrator bar (or `workyard orch pause B-1`). Running workers continue; **Drain** also stops starting anything while they finish. **Resume** to carry on. |
| Edit the plan safely | Pause, edit, resume. |
| Take over a worker | Card → Agent session → **Open in terminal**. It stops the orchestrator's worker, holds the session, and opens `claude --resume` in your terminal. When you are done, exit Claude and press **Release hold**; the orchestrator resumes it with everything it missed. |
| Allow a tool in `--permissions safe` | A refused tool shows **Allow once / Allow for this run / Deny** on the card; or `workyard orch perms list B-1 --status pending` and `orch perms decide <id> allow-once\|allow-run\|deny`. |
| Read a long write-up | A card's **Report** tab (investigations write one with `task report`). |
| Take work away from a worker | Force-release the card (`workyard orch force-release C-3 --reason "..." --expected-version N`), which stops its worker; then replan or cancel it. Only a card's owner can cancel it. |
| See what a worker is doing | `<db>.logs/<card>-<session>.log` holds its stream-json output. |

## Questions and side discoveries

Workers are told to ask, not guess. When a card leaves a real choice open (scope, user-visible behaviour or naming, a trade-off, anything the card's author would want to weigh in on) the worker raises a question with 2-4 options, a recommendation and a one-line reason, then pauses until you answer. Expect many questions; answer them in the phase graph or the stakeholder view. Workers do not ask about mechanical blockers (a refused command, a merge conflict, a failing test); they fix those.

When a worker finds a bug, missing piece or follow-up outside its card, it adds a Backlog card (`task add-card`, with what, where and why), orders it with `task depend` if needed, and links it from its own card's note. It adds at most 3 per card and never starts them, so review the Backlog after a run.

## What it does by itself

- **Retries.** A worker that exits unfinished is resumed; after three failures its session is detached with a note and the card goes back to the backlog for a fresh worker (up to twice); after that you are notified. A card cannot reach Done until its branch is on the target branch.
- **Usage limits.** When Claude reports a limit, the board stops starting and resuming work until it resets, shows "Waiting until …" in the orchestrator bar, and carries on afterwards. Interrupted cards resume rather than restart.
- **Instant failures.** Three workers in a row dying within seconds (bad auth, permissions, a flag) pause dispatch for ten minutes instead of failing the whole backlog. Check the logs.
- **Stale claims.** A claim whose owner has been silent for 15 minutes is flagged *Stale* on the card, unless its session is paused or an orchestrator is watching it.

## Checklist when something looks wrong

1. Orchestrator bar: is one connected? Is the board paused, draining, or waiting?
2. Card: is it blocked, stale, or held? Is there an unapproved commit?
3. The worker's log in `<db>.logs/`.
4. `workyard status B-1` is the one-screen summary (lanes, progress, running or paused, open questions, orchestrator and backoff; `--json` for scripts), and `workyard wait B-1 --until attention|done|<n>-done [--timeout 30m]` blocks until something needs you or the work is done, printing why (exit 0; exit 4 on timeout). `workyard orch status B-1` and `workyard events --since N` for the raw record.
