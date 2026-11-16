use std::process::Command;

use serde_json::Value;

/// Resolved when the test runs, not when it compiles (see workflow.rs).
fn exe() -> String {
    std::env::var("CARGO_BIN_EXE_workyard").expect("cargo sets CARGO_BIN_EXE_workyard when running tests")
}

#[test]
fn cli_question_discuss_wakes_the_paused_worker_and_show_returns_the_thread() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("workyard-thread-cli-{nonce}"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let run = |actor: &str, args: &[&str]| -> (bool, Value) {
        let output = Command::new(exe())
            .args(["--db", db.to_str().unwrap(), "--json", "--actor", actor])
            .args(args)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output.status.success(), value)
    };
    let call = |actor: &str, args: &[&str]| -> Value {
        let (ok, value) = run(actor, args);
        assert!(ok, "{value}");
        value["data"].clone()
    };
    let board = call("cli", &["plan", "create-board", "Threads"]);
    let board_key = board["key"].as_str().unwrap();
    let task = call("cli", &["task", "add-card", board_key, "Waiting task"]);
    let key = task["key"].as_str().unwrap();
    let claimed = call("agent:w", &["task", "claim", key, "--expected-version", "1"]);
    let version = claimed["version"].as_i64().unwrap().to_string();
    call("agent:w", &["task", "move", key, "In Progress", "--expected-version", &version]);
    call("cli", &["orch", "session", "attach", key, "--runner", "codex", "--session-id", "thread-session"]);
    let q = call("agent:w", &["task", "ask", board_key, "Which?", "--kind", "engineering", "--option", "A", "--option", "B", "--recommend", "A", "--why", "simple", "--blocks", key]);
    let qkey = q["key"].as_str().unwrap();
    call("cli", &["orch", "session", "pause", key, "--waiting-question", qkey]);
    assert!(call("cli", &["next", board_key]).is_null());

    call("cli", &["question", "discuss", qkey, "Why A over B?"]);
    let next = call("cli", &["next", board_key]);
    assert_eq!(next["dispatch_action"], "resume");
    assert_eq!(next["resume_reason"], "thread_message");

    call("agent:w", &["question", "discuss", qkey, "A is simpler"]);
    let shown = call("cli", &["question", "show", qkey]);
    assert_eq!(shown["status"], "open");
    let bodies: Vec<&str> = shown["messages"].as_array().unwrap().iter().map(|m| m["body"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["Why A over B?", "A is simpler"]);

    let (ok, refusal) = run("agent:supervisor-x", &["admin", "answer", qkey, "A", "--expected-version", "1"]);
    assert!(!ok, "{refusal}");
    assert!(!run("cli", &["question", "discuss", qkey, "   "]).0);
    std::fs::remove_dir_all(dir).unwrap();
}
