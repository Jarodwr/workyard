# Follow-ups

**Status (2026-11-16):** items 1 to 14 and 15 (except signing and the release dmg) are built and tested; 16 is still open. The second todo-app run on the new setup is the real test. Item 15 still needs the desktop to use the bundled CLI and plugin paths (`docs/PACKAGING.md`).

Changes found or requested during the first real sample run (a todo app built by Claude workers through
`workyard orch run`). Nothing here is built unless marked **done**. Do not start these while the sample run
is still in progress unless asked.

## Coordination and merging

1. **Workyard-managed worktrees instead of `claude --worktree`.** Claude Code's worktree isolation blocks
   `git -C <main checkout>`, so a worker cannot merge into `main` itself. Create the worktree with
   `git worktree add` and start the worker in it with no path restriction. The worker prompt already says: merge
   `main` into the branch, resolve conflicts, run the tests, `git -C <main> merge --ff-only <branch>`, and retry
   if `main` moved. Untested without `--worktree`; a dirty main checkout makes the fast-forward fail, and the worker
   should then stop and ask.
2. **Done gate checks the commit is on the target branch** with git's ancestry check, not the agent's word.
3. **Resume when the condition is met, not when a question is answered.** Each pause records what it waits for
   (an answer, an approval, "my branch is on the target branch"), and the worker resumes only when all of it is
   true. Reviews already work this way (`waiting_review` needs `can_finish`); generalise it.
4. **Fast-failure guard: done in code** (`engine.rs`). Only a worker that dies with a failure status within the
   window counts toward the board-wide pause; a clean exit without finishing counts against that card only.
   The running orchestrator needs a restart to pick it up.
5. **Do not raise a question for a mechanical blocker.** Q-2 and Q-3 were blockers dressed as decisions. Tell
   workers to fix or report them, and ask only for real choices.
6. **Merge trains.** On repositories with a train, the worker follows that repository's instructions; Workyard
   keeps no queue of its own (git's fast-forward refusal serialises integrations).

## Permissions and safety

7. **Dangerous-auto workers.** Run workers with permission checks off. A visible setting, on by default for local
   projects (a worktree isolates git state, not the machine).
8. **Unblock a stalled worker** for the non-dangerous mode: surface "requires approval" tool results from the
   stream, notify, offer Allow once / Allow for this run / Deny. Claude Code has `--permission-prompts host`
   (wire format not yet verified, test first). Add `Bash(npm:*)` to the default allowlist.
9. **Write/Edit, not Bash heredocs.** Added to the worker prompt (`engine.rs`); confirm it is in
   `plugin/skills/workyard/SKILL.md` too.

## Review

10. **Review off by default for local projects**, opt-in per project or board. The prompt should mention
    `--review-required` only when review is on. Humans approve in the desktop; never approve for them.

## Visibility

11. **Live phase graph** that updates as things change, and an **animation on running cards** (a card whose
    session exists and is not paused).
12. **Agents window:** a collapsible right-hand dock on every board view, toggled from the toolbar with a
    running-count badge. One row per agent: card, state, elapsed, last action, pending prompt. Expands to a log
    tail with note, open-in-terminal and stop. The same live view in card detail. The orchestrator must publish
    per-worker activity somewhere the desktop can read cheaply.
13. **Progress reporting.** Workers rarely call `task progress`, so cards sit at 0%. Strengthen the prompt and/or
    derive activity from the stream-json the runner already parses (last tool use, time since last event).

## Orchestrator agent

14. **A long-running orchestrator agent beside the Rust script.** The script stays the reliable layer (lease,
    pool, limits, backoff, starting and stopping processes). The agent is woken with events ("C-4 exited without
    finishing", "integration conflict on C-6") and makes judgement calls through the `workyard` CLI: notes to
    workers, resumes, holds, answering engineering questions, escalating business questions to a human, noticing
    quiet cards. Open questions: does the agent hold the lease or act as a delegate of the script; how much may it
    do without asking (suggest notes, resumes and retries first; merges, approvals and business answers stay human).

## Delivery

15. **Packaging.** Bundle the CLI as a sidecar, deliver the Claude plugin (`--plugin-dir`, marketplace), icons,
    dmg, signing if shared.
16. **Verify the desktop visually.** The Chrome extension was not connected, so the UI changes were never checked
    in a browser.

## To finish the sample run first

- Restart the orchestrator (picks up item 4), let C-4 finish (main already has its branch, Q-3 answered).
- C-6, C-7, C-8 still to run. The user approves commits in the desktop for this run only.
