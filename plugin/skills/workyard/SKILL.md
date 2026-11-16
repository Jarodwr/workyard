---
name: workyard
description: Use when assigned a Workyard card (a key like C-12) or asked to work from a Workyard board. Covers claiming, progress, questions, code-change links, and finishing.
---

# Working a Workyard card

Workyard is a local kanban shared with humans. Use the `workyard` CLI with `--json`. Your actor is `$WORKYARD_ACTOR`; do not change it, because card ownership is checked against it. Never edit the database directly.

## How Workyard works (read this once)

- **People and agents share one board.** A project holds boards; a board has phases (delivery order), three lanes (Backlog, In Progress, Done), cards and questions. A human reviews in the desktop; they cannot see your terminal, only what you write to the board: progress, notes, questions, reports, linked changes. If it is not on the board, the human does not know it.
- **You are one worker on one card.** A script (the orchestrator) claims the card for you, runs you in your own git worktree, and may stop and resume you. You may be stopped at any point, so keep the card's progress and note honest as you go; a resume starts from what the board says plus a briefing of events since you left.
- **Pausing is how you wait.** You never sit and wait for a human. You record why you are waiting (`orch session pause`), then stop. The orchestrator frees the slot and resumes you when the wait condition holds: a question answered or dropped, a thread message to reply to, a review approved, your branch integrated.
- **A question is a decision that belongs to a human.** It has a kind (`engineering` for technical trade-offs, `business` for product, scope, naming and priorities), 2-10 options (`Other` is added automatically), your recommendation and a one-line reason. It blocks the cards named with `--blocks`. It stays open until a human answers or drops it. Only humans resolve questions; you and the supervisor never do.
- **A question can be a conversation.** The human may write in the question's thread instead of choosing. You are resumed for that turn only: answer in the thread, adjust the options if warranted, pause again. The same question stays open.
- **Cards are small and independent.** Do your own card only. Anything else you find becomes a Backlog card, never your own side work.

## When to ask a question (and when not to)

Asking is normal and wanted here. A wrong guess costs a human far more than a question does, and a human answers in seconds. Ask when ANY of these is true:

- The card or its docs allow two or more reasonable behaviours a user would notice (what is shown, what is hidden, defaults, limits, error wording, naming of a command, flag, file or concept).
- You are choosing between designs with different costs (a new dependency, a schema or file-format change, a public interface, a rewrite versus an extension) and the card does not settle it.
- The card is ambiguous, contradicts the repository's docs or ADRs, or its done-criteria cannot be met as written.
- You would be removing, renaming or changing behaviour that other work or users depend on.
- Something affects scope: it would roughly double the work, or leave out something the card seems to promise.
- Priority, policy or money is involved (the human or a stakeholder owns it: use `--kind business`).

Do NOT ask about: a refused tool or command, a merge conflict you can resolve, a failing test you can fix, which of several equivalent implementation details to pick inside your own files, or anything the card, an ADR or the repository's CLAUDE.md already decides. Fix or decide those and note the decision in `task note`.

**Ask early.** Ask before you build on a guess, not after. If you can finish unrelated parts of the card first, do that, then ask; do not do work that depends on the answer. When a question needs asking, raise it, pause, and stop: do not keep going on your recommended option as if it were answered.

**Write a good question.** The human reads it cold, on a phone-sized card, with no context from your session.
- Title: the decision as a question, specific ("Should archived boards still appear in the board picker?"), not a topic ("Board picker").
- `--body`: two or three sentences of what you found, where in the repo, and what is at stake. Include a file path or card key where it helps.
- Options: 2-4 real, distinct, complete choices, each one a short phrase a human can pick without reading your body. Not "Yes/No" unless it truly is. Do not include `Other`.
- `--recommend`: one of your options; always have a view. `--why`: one plain-language line (at most 280 characters) with the reason, not a restatement.
- `--blocks`: every card that cannot proceed correctly without the answer, including your own (`--blocks C-12`). Pick `--kind` honestly.
- One decision per question. Two decisions are two questions.

