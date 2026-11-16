use std::path::PathBuf;
use serde_json::json;
use workyard_core::Store;
use workyard_core::types::{Progress, QuestionKind, QuestionOptions, QuestionOutcome, Version};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("workyard-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn setup() -> (TempDir, Store, String) {
    let dir = TempDir::new();
    let store = Store::new(dir.path().join("test.db"));
    let board = store.create_board("cli", "Project", "").unwrap();
    (dir, store, board["key"].as_str().unwrap().to_owned())
}

#[test]
fn paused_task_resumes_before_backlog_with_question_answer_in_briefing() {
    let (_dir, store, board) = setup();
    let backlog = store.create_card("cli", &board, "New work", "", "P2", None).unwrap();
    let task = store.create_card("cli", &board, "Waiting work", "", "P2", None).unwrap();
    let key = task["key"].as_str().unwrap();
    store.claim_card("agent:one", key, 1).unwrap();
    store.move_card("agent:one", key, "In Progress", None, 2).unwrap();
    let attached = store.attach_task_session("orchestrator", key, "codex", "session-1").unwrap();
    assert_eq!(attached["owner_actor"], "agent:one");
    assert_eq!(store.view_card(key).unwrap()["version"], 3);
    let event_count = store.poll_events(0, 100).unwrap()["events"].as_array().unwrap().len();
    assert_eq!(store.attach_task_session("orchestrator", key, "codex", "other-session").unwrap_err().code, "session_exists");
    assert_eq!(store.poll_events(0, 100).unwrap()["events"].as_array().unwrap().len(), event_count);
    let question = store.raise_question("agent:one", &board, "engineering", "Which design?", "", &["A".into(), "B".into()], "A", "Simpler", &[key.to_owned()]).unwrap();
    let paused = store.pause_task_session("orchestrator", key, Some(question["key"].as_str().unwrap())).unwrap();
    assert_eq!(paused["waiting_question_key"], question["key"]);
    let event_count = store.poll_events(0, 100).unwrap()["events"].as_array().unwrap().len();
    assert_eq!(store.pause_task_session("orchestrator", key, None).unwrap_err().code, "session_already_paused");
    assert_eq!(store.poll_events(0, 100).unwrap()["events"].as_array().unwrap().len(), event_count);
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], backlog["key"]);
    store.resolve_question("ui", question["key"].as_str().unwrap(), "B", Some("Better fit"), 1, false).unwrap();
    let next = store.next_ready_task(&board, None).unwrap();
    assert_eq!(next["key"], task["key"]);
    assert_eq!(next["dispatch_action"], "resume");
    let first_page = store.task_session_briefing(key, 1, None).unwrap();
    assert_eq!(first_page["has_more"], true);
    let second_page = store.task_session_briefing(key, 1, first_page["cursor"].as_i64()).unwrap();
    assert!(second_page["cursor"].as_i64().unwrap() > first_page["cursor"].as_i64().unwrap());
    let briefing = store.task_session_briefing(key, 100, None).unwrap();
    assert!(briefing["events"].as_array().unwrap().iter().any(|event| event["type"] == "question_answered" && event["after"]["answer"] == "B"));
    let cursor = briefing["cursor"].as_i64().unwrap();
    store.resume_task_session("orchestrator", key).unwrap();
    store.acknowledge_task_session("orchestrator", key, cursor).unwrap();
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], backlog["key"]);
    assert_eq!(store.view_card(key).unwrap()["version"], 4);
}

#[test]
fn paused_task_waiting_for_question_wakes_even_with_another_blocker() {
    let (_dir, store, board) = setup();
    let blocker = store.create_card("cli", &board, "Prerequisite", "", "P2", None).unwrap();
    let task = store.create_card("cli", &board, "Waiting work", "", "P2", None).unwrap();
    let key = task["key"].as_str().unwrap();
    store.claim_card("agent:one", key, 1).unwrap();
    store.move_card("agent:one", key, "In Progress", None, 2).unwrap();
    store.attach_task_session("orchestrator", key, "codex", "session-2").unwrap();
    let question = store.raise_question("agent:one", &board, "business", "Proceed?", "", &["Yes".into(), "No".into()], "Yes", "Needed", &[key.to_owned()]).unwrap();
    store.add_dependency("agent:one", key, blocker["key"].as_str().unwrap(), 4).unwrap();
    store.pause_task_session("orchestrator", key, Some(question["key"].as_str().unwrap())).unwrap();
    assert_eq!(store.resume_task_session("orchestrator", key).unwrap_err().code, "session_not_ready");
    store.resolve_question("ui", question["key"].as_str().unwrap(), "No", None, 1, false).unwrap();
    assert_eq!(store.view_card(key).unwrap()["blocked"], true);
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], task["key"]);
    assert_eq!(store.next_ready_task(&board, None).unwrap()["dispatch_action"], "resume");
    store.resume_task_session("orchestrator", key).unwrap();
    let version = store.view_card(key).unwrap()["version"].as_i64().unwrap();
    store.release_card("agent:one", key, version, None).unwrap();
    assert!(store.view_card(key).unwrap()["session"].is_null());
}

#[test]
fn briefing_keeps_partial_question_answers_until_other_blockers_clear() {
    let (_dir, store, board) = setup();
    let prerequisite = store.create_card("cli", &board, "Prerequisite", "", "P2", None).unwrap();
    let task = store.create_card("cli", &board, "Waiting work", "", "P2", None).unwrap();
    let key = task["key"].as_str().unwrap();
    store.claim_card("agent:one", key, 1).unwrap();
    store.move_card("agent:one", key, "In Progress", None, 2).unwrap();
    store.attach_task_session("orchestrator", key, "codex", "session-3").unwrap();
    let question = store.raise_question("agent:one", &board, "business", "Proceed?", "", &["Yes".into(), "No".into()], "Yes", "Needed", &[key.to_owned()]).unwrap();
    store.add_dependency("agent:one", key, prerequisite["key"].as_str().unwrap(), 4).unwrap();
    store.pause_task_session("orchestrator", key, None).unwrap();
    store.resolve_question("ui", question["key"].as_str().unwrap(), "Yes", None, 1, false).unwrap();
    assert_eq!(store.view_card(key).unwrap()["blocked"], true);
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], prerequisite["key"]);
    store.claim_card("agent:two", prerequisite["key"].as_str().unwrap(), 1).unwrap();
    store.move_card("agent:two", prerequisite["key"].as_str().unwrap(), "In Progress", None, 2).unwrap();
    store.move_card("agent:two", prerequisite["key"].as_str().unwrap(), "Done", None, 3).unwrap();
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], task["key"]);
    let briefing = store.task_session_briefing(key, 100, None).unwrap();
    assert!(briefing["events"].as_array().unwrap().iter().any(|event| event["type"] == "question_answered"));
    assert!(briefing["events"].as_array().unwrap().iter().any(|event| event["type"] == "card_moved" && event["entity_id"] == prerequisite["id"]));
}

#[test]
fn new_board_has_only_work_lanes() {
    let (_dir, store, board) = setup();
    let view = store.view_board(&board).unwrap();
    let lanes = view["lanes"].as_array().unwrap();
    assert_eq!(
        lanes
            .iter()
            .map(|lane| lane["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Backlog", "In Progress", "Done"]
    );
    let events = store.poll_events(0, 10).unwrap();
    assert_eq!(events["events"][0]["effects"].as_array().unwrap().len(), 4);
}

#[test]
fn typed_card_and_question_views_preserve_public_json() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Task", "", "P2", None)
        .unwrap();
    let card_key = card["key"].as_str().unwrap();
    let typed_card = store.read_card(card_key).unwrap();
    assert_eq!(typed_card.id.to_string(), card["id"].as_str().unwrap());
    assert_eq!(serde_json::to_value(&typed_card).unwrap(), card);

    let question = store
        .raise_question(
            "cli",
            &board,
            "business",
            "Choose?",
            "",
            &["A".into(), "B".into()],
            "A",
            "Simpler.",
            &[],
        )
        .unwrap();
    let typed_question = store
        .read_question(question["key"].as_str().unwrap())
        .unwrap();
    assert_eq!(typed_question.kind, QuestionKind::Business);
    assert_eq!(serde_json::to_value(&typed_question).unwrap(), question);
}

#[test]
fn validated_domain_values_reject_invalid_inputs() {
    assert!(Progress::try_from(-1).is_err());
    assert!(Progress::try_from(101).is_err());
    assert!(Version::try_from(0).is_err());
    let options = QuestionOptions::new(&["Yes".into(), "No".into()]).unwrap();
    assert_eq!(&*options, &["Yes", "No", "Other"]);
    assert!(QuestionOutcome::answer(&options, "Other", None).is_err());
    assert!(QuestionOutcome::answer(&options, "Maybe", Some("because")).is_err());
    assert!(QuestionOutcome::answer(&options, "Other", Some("Different answer")).is_ok());
}

