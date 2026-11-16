#!/usr/bin/env python3
"""Create and animate a sample board using only the Workyard CLI."""

from __future__ import annotations

import argparse
import json
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "demo" / "workyard.db"
CLI = ROOT / "target" / "debug" / "workyard"
BOARD_NAME = "Workyard showcase"


COMMAND_GROUPS = {
    "claim": "task",
    "release": "task",
    "move": "task",
    "progress": "task",
    "note": "task",
    "update": "task",
    "add-card": "task",
    "depend": "task",
    "undepend": "task",
    "ask": "task",
    "question-options": "task",
    "link-change": "task",
    "update-change": "task",
    "require-review": "task",
    "create-project": "plan",
    "create-board": "plan",
    "attach-board": "plan",
    "add-repository": "plan",
    "add-document": "plan",
    "add-stakeholder": "plan",
    "assign-stakeholder": "plan",
    "unassign-stakeholder": "plan",
    "phase": "plan",
    "cancel": "plan",
    "transfer": "orch",
    "force-release": "orch",
    "session": "orch",
    "archive-board": "admin",
    "unarchive-board": "admin",
    "archive-lane": "admin",
    "scope-dependency": "admin",
    "answer": "admin",
    "drop": "admin"
}


def call(*args: str, actor: str = "demo") -> dict | list | None:
    args = (COMMAND_GROUPS[args[0]], *args) if args[0] in COMMAND_GROUPS else args
    if not CLI.exists():
        raise SystemExit("Build the CLI first: cargo build -p workyard-cli")
    completed = subprocess.run(
        [str(CLI), "--db", str(DB), "--actor", actor, "--json", *args],
        capture_output=True,
        text=True,
        check=False,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(completed.stderr or completed.stdout) from exc
    if not result["ok"]:
        error = result["error"]
        raise RuntimeError(f"{error['code']}: {error['message']}")
    return result["data"]


def add(board: str, title: str, agent_type: str = "") -> dict:
    return call("add-card", board, title, "--agent-type", agent_type)


def move(card: dict, lane: str, actor: str = "demo") -> dict:
    return call("move", card["key"], lane, "--expected-version", str(card["version"]), actor=actor)


def progress(card: dict, amount: int, note: str, actor: str = "demo") -> dict:
    return call("progress", card["key"], str(amount), "--note", note, "--expected-version", str(card["version"]), actor=actor)


def remove_review_lane(board_key: str) -> None:
    """Move showcase work through normal commands, then archive its old lane."""
    view = call("board", board_key)
    review = next((lane for lane in view["lanes"] if lane["kind"] == "review" and lane["name"] == "Review"), None)
    if review is None:
        return
    for card in review["cards"]:
        current = call("card", card["key"])
        destination = "Backlog" if current["blocked"] else "In Progress"
        moved = move(current, destination, actor=current["owner_actor"] or "agent:demo")
        print(f"Moved {moved['key']} from Review to {destination}")
    call("archive-lane", review["id"], "--expected-version", str(review["version"]), actor="agent:demo")
    print("Archived empty Review lane")


def enrich_graph(board_key: str) -> None:
    """Add branching chains and questions to the existing showcase board."""
    view = call("board", board_key)
    cards = {card["title"]: card for lane in view["lanes"] for card in lane["cards"]}
    extra_cards = [
        ("Test agent client workflow", "Integration", "In Progress", 70),
        ("Handle version conflicts gracefully", "Engineering", "In Progress", 25),
        ("Build agent handoff workflow", "Orchestration", "Backlog", 0),
        ("Add reduced-motion support", "Frontend", "Backlog", 0),
        ("Audit accessibility", "Quality", "Backlog", 0),
        ("Write recovery playbook", "Documentation", "Backlog", 0),
        ("Prepare release checklist", "Release", "Backlog", 0),
    ]
    for title, agent_type, lane, amount in extra_cards:
        if title not in cards:
            card = add(board_key, title, agent_type)
            if lane != "Backlog":
                card = move(card, lane)
            if amount:
                card = progress(card, amount, "Example work is underway.")
            cards[title] = card

    links = [
        ("Wire the CLI and MCP tools", "Build the shared command layer"),
        ("Polish cross-lane animation", "Build the shared command layer"),
        ("Refine the question inbox", "Wire the CLI and MCP tools"),
        ("Add keyboard shortcuts", "Wire the CLI and MCP tools"),
        ("Write the onboarding guide", "Wire the CLI and MCP tools"),
        ("Notify agents when work becomes blocked", "Wire the CLI and MCP tools"),
        ("Package the macOS release", "Add keyboard shortcuts"),
        ("Package the macOS release", "Notify agents when work becomes blocked"),
        ("Test agent client workflow", "Wire the CLI and MCP tools"),
        ("Handle version conflicts gracefully", "Build the shared command layer"),
        ("Build agent handoff workflow", "Test agent client workflow"),
        ("Build agent handoff workflow", "Handle version conflicts gracefully"),
        ("Add reduced-motion support", "Polish cross-lane animation"),
        ("Audit accessibility", "Add reduced-motion support"),
        ("Audit accessibility", "Refine the question inbox"),
        ("Write recovery playbook", "Build agent handoff workflow"),
        ("Write recovery playbook", "Write the onboarding guide"),
        ("Prepare release checklist", "Audit accessibility"),
        ("Prepare release checklist", "Write recovery playbook"),
        ("Prepare release checklist", "Test agent client workflow"),
        ("Package the macOS release", "Prepare release checklist"),
    ]
    added = 0
    for dependent_title, prerequisite_title in links:
        dependent = cards[dependent_title]
        prerequisite = cards[prerequisite_title]
        current = call("card", dependent["key"])
        already_linked = any(
            edge["from_card_id"] == current["id"] and edge["to_card_id"] == prerequisite["id"]
            for edge in current["dependencies"]
        )
        if not already_linked:
            call(
                "depend", current["key"], "--on", prerequisite["key"],
                "--expected-version", str(current["version"]),
                actor=current["owner_actor"] or "agent:demo",
            )
            added += 1

    existing_questions = {question["title"]: question for question in call("questions", board_key)}
    distribution_title = "How should the first macOS build be distributed?"
    distribution_options = ["Signed DMG download", "Homebrew cask", "Both"]
    distribution_recommendation = "Signed DMG download"
    distribution_why = "A signed download is the simplest way for people to try the first macOS release."
    distribution = existing_questions.get(distribution_title)
    if distribution and distribution["status"] == "open" and (distribution["options"] != distribution_options + ["Other"] or distribution["recommended_option"] != distribution_recommendation or distribution["recommendation_reason"] != distribution_why):
        args = ["question-options", distribution["key"], "--expected-version", str(distribution["version"]), "--recommend", distribution_recommendation, "--why", distribution_why]
        for option in distribution_options:
            args.extend(["--option", option])
        call(*args, actor="agent:demo")
    new_questions = [
        ("business", "Which keyboard shortcuts should be global?", "Add keyboard shortcuts", ["Board navigation only", "Board navigation and question inbox", "No global shortcuts"], "Board navigation only", "Keep global shortcuts small so they do not interfere when someone types an answer or note."),
        ("engineering", "How should the question inbox handle event catch-up?", "Refine the question inbox", ["Refresh after each event page", "Refresh after the final page"], "Refresh after the final page", "Wait for all missed events, then refresh once so the board does not jump around."),
        ("engineering", "How should agents retry version conflicts?", "Handle version conflicts gracefully", ["Reread and retry automatically once", "Reread and ask the agent to reconcile"], "Reread and ask the agent to reconcile", "The agent should compare the latest changes before writing, so it does not overwrite someone else's work."),
    ]
    for kind, title, card_title, options, recommendation, why in new_questions:
        if title not in existing_questions:
            args = ["ask", board_key, title, "--kind", kind, "--blocks", cards[card_title]["key"], "--recommend", recommendation, "--why", why]
            for option in options:
                args.extend(["--option", option])
            call(*args, actor="agent:demo")
            added += 1
        elif existing_questions[title]["status"] == "open" and (existing_questions[title]["options"] != options + ["Other"] or existing_questions[title]["recommended_option"] != recommendation or existing_questions[title]["recommendation_reason"] != why):
            args = ["question-options", existing_questions[title]["key"], "--expected-version", str(existing_questions[title]["version"]), "--recommend", recommendation, "--why", why]
            for option in options:
                args.extend(["--option", option])
            call(*args, actor="agent:demo")
            added += 1
    print(f"Graph example ready: {len(cards)} cards; {added} new links/questions in {DB}")


def organize_phases(board_key: str) -> None:
    """Show current execution, opportunistic next-phase work, and release gates."""
    phases = {phase["name"]: phase for phase in call("phase", "list", board_key)}
    for name in ("Integration", "Release"):
        if name not in phases:
            phases[name] = call("phase", "add", board_key, name)

    # Move dependents first, so every dependency continues to point backward.
    assignments = [
        ("Package the macOS release", "Release"),
        ("Prepare release checklist", "Release"),
        ("Audit accessibility", "Integration"),
        ("Write recovery playbook", "Integration"),
        ("Build agent handoff workflow", "Integration"),
        ("Add reduced-motion support", "Integration"),
        ("Notify agents when work becomes blocked", "Integration"),
        ("Add keyboard shortcuts", "Integration"),
        ("Test agent client workflow", "Integration"),
        ("Write the onboarding guide", "Integration"),
    ]
    for title, phase_name in assignments:
        view = call("board", board_key)
        card = next((card for lane in view["lanes"] for card in lane["cards"] if card["title"] == title), None)
        if not card or card["phase_id"] == phases[phase_name]["id"] or card["owner_actor"] or card["cancelled_at"]:
            continue
        lane = next(lane for lane in view["lanes"] if lane["id"] == card["lane_id"])
        if lane["kind"] == "backlog":
            call("phase", "assign", card["key"], phase_name, "--expected-version", str(card["version"]))

    for title, phase_name in [
        ("How should the first macOS build be distributed?", "Release"),
        ("Which keyboard shortcuts should be global?", "Integration"),
    ]:
        question = next((item for item in call("questions", board_key) if item["title"] == title), None)
        if question and question["phase_id"] != phases[phase_name]["id"]:
            call("phase", "assign", question["key"], phase_name, "--expected-version", str(question["version"]))

    cards = [card for lane in call("board", board_key)["lanes"] for card in lane["cards"]]
    if not any(card["title"] == "Sketch agent handoff metrics" for card in cards):
        call("add-card", board_key, "Sketch agent handoff metrics", "--agent-type", "Research", "--phase", "Integration")


def describe_phases(board_key: str, descriptions: dict[str, str]) -> None:
    for phase in call("phase", "list", board_key):
        description = descriptions.get(phase["name"])
        if description and phase["description"] != description and phase["status"] != "closed":
            call("phase", "describe", board_key, phase["id"], description, "--expected-version", str(phase["version"]))


def deepen_demo(board_key: str) -> None:
    """Keep the showcase useful for testing dense boards and phase graphs."""
    cards = {card["title"]: card for lane in call("board", board_key)["lanes"] for card in lane["cards"]}
    additions = [
        ("Map command ownership boundaries", "Current work", "Architecture"),
        ("Investigate SQLite contention", "Current work", "Research"),
        ("Verify CLI error recovery", "Current work", "Quality"),
        ("Design orchestrator heartbeat", "Integration", "Orchestration"),
        ("Prototype worker pool limits", "Integration", "Engineering"),
        ("Show agent connection status", "Integration", "Frontend"),
        ("Recover abandoned agent sessions", "Integration", "Orchestration"),
        ("Test question escalation", "Integration", "Quality"),
        ("Measure graph layout readability", "Integration", "Research"),
        ("Document worker handoff protocol", "Integration", "Documentation"),
        ("Validate local merge ordering", "Release", "Integration"),
        ("Run end-to-end accessibility pass", "Release", "Quality"),
        ("Draft release notes", "Release", "Documentation"),
        ("Test fresh-machine setup", "Release", "Quality"),
        ("Prepare rollback instructions", "Release", "Documentation"),
    ]
    for title, phase, agent_type in additions:
        if title not in cards:
            cards[title] = call("add-card", board_key, title, "--agent-type", agent_type, "--phase", phase)

    links = [
        ("Verify CLI error recovery", "Investigate SQLite contention"),
        ("Prototype worker pool limits", "Map command ownership boundaries"),
        ("Design orchestrator heartbeat", "Sketch agent handoff metrics"),
        ("Show agent connection status", "Design orchestrator heartbeat"),
        ("Recover abandoned agent sessions", "Design orchestrator heartbeat"),
        ("Recover abandoned agent sessions", "Prototype worker pool limits"),
        ("Test question escalation", "Refine the question inbox"),
        ("Test question escalation", "Build agent handoff workflow"),
        ("Measure graph layout readability", "Audit accessibility"),
        ("Document worker handoff protocol", "Build agent handoff workflow"),
        ("Document worker handoff protocol", "Recover abandoned agent sessions"),
        ("Validate local merge ordering", "Handle version conflicts gracefully"),
        ("Run end-to-end accessibility pass", "Audit accessibility"),
        ("Draft release notes", "Write the onboarding guide"),
        ("Test fresh-machine setup", "Package the macOS release"),
        ("Prepare rollback instructions", "Validate local merge ordering"),
        ("Prepare rollback instructions", "Recover abandoned agent sessions"),
    ]
    for dependent_title, prerequisite_title in links:
        current = call("card", cards[dependent_title]["key"])
        prerequisite = cards[prerequisite_title]
        if not any(edge["from_card_id"] == current["id"] and edge["to_card_id"] == prerequisite["id"] for edge in current["dependencies"]):
            call("depend", current["key"], "--on", prerequisite["key"], "--expected-version", str(current["version"]), actor=current["owner_actor"] or "agent:demo")

    existing = {item["title"] for item in call("questions", board_key)}
    questions = [
        ("engineering", "What should count as a live orchestrator?", "Integration", "Show agent connection status", ["Recent heartbeat", "Open process only", "Both"], "Recent heartbeat", "A heartbeat survives UI navigation and reflects actual activity."),
        ("engineering", "How many workers should a small machine run?", "Integration", "Prototype worker pool limits", ["Two by default", "Four by default", "Auto-detect CPU capacity"], "Two by default", "A conservative default avoids competing local agents."),
        ("business", "What should release notes emphasize?", "Release", "Draft release notes", ["Agent workflow", "Local-first privacy", "Both"], "Both", "The release story needs both benefits."),
        ("engineering", "Which failure should rollback cover first?", "Release", "Prepare rollback instructions", ["Migration failure", "Broken desktop bundle", "Both"], "Both", "Either issue can prevent someone from using the workspace."),
    ]
    for kind, title, phase, blocked_title, options, recommendation, reason in questions:
        if title not in existing:
            args = ["ask", board_key, title, "--kind", kind, "--phase", phase, "--blocks", cards[blocked_title]["key"], "--recommend", recommendation, "--why", reason]
            for option in options:
                args.extend(["--option", option])
            call(*args, actor="agent:demo")
    print(f"Complex showcase ready: {len(cards)} cards across three phases")


def ensure_project_context(board_key: str) -> None:
    """Give the navigation real project documents, people, and multiple boards."""
    projects = {project["name"]: project for project in call("projects")}
    project = projects.get("Workyard") or call("create-project", "Workyard", "--description", "Local agent workspace and desktop dashboard")
    project_id = project["id"]
    board = call("board", board_key)
    if board["project_id"] != project_id:
        call("attach-board", board_key, project_id, "--expected-version", str(board["version"]))

    details = call("project", project_id)
    repo = next((item for item in details["repositories"] if item["path"] == str(ROOT)), None)
    if repo is None:
        repo = call("add-repository", project_id, str(ROOT), "--target-branch", "main")
    documents = {item["path"] for item in call("project", project_id)["documents"]}
    for path, kind, title in [
        ("docs/adr/0001-board-phases-and-scheduling.md", "adr", "Board phases and scheduling"),
        ("docs/adr/0002-cancelling-work.md", "adr", "Cancelling work"),
        ("docs/architecture.md", "diagram", "Workyard architecture"),
        ("docs/SPEC.md", "documentation", "Build specification"),
    ]:
        if path not in documents:
            call("add-document", project_id, repo["id"], path, "--kind", kind, "--title", title)

    stakeholders = {item["name"]: item for item in call("project", project_id)["stakeholders"]}
    if "Product lead" not in stakeholders:
        stakeholders["Product lead"] = call("add-stakeholder", project_id, "Product lead", "--role", "Business decisions", "--contact", "Local demo contact")
    if "Engineering lead" not in stakeholders:
        stakeholders["Engineering lead"] = call("add-stakeholder", project_id, "Engineering lead", "--role", "Architecture review", "--contact", "Local demo contact")
    question = next((item for item in call("questions", board_key) if item["title"] == "How should the first macOS build be distributed?"), None)
    if question and question["stakeholder_id"] != stakeholders["Product lead"]["id"]:
        call("assign-stakeholder", question["key"], stakeholders["Product lead"]["id"], "--expected-version", str(question["version"]))
    for title, stakeholder_name in [
        ("Which keyboard shortcuts should be global?", "Product lead"),
        ("How should agents retry version conflicts?", "Engineering lead"),
        ("What should count as a live orchestrator?", "Engineering lead"),
    ]:
        assigned_question = next((item for item in call("questions", board_key) if item["title"] == title), None)
        if assigned_question and assigned_question["stakeholder_id"] != stakeholders[stakeholder_name]["id"]:
            call("assign-stakeholder", assigned_question["key"], stakeholders[stakeholder_name]["id"], "--expected-version", str(assigned_question["version"]))

    code_card = next(card for lane in call("board", board_key)["lanes"] for card in lane["cards"] if card["title"] == "Wire the CLI and MCP tools")
    detail = call("card", code_card["key"])
    if not any(change["working_branch"] == "demo/mcp-adapter" for change in detail["changes"]):
        call("link-change", code_card["key"], repo["id"], "demo/mcp-adapter", "--review-required", "--expected-version", str(detail["version"]), actor=detail["owner_actor"] or "agent:demo")

    boards = {item["name"]: item for item in call("boards")}
    runtime = boards.get("Agent runtime") or call("create-board", "Agent runtime", "--description", "Orchestrator events, sessions, and handoffs")
    if runtime["project_id"] != project_id:
        call("attach-board", runtime["key"], project_id, "--expected-version", str(runtime["version"]))
    runtime_cards = {card["title"] for lane in call("board", runtime["key"])["lanes"] for card in lane["cards"]}
    if "Define the event handoff contract" not in runtime_cards:
        first = add(runtime["key"], "Define the event handoff contract", "Architecture")
        move(first, "Done")
    if "Persist orchestrator sessions" not in runtime_cards:
        second = add(runtime["key"], "Persist orchestrator sessions", "Engineering")
        move(second, "In Progress")
    if "Notify the ticket owner about blockers" not in runtime_cards:
        third = add(runtime["key"], "Notify the ticket owner about blockers", "Orchestration")
        first = next(card for lane in call("board", runtime["key"])["lanes"] for card in lane["cards"] if card["title"] == "Define the event handoff contract")
        call("depend", third["key"], "--on", first["key"], "--expected-version", str(third["version"]))

    research = projects.get("Field Notes") or call("create-project", "Field Notes", "--description", "A small non-code research project")
    research_board = next((item for item in call("boards") if item["name"] == "Research backlog"), None)
    if research_board is None:
        research_board = call("create-board", "Research backlog", "--description", "Investigations and decisions without a code repository")
    if research_board["project_id"] != research["id"]:
        call("attach-board", research_board["key"], research["id"], "--expected-version", str(research_board["version"]))
    research_cards = {card["title"] for lane in call("board", research_board["key"])["lanes"] for card in lane["cards"]}
    if "Interview local-first users" not in research_cards:
        add(research_board["key"], "Interview local-first users", "Research")
    if "Summarize onboarding friction" not in research_cards:
        add(research_board["key"], "Summarize onboarding friction", "Research")
    describe_phases(runtime["key"], {"Current work": "Coordinate agent sessions, worker handoffs, and blocker notifications."})
    describe_phases(research_board["key"], {"Current work": "Investigate local-first onboarding and summarize user needs."})


def seed() -> None:
    DB.parent.mkdir(parents=True, exist_ok=True)
    existing = next((board for board in call("boards") if board["name"] == BOARD_NAME), None)
    if existing:
        print(f"Showcase board already exists: {existing['key']} in {DB}")
        remove_review_lane(existing["key"])
        enrich_graph(existing["key"])
        organize_phases(existing["key"])
        deepen_demo(existing["key"])
        ensure_project_context(existing["key"])
        describe_phases(existing["key"], {"Current work": "Build stable commands, event storage, and the core board interactions.", "Integration": "Connect agents, questions, and the desktop into a reliable workflow.", "Release": "Verify distribution, accessibility, recovery, and local integration."})
        return
    board = call("create-board", BOARD_NAME, "--description", "A sample agent project with work at every stage.")
    key = board["key"]

    core = add(key, "Build the shared command layer", "Engineering")
    core = move(core, "Done")
    progress(core, 100, "SQLite migrations, ownership, and events are working.")

    mcp = add(key, "Wire the CLI and MCP tools", "Integration")
    mcp = move(mcp, "In Progress")
    progress(mcp, 85, "Tool descriptions and JSON output are ready for review.")

    motion = add(key, "Polish cross-lane animation", "Frontend")
    motion = move(motion, "In Progress")
    motion = progress(motion, 55, "External moves tween across lanes; checking edge cases.")
    call("claim", motion["key"], "--expected-version", str(motion["version"]), actor="agent:motion")

    inbox = add(key, "Refine the question inbox", "Frontend")
    inbox = move(inbox, "In Progress")
    progress(inbox, 35, "Open decisions are grouped and answerable inline.")

    add(key, "Add keyboard shortcuts", "Frontend")
    guide = add(key, "Write the onboarding guide", "Documentation")
    progress(guide, 10, "Outline drafted; screenshots still needed.")

    release = add(key, "Package the macOS release", "Release")
    call("ask", key, "How should the first macOS build be distributed?", "--kind", "business", "--blocks", release["key"], "--option", "Signed DMG download", "--option", "Homebrew cask", "--option", "Both", "--recommend", "Signed DMG download", "--why", "A signed download is the simplest way for people to try the first macOS release.")

    notify = add(key, "Notify agents when work becomes blocked", "Orchestration")
    call("depend", notify["key"], "--on", mcp["key"], "--expected-version", str(notify["version"]))

    enrich_graph(key)
    organize_phases(key)
    deepen_demo(key)
    ensure_project_context(key)
    describe_phases(key, {"Current work": "Build stable commands, event storage, and the core board interactions.", "Integration": "Connect agents, questions, and the desktop into a reliable workflow.", "Release": "Verify distribution, accessibility, recovery, and local integration."})

    print(f"Created {BOARD_NAME} ({key}) with 15 cards in {DB}")
    print("Launch: WORKYARD_DB=\"$PWD/demo/workyard.db\" mise run dev")
    print("Then animate: python3 scripts/demo.py animate")


def animate(delay: float) -> None:
    if not DB.exists():
        raise SystemExit("Seed the demo first: python3 scripts/demo.py seed")
    board = next((item for item in call("boards") if item["name"] == BOARD_NAME), None)
    if not board:
        raise SystemExit("Showcase board not found. Run: python3 scripts/demo.py seed")
    view = call("board", board["key"])
    card = next(card for lane in view["lanes"] for card in lane["cards"] if card["title"] == "Polish cross-lane animation")
    for lane in ("Backlog", "In Progress", "Backlog"):
        card = call("card", card["key"])
        card = move(card, lane, actor=card["owner_actor"] or "agent:demo")
        print(f"Moved {card['key']} to {lane}", flush=True)
        time.sleep(delay)
        if lane == "In Progress":
            card = progress(card, 65, "Shared layout animation is working.", actor=card["owner_actor"] or "agent:demo")
            print(f"Updated {card['key']} to 65%", flush=True)
            time.sleep(delay)
    print("Animation sequence complete.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("seed", "animate"))
    parser.add_argument("--delay", type=float, default=1.6, help="Seconds between demo changes")
    options = parser.parse_args()
    if options.action == "seed":
        seed()
    else:
        animate(options.delay)
