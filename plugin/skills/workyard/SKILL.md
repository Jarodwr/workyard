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
5. Code changes: `task link-change C-12 <repository> <branch> --review-required`. After each new commit run `task update-change <change> <commit>`, which clears earlier approval. A human approves in the desktop app; you cannot approve your own change.
6. Finish: `task move C-12 Done` only when the work is integrated and checks pass. Otherwise `task release C-12`.

## Rules

- Add a note for a human with `task note C-12 "text"`. Notes never need a version.
- If a command fails with `version_conflict`, reread the card and retry. On `ownership_conflict`, stop; someone else owns it.
- New work you discover: `task add-card <board> "Title"`; order it with `task depend`.
- Only the planner role uses `plan`; only the orchestrator uses `orch`; `admin` is for humans.
- Reads: `boards`, `board <B-1>`, `card <C-1>`, `questions`, `history`.