#[test]
fn project_context_links_board_repository_documents_and_stakeholder() {
    let (_dir, store, board) = setup();
    let project = store
        .create_project("agent", "Workyard", "Agent workspace")
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    let board_after = store
        .attach_board_to_project("agent", &board, project_id, 1)
        .unwrap();
    assert_eq!(board_after["project_id"], project_id);
    let repo = store
        .add_project_repository("agent", project_id, "/tmp/workyard", "main", None)
        .unwrap();
    let duplicate = store
        .add_project_repository("agent", project_id, "/tmp/another-repo", "main", None)
        .unwrap_err();
    assert_eq!(duplicate.code, "repository_exists");
    let repo_id = repo["id"].as_str().unwrap();
    store
        .add_project_document(
            "agent",
            project_id,
            repo_id,
            "docs/ADR-1.md",
            "adr",
            "Storage decision",
        )
        .unwrap();
    let person = store
        .add_project_stakeholder("agent", project_id, "Alex", "Sponsor", "alex@example.test")
        .unwrap();
    let question = store
        .raise_question(
            "agent",
            &board,
            "business",
            "Launch?",
            "",
            &["Yes".into(), "No".into()],
            "Yes",
            "It is ready.",
            &[],
        )
        .unwrap();
    let assigned = store
        .assign_question_stakeholder(
            "agent",
            question["key"].as_str().unwrap(),
            person["id"].as_str().unwrap(),
            1,
        )
        .unwrap();
    assert_eq!(assigned["stakeholder_id"], person["id"]);
    let engineering = store
        .raise_question(
            "agent",
            &board,
            "engineering",
            "Which storage layout?",
            "",
            &["Single database".into(), "Per-board database".into()],
            "Single database",
            "Keep local setup simple.",
            &[],
        )
        .unwrap();
    let engineering_assigned = store
        .assign_question_stakeholder(
            "agent",
            engineering["key"].as_str().unwrap(),
            person["id"].as_str().unwrap(),
            1,
        )
        .unwrap();
    assert_eq!(engineering_assigned["stakeholder_id"], person["id"]);
    let unassigned = store
        .unassign_question_stakeholder(
            "agent",
            engineering["key"].as_str().unwrap(),
            engineering_assigned["version"].as_i64().unwrap(),
        )
        .unwrap();
    assert!(unassigned["stakeholder_id"].is_null());
    assert_eq!(unassigned["status"], "open");
    assert!(store
        .list_questions(Some(&board), None)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == unassigned["id"])
        .unwrap()["stakeholder"]
        .is_null());
    let view = store.view_project(project_id).unwrap();
    assert_eq!(view["boards"].as_array().unwrap().len(), 1);
    assert_eq!(view["repositories"].as_array().unwrap().len(), 1);
    assert_eq!(view["documents"].as_array().unwrap().len(), 1);
    assert_eq!(view["stakeholders"].as_array().unwrap().len(), 1);
    assert_eq!(store.list_projects().unwrap()[0]["board_count"], 1);
}

#[test]
fn v6_event_history_survives_project_migration() {
    let dir = TempDir::new();
    let path = dir.path().join("v6.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    for migration in [
        include_str!("../migrations/001.sql"),
        include_str!("../migrations/002.sql"),
        include_str!("../migrations/003.sql"),
        include_str!("../migrations/004.sql"),
        include_str!("../migrations/005.sql"),
        include_str!("../migrations/006.sql"),
    ] {
        db.execute_batch(migration).unwrap();
    }
    db.execute("INSERT INTO boards(id,key,name,created_at) VALUES ('board-1','B-1','Existing','2026-01-01')", []).unwrap();
    db.execute("INSERT INTO events(ts,actor,type,board_id,entity_type,entity_id) VALUES ('2026-01-01','agent','board_created','board-1','board','board-1')", []).unwrap();
    db.execute_batch("PRAGMA user_version = 6").unwrap();
    drop(db);
    let store = Store::new(path);
    assert_eq!(store.latest_event_id().unwrap(), 1);
    assert_eq!(
        store.poll_events(0, 10).unwrap()["events"][0]["board_id"],
        "board-1"
    );
    store.create_project("agent", "New project", "").unwrap();
    let events = store.poll_events(1, 10).unwrap();
    assert_eq!(events["events"][0]["id"], 2);
    assert!(events["events"][0]["board_id"].is_null());
}

#[test]
fn v7_database_gets_phases_and_preserves_existing_work() {
    let dir = TempDir::new();
    let path = dir.path().join("v7.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    for migration in [
        include_str!("../migrations/001.sql"),
        include_str!("../migrations/002.sql"),
        include_str!("../migrations/003.sql"),
        include_str!("../migrations/004.sql"),
        include_str!("../migrations/005.sql"),
        include_str!("../migrations/006.sql"),
        include_str!("../migrations/007.sql"),
    ] {
        db.execute_batch(migration).unwrap();
    }
    db.execute("INSERT INTO boards(id,key,name,created_at) VALUES ('board-1','B-1','Existing','2026-01-01')",[]).unwrap();
    db.execute("INSERT INTO lanes(id,board_id,name,position,kind) VALUES ('lane-1','board-1','Backlog',0,'backlog')",[]).unwrap();
    db.execute("INSERT INTO cards(id,key,board_id,lane_id,position,title,created_at,updated_at) VALUES ('card-1','C-1','board-1','lane-1',0,'Existing card','2026-01-01','2026-01-01')",[]).unwrap();
    db.execute_batch("PRAGMA user_version = 7").unwrap();
    drop(db);
    let store = Store::new(path);
    let board = store.view_board("B-1").unwrap();
    assert_eq!(board["phases"][0]["status"], "current");
    assert_eq!(
        store.view_card("C-1").unwrap()["phase_id"],
        board["current_phase_id"]
    );
    assert_eq!(
        store
            .open()
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        21
    );
}

#[test]
fn finish_dependency_and_review_approval_gate_completion_without_blocking_work() {
    let (_dir, store, board) = setup();
    let project = store.create_project("agent", "Code", "").unwrap();
    let project_id = project["id"].as_str().unwrap();
    store
        .attach_board_to_project("agent", &board, project_id, 1)
        .unwrap();
    let repo = store
        .add_project_repository("agent", project_id, "/tmp/code", "main", None)
        .unwrap();
    let first = store
        .create_card("agent", &board, "First", "", "P2", None)
        .unwrap();
    let second = store
        .create_card("agent", &board, "Second", "", "P2", None)
        .unwrap();
    let first_key = first["key"].as_str().unwrap();
    let second_key = second["key"].as_str().unwrap();
    store
        .add_dependency_scoped("agent", second_key, first_key, "finish", 1)
        .unwrap();
    let pending = store.view_card(second_key).unwrap();
    assert_eq!(pending["blocked"], false);
    assert_eq!(pending["can_finish"], false);
    let active = store
        .move_card("agent", second_key, "In Progress", None, 2)
        .unwrap();
    assert_eq!(active["version"], 3);
    assert_eq!(
        store
            .move_card("agent", second_key, "Done", None, 3)
            .unwrap_err()
            .code,
        "finish_blocked"
    );
    store
        .move_card("agent", first_key, "Done", None, 1)
        .unwrap();
    assert_eq!(store.view_card(second_key).unwrap()["can_finish"], true);
    let change = store
        .link_card_change(
            "agent",
            second_key,
            repo["id"].as_str().unwrap(),
            "feature/second",
            None,
            "abc123",
            false,
            3,
        )
        .unwrap();
    assert_eq!(store.view_card(second_key).unwrap()["can_finish"], true);
    let change = store
        .set_change_review_required("agent", change["id"].as_str().unwrap(), true, 1, 4)
        .unwrap();
    assert_eq!(store.view_card(second_key).unwrap()["can_finish"], false);
    let cursor = store.latest_event_id().unwrap();
    let approved = store
        .approve_card_change("reviewer", change["id"].as_str().unwrap(), "Alex", 2)
        .unwrap();
    assert_eq!(store.view_card(second_key).unwrap()["can_finish"], true);
    let events = store.poll_events(cursor, 10).unwrap();
    assert_eq!(events["events"].as_array().unwrap().len(), 1);
    assert!(
        events["events"][0]["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["type"] == "card_can_finish")
    );
    store
        .update_card_change(
            "agent",
            change["id"].as_str().unwrap(),
            "def456",
            approved["version"].as_i64().unwrap(),
        )
        .unwrap();
    assert_eq!(store.view_card(second_key).unwrap()["can_finish"], false);
}

#[test]
fn start_dependency_cannot_be_bypassed_by_moving_directly_to_done() {
    let (_dir, store, board) = setup();
    let prerequisite = store
        .create_card("agent", &board, "Prerequisite", "", "P2", None)
        .unwrap();
    let dependent = store
        .create_card("agent", &board, "Dependent", "", "P2", None)
        .unwrap();
    let source = dependent["key"].as_str().unwrap();
    store
        .add_dependency("agent", source, prerequisite["key"].as_str().unwrap(), 1)
        .unwrap();
    assert_eq!(store.view_card(source).unwrap()["can_finish"], false);
    assert_eq!(
        store
            .move_card("agent", source, "Done", None, 2)
            .unwrap_err()
            .code,
        "finish_blocked"
    );
    let dependency_id = store.view_card(source).unwrap()["dependencies"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .set_dependency_scope("agent", &dependency_id, "finish", 2)
        .unwrap();
    let card = store.view_card(source).unwrap();
    assert_eq!(card["blocked"], false);
    assert_eq!(card["can_finish"], false);
    assert_eq!(card["dependencies"][0]["scope"], "finish");
    store
        .move_card("agent", source, "In Progress", None, 3)
        .unwrap();
}

#[test]
fn later_phase_work_is_offered_after_current_but_never_before() {
    let (_dir, store, board) = setup();
    store.add_phase("agent", &board, "Second").unwrap();
    store.add_phase("agent", &board, "Third").unwrap();
    let mk = |title: &str, phase: Option<&str>| {
        store
            .create_card_in_phase("agent", &board, title, "", "P2", None, "", phase)
            .unwrap()
    };
    let third = mk("Third task", Some("Third"));
    let second = mk("Second task", Some("Second"));
    let current = mk("Current task", None);
    assert_eq!(store.orchestrator_status(&board).unwrap()["held_back_by_phase"], 0);
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], current["key"]);
    store.claim_card("agent", current["key"].as_str().unwrap(), 1).unwrap();
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], second["key"]);
    store.claim_card("agent", second["key"].as_str().unwrap(), 1).unwrap();
    // Two phases ahead is offered, and can be claimed.
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], third["key"]);
    store.claim_card("agent", third["key"].as_str().unwrap(), 1).unwrap();
}

