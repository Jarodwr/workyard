# Workyard showcase board

`workyard.db` is a separate sample database, created with the normal Workyard CLI commands. It contains two projects and three boards. The Workyard showcase board has 31 cards across Current work, Integration, and Release phases, with branching dependencies, engineering and business questions, and a linked change awaiting review. Product and Engineering leads each have two assigned questions in the Stakeholders view. An independent Integration card demonstrates the next-phase scheduling allowance. Its former Review lane is archived. The phase graph highlights chokepoints and cross-phase links.

From the Workyard project folder, start the desktop app on this database:

```sh
WORKYARD_DB="$PWD/demo/workyard.db" mise run dev
```

With the app open, run this in another terminal to watch a card move through three lanes with progress updates:

```sh
python3 scripts/demo.py animate
```

Open the **Workyard** project, then the **Workyard showcase** board in the sidebar. It opens on the **Phase graph**. Click a phase title to open its phase-only Kanban, and use **Back to phase graph** to return. Open question nodes on the graph can be assigned and answered in place. Click a stakeholder in the sidebar to answer all questions assigned to that person.

The animation sequence is repeatable. To create or enrich the database, run `python3 scripts/demo.py seed` after building the `workyard` CLI. Re-running seed adds missing project context, phases, links, and questions without duplicating them, and archives a legacy Review lane after moving its cards through normal commands.
