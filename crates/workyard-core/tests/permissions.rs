//! Tool requests from safe-mode workers: opened by the orchestrator, decided by a human.
use serde_json::json;
use workyard_core::Store;

fn world() -> (Store, String, String, String) {
    let dir = std::env::temp_dir().join(format!("workyard-perm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(dir.join("test.db"));
    let board = store.create_board("cli", "Project", "").unwrap()["key"].as_str().unwrap().to_owned();
    let card = store.create_card("cli", &board, "Work", "", "P2", None).unwrap()["key"].as_str().unwrap().to_owned();
    let lease = store.orchestrator_acquire(&board, "run", "h", None, 30, None).unwrap();
    let actor = lease["actor"].as_str().unwrap().to_owned();
    (store, board, card, actor)
}

fn open(store: &Store, actor: &str, board: &str, card: &str, id: &str) -> serde_json::Value {
    store
        .permission_request_open(actor, board, card, id, "toolu_1", "Bash", &json!({"command": "touch /tmp/a.txt"}), "Create a file")
        .unwrap()
}

#[test]
fn a_request_is_pending_until_a_human_decides_it() {
    let (store, board, card, actor) = world();
    let before = store.latest_event_id().unwrap();
    let request = open(&store, &actor, &board, &card, "r1");
    assert_eq!(request["status"], "pending");
    assert_eq!(request["card_key"], card.as_str());
    assert!(store.latest_event_id().unwrap() > before);
    // The runtime resending the same request does not duplicate it.
    assert_eq!(open(&store, &actor, &board, &card, "r1")["id"], request["id"]);
    assert_eq!(store.permission_requests(&board, Some("pending")).unwrap().as_array().unwrap().len(), 1);

    let id = request["id"].as_str().unwrap();
    let decided = store.permission_decide("human:me", id, "allow_run", None, "").unwrap();
    assert_eq!(decided["status"], "allowed_run");
    assert_eq!(decided["run_rule"], "Bash(touch *)");
    assert_eq!(store.permission_run_rules(&card).unwrap(), vec!["Bash(touch *)"]);
    assert!(store.permission_requests(&board, Some("pending")).unwrap().as_array().unwrap().is_empty());
    // Decided once.
    assert_eq!(store.permission_decide("human:me", id, "deny", None, "").unwrap_err().code, "not_pending");
    // The decision is an event.
    let events = store.poll_events(0, 500).unwrap().to_string();
    assert!(events.contains("permission_requested") && events.contains("permission_decided"), "{events}");
}

#[test]
fn only_a_human_decides_and_only_with_a_known_decision() {
    let (store, board, card, actor) = world();
    let id = open(&store, &actor, &board, &card, "r1")["id"].as_str().unwrap().to_owned();
    for who in ["agent:supervisor-B-1", "agent:claude-1", &actor] {
        assert!(store.permission_decide(who, &id, "allow_once", None, "").is_err(), "{who}");
    }
    assert_eq!(
        store.permission_decide("agent:supervisor-B-1", &id, "allow_once", None, "").unwrap_err().code,
        "forbidden_for_supervisor"
    );
    assert_eq!(store.permission_decide("human:me", &id, "maybe", None, "").unwrap_err().code, "validation_error");
    // A worker cannot open requests either: only the lease holder can.
    assert!(store.permission_request_open("agent:claude-1", &board, &card, "r2", "", "Bash", &json!({}), "").is_err());
    assert_eq!(store.permission_requests(&board, Some("pending")).unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn a_custom_rule_wins_and_expiry_closes_pending_requests() {
    let (store, board, card, actor) = world();
    let a = open(&store, &actor, &board, &card, "r1")["id"].as_str().unwrap().to_owned();
    let decided = store.permission_decide("human:me", &a, "allow_run", Some("Bash(touch /tmp/*)"), "").unwrap();
    assert_eq!(decided["run_rule"], "Bash(touch /tmp/*)");
    let denied = open(&store, &actor, &board, &card, "r2")["id"].as_str().unwrap().to_owned();
    assert_eq!(store.permission_decide("human:me", &denied, "deny", None, "no").unwrap()["status"], "denied");
    assert_eq!(store.permission_run_rules(&card).unwrap(), vec!["Bash(touch /tmp/*)"], "a denial grants nothing");
    open(&store, &actor, &board, &card, "r3");
    assert_eq!(store.permission_requests_expire(&actor, &board, Some(&card)).unwrap(), 1);
    assert_eq!(store.permission_requests(&board, Some("expired")).unwrap().as_array().unwrap().len(), 1);
}
