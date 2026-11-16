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
    let board = call("cli", &["plan", "create-board", "CLI test"]);
    let board_key = board["key"].as_str().unwrap();
    let card = call(
        "cli",
        &[
            "task", "add-card",
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
    let claimed = call("agent:one", &["task", "claim", card_key, "--expected-version", "1"]);
    let version = claimed["version"].as_i64().unwrap().to_string();
    let moved = call(
        "agent:one",
        &[
            "task", "move",
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
            "task", "ask",
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
            "admin", "answer",
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
    let board = call(&["plan", "create-board", "Phases"]);
    let key = board["key"].as_str().unwrap();
    call(&["plan", "phase", "add", key, "Integration"]);
    let current = call(&["task", "add-card", key, "Current"]);
    let future = call(&["task", "add-card", key, "Later", "--phase", "Integration"]);
    let filtered = call(&["board", key, "--phase", "Integration"]);
    assert_eq!(filtered["lanes"][0]["cards"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["lanes"][0]["cards"][0]["key"], future["key"]);
    assert_eq!(call(&["next", key])["key"], current["key"]);
    call(&[
        "plan", "cancel",
        current["key"].as_str().unwrap(),
        "--reason",
        "Superseded",
        "--expected-version",
        "1",
    ]);
    assert_eq!(call(&["next", key])["key"], future["key"]);
    call(&["plan", "phase", "advance", key]);
    assert_eq!(call(&["plan", "phase", "list", key])[1]["status"], "current");
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
    let board = call(&["plan", "create-board", "Sessions"]);
    let board_key = board["key"].as_str().unwrap();
    let backlog = call(&["task", "add-card", board_key, "Next task"]);
    let task = call(&["task", "add-card", board_key, "Paused task"]);
    let key = task["key"].as_str().unwrap();
    let claimed = call(&["task", "claim", key, "--expected-version", "1"]);
    let version = claimed["version"].as_i64().unwrap().to_string();
    call(&["task", "move", key, "In Progress", "--expected-version", &version]);
    call(&["orch", "session", "attach", key, "--runner", "codex", "--session-id", "cli-session"]);
    call(&["orch", "session", "pause", key]);
    assert_eq!(call(&["next", board_key])["dispatch_action"], "resume");
    assert_eq!(call(&["next", board_key])["key"], task["key"]);
    let briefing = call(&["orch", "session", "briefing", key]);
    let cursor = briefing["cursor"].as_i64().unwrap().to_string();
    call(&["orch", "session", "resume", key]);
    call(&["orch", "session", "ack", key, "--through-event-id", &cursor]);
    assert_eq!(call(&["next", board_key])["key"], backlog["key"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_orchestrator_lease_pause_dispatch_and_follow() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-orch-cli-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let run = |actor: &str, args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_workyard"))
            .args(["--db", db.to_str().unwrap(), "--actor", actor, "--json"])
            .args(args)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output.status.code().unwrap(), value)
    };
    let call = |actor: &str, args: &[&str]| -> Value {
        let (code, value) = run(actor, args);
        assert_eq!(code, 0, "{value}");
        value["data"].clone()
    };
    let board = call("cli", &["plan", "create-board", "Orchestrated"]);
    let key = board["key"].as_str().unwrap();
    call("cli", &["task", "add-card", key, "First"]);
    call("cli", &["task", "add-card", key, "Second"]);

    // A follower started now sees every later commit without polling the database.
    let mut follower = Command::new(env!("CARGO_BIN_EXE_workyard"))
        .args(["--db", db.to_str().unwrap(), "events", "--follow", "--since", "0"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = follower.stdout.take().unwrap();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let wait = std::time::Duration::from_secs(5);
    for _ in 0..3 {
        rx.recv_timeout(wait).expect("backlog event");
    }

    let lease = call("cli", &["orch", "acquire", key, "--holder", "test", "--pool", "1"]);
    let actor = lease["actor"].as_str().unwrap().to_owned();
    assert_eq!(actor, format!("orch:{key}#1"));
    let line = rx.recv_timeout(wait).expect("acquire event");
    assert!(line.contains("orchestrator_acquired"), "{line}");

    // Only one orchestrator per board.
    let (code, conflict) = run("cli", &["orch", "acquire", key, "--holder", "second"]);
    assert_eq!(code, 2);
    assert_eq!(conflict["error"]["code"], "orchestrator_conflict");

    // Pausing stops dispatch; resuming lets it claim, move, and attach in one step.
    call("cli", &["orch", "pause", key]);
    let idle = call(&actor, &["orch", "dispatch", key, "--worker", "agent:w1", "--runner", "claude", "--session-id", "s-1"]);
    assert_eq!(idle["reason"], "paused");
    call("cli", &["orch", "resume", key]);
    let started = call(&actor, &["orch", "dispatch", key, "--worker", "agent:w1", "--runner", "claude", "--session-id", "s-1"]);
    assert_eq!(started["card"]["owner_actor"], "agent:w1");
    assert_eq!(started["session"]["external_session_id"], "s-1");
    assert_eq!(call(&actor, &["orch", "heartbeat", key])["desired_state"], "running");

    // The owner can report progress without a version; a stranger cannot.
    let card = started["card"]["key"].as_str().unwrap();
    call("agent:w1", &["task", "progress", card, "30", "--note", "going"]);
    let (code, denied) = run("agent:other", &["task", "progress", card, "40"]);
    assert_eq!(code, 3);
    assert_eq!(denied["error"]["code"], "ownership_conflict");

    // After release the old actor is fenced out of everything.
    call(&actor, &["orch", "release", key]);
    let (_, stale) = run(&actor, &["orch", "heartbeat", key]);
    assert_eq!(stale["error"]["code"], "stale_epoch");
    let (_, stale) = run(&actor, &["task", "note", card, "late"]);
    assert_eq!(stale["error"]["code"], "stale_epoch");

    follower.kill().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_review_setting_permissions_and_integration_pause() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-cli-test-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let exe = env!("CARGO_BIN_EXE_workyard");
    let run = |args: &[&str]| -> (bool, Value) {
        let output = Command::new(exe)
            .args(["--db", db.to_str().unwrap(), "--actor", "agent:w", "--json"])
            .args(args)
            .output()
            .unwrap();
        (output.status.success(), serde_json::from_slice(&output.stdout).unwrap())
    };
    let ok = |args: &[&str]| -> Value {
        let (success, value) = run(args);
        assert!(success, "{value}");
        value["data"].clone()
    };
    let board = ok(&["plan", "create-board", "CLI"]);
    let key = board["key"].as_str().unwrap();
    assert_eq!(board["review_required"], false);
    assert_eq!(ok(&["plan", "board-review", key, "on"])["review_required"], true);
    assert_eq!(ok(&["plan", "board-review", key, "off"])["review_required"], false);
    assert_eq!(ok(&["orch", "status", key])["permissions"], "full");

    // Pausing for integration is a real wait kind, and it combines with the others.
    let card = ok(&["task", "add-card", key, "Work"]);
    let card_key = card["key"].as_str().unwrap();
    ok(&["task", "claim", card_key, "--expected-version", "1"]);
    ok(&["task", "move", card_key, "In Progress"]);
    ok(&["orch", "session", "attach", card_key, "--runner", "claude", "--session-id", "s-1"]);
    let paused = ok(&["orch", "session", "pause", card_key, "--for-review", "--for-integration"]);
    assert_eq!(paused["waiting_integration"], true);
    assert_eq!(paused["waiting_review"], true);
}

#[test]
fn cli_permission_flags_become_claude_arguments() {
    // Run the `--help` text rather than a worker: the contract is that full is the default.
    let help = Command::new(env!("CARGO_BIN_EXE_workyard")).args(["orch", "run", "--help"]).output().unwrap();
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("--permissions") && text.contains("[default: full]"), "{text}");
    assert!(text.contains("--allow"), "{text}");
}

#[test]
fn cli_permission_requests_are_listed_and_only_a_human_may_decide_them() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-cli-perms-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let run = |actor: &str, args: &[&str]| -> (bool, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_workyard"))
            .args(["--db", db.to_str().unwrap(), "--actor", actor, "--json"])
            .args(args)
            .output()
            .unwrap();
        (output.status.success(), serde_json::from_slice(&output.stdout).unwrap())
    };
    let (_, board) = run("human", &["plan", "create-board", "Perms"]);
    let key = board["data"]["key"].as_str().unwrap();
    let (ok, listed) = run("human", &["orch", "perms", "list", key, "--status", "pending"]);
    assert!(ok && listed["data"] == serde_json::json!([]), "{listed}");
    for (actor, code) in [("agent:supervisor-b-1", "forbidden_for_supervisor"), ("agent:claude-1", "forbidden_for_agents")] {
        let (ok, refused) = run(actor, &["orch", "perms", "decide", "any", "allow-run"]);
        assert!(!ok && refused["error"]["code"] == code, "{actor}: {refused}");
    }
    let (ok, missing) = run("human", &["orch", "perms", "decide", "nope", "deny"]);
    assert!(!ok && missing["error"]["code"] == "not_found", "{missing}");
    std::fs::remove_dir_all(dir).unwrap();
}