#[test]
fn phase_window_limits_what_is_offered_and_is_reported() {
    let (_dir, store, board) = setup();
    store.add_phase("agent", &board, "Second").unwrap();
    store.add_phase("agent", &board, "Third").unwrap();
    let mk = |title: &str, phase: Option<&str>| {
        store
            .create_card_in_phase("agent", &board, title, "", "P2", None, "", phase)
            .unwrap()
    };
    let current = mk("Current task", None);
    let second = mk("Second task", Some("Second"));
    let third = mk("Third task", Some("Third"));
    let window = store.set_board_phase_window("agent", &board, Some(1)).unwrap();
    assert_eq!(window["phase_window"], 1);
    let status = store.orchestrator_status(&board).unwrap();
    assert_eq!(status["held_back_by_phase"], 2);
    assert_eq!(status["held_back_cards"], json!([second["key"], third["key"]]));
    assert_eq!(
        store.claim_card("agent", second["key"].as_str().unwrap(), 1).unwrap_err().code,
        "current_phase_queue_not_empty"
    );
    store.claim_card("agent", current["key"].as_str().unwrap(), 1).unwrap();
    assert_eq!(store.next_ready_task(&board, None).unwrap()["key"], second["key"]);
    assert_eq!(store.orchestrator_status(&board).unwrap()["held_back_by_phase"], 1);
    assert_eq!(
        store.claim_card("agent", third["key"].as_str().unwrap(), 1).unwrap_err().code,
        "phase_planned"
    );
    store.set_board_phase_window("agent", &board, None).unwrap();
    assert_eq!(store.orchestrator_status(&board).unwrap()["held_back_by_phase"], 0);
    assert_eq!(store.set_board_phase_window("agent", &board, Some(0)).unwrap_err().code, "validation_error");
    assert!(workyard_core::parse_phase_window("all").unwrap().is_none());
    assert_eq!(workyard_core::parse_phase_window("2").unwrap(), Some(2));
    assert!(workyard_core::parse_phase_window("x").is_err());
}

#[test]
fn next_phase_work_starts_only_when_current_queue_is_exhausted() {
    let (_dir, store, board) = setup();
    store.set_board_phase_window("agent", &board, Some(1)).unwrap();
    let future = store.add_phase("agent", &board, "Integration").unwrap();
    let current = store
        .create_card("agent", &board, "Current task", "", "P2", None)
        .unwrap();
    let later = store
        .create_card_in_phase(
            "agent",
            &board,
            "Later task",
            "",
            "P2",
            None,
            "",
            Some("Integration"),
        )
        .unwrap();
    assert_eq!(
        store.next_ready_task(&board, None).unwrap()["key"],
        current["key"]
    );
    assert_eq!(
        store
            .claim_card("agent", later["key"].as_str().unwrap(), 1)
            .unwrap_err()
            .code,
        "current_phase_queue_not_empty"
    );
    store
        .claim_card("agent", current["key"].as_str().unwrap(), 1)
        .unwrap();
    assert_eq!(
        store.next_ready_task(&board, None).unwrap()["key"],
        later["key"]
    );
    store
        .claim_card("agent", later["key"].as_str().unwrap(), 1)
        .unwrap();
    store
        .move_card(
            "agent",
            later["key"].as_str().unwrap(),
            "In Progress",
            None,
            2,
        )
        .unwrap();
    let new_current = store
        .create_card("agent", &board, "New current task", "", "P2", None)
        .unwrap();
    assert_eq!(
        store.next_ready_task(&board, None).unwrap()["key"],
        new_current["key"]
    );
    store
        .move_card("agent", later["key"].as_str().unwrap(), "Done", None, 3)
        .unwrap();
    assert_eq!(
        store
            .add_dependency(
                "agent",
                new_current["key"].as_str().unwrap(),
                later["key"].as_str().unwrap(),
                1
            )
            .unwrap_err()
            .code,
        "future_dependency"
    );
    assert_eq!(future["name"], "Integration");
}

#[test]
fn cancelled_prerequisite_stops_blocking_and_stays_visible() {
    let (_dir, store, board) = setup();
    store.add_phase("agent", &board, "Next").unwrap();
    let obsolete = store
        .create_card("agent", &board, "Obsolete", "", "P2", None)
        .unwrap();
    let later = store
        .create_card_in_phase("agent", &board, "Dependent", "", "P2", None, "", Some("Next"))
        .unwrap();
    let (obs, dep) = (obsolete["key"].as_str().unwrap(), later["key"].as_str().unwrap());
    store.add_dependency("agent", dep, obs, 1).unwrap();
    assert_eq!(store.view_card(dep).unwrap()["blocked"], true);
    let before_cancel = store.latest_event_id().unwrap();
    let cancelled = store.cancel_card("agent", obs, "No longer needed", 1).unwrap();
    assert_eq!(store.latest_event_id().unwrap(), before_cancel + 1);
    assert_eq!(cancelled["cancellation_reason"], "No longer needed");
    let event = store.poll_events(before_cancel, 10).unwrap();
    let effects = event["events"][0]["effects"].as_array().unwrap();
    assert!(effects.iter().any(|e| e["type"] == "card_became_ready" && e["card_key"] == dep));
    let view = store.view_card(dep).unwrap();
    assert_eq!(view["blocked"], false);
    assert_eq!(view["can_finish"], true);
    assert_eq!(view["blockers"].as_array().unwrap().len(), 0);
    assert_eq!(view["cancelled_prerequisites"][0]["key"], obs);
    assert_eq!(view["cancelled_prerequisites"][0]["cancelled"], true);
    // The phase can close without repairing the link.
    let board_after = store.advance_phase("agent", &board).unwrap();
    assert_eq!(
        board_after["current_phase_id"],
        store.list_phases(&board).unwrap()[1]["id"]
    );
    assert!(store.list_phases(&board).unwrap()[0]["closed_at"].is_string());
    assert_eq!(
        store
            .create_card_in_phase(
                "agent",
                &board,
                "Too late",
                "",
                "P2",
                None,
                "",
                Some("Current work")
            )
            .unwrap_err()
            .code,
        "phase_closed"
    );
    assert_eq!(
        store
            .claim_card("agent", obsolete["key"].as_str().unwrap(), 2)
            .unwrap_err()
            .code,
        "card_cancelled"
    );
}

#[test]
fn phase_description_and_explicit_final_completion() {
    let (_dir, store, board) = setup();
    let described = store
        .describe_phase("agent", &board, "Current work", "Build the command foundation", 1)
        .unwrap();
    assert_eq!(described["description"], "Build the command foundation");
    assert_eq!(described["version"], 2);
    assert_eq!(
        store.describe_phase("agent", &board, "Current work", "Stale update", 1)
            .unwrap_err().code,
        "version_conflict"
    );
    let card = store.create_card("agent", &board, "Finish setup", "", "P2", None).unwrap();
    let before_failed = store.latest_event_id().unwrap();
    assert_eq!(store.complete_phase("agent", &board).unwrap_err().code, "phase_incomplete");
    assert_eq!(store.latest_event_id().unwrap(), before_failed);
    store.move_card("agent", card["key"].as_str().unwrap(), "Done", None, 1).unwrap();
    let before_completed = store.latest_event_id().unwrap();
    let completed = store.complete_phase("agent", &board).unwrap();
    assert_eq!(store.latest_event_id().unwrap(), before_completed + 1);
    assert!(completed["closed_at"].is_string());
    assert_eq!(store.list_phases(&board).unwrap()[0]["status"], "closed");
    assert_eq!(store.create_card("agent", &board, "Too late", "", "P2", None).unwrap_err().code, "phase_closed");
    store.add_phase_with_description("agent", &board, "Next", "New phase goal").unwrap();
    store.advance_phase("agent", &board).unwrap();
    assert_eq!(store.list_phases(&board).unwrap()[1]["status"], "current");
}

#[test]
fn reassigning_work_cannot_reverse_dependency_order() {
    let (_dir, store, board) = setup();
    store.add_phase("agent", &board, "Next").unwrap();
    let prerequisite = store
        .create_card("agent", &board, "Prerequisite", "", "P2", None)
        .unwrap();
    let dependent = store
        .create_card("agent", &board, "Dependent", "", "P2", None)
        .unwrap();
    store
        .add_dependency(
            "agent",
            dependent["key"].as_str().unwrap(),
            prerequisite["key"].as_str().unwrap(),
            1,
        )
        .unwrap();
    assert_eq!(
        store
            .assign_phase("agent", prerequisite["key"].as_str().unwrap(), "Next", 1)
            .unwrap_err()
            .code,
        "future_dependency"
    );
    store
        .assign_phase("agent", dependent["key"].as_str().unwrap(), "Next", 2)
        .unwrap();
    store
        .assign_phase("agent", prerequisite["key"].as_str().unwrap(), "Next", 1)
        .unwrap();
}

#[test]
fn open_questions_hold_phase_and_question_assignment_preserves_dependency_order() {
    let (_dir, store, board) = setup();
    store.add_phase("agent", &board, "Next").unwrap();
    let card = store
        .create_card("agent", &board, "Work", "", "P2", None)
        .unwrap();
    let question = store
        .raise_question(
            "agent",
            &board,
            "engineering",
            "Decision?",
            "",
            &["A".into(), "B".into()],
            "A",
            "Simple choice",
            &[],
        )
        .unwrap();
    store
        .move_card("agent", card["key"].as_str().unwrap(), "Done", None, 1)
        .unwrap();
    assert_eq!(
        store.advance_phase("agent", &board).unwrap_err().code,
        "phase_incomplete"
    );
    let future = store
        .create_card_in_phase("agent", &board, "Future", "", "P2", None, "", Some("Next"))
        .unwrap();
    store
        .add_dependency(
            "agent",
            future["key"].as_str().unwrap(),
            question["key"].as_str().unwrap(),
            1,
        )
        .unwrap();
    store
        .resolve_question("ui", question["key"].as_str().unwrap(), "A", None, 1, false)
        .unwrap();
    store.advance_phase("agent", &board).unwrap();
    assert_eq!(
        store
            .assign_phase("agent", question["key"].as_str().unwrap(), "Next", 2)
            .unwrap_err()
            .code,
        "phase_closed"
    );
}

