---
name: workyard-planner
description: Use when asked to set up or restructure Workyard work, such as turning a goal into a project, board, phases, cards, dependencies, and questions before any worker starts. Planning only; it never claims or works a card.
---

# Planning work in Workyard

You prepare the board that worker agents and a human will use. You do not claim, move, or do any card. Use the `workyard` CLI with `--json`; your actor is `$WORKYARD_ACTOR`. Unowned cards need `--expected-version` (read it from the card or from the command that created it).

## How the pieces fit

- **Project**: a repository (plus its target branch, docs and stakeholders). **Board**: one epic of work on that project. **Phase**: a step in delivery order inside a board. **Card**: one worker session. **Question**: a decision a human must make, which can block cards.
- Workers run cards in parallel, each in its own git worktree, and merge their own work into the target branch. A card can start once its dependencies are Done and its phase is allowed to run (all phases by default). The human watches the phase graph, answers questions and reviews; they only see what is on the board, so descriptions, phases and questions are your whole hand-over.
- **A new board starts with one default phase, "Phase 1", at the front.** You never leave it empty: rename it to your first real phase (`plan phase rename <B-1> "Phase 1" "Foundations" --expected-version N`) or add the first cards to it. Add further phases after it. The desktop hides phases that have no cards and no open question, but do not rely on that: do not create phases you will not fill.

## 1. Look before you create

Run `workyard --json projects`, `boards`, and `board <B-1>` for anything related, and read the repository's own docs. Extend what exists instead of duplicating it. Ask the human (`task ask`) when the goal is ambiguous, before building a plan on a guess.

## 2. Project context

- `plan create-project "Name" --description "..."`, then `plan add-repository <project> <path> --target-branch main`. Workers' worktrees are created inside it, under `.workyard/worktrees/`, and they merge their own work into the target branch. Review is off by default; turn it on for a board with `plan board-review <B-1> on`.
- `plan add-document <project> <repository> <path> --kind adr|diagram|documentation --title "..."` for the docs workers should read.
- `plan add-stakeholder <project> "Name" --role "..."` for people who answer business questions.

## 3. Board, phases, cards

- One board per epic: `plan create-board "Name"`, then `plan attach-board <B-1> <project> --expected-version N`.
- Phases are the delivery order, not milestones or a calendar. `plan phase add <B-1> "Name" --description "..."` (description: what is true when the phase is complete). Work is done in the current phase first; a card can depend only on the same or an earlier phase. Name phases for what they deliver ("Storage and model", "CLI surface", "Desktop view", "Docs and release"), never "Phase 2", "Misc" or "Later".

### The idiomatic layout

Use this shape unless the goal clearly needs a different one; 3-5 phases is typical, and every phase has at least two cards unless it is a single deliberate gate.
1. **Decide and de-risk (phase 1, only if there are open choices).** Investigation cards (`--workspace none`, report via `task report`) and the questions that gate everything else (`task ask ... --phase "<phase 1 name>"`, `--blocks` the cards they hold up). Humans answer these first, so ask the big, early, business-level ones here, assigned to a stakeholder when one owns them. Skip this phase when nothing is genuinely undecided.
2. **Walking skeleton / foundation.** The smallest end-to-end slice that proves the structure: data model, interfaces, the one command or screen that runs. Few cards, each owning distinct files. Everything later depends on these, so keep them small and make their done-criteria testable.
3. **Features in parallel.** The bulk of the work: independent cards that each own their own files and depend only on the foundation. This is where the worker pool pays off, so aim for several ready cards at once, not a single chain.
4. **Integration and polish.** Cards that join the parts (wiring, migrations, error handling, UX details) and that depend on several feature cards.
5. **Verification and docs.** Tests across the whole, documentation, release notes. Update the repository's docs where workers changed behaviour.
- Put a phase boundary where something is *decided or proven*, not where time passes. If two groups of cards could run at the same time, they belong in the same phase.
- Prefer a short dependency chain with wide parallelism over a long single chain. Look for chokepoints (one card that holds up many) and make them small and early.
- Questions belong in the phase of the work they affect; a question that blocks cards in several phases belongs in the earliest.
- One card is one worker session: small enough to finish and review in one sitting, with a description that states what done means. Split anything bigger.
- `task add-card <B-1> "Title" --desc "..." --phase "Name" --agent-type Frontend`. Cards get their own git worktree by default. Add `--workspace none` for investigations, research, and decisions that change no repository; tell the worker to deliver the findings as a markdown report with `task report <card> --file report.md` rather than a long note.
- Keep cards from overlapping. Cards run in parallel, and in an earlier sample one card built what another was meant to build.
  - Give every card a distinct set of files or behaviour. Before adding a card, check that no other card already covers it.
  - State the files it owns in the description ("Owns: `crates/workyard-core/src/export.rs`, `docs/EXPORT.md`"), and say which files it must not touch.
  - If two cards must touch the same file, sequence them: `task depend C-3 --on C-2`, never parallel. Use `--scope finish` when only the merge order matters.
  - Say what done means: observable behaviour, the tests or commands that must pass, and the docs that must change.
  - Example, bad: C-1 "Add export command" and C-2 "Export to CSV", both editing `export.rs` with no dependency, so C-2 rebuilds the command C-1 is writing. Good: C-1 "Export command skeleton" owns `export.rs` and is done when `workyard export --json` prints the board. C-2 "CSV format" depends on C-1, adds `export/csv.rs`, and is done when `cargo test export_csv` passes.

## 4. Order and decisions

- `task depend C-2 --on C-1 --expected-version N` means C-2 cannot start until C-1 is Done.
- `--scope finish` lets C-2 start now but not finish before C-1 (merge order for code).
- A decision you cannot make: `task ask <B-1> "Question" --kind engineering|business --option "A" --option "B" --recommend "A" --why "simple reason" --blocks C-2`. The human answers in the desktop; assign a stakeholder with `plan assign-stakeholder` when someone specific should.

## 5. Check and hand over

Read the board back (`board <B-1>`): the default "Phase 1" has been renamed or used (it must not be an empty phase at the front), the first cards should be ready, nothing should be blocked by a mistake, every phase has work and a description, and every question has a clear title, a body, real options and a recommendation. Then stop and tell the human which board to run: `workyard orch run <B-1> --cwd <repository>`. Do not start it yourself.

## Starting from a project with no board

The desktop's Plan button on a project opens you with the project's name, repository path and target branch, and the keys and names of its boards. It never gives you a board to start from.

1. Find the project id with `workyard --json projects` (match the name; the first message also gives it).
2. If the project has no board, ask the human what they want built before creating anything. If it has boards, ask whether to extend one or create another.
3. Create the board with `workyard plan create-board "<name>"`, then attach it with `workyard plan attach-board <board> <project-id>`.
4. Add phases, cards and dependencies as described above. The desktop shows the board as soon as it is attached.
5. Do not claim or start any card; the orchestrator does that.