**After you pause.** You are resumed by one of: an answer (the selected option plus an optional explanation arrive in your briefing; if they chose `Other`, the explanation is the instruction), a drop (the human decided the question no longer matters: carry on with the card's own wording or your note), or a thread message (see Question threads). Read the answer literally, record in `task note` what you decided because of it, and continue.

## Loop

1. Read the card: `workyard --json card C-12`. Note its `version`, blockers, notes, and phase.
2. Claim it if unclaimed: `workyard --json task claim C-12 --expected-version N`, then `task move C-12 "In Progress"`.
3. Work. After each meaningful step report it: `workyard --json task progress C-12 40 --note "short status"`. Each write returns the card's new `version`; reuse it.
4. Asking is normal. When the card leaves a real choice open (scope, user-visible behaviour or naming, a trade-off, anything its author would want to weigh in on), do not guess: ask with a recommendation. Never ask about a mechanical blocker (a refused command, a resolvable merge conflict, a fixable failing test); fix it. Ask: `workyard --json task ask <board> "Title" --kind engineering|business --option "A" --option "B" --recommend "A" --why "simple reason" --blocks C-12`. Give 2-10 real choices; `Other` is added for you. Then pause your session with `workyard orch session pause C-12 --waiting-question Q-3` and stop. The orchestrator resumes you when the question is answered, and the answer arrives in your briefing.

   Question threads: a human may reply in the question's thread instead of answering. That resumes you for a conversation turn only, and the question is still open. Read it with `workyard --json question show Q-3`, reply with `workyard question discuss Q-3 "your reply"`, and revise the options or recommendation with `workyard task question-options Q-3 --expected-version N --option ... --recommend ... --why ...` if the discussion warrants it. Post your reply before you pause again, then pause with `--waiting-question Q-3` and stop. Never answer or drop the question yourself; only a human does.

   Worked example: title `Should archived boards still show in the board picker?`; options `Hide them`, `Show them dimmed`, `Behind a toggle`; `--recommend "Behind a toggle"`; `--why "keeps the picker short without making archived work unreachable, and the toggle is cheap"`. A good question has 2-4 real options, one recommendation and a one-line reason.
5. Code changes: you decide whether the card changes code. If you changed code, link your branch with `task link-change` (no `--expected-version` needed for the card's owner) and integrate it before `task move Done`; if you changed nothing, say so in the card note and finish. You are in your own git worktree on your own branch (`git branch --show-current`); commit there and never touch another checkout. Record the branch with `task link-change C-12 <repository-id> <your-branch>` (the claim prompt gives the exact line with the real repository id; only look it up with `projects`/`project <id>` if it is missing), and after each new commit run `task update-change <change> <commit>`. Done means integrated, and `task move Done` is refused (`not_integrated`) until your branch is on the target branch. Loop: in your worktree run `git merge main` (resolve conflicts yourself), run the project's tests, then from the main checkout (first entry of `git worktree list`) run `git -C <path> merge --ff-only <your-branch>`; if main moved, merge it in again, re-test and retry. If the repository documents a merge queue or train (CONTRIBUTING, CLAUDE.md, docs), follow that instead. If you cannot integrate yourself, pause with `workyard orch session pause C-12 --for-integration` and stop; you are resumed once the branch is on the target branch. Review is off unless the board turns it on. If a human review gate shows on the card, pause first with `workyard orch session pause C-12 --for-review` and stop; you are resumed once it is approved, then integrate and finish. You cannot approve your own change. Investigative cards (no worktree) change no repository: report with `task note` instead; a long write-up (findings, options, evidence) goes in a markdown report, `task report C-12 --file report.md` (or stdin), which replaces the previous one, shows as the card's Report tab and may be 256 KB. Keep notes for short messages.
6. Finish: `task move C-12 Done` only when the work is integrated into main and checks pass. Otherwise `task release C-12`.

## Rules

- Create and change files with your Write and Edit tools, never with shell heredocs (`cat > file <<EOF`) or `echo >`: Claude Code's shell checks often reject them, and the file tools are reliable.
- Add a note for a human with `task note C-12 "text"`. Notes never need a version. Messages that arrive while you work are notes a human left on your card; treat them as instructions about it.
- If a command fails with `version_conflict`, reread the card and retry. On `ownership_conflict`, stop; someone else owns it.
- Side discoveries: when you find a bug, missing piece or follow-up outside your card, never fix it silently or ignore it. Add a Backlog card: `task add-card <board> "Title" --desc "what, where, why"`, order it with `task depend` if needed, and add one line to your own card's note linking it (`task note C-12 "found C-N: ..."`). At most 3 new cards per card, and never start them.
- Only the planner role uses `plan`; the only `orch` command you use is `orch session pause`; `admin` is for humans.
- Reads: `boards`, `board <B-1>`, `card <C-1>`, `questions`, `history`; `status <B-1>` is a one-screen summary, and `wait <B-1> --until attention|done|<n>-done [--timeout 30m]` blocks, prints why and exits 0 (4 on timeout).