#[test]
fn notes_on_claimed_cards_do_not_change_owner_or_version() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("agent:one", &board, "Owned work", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    store.claim_card("agent:one", key, 1).unwrap();
    let before = store.view_card(key).unwrap();
    let previous_events = store.latest_event_id().unwrap();
    let note = store
        .add_card_note("ui", key, "Please check the edge case")
        .unwrap();
    assert_eq!(note["actor"], "ui");
    let after = store.view_card(key).unwrap();
    assert_eq!(after["owner_actor"], "agent:one");
    assert_eq!(after["version"], before["version"]);
    assert_eq!(after["notes"][0]["body"], "Please check the edge case");
    let events = store.poll_events(previous_events, 10).unwrap();
    assert_eq!(events["events"].as_array().unwrap().len(), 1);
    assert_eq!(events["events"][0]["type"], "card_note_added");
    assert_eq!(
        store.add_card_note("ui", key, "  ").unwrap_err().code,
        "validation_error"
    );
    assert_eq!(store.latest_event_id().unwrap(), previous_events + 1);
}

#[test]
fn existing_v1_database_migrates_to_agent_types_and_notes() {
    let dir = TempDir::new();
    let path = dir.path().join("existing.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    db.execute(
        "INSERT INTO schema_version(version,applied_at) VALUES (1,'2026-01-01')",
        [],
    )
    .unwrap();
    db.execute_batch("PRAGMA user_version = 1").unwrap();
    drop(db);
    let store = Store::new(path);
    let board = store.create_board("agent:one", "Existing", "").unwrap();
    let card = store
        .create_card(
            "agent:one",
            board["key"].as_str().unwrap(),
            "Task",
            "",
            "P2",
            None,
        )
        .unwrap();
    store
        .add_card_note("ui", card["key"].as_str().unwrap(), "Migrated")
        .unwrap();
    let db = store.open().unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 21);
}

#[test]
fn existing_questions_get_options_without_changing_history() {
    let dir = TempDir::new();
    let path = dir.path().join("existing-questions.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/002.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/003.sql"))
        .unwrap();
    db.execute("INSERT INTO boards(id,key,name,created_at) VALUES ('board-1','B-1','Existing','2026-01-01')", []).unwrap();
    db.execute("INSERT INTO questions(id,key,board_id,kind,title,created_at) VALUES ('question-1','Q-1','board-1','business','Existing decision','2026-01-01')", []).unwrap();
    db.execute_batch("PRAGMA user_version = 3").unwrap();
    drop(db);

    let store = Store::new(path);
    let questions = store.list_questions(Some("B-1"), None).unwrap();
    assert_eq!(
        questions[0]["options"],
        serde_json::json!(["Yes", "No", "Other"])
    );
    assert_eq!(questions[0]["version"], 1);
    assert_eq!(questions[0]["recommended_option"], "");
    assert_eq!(questions[0]["recommendation_reason"], "");
    assert_eq!(store.latest_event_id().unwrap(), 0);
}

#[test]
fn existing_question_choices_gain_other_without_changing_decisions() {
    let dir = TempDir::new();
    let path = dir.path().join("existing-choices.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/002.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/003.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/004.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/005.sql"))
        .unwrap();
    db.execute("INSERT INTO boards(id,key,name,created_at) VALUES ('board-1','B-1','Existing','2026-01-01')", []).unwrap();
    db.execute("INSERT INTO questions(id,key,board_id,kind,title,created_at,options_json,recommended_option,recommendation_reason) VALUES ('question-1','Q-1','board-1','business','Existing decision','2026-01-01','[\"A\",\"B\"]','A','It is simpler.')", []).unwrap();
    db.execute("INSERT INTO questions(id,key,board_id,kind,title,created_at,options_json,recommended_option,recommendation_reason) VALUES ('question-2','Q-2','board-1','engineering','Earlier escape hatch','2026-01-02','[\"A\",\"other\",\"B\"]','other','It needs another path.')", []).unwrap();
    db.execute_batch("PRAGMA user_version = 5").unwrap();
    drop(db);

    let store = Store::new(path);
    let questions = store.list_questions(Some("B-1"), None).unwrap();
    assert_eq!(
        questions[0]["options"],
        serde_json::json!(["A", "B", "Other"])
    );
    assert_eq!(questions[0]["recommended_option"], "A");
    assert_eq!(
        questions[1]["options"],
        serde_json::json!(["A", "B", "Other"])
    );
    assert_eq!(questions[1]["recommended_option"], "Other");
    assert_eq!(questions[0]["version"], 1);
    assert_eq!(questions[1]["version"], 1);
    assert_eq!(store.latest_event_id().unwrap(), 0);
}

#[test]
fn agent_type_is_distinct_from_owner_and_selects_ready_work() {
    let (_dir, store, board) = setup();
    let typed = store
        .create_card_with_type(
            "agent:planner",
            &board,
            "Implement UI",
            "",
            "P1",
            None,
            "Frontend",
        )
        .unwrap();
    let other = store
        .create_card_with_type(
            "agent:planner",
            &board,
            "Write guide",
            "",
            "P2",
            None,
            "Documentation",
        )
        .unwrap();
    assert_eq!(typed["agent_type"], "Frontend");
    assert!(typed["owner_actor"].is_null());
    assert_eq!(
        store
            .next_ready_task(&board, Some("Documentation"))
            .unwrap()["key"],
        other["key"]
    );
    let updated = store
        .update_card(
            "agent:planner",
            typed["key"].as_str().unwrap(),
            1,
            None,
            None,
            None,
            Some("Design"),
        )
        .unwrap();
    assert_eq!(updated["agent_type"], "Design");
    assert_eq!(updated["version"], 2);
    assert!(
        store
            .next_ready_task(&board, Some("Frontend"))
            .unwrap()
            .is_null()
    );
    assert_eq!(
        store.next_ready_task(&board, Some("Design")).unwrap()["key"],
        typed["key"]
    );
}

#[test]
fn next_ready_task_uses_distinct_unresolved_downstream_cards() {
    let (_dir, store, board) = setup();
    let first = store
        .create_card("cli", &board, "First", "", "P0", None)
        .unwrap();
    let bottleneck = store
        .create_card("cli", &board, "Bottleneck", "", "P3", None)
        .unwrap();
    let left = store
        .create_card("cli", &board, "Left", "", "P2", None)
        .unwrap();
    let right = store
        .create_card("cli", &board, "Right", "", "P2", None)
        .unwrap();
    let end = store
        .create_card("cli", &board, "End", "", "P2", None)
        .unwrap();
    fn key(card: &serde_json::Value) -> &str {
        card["key"].as_str().unwrap()
    }
    store
        .add_dependency("cli", key(&left), key(&bottleneck), 1)
        .unwrap();
    store
        .add_dependency("cli", key(&right), key(&bottleneck), 1)
        .unwrap();
    store
        .add_dependency("cli", key(&end), key(&left), 1)
        .unwrap();
    store
        .add_dependency("cli", key(&end), key(&right), 2)
        .unwrap();
    assert_eq!(
        store.view_card(key(&bottleneck)).unwrap()["blocking_impact"],
        3
    );
    assert_eq!(
        store.next_ready_task(&board, None).unwrap()["key"],
        bottleneck["key"]
    );
    store
        .move_card("cli", key(&bottleneck), "Done", None, 1)
        .unwrap();
    assert_eq!(
        store.view_card(key(&bottleneck)).unwrap()["blocking_impact"],
        0
    );
    assert_eq!(
        store.next_ready_task(&board, None).unwrap()["key"],
        left["key"]
    );
    assert_eq!(store.view_card(key(&first)).unwrap()["blocking_impact"], 0);
}

#[test]
fn question_resolution_has_one_event_with_ready_effect() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Build core", "", "P2", None)
        .unwrap();
    let card_key = card["key"].as_str().unwrap();
    let question = store
        .raise_question(
            "cli",
            &board,
            "engineering",
            "Which DB?",
            "",
            &["SQLite".into(), "PostgreSQL".into()],
            "SQLite",
            "It is already local and keeps setup simple.",
            &[card_key.to_owned()],
        )
        .unwrap();
    assert_eq!(store.view_card(card_key).unwrap()["blocked"], true);
    let error = store
        .move_card("cli", card_key, "In Progress", None, 2)
        .unwrap_err();
    assert_eq!(error.code, "blocked");
    let answered = store
        .resolve_question(
            "cli",
            question["key"].as_str().unwrap(),
            "SQLite",
            None,
            1,
            false,
        )
        .unwrap();
    assert_eq!(answered["status"], "answered");
    assert_eq!(store.view_card(card_key).unwrap()["blocked"], false);
    let events = store.poll_events(0, 100).unwrap();
    assert_eq!(events["events"].as_array().unwrap().len(), 4);
    let last = events["events"].as_array().unwrap().last().unwrap();
    assert_eq!(last["type"], "question_answered");
    assert_eq!(last["effects"].as_array().unwrap().len(), 3);
    assert_eq!(last["effects"][0]["type"], "card_became_ready");
    assert_eq!(last["effects"][1]["type"], "card_can_finish");
    assert_eq!(last["effects"][2]["type"], "question_resolved_for_card");
}

