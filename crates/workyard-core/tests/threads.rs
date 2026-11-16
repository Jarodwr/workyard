//! Question threads: humans and agents talk about an open question; only a human resolves it.
use std::path::PathBuf;
use workyard_core::Store;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A board with one claimed card whose attached session is paused on an open question.
fn paused_on_question() -> (TempDir, Store, String, String, String) {
    let dir = std::env::temp_dir().join(format!("workyard-thread-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(dir.join("test.db"));
    let board = store.create_board("cli", "Project", "").unwrap()["key"].as_str().unwrap().to_owned();
    let task = store.create_card("cli", &board, "Waiting work", "", "P2", None).unwrap();
    let key = task["key"].as_str().unwrap().to_owned();
    store.claim_card("agent:one", &key, 1).unwrap();
    store.move_card("agent:one", &key, "In Progress", None, 2).unwrap();
    store.attach_task_session("orchestrator", &key, "codex", "session-t").unwrap();
    let question = store
        .raise_question("agent:one", &board, "engineering", "Which design?", "", &["A".into(), "B".into()], "A", "Simpler", &[key.clone()])
        .unwrap();
    let q = question["key"].as_str().unwrap().to_owned();
    store.pause_task_session("orchestrator", &key, Some(&q)).unwrap();
    (TempDir(dir), store, board, key, q)
}

fn offered(store: &Store, board: &str) -> Option<serde_json::Value> {
    let next = store.next_ready_task(board, None).unwrap();
    (next["dispatch_action"] == "resume").then_some(next)
}

/// Run the orchestrator's resume protocol: read the briefing, resume, acknowledge.
fn resume_and_ack(store: &Store, key: &str) -> serde_json::Value {
    let briefing = store.task_session_briefing(key, 100, None).unwrap();
    store.resume_task_session("orchestrator", key).unwrap();
    store.acknowledge_task_session("orchestrator", key, briefing["cursor"].as_i64().unwrap()).unwrap();
    briefing
}

#[test]
fn messages_are_listed_in_order_and_validated() {
    let (_dir, store, _board, _key, q) = paused_on_question();
    assert_eq!(store.post_question_message("ui", &q, "   ").unwrap_err().code, "validation_error");
    let too_long = "x".repeat(workyard_core::MAX_MESSAGE_CHARS + 1);
    assert_eq!(store.post_question_message("ui", &q, &too_long).unwrap_err().code, "validation_error");
    assert_eq!(store.post_question_message("ui", "Q-999", "hi").unwrap_err().code, "not_found");
    let first = store.post_question_message("ui", &q, "  Why A?  ").unwrap();
    assert_eq!(first["body"], "Why A?");
    assert_eq!(first["author_is_human"], true);
    store.post_question_message("agent:one", &q, "Because it is simpler").unwrap();
    let thread = store.list_question_messages(&q).unwrap();
    let bodies: Vec<_> = thread.as_array().unwrap().iter().map(|m| m["body"].as_str().unwrap()).collect();
    assert_eq!(bodies, ["Why A?", "Because it is simpler"]);
    assert_eq!(thread[1]["author_is_human"], false);
    let view = store.view_question(&q).unwrap();
    assert_eq!(view["messages"].as_array().unwrap().len(), 2);
    assert_eq!(view["status"], "open");
}

#[test]
fn a_message_emits_an_event_and_ticks_the_file() {
    let (_dir, store, _board, _key, q) = paused_on_question();
    store.post_question_message("ui", &q, "Hello").unwrap();
    let events = store.poll_events(0, 1000).unwrap();
    let last = events["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["type"], "question_message_posted");
    assert_eq!(last["after"]["body"], "Hello");
    assert_eq!(last["after"]["question_key"], q);
    let tick = std::fs::read_to_string(store.tick_path()).unwrap();
    assert_eq!(tick.trim().parse::<i64>().unwrap(), last["id"].as_i64().unwrap());
}

#[test]
fn closed_questions_refuse_messages_and_messages_never_resolve() {
    let (_dir, store, _board, _key, q) = paused_on_question();
    store.post_question_message("ui", &q, "Thinking").unwrap();
    assert_eq!(store.read_question(&q).unwrap().status.as_str(), "open");
    store.resolve_question("ui", &q, "A", None, 1, false).unwrap();
    assert_eq!(store.post_question_message("ui", &q, "Too late").unwrap_err().code, "validation_error");
    assert_eq!(store.list_question_messages(&q).unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn supervisor_may_post_but_still_cannot_resolve() {
    let (_dir, store, _board, _key, q) = paused_on_question();
    store.post_question_message("agent:supervisor-B-1", &q, "The worker is waiting on you").unwrap();
    assert_eq!(store.resolve_question("agent:supervisor-B-1", &q, "A", None, 1, false).unwrap_err().code, "forbidden_for_supervisor");
    assert_eq!(store.resolve_question("agent:supervisor-B-1", &q, "no longer needed", None, 1, true).unwrap_err().code, "forbidden_for_supervisor");
}

#[test]
fn a_human_message_wakes_the_paused_worker_once_and_the_question_stays_open() {
    let (_dir, store, board, key, q) = paused_on_question();
    assert!(offered(&store, &board).is_none(), "nothing to do before anyone writes");
    assert_eq!(store.resume_task_session("orchestrator", &key).unwrap_err().code, "session_not_ready");

    store.post_question_message("ui", &q, "Could B work if we cache it?").unwrap();
    let next = offered(&store, &board).expect("the human message offers the session for resume");
    assert_eq!(next["key"], key.as_str());
    assert_eq!(next["resume_reason"], "thread_message");

    // The briefing carries the thread event.
    let briefing = resume_and_ack(&store, &key);
    assert!(briefing["events"].as_array().unwrap().iter().any(|e| e["type"] == "question_message_posted" && e["after"]["body"] == "Could B work if we cache it?"));

    // Running: the worker replies, then pauses on the same question again.
    store.post_question_message("agent:one", &q, "Yes, with a cache B is fine").unwrap();
    store.pause_task_session("orchestrator", &key, Some(&q)).unwrap();
    assert!(offered(&store, &board).is_none(), "an agent reply does not wake it again");
    assert_eq!(store.read_question(&q).unwrap().status.as_str(), "open");

    // Another human message wakes it again.
    store.post_question_message("ui", &q, "OK, but what about memory?").unwrap();
    assert!(offered(&store, &board).is_some());
}

#[test]
fn only_human_messages_wake_and_a_message_posted_while_running_is_not_lost() {
    let (_dir, store, board, key, q) = paused_on_question();
    store.post_question_message("agent:supervisor-B-1", &q, "ping").unwrap();
    store.post_question_message("agent:other", &q, "me too").unwrap();
    assert!(offered(&store, &board).is_none());

    store.post_question_message("ui", &q, "first").unwrap();
    resume_and_ack(&store, &key);
    // The human writes while the worker is still running its reply turn.
    store.post_question_message("ui", &q, "second").unwrap();
    store.post_question_message("agent:one", &q, "reply to first").unwrap();
    store.pause_task_session("orchestrator", &key, Some(&q)).unwrap();
    assert!(offered(&store, &board).is_some(), "the unseen human message still wakes the worker");
}

#[test]
fn answering_resumes_with_the_answer_reason_and_held_sessions_do_not_wake() {
    let (_dir, store, board, key, q) = paused_on_question();
    store.hold_task_session("ui", &key, "human").unwrap();
    store.post_question_message("ui", &q, "hello").unwrap();
    assert!(offered(&store, &board).is_none(), "a held session is never offered");
    store.release_task_session_hold("ui", &key).unwrap();
    assert!(offered(&store, &board).is_some());
    store.resolve_question("ui", &q, "B", Some("fine"), 1, false).unwrap();
    assert_eq!(offered(&store, &board).unwrap()["resume_reason"], "resolved");
}

#[test]
fn unread_agent_replies_are_counted_until_a_human_marks_them_read() {
    let (_dir, store, board, _key, q) = paused_on_question();
    let question = || store.list_questions(Some(&board), Some("open")).unwrap()[0].clone();
    assert_eq!(question()["unread_messages"], 0);
    store.post_question_message("ui", &q, "Q?").unwrap();
    assert_eq!(question()["unread_messages"], 0, "a human's own message is never unread");
    store.post_question_message("agent:one", &q, "A1").unwrap();
    store.post_question_message("agent:one", &q, "A2").unwrap();
    assert_eq!(question()["unread_messages"], 2);
    assert_eq!(question()["message_count"], 3);
    assert_eq!(store.mark_question_messages_read(&q).unwrap(), 2);
    assert_eq!(question()["unread_messages"], 0);
    assert_eq!(store.mark_question_messages_read(&q).unwrap(), 0);
}

#[test]
fn migration_creates_the_thread_table() {
    let (_dir, store, _board, _key, _q) = paused_on_question();
    let db = store.open().unwrap();
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, workyard_core::SCHEMA_VERSION);
    assert!(version >= 22);
    let columns: i64 = db.query_row("SELECT COUNT(*) FROM pragma_table_info('question_messages')", [], |r| r.get(0)).unwrap();
    assert!(columns >= 8);
}
