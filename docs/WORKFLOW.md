# Operating Workyard

How a person runs work through Workyard: prepare a board with a planner agent, start the orchestrator, then supervise from the desktop.

## One-time setup

1. Build and put `workyard` on your PATH: `cargo build --release -p workyard-cli` and link `target/release/workyard`.
2. Install the Claude Code plugin from `plugin/` so agents get the `workyard` and `workyard-planner` skills.
3. Run the desktop: `mise run dev` (or `WORKYARD_DB=... mise run dev` for another database).
4. Make sure `ANTHROPIC_API_KEY` is **not** set if you want workers to use your Claude subscription; a key in the environment overrides the login.

## 1. Plan (once per piece of work)

Planning happens before any worker starts and is done by a separate agent, so it can be slow and careful. Start a Claude Code session in the repository and give it a prompt like:

> Use the workyard-planner skill. Goal: *<what you want built>*. The repository is *<path>*. Set up a project, a board with phases, small cards with clear done-criteria and dependencies, mark investigations with `--workspace none`, and raise a question for anything you cannot decide. Do not start any card. Tell me which board to run.

Then open the desktop and review what it made: the phase graph (order, chokepoints, blocked work) and each card. Fix it by asking the planner to change things, or answer the questions it raised. Planning edits are safe while an orchestrator runs: a paused board starts nothing, and a running one picks up changes on its next pass.

## 2. Run

```sh
workyard orch run B-1 --cwd /path/to/repo --pool 2
```

- It stays in the foreground; Ctrl-C stops the workers, pauses their sessions, and releases the board.
- Each worker runs `claude -p` in its own git worktree (`.claude/worktrees/<card>`) on its own branch. Investigations (`--workspace none`) run in the repository without a worktree.
- Only one orchestrator can hold a board; a second `orch run` is refused and names the holder.
- macOS notifications tell you when a question needs an answer, a commit needs review, a card needs a human, or the board is waiting on a usage limit. Add `--no-notify` to silence them.

## 3. Supervise

| You want to | Do this |
|---|---|
| Answer a question | Open it in the phase graph or the stakeholder view and pick an option. The worker resumes with your answer. |
| Approve code | Card → Code changes → **Approve this commit**. A card with a review gate cannot reach Done without it. |
| Tell a worker something | Add a note on its card. It reaches the running worker within moments (at its next tool step), or its next briefing if it is paused. |
| Pause new work | **Pause** in the board's orchestrator bar (or `workyard orch pause B-1`). Running workers continue; **Drain** also stops starting anything while they finish. **Resume** to carry on. |
| Edit the plan safely | Pause, edit, resume. |
| Take over a worker | Card → Agent session → **Open in terminal**. It stops the orchestrator's worker, holds the session, and opens `claude --resume` in your terminal. When you are done, exit Claude and press **Release hold**; the orchestrator resumes it with everything it missed. |
| Take work away from a worker | Force-release the card (`workyard orch force-release C-3 --reason "..." --expected-version N`), which stops its worker; then replan or cancel it. Only a card's owner can cancel it. |
| See what a worker is doing | `<db>.logs/<card>-<session>.log` holds its stream-json output. |

## What it does by itself

- **Retries.** A worker that exits unfinished is resumed; after three failures its session is detached with a note and you are notified.
- **Usage limits.** When Claude reports a limit, the board stops starting and resuming work until it resets, shows "Waiting until …" in the orchestrator bar, and carries on afterwards. Interrupted cards resume rather than restart.
- **Instant failures.** Three workers in a row dying within seconds (bad auth, permissions, a flag) pause dispatch for ten minutes instead of failing the whole backlog. Check the logs.
- **Stale claims.** A claim whose owner has been silent for 15 minutes is flagged *Stale* on the card, unless its session is paused or an orchestrator is watching it.

## Checklist when something looks wrong

1. Orchestrator bar: is one connected? Is the board paused, draining, or waiting?
2. Card: is it blocked, stale, or held? Is there an unapproved commit?
3. The worker's log in `<db>.logs/`.
4. `workyard orch status B-1` and `workyard events --since N` for the raw record.
