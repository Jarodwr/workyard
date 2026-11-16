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
4. If you need a human decision, do not guess. Ask: `workyard --json task ask <board> "Title" --kind engineering|business --option "A" --option "B" --recommend "A" --why "simple reason" --blocks C-12`. Give 2-10 real choices; `Other` is added for you. Then pause your session with `workyard orch session pause C-12 --waiting-question Q-3` and stop. The orchestrator resumes you when the question is answered, and the answer arrives in your briefing.
5. Code changes: you are in your own git worktree on your own branch (`git branch --show-current`); commit there and never touch another checkout. Record the branch with `task link-change C-12 <repository-id> <your-branch>`, and after each new commit run `task update-change <change> <commit>`. Done means integrated: in your worktree run `git merge main` (resolve conflicts) and the tests, then from the main checkout (first entry of `git worktree list`) run `git -C <path> merge --ff-only <your-branch>`; if main moved, merge it in again and retry. If a human review gate shows on the card, pause first with `workyard orch session pause C-12 --for-review` and stop; you are resumed once it is approved, then integrate and finish. You cannot approve your own change. Investigative cards (no worktree) change no repository: report with `task note` instead.
6. Finish: `task move C-12 Done` only when the work is integrated into main and checks pass. Otherwise `task release C-12`.

## Rules

- Add a note for a human with `task note C-12 "text"`. Notes never need a version. Messages that arrive while you work are notes a human left on your card; treat them as instructions about it.
- If a command fails with `version_conflict`, reread the card and retry. On `ownership_conflict`, stop; someone else owns it.
- New work you discover: `task add-card <board> "Title"`; order it with `task depend`.
- Only the planner role uses `plan`; the only `orch` command you use is `orch session pause`; `admin` is for humans.
- Reads: `boards`, `board <B-1>`, `card <C-1>`, `questions`, `history`.