#[test]
fn questions_require_distinct_options_and_answers_select_one() {
    let (_dir, store, board) = setup();
    let initial_events = store.latest_event_id().unwrap();
    assert_eq!(
        store
            .raise_question(
                "cli",
                &board,
                "business",
                "Choose?",
                "",
                &[],
                "A",
                "Pick A.",
                &[]
            )
            .unwrap_err()
            .code,
        "validation_error"
    );
    assert_eq!(
        store
            .raise_question(
                "cli",
                &board,
                "business",
                "Choose?",
                "",
                &["Yes".into(), " yes ".into()],
                "Yes",
                "Pick yes.",
                &[]
            )
            .unwrap_err()
            .code,
        "validation_error"
    );
    assert_eq!(store.latest_event_id().unwrap(), initial_events);

    assert_eq!(
        store
            .raise_question(
                "cli",
                &board,
                "business",
                "Choose?",
                "",
                &["A".into(), "B".into()],
                "C",
                "It is simpler.",
                &[]
            )
            .unwrap_err()
            .code,
        "validation_error"
    );
    assert_eq!(store.latest_event_id().unwrap(), initial_events);

    assert_eq!(
        store
            .raise_question(
                "cli",
                &board,
                "business",
                "Choose?",
                "",
                &["A".into(), "B".into(), "Other".into()],
                "A",
                "It is simpler.",
                &[]
            )
            .unwrap_err()
            .code,
        "validation_error"
    );
    assert_eq!(store.latest_event_id().unwrap(), initial_events);

    let question = store
        .raise_question(
            "cli",
            &board,
            "business",
            "Choose?",
            "",
            &["A".into(), "B".into()],
            "A",
            "It is simpler.",
            &[],
        )
        .unwrap();
    let key = question["key"].as_str().unwrap();
    assert_eq!(question["options"], serde_json::json!(["A", "B", "Other"]));
    assert_eq!(question["recommended_option"], "A");
    assert_eq!(question["recommendation_reason"], "It is simpler.");
    assert_eq!(
        store
            .resolve_question("ui", key, "C", None, 1, false)
            .unwrap_err()
            .code,
        "validation_error"
    );
    assert_eq!(store.latest_event_id().unwrap(), initial_events + 1);
    assert_eq!(
        store
            .update_question_options(
                "cli",
                key,
                1,
                &["A".into(), "B".into(), " other ".into()],
                "A",
                "It is simpler."
            )
            .unwrap_err()
            .code,
        "validation_error"
    );
    assert_eq!(store.latest_event_id().unwrap(), initial_events + 1);
    let updated = store
        .update_question_options(
            "cli",
            key,
            1,
            &["A".into(), "B".into()],
            "A",
            "It is simpler.",
        )
        .unwrap();
    assert_eq!(updated["version"], 2);
    assert_eq!(updated["options"], serde_json::json!(["A", "B", "Other"]));
    assert_eq!(
        store
            .update_question_options(
                "cli",
                key,
                1,
                &["A".into(), "B".into()],
                "A",
                "It is simpler."
            )
            .unwrap_err()
            .code,
        "version_conflict"
    );
    assert_eq!(
        store
            .resolve_question("ui", key, "Other", None, 2, false)
            .unwrap_err()
            .code,
        "validation_error"
    );
    let answered = store
        .resolve_question("ui", key, "Other", Some("Use a third approach"), 2, false)
        .unwrap();
    assert_eq!(answered["answer"], "Other");
    assert_eq!(answered["answer_note"], "Use a third approach");
    assert_eq!(store.latest_event_id().unwrap(), initial_events + 3);
}

#[test]
fn claimed_card_rejects_other_actor_and_stale_owner_version() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Owned work", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    let claimed = store.claim_card("agent:one", key, 1).unwrap();
    assert_eq!(claimed["version"], 2);
    let error = store
        .update_progress("agent:two", key, 20, Some("started"), 2)
        .unwrap_err();
    assert_eq!(error.code, "ownership_conflict");
    let error = store
        .update_progress("agent:one", key, 20, Some("started"), 1)
        .unwrap_err();
    assert_eq!(error.code, "version_conflict");
    assert_eq!(
        store.poll_events(0, 100).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn card_dependencies_reject_cycles_and_cross_board_links() {
    let (_dir, store, board) = setup();
    let a = store
        .create_card("cli", &board, "A", "", "P2", None)
        .unwrap();
    let b = store
        .create_card("cli", &board, "B", "", "P2", None)
        .unwrap();
    let a_key = a["key"].as_str().unwrap();
    let b_key = b["key"].as_str().unwrap();
    store.add_dependency("cli", a_key, b_key, 1).unwrap();
    let error = store.add_dependency("cli", b_key, a_key, 1).unwrap_err();
    assert_eq!(error.code, "dependency_cycle");
    let other = store.create_board("cli", "Other", "").unwrap();
    let c = store
        .create_card("cli", other["key"].as_str().unwrap(), "C", "", "P2", None)
        .unwrap();
    let error = store
        .add_dependency("cli", b_key, c["key"].as_str().unwrap(), 1)
        .unwrap_err();
    assert_eq!(error.code, "cross_board_dependency");
}

#[test]
fn archived_board_rejects_dependency_removal_without_event() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Blocked task", "", "P2", None)
        .unwrap();
    let prerequisite = store
        .create_card("cli", &board, "Prerequisite", "", "P2", None)
        .unwrap();
    let card_key = card["key"].as_str().unwrap();
    let target_key = prerequisite["key"].as_str().unwrap();
    store
        .add_dependency("cli", card_key, target_key, 1)
        .unwrap();
    store.set_board_archived("cli", &board, 1, true).unwrap();
    let event_count = store.poll_events(0, 100).unwrap()["events"]
        .as_array()
        .unwrap()
        .len();

    let error = store
        .remove_dependency("cli", card_key, target_key, 2)
        .unwrap_err();
    assert_eq!(error.code, "board_archived");
    assert_eq!(store.view_card(card_key).unwrap()["blocked"], true);
    assert_eq!(
        store.poll_events(0, 100).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        event_count
    );
}

#[test]
fn prerequisite_regression_blocks_owned_card_without_editing_it() {
    let (_dir, store, board) = setup();
    let owner_card = store
        .create_card("cli", &board, "Owned", "", "P2", None)
        .unwrap();
    let prerequisite = store
        .create_card("cli", &board, "Prerequisite", "", "P2", None)
        .unwrap();
    let owner_key = owner_card["key"].as_str().unwrap();
    let prerequisite_key = prerequisite["key"].as_str().unwrap();
    store
        .move_card("cli", prerequisite_key, "Done", None, 1)
        .unwrap();
    store
        .add_dependency("cli", owner_key, prerequisite_key, 1)
        .unwrap();
    let claimed = store.claim_card("agent:owner", owner_key, 2).unwrap();
    assert_eq!(claimed["blocked"], false);
    let owner_version = claimed["version"].as_i64().unwrap();
    store
        .move_card("cli", prerequisite_key, "Backlog", None, 2)
        .unwrap();
    let after = store.view_card(owner_key).unwrap();
    assert_eq!(after["blocked"], true);
    assert_eq!(after["version"], owner_version);
    assert_eq!(after["owner_actor"], "agent:owner");
    let events = store.poll_events(0, 100).unwrap();
    let last = events["events"].as_array().unwrap().last().unwrap();
    assert_eq!(last["effects"][0]["type"], "card_became_blocked");
    assert_eq!(last["effects"][0]["owner_actor"], "agent:owner");
}

#[test]
fn done_move_sets_progress_and_progress_100_does_not_move() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Finish", "", "P1", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    let progressed = store
        .update_progress("cli", key, 100, Some("Ready for review"), 1)
        .unwrap();
    assert_eq!(progressed["progress"], 100);
    let still_backlog = store.view_board(&board).unwrap();
    assert_eq!(still_backlog["lanes"][0]["cards"][0]["key"], key);
    let moved = store.move_card("cli", key, "Done", None, 2).unwrap();
    assert_eq!(moved["progress"], 100);
    let reopened = store.move_card("cli", key, "Backlog", None, 3).unwrap();
    assert_eq!(reopened["progress"], 100);
    assert_eq!(
        store
            .update_progress("cli", key, 101, None, 4)
            .unwrap_err()
            .code,
        "validation_error"
    );
}

#[test]
fn fixed_lanes_cannot_be_changed_or_archived() {
    let (_dir, store, board) = setup();
    let view = store.view_board(&board).unwrap();
    let backlog_id = view["lanes"][0]["id"].as_str().unwrap();
    let card = store
        .create_card("cli", &board, "Task", "", "P2", None)
        .unwrap();
    assert_eq!(card["lane_id"], backlog_id);
    let error = store
        .set_lane_archived("cli", backlog_id, 1, true)
        .unwrap_err();
    assert_eq!(error.code, "fixed_lane");
    assert_eq!(
        store.view_board(&board).unwrap()["lanes"][0]["name"],
        "Backlog"
    );
}

