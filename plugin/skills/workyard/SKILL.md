---
name: workyard
description: Use when assigned a Workyard card (a key like C-12) or asked to work from a Workyard board. Covers claiming, progress, questions, code-change links, and finishing.
---

# Working a Workyard card

Workyard is a local kanban shared with humans. Use the `workyard` CLI with `--json`. Your actor is `$WORKYARD_ACTOR`; do not change it, because card ownership is checked against it. Never edit the database directly.

## Loop

1. Read the card: `workyard --json card C-12`. Note its `version`, blockers, notes, and phase.
2. Claim it if unclaimed: `workyard --json task claim C-12 --expected-version N`, then `task move C-12 "In Progress"`.
3. Work. After each meaningful step report it: `workyard --json task progress C-12 40 --note "short status"`. Each write returns the card's new `version`; reuse it.
4. Asking is normal. When the card leaves a real choice open (scope, user-visible behaviour or naming, a trade-off, anything its author would want to weigh in on), do not guess: ask with a recommendation. Never ask about a mechanical blocker (a refused command, a resolvable merge conflict, a fixable failing test); fix it. Ask: `workyard --json task ask <board> "Title" --kind engineering|business --option "A" --option "B" --recommend "A" --why "simple reason" --blocks C-12`. Give 2-10 real choices; `Other` is added for you. Then pause your session with `workyard orch session pause C-12 --waiting-question Q-3` and stop. The orchestrator resumes you when the question is answered, and the answer arrives in your briefing.

   Worked example: title `Should archived boards still show in the board picker?`; options `Hide them`, `Show them dimmed`, `Behind a toggle`; `--recommend "Behind a toggle"`; `--why "keeps the picker short without making archived work unreachable, and the toggle is cheap"`. A good question has 2-4 real options, one recommendation and a one-line reason.
5. Code changes: you are in your own git worktree on your own branch (`git branch --show-current`); commit there and never touch another checkout. Record the branch with `task link-change C-12 <repository-id> <your-branch>` (the claim prompt gives the exact line with the real repository id; only look it up with `projects`/`project <id>` if it is missing), and after each new commit run `task update-change <change> <commit>`. Done means integrated, and `task move Done` is refused (`not_integrated`) until your branch is on the target branch. Loop: in your worktree run `git merge main` (resolve conflicts yourself), run the project's tests, then from the main checkout (first entry of `git worktree list`) run `git -C <path> merge --ff-only <your-branch>`; if main moved, merge it in again, re-test and retry. If the repository documents a merge queue or train (CONTRIBUTING, CLAUDE.md, docs), follow that instead. If you cannot integrate yourself, pause with `workyard orch session pause C-12 --for-integration` and stop; you are resumed once the branch is on the target branch. Review is off unless the board turns it on. If a human review gate shows on the card, pause first with `workyard orch session pause C-12 --for-review` and stop; you are resumed once it is approved, then integrate and finish. You cannot approve your own change. Investigative cards (no worktree) change no repository: report with `task note` instead; a long write-up (findings, options, evidence) goes in a markdown report, `task report C-12 --file report.md` (or stdin), which replaces the previous one, shows as the card's Report tab and may be 256 KB. Keep notes for short messages.
6. Finish: `task move C-12 Done` only when the work is integrated into main and checks pass. Otherwise `task release C-12`.

## Rules

- Create and change files with your Write and Edit tools, never with shell heredocs (`cat > file <<EOF`) or `echo >`: Claude Code's shell checks often reject them, and the file tools are reliable.
- Add a note for a human with `task note C-12 "text"`. Notes never need a version. Messages that arrive while you work are notes a human left on your card; treat them as instructions about it.
- If a command fails with `version_conflict`, reread the card and retry. On `ownership_conflict`, stop; someone else owns it.
- Side discoveries: when you find a bug, missing piece or follow-up outside your card, never fix it silently or ignore it. Add a Backlog card: `task add-card <board> "Title" --desc "what, where, why"`, order it with `task depend` if needed, and add one line to your own card's note linking it (`task note C-12 "found C-N: ..."`). At most 3 new cards per card, and never start them.
- Only the planner role uses `plan`; the only `orch` command you use is `orch session pause`; `admin` is for humans.
- Reads: `boards`, `board <B-1>`, `card <C-1>`, `questions`, `history`; `status <B-1>` is a one-screen summary, and `wait <B-1> --until attention|done|<n>-done [--timeout 30m]` blocks, prints why and exits 0 (4 on timeout).
