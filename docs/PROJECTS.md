# Projects: design notes

Board phase scheduling, cancellation, and the phase graph are specified in [ADR 0001](adr/0001-board-phases-and-scheduling.md), [ADR 0002](adr/0002-cancelling-work.md), and [ADR 0003](adr/0003-phase-dependency-graph.md). The core, CLI, and desktop now implement these rules.

Status: project context, code links, review approval, and start/finish dependencies are implemented. The local Git train runner and hosted review observation remain open.

## Hierarchy and navigation

- A **project** is the durable context for one or more boards. A **board** represents an epic within that project; cards and questions remain on their board.
- Projects have stable IDs, names, descriptions, and versions. A board gets an optional `project_id` so existing boards remain readable until assigned.
- Projects are selected on a full-width picker without a sidebar. Selecting one opens the workspace sidebar with its name and a back arrow. Overview sits above separate Stakeholders and Boards sections. Each stakeholder is listed in the sidebar with the number of open questions assigned to them, including zero; selecting one opens their answerable question cards. The Boards section contains only that project's boards. Overview shows the project's single repository. Opening a board lands on the phase graph; a phase title opens its phase-only Kanban with a back button. Unassigned legacy boards remain accessible through an Unassigned boards entry until an agent attaches them to a project.
- Keep the large Workyard logo/header block out of the sidebar to make room for project context.
- Project and board management remain agent actions through named core/CLI commands. The desktop stays a human review surface.

## Repositories and code changes

- A project may reference one **local Git repository**. A remote host, PR/MR, and provider account are optional. No network connection is required to use a project.
- The repository has a local path and a default target branch. A linked code change records its repository, working branch, target branch (which may override the default), and commit identity. Hosted PR/MR details can be attached when they exist.
- A card may have **no code change**. Investigations, decisions, documentation, and other non-code work need no integration metadata. Workyard has no separate delivery-state field.
- A code card stays In Progress until its linked change is integrated into the target branch and the required checks pass on that integrated result. Its owning agent makes the move to Done. The core currently enforces finish dependencies and review approval; Git integration and test verification are still agent responsibilities until the runner exists.
- Integration order, local Git testing, and hosted review are distinct concerns. A code card waiting for another card to finish uses a **finish dependency** on that predecessor card. The owner can keep working while the predecessor is unfinished; integration waits until the predecessor is Done.

## Dependencies and review gates

- Keep **Backlog, In Progress, and Done** as the only work lanes. Do not add a Waiting, Queue, or Review lane. A card in In Progress shows a compact reason such as “waiting for review” or “merges after C-12” when applicable.
- Extend the existing card/question dependency with a scope: **blocks start** (the current behavior and default for existing rows) or **blocks finish**. Derive starting readiness and `can_finish` separately. An unresolved start dependency also prevents moving directly to Done; a finish dependency allows entering and remaining In Progress but prevents Done.
- For merge order, B has a finish dependency on A. A is already a graph node, so draw the B → A dependency arrow; mark it as a finish dependency. Do not create a separate gate node. The card dependency still follows the existing cycle and ownership rules.
- Keep question dependencies' current resolution rule: answering or dropping a question resolves that dependency, regardless of the selected option. The owning agent interprets the answer and raises another question when more review or clarification is needed. Do not add a required-answer predicate.
- The only new gate kind is **review approved**, optional and attached to the relevant card/code change. It is shown as a badge on the card, not as a node in the dependency graph. An agent can record approval for the current commit. Automated hosted review observation is not yet implemented. A local-only or investigative card need not have this gate.
- Do not model **checks passed** as a separate gate. A code ticket is not complete until its integrated result is working and required tests have passed, but test execution and evidence belong to the integration workflow. Its owner moves it to Done after those conditions hold.
- The card owner may add or remove a finish dependency or review gate after the card was created, using an expected version. Adding an unmet requirement immediately changes derived `can_finish` and appears in the command's single event. If an unmet requirement is added to a Done card, flag it for the owner to reopen; do not silently edit an owned ticket.
- A review approval is tied to the relevant change revision. A new commit or withdrawn approval makes the gate pending again. Recompute dependent card state, emit the transition as an effect of the one recorded action, and notify the owner. The owner makes any resulting ticket move.
- Gates are conditions attached to cards or changes. The append-only `events` table remains the audit trail of commands and resulting state changes; cards do not depend on historical event rows.

## Decisions and documents

- ADRs and architecture diagrams remain files in the project repository. Workyard does not copy their content into SQLite. The separate document views are removed from project navigation.
- Existing project document references remain readable in the data model, but we should not expand them into a document catalog or event-trigger system. Agent instructions in the repository should say where the documentation lives and when to update it. The source files supply their content and version history.
- Prefer repository-relative paths, so a project can be moved without rewriting every document reference. A missing file should be shown as missing, not silently removed from the project.

## Stakeholders and questions

- A project can list stakeholders with name, role, and contact details. Questions do not require a stakeholder. Any open business or engineering question can optionally be assigned to a stakeholder in the same project; the question still belongs to a board. The graph and stakeholder question cards can assign, reassign, or clear the stakeholder. Selecting a stakeholder shows answerable open questions and resolved history across the project's boards.
- **Assigned contact** and **actual respondent** are separate facts. Assigning a contact must not imply that a message was sent. Record delivery/notification only when an integrated orchestrator confirms it.
- Engineering and business questions use the same answer card in the stakeholder view; only their colors differ. Answered questions are omitted from the phase graph; open questions can be assigned to a stakeholder and answered from that graph. Unassigned questions remain discoverable on their board's phase graph.

## Commands to design with the implementation

Implemented named commands cover creating projects, linking boards, registering the local repository, adding document references and stakeholders, optionally assigning or unassigning a stakeholder on a question, linking/updating code changes, adding or removing review requirements after the fact, recording approval, and adding start/finish dependencies. Project editing, some removal commands, and automated Git observation remain future work. Card ownership, optimistic versions, and one event per write still apply. Do not expose arbitrary SQL or unrestricted file access through MCP.

## Open decisions

1. How the local Git train builds and tests cumulative changes, promotes a tested result, and invalidates results when a branch or target moves.
2. How hosted review approval is observed and invalidated when a remote is linked, without making a remote prerequisite for local projects.
3. How a single project repository configures its local integration and test commands without assuming a Git host or queue.

## Agent runtime proposal

The board is the orchestrator session boundary. Show whether its orchestrator is connected using a real heartbeat or session lease; an open desktop window is not proof of connection. A terminal tab is optional and is not needed for core board monitoring. Put the default worker pool size on the project, since it reflects local resource capacity and applies to its boards. Add a board override only after a real workflow needs one. Task session attachments and query-based resume priority are implemented; orchestrator heartbeat, runtime messaging, and pool configuration are not yet implemented.