#[test]
fn empty_legacy_review_lane_can_be_archived() {
    let (_dir, store, board) = setup();
    let board_id = store.view_board(&board).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let legacy_id = uuid::Uuid::new_v4().to_string();
    store
        .open()
        .unwrap()
        .execute(
            "INSERT INTO lanes(id,board_id,name,position,kind) VALUES (?1,?2,'Review',3,'review')",
            rusqlite::params![legacy_id, board_id],
        )
        .unwrap();
    let archived = store.set_lane_archived("cli", &legacy_id, 1, true).unwrap();
    assert_eq!(archived["archived"], true);
    assert_eq!(
        store.view_board(&board).unwrap()["lanes"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn legacy_lane_accepts_only_moves_out() {
    let (_dir, store, board) = setup();
    let board_id = store.view_board(&board).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let legacy_id = uuid::Uuid::new_v4().to_string();
    store
        .open()
        .unwrap()
        .execute(
            "INSERT INTO lanes(id,board_id,name,position,kind) VALUES (?1,?2,'Review',3,'review')",
            rusqlite::params![legacy_id, board_id],
        )
        .unwrap();
    assert_eq!(
        store
            .create_card("cli", &board, "New review work", "", "P2", Some("Review"))
            .unwrap_err()
            .code,
        "fixed_lane"
    );
    let card = store
        .create_card("cli", &board, "Old review work", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    assert_eq!(
        store
            .move_card("cli", key, "Review", None, 1)
            .unwrap_err()
            .code,
        "fixed_lane"
    );
    store
        .open()
        .unwrap()
        .execute(
            "UPDATE cards SET lane_id=?1 WHERE id=?2",
            rusqlite::params![legacy_id, card["id"].as_str().unwrap()],
        )
        .unwrap();
    let moved = store.move_card("cli", key, "Backlog", None, 1).unwrap();
    assert_eq!(moved["key"], key);
    assert_eq!(moved["version"], 2);
}

#[test]
fn card_history_keeps_second_question_even_when_already_blocked() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Design", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    store
        .raise_question(
            "cli",
            &board,
            "engineering",
            "First?",
            "",
            &["A".into(), "B".into()],
            "A",
            "It is straightforward.",
            &[key.to_owned()],
        )
        .unwrap();
    store
        .raise_question(
            "cli",
            &board,
            "business",
            "Second?",
            "",
            &["Yes".into(), "No".into()],
            "Yes",
            "It keeps the project moving.",
            &[key.to_owned()],
        )
        .unwrap();
    let history = store.view_card(key).unwrap()["events"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        history
            .iter()
            .filter(|event| event["type"] == "question_raised")
            .count(),
        2
    );
    assert_eq!(
        store.view_card(key).unwrap()["blockers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn resolving_question_on_done_card_does_not_call_it_ready() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Finished", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    store.move_card("cli", key, "Done", None, 1).unwrap();
    let question = store
        .raise_question(
            "cli",
            &board,
            "business",
            "Confirm?",
            "",
            &["Yes".into(), "No".into()],
            "Yes",
            "It confirms the work is done.",
            &[key.to_owned()],
        )
        .unwrap();
    store
        .resolve_question(
            "cli",
            question["key"].as_str().unwrap(),
            "Yes",
            None,
            1,
            false,
        )
        .unwrap();
    let events = store.poll_events(0, 100).unwrap();
    let last = events["events"].as_array().unwrap().last().unwrap();
    assert_eq!(last["effects"][0]["type"], "card_unblocked");
    assert_eq!(store.view_card(key).unwrap()["ready"], false);
}

#[test]
fn concurrent_claims_have_one_winner_and_one_event() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "One owner", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap().to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for actor in ["agent:one", "agent:two"] {
        let worker = store.clone();
        let key = key.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            worker.claim_card(actor, &key, 1)
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
    let events = store.poll_events(0, 100).unwrap();
    assert_eq!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "card_claimed")
            .count(),
        1
    );
}

#[test]
fn concurrent_session_resumes_have_one_winner() {
    let (_dir, store, board) = setup();
    let task = store.create_card("cli", &board, "Resume once", "", "P2", None).unwrap();
    let key = task["key"].as_str().unwrap().to_owned();
    store.claim_card("agent:one", &key, 1).unwrap();
    store.move_card("agent:one", &key, "In Progress", None, 2).unwrap();
    store.attach_task_session("orchestrator", &key, "codex", "concurrent-session").unwrap();
    store.pause_task_session("orchestrator", &key, None).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let worker = store.clone();
        let key = key.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            worker.resume_task_session("orchestrator", &key)
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.as_ref().is_err_and(|error| error.code == "session_not_ready")).count(), 1);
    let events = store.poll_events(0, 100).unwrap();
    assert_eq!(events["events"].as_array().unwrap().iter().filter(|event| event["type"] == "task_session_resumed").count(), 1);
}

#[test]
fn owner_may_omit_version_but_others_may_not() {
    let (_dir, store, board) = setup();
    let card = store
        .create_card("cli", &board, "Task", "", "P2", None)
        .unwrap();
    let key = card["key"].as_str().unwrap();
    // Unclaimed cards still require a version.
    let err = store.update_progress("agent:a", key, 10, None, None::<i64>).unwrap_err();
    assert_eq!(err.code, "version_required");
    let claimed = store.claim_card("agent:a", key, 1).unwrap();
    // The owner can write without a version, and the result carries the new one.
    let moved = store.move_card("agent:a", key, "In Progress", None, None::<i64>).unwrap();
    assert_eq!(moved["version"].as_i64().unwrap(), claimed["version"].as_i64().unwrap() + 1);
    let progressed = store.update_progress("agent:a", key, 40, Some("half"), None::<i64>).unwrap();
    assert_eq!(progressed["progress"], 40);
    // Another actor is told who owns it, not just that a version is missing.
    let err = store.update_progress("agent:b", key, 50, None, None::<i64>).unwrap_err();
    assert_eq!(err.code, "ownership_conflict");
    // An explicit stale version still conflicts for the owner.
    let err = store.update_progress("agent:a", key, 50, None, 1).unwrap_err();
    assert_eq!(err.code, "version_conflict");
    // Force release always needs the version.
    let err = store.release_card("cli", key, None::<i64>, Some("agent died")).unwrap_err();
    assert_eq!(err.code, "version_required");
    store.release_card("agent:a", key, None::<i64>, None).unwrap();
}

fn expire_lease(store: &Store, board_key: &str) {
    store
        .open()
        .unwrap()
        .execute(
            "UPDATE orchestrator_sessions SET lease_expires_at='2000-01-01T00:00:00+00:00' WHERE board_id=(SELECT id FROM boards WHERE key=?1)",
            [board_key],
        )
        .unwrap();
}

#[test]
fn only_one_orchestrator_holds_a_board_and_takeover_fences_the_old_one() {
    let (_dir, store, board) = setup();
    let first = store.orchestrator_acquire(&board, "run-1", "h", None, 30, Some(2)).unwrap();
    assert_eq!(first["epoch"], 1);
    assert_eq!(first["actor"], format!("orch:{board}#1"));
    let err = store.orchestrator_acquire(&board, "run-2", "h", None, 30, None).unwrap_err();
    assert_eq!(err.code, "orchestrator_conflict");
    assert_eq!(err.details.as_ref().unwrap()["current"]["holder"], "run-1");
    let old = first["actor"].as_str().unwrap().to_owned();
    store.orchestrator_heartbeat(&old, &board, 30).unwrap();

    // Once the lease expires another process may take over, with a higher epoch.
    expire_lease(&store, &board);
    let second = store.orchestrator_acquire(&board, "run-2", "h", None, 30, None).unwrap();
    assert_eq!(second["epoch"], 2);
    assert_eq!(second["pool_size"], 2, "pool size survives takeover");

    // The replaced orchestrator is fenced out of every write, not only its own commands.
    assert_eq!(store.orchestrator_heartbeat(&old, &board, 30).unwrap_err().code, "stale_epoch");
    let card = store.create_card("cli", &board, "Task", "", "P2", None).unwrap();
    let err = store
        .add_card_note(&old, card["key"].as_str().unwrap(), "late write")
        .unwrap_err();
    assert_eq!(err.code, "stale_epoch");
    store.add_card_note(second["actor"].as_str().unwrap(), card["key"].as_str().unwrap(), "ok").unwrap();

    // Releasing frees the board but keeps the epoch, so the released actor stays fenced.
    store.orchestrator_release(second["actor"].as_str().unwrap(), &board).unwrap();
    assert_eq!(
        store.add_card_note(second["actor"].as_str().unwrap(), card["key"].as_str().unwrap(), "x").unwrap_err().code,
        "stale_epoch"
    );
    assert_eq!(store.orchestrator_acquire(&board, "run-3", "h", None, 30, None).unwrap()["epoch"], 3);
}

#[test]
fn dead_holder_on_same_host_can_be_replaced_before_lease_expiry() {
    let (_dir, store, board) = setup();
    store.orchestrator_acquire(&board, "run-1", "this-host", Some(999_999_999), 600, None).unwrap();
    // A different host cannot tell, so it must wait for expiry.
    assert_eq!(
        store.orchestrator_acquire(&board, "run-2", "other-host", None, 30, None).unwrap_err().code,
        "orchestrator_conflict"
    );
    // The same host sees the pid is gone.
    assert_eq!(store.orchestrator_acquire(&board, "run-2", "this-host", None, 30, None).unwrap()["epoch"], 2);
}

#[test]
fn dispatch_starts_work_atomically_and_respects_pause_and_pool_size() {
    let (_dir, store, board) = setup();
    let a = store.create_card("cli", &board, "A", "", "P2", None).unwrap();
    store.create_card("cli", &board, "B", "", "P2", None).unwrap();
    let lease = store.orchestrator_acquire(&board, "run-1", "h", None, 30, Some(1)).unwrap();
    let actor = lease["actor"].as_str().unwrap().to_owned();

    store.orchestrator_set_state("cli", &board, "paused").unwrap();
    let idle = store.orchestrator_dispatch(&actor, &board, "agent:w1", "claude", "sess-1", None).unwrap();
    assert_eq!(idle["reason"], "paused");
    assert!(store.next_ready_task(&board, None).unwrap().is_object(), "planning edits stay visible");

    store.orchestrator_set_state("cli", &board, "running").unwrap();
    let events_before = store.latest_event_id().unwrap();
    let started = store.orchestrator_dispatch(&actor, &board, "agent:w1", "claude", "sess-1", None).unwrap();
    assert_eq!(started["dispatch"], "claim");
    assert_eq!(started["card"]["key"], a["key"]);
    assert_eq!(started["card"]["owner_actor"], "agent:w1");
    assert_eq!(started["session"]["external_session_id"], "sess-1");
    assert_eq!(store.latest_event_id().unwrap() - events_before, 4, "claim, move, attach, dispatched");

    // Pool size 1 is full until the worker pauses or finishes.
    let full = store.orchestrator_dispatch(&actor, &board, "agent:w2", "claude", "sess-2", None).unwrap();
    assert_eq!(full["reason"], "pool_full");
    store.pause_task_session("agent:w1", a["key"].as_str().unwrap(), None).unwrap();
    // A paused task is offered for resume before any new Backlog work, without being changed.
    let resume = store.orchestrator_dispatch(&actor, &board, "agent:w2", "claude", "sess-2", None).unwrap();
    assert_eq!(resume["dispatch"], "resume");
    assert_eq!(resume["card"]["key"], a["key"]);
    store.detach_task_session("agent:w1", a["key"].as_str().unwrap()).unwrap();
    let second = store.orchestrator_dispatch(&actor, &board, "agent:w2", "claude", "sess-2", None).unwrap();
    assert_eq!(second["card"]["title"], "B");

    // A failed dispatch rolls back completely: the duplicate session ID is rejected.
    store.orchestrator_set_pool("cli", &board, 4).unwrap();
    store.create_card("cli", &board, "C", "", "P2", None).unwrap();
    let before = store.latest_event_id().unwrap();
    assert!(store.orchestrator_dispatch(&actor, &board, "agent:w3", "claude", "sess-2", None).is_err());
    assert_eq!(store.latest_event_id().unwrap(), before);
    assert_eq!(store.view_card("C-3").unwrap()["owner_actor"], serde_json::Value::Null);
}

#[test]
fn committed_writes_publish_the_latest_event_id_to_the_tick_file() {
    let (_dir, store, board) = setup();
    let marker = store.tick_marker();
    store.create_card("cli", &board, "Task", "", "P2", None).unwrap();
    let tick = std::fs::read_to_string(store.tick_path()).unwrap();
    assert_eq!(tick.trim().parse::<i64>().unwrap(), store.latest_event_id().unwrap());
    let next = store.wait_for_tick(&marker, std::time::Duration::from_secs(1));
    assert_ne!(next, marker);
    // Nothing changed since: waiting times out with the same marker.
    assert_eq!(store.wait_for_tick(&next, std::time::Duration::from_millis(60)), next);
}

#[test]
fn silent_owner_of_active_work_is_stale_unless_paused_or_orchestrated() {
    let (_dir, store, board) = setup();
    let card = store.create_card("cli", &board, "Work", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    store.claim_card("agent:a", key, 1).unwrap();
    store.move_card("agent:a", key, "In Progress", None, None::<i64>).unwrap();
    let id = store.view_card(key).unwrap()["id"].as_str().unwrap().to_owned();
    assert_eq!(store.view_card(key).unwrap()["stale"], false, "just written");

    let backdate = |when: &str| {
        let db = store.open().unwrap();
        db.execute("UPDATE cards SET updated_at=?2 WHERE id=?1", [&id, when]).unwrap();
        db.execute("UPDATE events SET ts=?2 WHERE entity_id=?1", [&id, when]).unwrap();
    };
    backdate("2000-01-01T00:00:00+00:00");
    let stale = store.view_card(key).unwrap();
    assert_eq!(stale["stale"], true);
    assert_eq!(stale["last_activity_at"], "2000-01-01T00:00:00+00:00");
    let on_board = store.view_board(&board).unwrap();
    let listed = on_board["lanes"][1]["cards"][0].clone();
    assert_eq!(listed["stale"], true, "board view carries the flag too");

    // Other actors' writes are not the owner's activity.
    store.add_card_note("human", key, "ping").unwrap();
    assert_eq!(store.view_card(key).unwrap()["stale"], true);

    // A paused session is waiting by design.
    store.attach_task_session("agent:a", key, "claude", "sess").unwrap();
    store.pause_task_session("agent:a", key, None).unwrap();
    assert_eq!(store.view_card(key).unwrap()["stale"], false);
    store.resume_task_session("agent:a", key).unwrap();
    assert_eq!(store.view_card(key).unwrap()["stale"], true);

    // A connected orchestrator tracks its own workers, so silence alone proves nothing.
    store.orchestrator_acquire(&board, "run", "h", None, 30, None).unwrap();
    assert_eq!(store.view_card(key).unwrap()["stale"], false);

    // Owner activity clears the flag; the stored event history never contains it.
    store.orchestrator_release(&format!("orch:{board}#1"), &board).unwrap();
    store.update_progress("agent:a", key, 10, None, None::<i64>).unwrap();
    assert_eq!(store.view_card(key).unwrap()["stale"], false);
    let events = store.poll_events(0, 1000).unwrap().to_string();
    assert!(!events.contains("\"stale\""));
}

#[test]
fn backoff_stops_dispatch_until_it_expires_or_is_cleared_and_never_shortens() {
    let (_dir, store, board) = setup();
    store.create_card("cli", &board, "Task", "", "P2", None).unwrap();
    let lease = store.orchestrator_acquire(&board, "run", "h", None, 30, None).unwrap();
    let actor = lease["actor"].as_str().unwrap().to_owned();
    let dispatch = |worker: &str, session: &str| {
        store.orchestrator_dispatch(&actor, &board, worker, "claude", session, None).unwrap()
    };

    let later = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let sooner = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
    let status = store.orchestrator_set_backoff(&actor, &board, Some(&later), "usage limit").unwrap();
    assert_eq!(status["backing_off"], true);
    let blocked = dispatch("agent:w1", "s-1");
    assert_eq!(blocked["reason"], "backing_off");
    assert_eq!(blocked["why"], "usage limit");
    assert!(store.next_ready_task(&board, None).unwrap().is_object(), "work is still visible");

    // A worker reporting an earlier reset cannot undo the longer backoff.
    let kept = store.orchestrator_set_backoff(&actor, &board, Some(&sooner), "other").unwrap();
    assert_eq!(kept["backoff_reason"], "usage limit");
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(kept["backoff_until"].as_str().unwrap()).unwrap(),
        chrono::DateTime::parse_from_rfc3339(&later).unwrap()
    );
    assert!(store.orchestrator_set_backoff(&actor, &board, Some("tomorrow-ish"), "").is_err());

    // Clearing resumes dispatch; so does simply passing the time.
    store.orchestrator_set_backoff(&actor, &board, None, "").unwrap();
    assert_eq!(dispatch("agent:w1", "s-1")["dispatch"], "claim");
    let past = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    let expired = store.orchestrator_set_backoff(&actor, &board, Some(&past), "old").unwrap();
    assert_eq!(expired["backing_off"], false);

    // A stale orchestrator cannot set one.
    store.orchestrator_release(&actor, &board).unwrap();
    assert_eq!(
        store.orchestrator_set_backoff(&actor, &board, Some(&later), "x").unwrap_err().code,
        "stale_epoch"
    );
}

#[test]
fn cards_default_to_a_worktree_and_investigations_opt_out() {
    let (_dir, store, board) = setup();
    let card = store.create_card("cli", &board, "Research", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    assert_eq!(card["workspace"], "worktree");
    let err = store.set_card_workspace("cli", key, "somewhere", 1).unwrap_err();
    assert_eq!(err.code, "validation_error");
    let set = store.set_card_workspace("cli", key, "none", 1).unwrap();
    assert_eq!(set["workspace"], "none");
    assert_eq!(set["version"], 2);
    // No change, no new version or event.
    let events = store.latest_event_id().unwrap();
    assert_eq!(store.set_card_workspace("cli", key, "none", 2).unwrap()["version"], 2);
    assert_eq!(store.latest_event_id().unwrap(), events);
    // Owner rules apply like any other card edit.
    store.claim_card("agent:a", key, 2).unwrap();
    assert_eq!(store.set_card_workspace("agent:b", key, "worktree", None::<i64>).unwrap_err().code, "ownership_conflict");
    assert_eq!(store.set_card_workspace("agent:a", key, "worktree", None::<i64>).unwrap()["workspace"], "worktree");
}

#[test]
fn a_held_session_is_never_offered_for_resume_until_released() {
    let (_dir, store, board) = setup();
    let card = store.create_card("cli", &board, "Takeover", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    store.claim_card("agent:a", key, 1).unwrap();
    store.move_card("agent:a", key, "In Progress", None, None::<i64>).unwrap();
    store.attach_task_session("agent:a", key, "claude", "sess").unwrap();

    // Holding a running session pauses it and records who has it.
    let held = store.hold_task_session("ui", key, "jarod").unwrap();
    assert_eq!(held["held_by"], "jarod");
    assert!(held["paused_at"].is_string());
    assert_eq!(store.hold_task_session("ui", key, "someone-else").unwrap_err().code, "session_held");
    // A general pause would normally be offered at once; a hold is not.
    assert!(store.next_ready_task(&board, None).unwrap().is_null());
    assert_eq!(store.resume_task_session("orch", key).unwrap_err().code, "session_not_ready");

    // Releasing keeps it paused and makes it resumable through the normal protocol.
    let released = store.release_task_session_hold("ui", key).unwrap();
    assert!(released["held_by"].is_null() && released["paused_at"].is_string());
    assert_eq!(store.next_ready_task(&board, None).unwrap()["dispatch_action"], "resume");
    store.resume_task_session("orch", key).unwrap();
}

#[test]
fn a_session_waiting_for_review_resumes_only_once_the_commit_is_approved() {
    let (_dir, store, board) = setup();
    let project = store.create_project("agent", "Code", "").unwrap();
    let project_id = project["id"].as_str().unwrap();
    store.attach_board_to_project("agent", &board, project_id, 1).unwrap();
    let repo = store.add_project_repository("agent", project_id, "/tmp/code", "main", None).unwrap();
    let card = store.create_card("agent", &board, "Feature", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    store.claim_card("worker", key, 1).unwrap();
    store.move_card("worker", key, "In Progress", None, None::<i64>).unwrap();
    store.attach_task_session("worker", key, "claude", "sess").unwrap();
    let version = store.view_card(key).unwrap()["version"].as_i64().unwrap();
    let change = store
        .link_card_change("worker", key, repo["id"].as_str().unwrap(), "worktree-c-1", None, "abc123", true, version)
        .unwrap();

    // Both pauses are plain pauses to the worker, but only the review one waits for approval.
    assert_eq!(
        store.pause_task_session_for("worker", key, None, true).unwrap()["waiting_review"],
        true
    );
    assert!(store.next_ready_task(&board, None).unwrap().is_null(), "resumed before approval");
    assert_eq!(store.resume_task_session("orch", key).unwrap_err().code, "session_not_ready");

    store.approve_card_change("human", change["id"].as_str().unwrap(), "Alex", change["version"].as_i64().unwrap()).unwrap();
    let next = store.next_ready_task(&board, None).unwrap();
    assert_eq!(next["dispatch_action"], "resume");
    assert_eq!(store.resume_task_session("orch", key).unwrap()["waiting_review"], false);

    // A plain pause is unchanged: eligible straight away.
    store.pause_task_session("worker", key, None).unwrap();
    assert_eq!(store.next_ready_task(&board, None).unwrap()["dispatch_action"], "resume");
    // Waiting for a question and for review at once is allowed; see the integration tests.
}

#[test]
fn schema_too_new_names_both_versions_and_says_what_to_do() {
    let (dir, store, _) = setup();
    let db = rusqlite::Connection::open(dir.path().join("test.db")).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    let err = store.list_boards().unwrap_err();
    assert_eq!(err.code, "schema_too_new");
    assert!(err.message.contains("version 999"), "{}", err.message);
    assert!(err.message.contains("binary is older"), "{}", err.message);
    assert!(err.hint.as_deref().unwrap().contains("Rebuild or reinstall"));
    let details = err.details.unwrap();
    assert_eq!(details["database_schema_version"], 999);
    assert!(details["binary_schema_version"].as_i64().unwrap() < 999);
}

#[test]
fn supervisor_limits_are_enforced_in_the_core() {
    let (_dir, store, board) = setup();
    let sup = format!("agent:supervisor-{board}");
    let card = store.create_card("cli", &board, "Work", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    let code = |r: workyard_core::Result<serde_json::Value>| r.unwrap_err().code;

    // Lane moves, cancel and transfer are refused; force release too.
    assert_eq!(code(store.move_card(&sup, key, "In Progress", None, 1)), "forbidden_for_supervisor");
    assert_eq!(code(store.cancel_card(&sup, key, "no", 1)), "forbidden_for_supervisor");
    assert_eq!(code(store.transfer_card(&sup, key, 1, "agent:x")), "forbidden_for_supervisor");
    assert_eq!(code(store.release_card(&sup, key, 1, Some("stuck"))), "forbidden_for_supervisor");
    assert_eq!(code(store.approve_card_change(&sup, "nope", "Alex", 1)), "forbidden_for_supervisor");
    let err = store.move_card(&sup, key, "Done", None, 1).unwrap_err();
    assert!(err.hint.is_some());
    assert_eq!(store.view_card(key).unwrap()["version"], 1);

    // Notes and reads keep working.
    store.add_card_note(&sup, key, "steer").unwrap();
    store.view_card(key).unwrap();

    // Questions: only engineering with a recommendation, answered with exactly it.
    let opts = ["A".to_owned(), "B".to_owned()];
    let biz = store.raise_question("agent:one", &board, "business", "Biz?", "", &opts, "A", "r", &[]).unwrap();
    let eng = store.raise_question("agent:one", &board, "engineering", "Eng?", "", &opts, "A", "r", &[]).unwrap();
    let k = |q: &serde_json::Value| q["key"].as_str().unwrap().to_owned();
    assert_eq!(code(store.resolve_question(&sup, &k(&biz), "A", None, 1, false)), "forbidden_for_supervisor");
    assert_eq!(code(store.resolve_question(&sup, &k(&eng), "B", None, 1, false)), "forbidden_for_supervisor");
    assert_eq!(code(store.resolve_question(&sup, &k(&eng), "A", None, 1, true)), "forbidden_for_supervisor");
    let done = store.resolve_question(&sup, &k(&eng), "A", None, 1, false).unwrap();
    assert_eq!(done["status"], "answered");
    // A human is unaffected.
    store.resolve_question("ui", &k(&biz), "B", None, 1, false).unwrap();
}

/// Put the database back to schema 17, as an older binary leaves it: no version on the lease.
fn downgrade_to_17(dir: &TempDir) {
    let db = rusqlite::Connection::open(dir.path().join("test.db")).unwrap();
    db.execute_batch("ALTER TABLE orchestrator_sessions DROP COLUMN binary_version; ALTER TABLE orchestrator_sessions DROP COLUMN schema_version; DELETE FROM schema_version WHERE version>=18; DROP TABLE card_reports; ALTER TABLE orchestrator_sessions DROP COLUMN worker_env_keys; ALTER TABLE boards DROP COLUMN worker_env; ALTER TABLE boards DROP COLUMN integration; PRAGMA user_version = 17").unwrap();
}

#[test]
fn lease_records_the_binary_version_and_release_clears_it() {
    let (_dir, store, board) = setup();
    let lease = store.orchestrator_acquire(&board, "run", "h", None, 30, None).unwrap();
    assert_eq!(lease["binary_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(lease["schema_version"], 21);
    let released = store.orchestrator_release(lease["actor"].as_str().unwrap(), &board).unwrap();
    assert!(released["schema_version"].is_null());
}

#[test]
fn migration_is_refused_under_a_live_older_orchestrator_unless_allowed() {
    let (dir, store, board) = setup();
    store.orchestrator_acquire(&board, "run", "h", None, 300, None).unwrap();
    downgrade_to_17(&dir);
    let err = store.list_boards().unwrap_err();
    assert_eq!(err.code, "migration_blocked");
    assert!(err.message.contains("run"), "{}", err.message);
    assert!(err.hint.as_deref().unwrap().contains("--allow-migrate"));
    // Still blocked, and the database was not touched.
    assert_eq!(store.list_boards().unwrap_err().code, "migration_blocked");
    let migrated = Store::new(dir.path().join("test.db")).allow_migrate(true);
    migrated.list_boards().unwrap();
    store.list_boards().unwrap();
}

#[test]
fn migration_proceeds_when_the_older_lease_is_released_or_dead() {
    let (dir, store, board) = setup();
    let lease = store.orchestrator_acquire(&board, "run", "h", None, 300, None).unwrap();
    store.orchestrator_release(lease["actor"].as_str().unwrap(), &board).unwrap();
    downgrade_to_17(&dir);
    store.list_boards().unwrap();

    // A holder on this host whose process is gone does not block either.
    store.orchestrator_acquire(&board, "run", "h", Some(999_999_999), 300, None).unwrap();
    downgrade_to_17(&dir);
    store.list_boards().unwrap();
}

#[test]
fn board_worker_env_stores_values_but_reports_and_records_names_only() {
    let (_dir, store, board) = setup();
    let pairs = vec![workyard_core::parse_env_pair("CARGO_TARGET_DIR=/tmp/shared").unwrap(), workyard_core::parse_env_pair("X=a=b").unwrap()];
    let result = store.set_board_worker_env("agent", &board, &pairs, &[]).unwrap();
    assert_eq!(result["worker_env_keys"], json!(["CARGO_TARGET_DIR", "X"]));
    assert_eq!(store.board_worker_env(&board).unwrap()["X"], "a=b");
    store.set_board_worker_env("agent", &board, &[], &["X".to_owned()]).unwrap();
    assert_eq!(store.board_worker_env(&board).unwrap().len(), 1);
    let status = store.orchestrator_status(&board).unwrap();
    assert_eq!(status["board_worker_env_keys"], json!(["CARGO_TARGET_DIR"]));
    let events = store.poll_events(0, 1000).unwrap().to_string();
    assert!(events.contains("board_worker_env_changed") && !events.contains("/tmp/shared"));
    for bad in ["NOEQUALS", "1A=x", "A-B=x", "WORKYARD_DB=x", "=x"] {
        assert_eq!(workyard_core::parse_env_pair(bad).unwrap_err().code, "validation_error", "{bad}");
    }
}

#[test]
fn card_report_is_stored_replaced_and_limited() {
    let (_dir, store, board) = setup();
    let card = store.create_card("cli", &board, "Investigate", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    assert!(store.view_card(key).unwrap()["report"].is_null());
    let first = store.set_card_report("agent:one", key, "# Findings\n\n- one").unwrap();
    assert_eq!(first["version"], 1);
    store.set_card_report("agent:one", key, "# Findings\n\n- two").unwrap();
    let view = store.view_card(key).unwrap();
    assert_eq!(view["report"]["body"], "# Findings\n\n- two");
    assert_eq!(view["report"]["version"], 2);
    assert_eq!(view["version"], card["version"], "a report does not change the card version");
    let events = view["events"].as_array().unwrap();
    assert_eq!(events.iter().filter(|e| e["type"] == "card_report_set").count(), 2);
    assert!(store.set_card_report("agent:one", key, "  \n").is_err());
    let huge = "x".repeat(256 * 1024 + 1);
    assert_eq!(store.set_card_report("agent:one", key, &huge).unwrap_err().code, "validation_error");
    assert!(store.set_card_report("agent:one", key, &"x".repeat(256 * 1024)).is_ok());
}
