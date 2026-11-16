# ADR 0003: Show dependency graphs inside ordered phases

Status: Implemented in the desktop UI

## Context

The current dependency graph ranks cards by prerequisite depth. That is useful for identifying chokepoints but makes newly discovered, unblocked work look as though it belongs at the start of the epic. The board needs to show both its planned phases and real card/question dependencies.

## Decision

- The board's graph view has ordered phase regions. Each region contains its own card and question dependency graph. Place nodes within their assigned phase even if they have no blockers; use dependency layout only to order nodes **within** that region.
- Draw cross-phase dependency arrows from a dependent card to a prerequisite in the same or an earlier phase. Keep start and finish scope visually distinct. Never create synthetic edges merely because one phase follows another.
- Show the current phase clearly, with closed and future phases visually distinct. A next-phase card that has been started early remains in its assigned phase and is marked In Progress. Closed phases may collapse to a summary while retaining cross-phase arrows or endpoints needed for context.
- Filtering to one phase shows that phase's full nodes plus compact read-only context nodes for dependencies outside it. A filtered card must never appear mysteriously blocked. Questions stay in their assigned phase. Review approval stays a badge on the linked card/change, not a separate graph node. Cancelled prerequisites appear muted with an explicit cancelled label.
- Preserve chokepoint indicators using the count of live downstream cards, including downstream cards in later phases. The graph must not present phase position as a calculated critical path or imply a date estimate.
- Opening a board shows its phase graph. Completed phases are hidden by default; a toggle reveals them for history. Phase headings include descriptions and open a Kanban scoped to that phase, with a back button to the graph. The UI may zoom or focus a phase without changing the underlying board or dependency data. The CLI's board read accepts a phase filter; card reads expose blockers and their phase context. When a phase is selected in the desktop graph, show only that phase's nodes and internal links, and scope graph metrics to that phase. The default graph shows cross-phase dependencies among current and future phases. Phase headings stay visible at the top while the graph scrolls vertically.

## Consequences

Phase placement answers “where are we in the epic?” and arrows answer “what is waiting on what?” A new Phase 2 task with no dependencies stays in Phase 2, while a Phase 3 card can visibly depend on its result.
