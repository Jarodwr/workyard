use std::process::Command;

use serde_json::Value;

#[test]
fn cli_json_workflow_uses_one_event_per_write() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-cli-test-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let exe = env!("CARGO_BIN_EXE_workyard");
    let call = |actor: &str, args: &[&str]| -> Value {
        let output = Command::new(exe)
            .args(["--db", db.to_str().unwrap(), "--actor", actor, "--json"])
            .args(args)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{value}");
        assert_eq!(value["ok"], true);
        value["data"].clone()
    };
    let board = call("cli", &["create-board", "CLI test"]);
    let board_key = board["key"].as_str().unwrap();
    let card = call(
        "cli",
        &[
            "add-card",
            board_key,
            "First task",
            "--agent-type",
            "Engineering",
        ],
    );
    let card_key = card["key"].as_str().unwrap();
    assert_eq!(card["agent_type"], "Engineering");
    assert_eq!(
        call("cli", &["next", board_key, "--agent-type", "Engineering"])["key"],
        card["key"]
    );
    let claimed = call("agent:one", &["claim", card_key, "--expected-version", "1"]);
    let version = claimed["version"].as_i64().unwrap().to_string();
    let moved = call(
        "agent:one",
        &[
            "move",
            card_key,
            "In Progress",
            "--expected-version",
            &version,
        ],
    );
    assert_eq!(moved["version"], 3);
    let history = call("cli", &["events", "--since", "0"]);
    assert_eq!(history["events"].as_array().unwrap().len(), 4);
    let question = call(
        "agent:one",
        &[
            "ask",
            board_key,
            "Which renderer?",
            "--kind",
            "engineering",
            "--option",
            "Canvas",
            "--option",
            "SVG",
            "--recommend",
            "SVG",
            "--why",
            "Text stays clear when zoomed.",
        ],
    );
    assert_eq!(
        question["options"],
        serde_json::json!(["Canvas", "SVG", "Other"])
    );
    let answered = call(
        "cli",
        &[
            "answer",
            question["key"].as_str().unwrap(),
            "SVG",
            "--note",
            "Better text rendering",
            "--expected-version",
            "1",
        ],
    );
    assert_eq!(answered["answer_note"], "Better text rendering");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_phases_and_cancellation_control_next_work() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-phase-cli-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let call = |args: &[&str]| -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_workyard"))
            .args(["--db", db.to_str().unwrap(), "--json"])
            .args(args)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{value}");
        value["data"].clone()
    };
    let board = call(&["create-board", "Phases"]);
    let key = board["key"].as_str().unwrap();
    call(&["phase", "add", key, "Integration"]);
    let current = call(&["add-card", key, "Current"]);
    let future = call(&["add-card", key, "Later", "--phase", "Integration"]);
    let filtered = call(&["board", key, "--phase", "Integration"]);
    assert_eq!(filtered["lanes"][0]["cards"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["lanes"][0]["cards"][0]["key"], future["key"]);
    assert_eq!(call(&["next", key])["key"], current["key"]);
    call(&[
        "cancel",
        current["key"].as_str().unwrap(),
        "--reason",
        "Superseded",
        "--expected-version",
        "1",
    ]);
    assert_eq!(call(&["next", key])["key"], future["key"]);
    call(&["phase", "advance", key]);
    assert_eq!(call(&["phase", "list", key])[1]["status"], "current");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_session_commands_select_paused_work_before_backlog() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-session-cli-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let call = |args: &[&str]| -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_workyard"))
            .args(["--db", db.to_str().unwrap(), "--json"])
            .args(args)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{value}");
        value["data"].clone()
    };
    let board = call(&["create-board", "Sessions"]);
    let board_key = board["key"].as_str().unwrap();
    let backlog = call(&["add-card", board_key, "Next task"]);
    let task = call(&["add-card", board_key, "Paused task"]);
    let key = task["key"].as_str().unwrap();
    let claimed = call(&["claim", key, "--expected-version", "1"]);
    let version = claimed["version"].as_i64().unwrap().to_string();
    call(&["move", key, "In Progress", "--expected-version", &version]);
    call(&["session", "attach", key, "--runner", "codex", "--session-id", "cli-session"]);
    call(&["session", "pause", key]);
    assert_eq!(call(&["next", board_key])["dispatch_action"], "resume");
    assert_eq!(call(&["next", board_key])["key"], task["key"]);
    let briefing = call(&["session", "briefing", key]);
    let cursor = briefing["cursor"].as_i64().unwrap().to_string();
    call(&["session", "resume", key]);
    call(&["session", "ack", key, "--through-event-id", &cursor]);
    assert_eq!(call(&["next", board_key])["key"], backlog["key"]);
    std::fs::remove_dir_all(dir).unwrap();
}
