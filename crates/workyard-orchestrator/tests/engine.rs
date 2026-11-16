use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use workyard_core::Store;
use workyard_orchestrator::{LimitHit, Launch, Notifier, Options, Orchestrator, Runner, Worker, WorkerState, hostname};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A runner whose "agent" is a closure run on a thread, driving the real core commands as
/// the worker actor, exactly as `workyard task ...` would.
type Behavior = Arc<dyn Fn(&Launch, &Store) + Send + Sync>;
struct FakeRunner {
    db: PathBuf,
    behavior: Behavior,
    running: Arc<AtomicUsize>,
    max_running: Arc<AtomicUsize>,
    launches: Arc<Mutex<Vec<(String, bool, String)>>>,
    /// Reported (once) by the next worker to exit, as if it had run into a usage limit.
    limit_next: Arc<Mutex<Option<LimitHit>>>,
    /// Everything `send` delivered to any worker, and how many workers were stopped.
    messages: Arc<Mutex<Vec<String>>>,
    stops: Arc<AtomicUsize>,
}
struct FakeWorker {
    done: Arc<AtomicBool>,
    limit: Arc<Mutex<Option<LimitHit>>>,
    messages: Arc<Mutex<Vec<String>>>,
    stops: Arc<AtomicUsize>,
}
impl Worker for FakeWorker {
    fn send(&mut self, message: &str) -> std::io::Result<()> {
        self.messages.lock().unwrap().push(message.to_owned());
        Ok(())
    }
    fn poll(&mut self) -> WorkerState {
        if self.done.load(Ordering::SeqCst) {
            WorkerState::Exited { success: true, limit: self.limit.lock().unwrap().take() }
        } else {
            WorkerState::Running
        }
    }
    fn stop(&mut self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        // Let the fake agent's thread end, as a killed process would.
        self.done.store(true, Ordering::SeqCst);
    }
}
impl Runner for FakeRunner {
    fn name(&self) -> &str {
        "fake"
    }
    fn start(&self, launch: &Launch) -> std::io::Result<Box<dyn Worker>> {
        self.launches
            .lock()
            .unwrap()
            .push((launch.card_key.clone(), launch.resume, launch.prompt.clone()));
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_running.fetch_max(now, Ordering::SeqCst);
        let store = Store::new(self.db.clone());
        let behavior = Arc::clone(&self.behavior);
        let running = Arc::clone(&self.running);
        let launch = Launch {
            board_key: launch.board_key.clone(),
            card_key: launch.card_key.clone(),
            actor: launch.actor.clone(),
            session_id: launch.session_id.clone(),
            prompt: launch.prompt.clone(),
            resume: launch.resume,
            workspace: launch.workspace,
        };
        std::thread::spawn(move || {
            behavior(&launch, &store);
            running.fetch_sub(1, Ordering::SeqCst);
            flag.store(true, Ordering::SeqCst);
        });
        Ok(Box::new(FakeWorker {
            done,
            limit: Arc::clone(&self.limit_next),
            messages: Arc::clone(&self.messages),
            stops: Arc::clone(&self.stops),
        }))
    }
}

struct Harness {
    _dir: TempDir,
    db: PathBuf,
    store: Store,
    board: String,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicUsize>,
    max_running: Arc<AtomicUsize>,
    launches: Arc<Mutex<Vec<(String, bool, String)>>>,
    limit_next: Arc<Mutex<Option<LimitHit>>>,
    messages: Arc<Mutex<Vec<String>>>,
    stops: Arc<AtomicUsize>,
    notices: Arc<Mutex<Vec<(String, String)>>>,
}

struct Recorder(Arc<Mutex<Vec<(String, String)>>>);
impl Notifier for Recorder {
    fn notify(&self, title: &str, body: &str) {
        self.0.lock().unwrap().push((title.to_owned(), body.to_owned()));
    }
}

impl Harness {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("workyard-orch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        let store = Store::new(db.clone());
        let board = store.create_board("cli", "Engine", "").unwrap()["key"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            _dir: TempDir(dir),
            db,
            store,
            board,
            stop: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicUsize::new(0)),
            max_running: Arc::new(AtomicUsize::new(0)),
            launches: Arc::new(Mutex::new(Vec::new())),
            limit_next: Arc::new(Mutex::new(None)),
            messages: Arc::new(Mutex::new(Vec::new())),
            stops: Arc::new(AtomicUsize::new(0)),
            notices: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn card(&self, title: &str) -> String {
        self.store.create_card("cli", &self.board, title, "", "P2", None).unwrap()["key"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn start(
        &self,
        pool: i64,
        max_failures: u32,
        behavior: impl Fn(&Launch, &Store) + Send + Sync + 'static,
    ) -> std::thread::JoinHandle<workyard_core::Result<()>> {
        self.start_with(
            Options {
                holder: "test".into(),
                pool: Some(pool),
                max_failures,
                idle_wait: Duration::from_millis(40),
                limit_margin: Duration::ZERO,
                ..Options::default()
            },
            behavior,
        )
    }

    fn start_with(
        &self,
        options: Options,
        behavior: impl Fn(&Launch, &Store) + Send + Sync + 'static,
    ) -> std::thread::JoinHandle<workyard_core::Result<()>> {
        let runner = FakeRunner {
            db: self.db.clone(),
            behavior: Arc::new(behavior),
            running: Arc::clone(&self.running),
            max_running: Arc::clone(&self.max_running),
            launches: Arc::clone(&self.launches),
            limit_next: Arc::clone(&self.limit_next),
            messages: Arc::clone(&self.messages),
            stops: Arc::clone(&self.stops),
        };
        let notices = Arc::clone(&self.notices);
        let (db, board, stop) = (self.db.clone(), self.board.clone(), Arc::clone(&self.stop));
        std::thread::spawn(move || {
            let store = Store::new(db);
            Orchestrator::new(&store, &board, Box::new(runner), options)
                .with_notifier(Box::new(Recorder(notices)))
                .run(&stop)
        })
    }

    fn wait(&self, what: &str, condition: impl Fn(&Store) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if condition(&self.store) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("timed out waiting for {what}");
    }

    fn done_count(&self, store: &Store) -> usize {
        let board = store.view_board(&self.board).unwrap();
        board["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|lane| lane["name"] == "Done")
            .map(|lane| lane["cards"].as_array().unwrap().len())
            .sum()
    }

    fn event_types(&self) -> Vec<String> {
        self.store.poll_events(0, 1000).unwrap()["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_owned())
            .collect()
    }

    fn finish(&self, handle: std::thread::JoinHandle<workyard_core::Result<()>>) {
        self.stop.store(true, Ordering::SeqCst);
        handle.join().unwrap().unwrap();
    }
}

fn finish_card(launch: &Launch, store: &Store) {
    store
        .update_progress(&launch.actor, &launch.card_key, 50, Some("half"), None::<i64>)
        .unwrap();
    store
        .move_card(&launch.actor, &launch.card_key, "Done", None, None::<i64>)
        .unwrap();
}

#[test]
fn dispatches_cards_one_at_a_time_within_the_pool_and_releases_the_lease() {
    let h = Harness::new();
    h.card("One");
    h.card("Two");
    let handle = h.start(1, 3, |launch, store| {
        std::thread::sleep(Duration::from_millis(80));
        finish_card(launch, store);
    });
    h.wait("both cards done", |s| h.done_count(s) == 2);
    h.finish(handle);
    assert_eq!(h.max_running.load(Ordering::SeqCst), 1, "pool of one never ran two workers");
    let status = h.store.orchestrator_status(&h.board).unwrap();
    assert!(status["holder"].is_null(), "lease released on shutdown");
    assert_eq!(
        h.event_types().iter().filter(|t| *t == "orchestrator_dispatched").count(),
        2
    );
}

#[test]
fn a_second_orchestrator_cannot_take_a_running_board() {
    let h = Harness::new();
    let handle = h.start(1, 3, |_, _| {});
    h.wait("lease taken", |s| s.orchestrator_status(&h.board).unwrap()["holder"].is_string());
    let err = h
        .store
        .orchestrator_acquire(&h.board, "intruder", &hostname(), None, 30, None)
        .unwrap_err();
    assert_eq!(err.code, "orchestrator_conflict");
    h.finish(handle);
}

#[test]
fn paused_board_dispatches_nothing_until_resumed() {
    let h = Harness::new();
    h.card("One");
    h.store.orchestrator_set_state("cli", &h.board, "paused").unwrap();
    let handle = h.start(1, 3, finish_card);
    h.wait("lease taken", |s| s.orchestrator_status(&h.board).unwrap()["holder"].is_string());
    std::thread::sleep(Duration::from_millis(400));
    assert!(h.launches.lock().unwrap().is_empty(), "paused board started a worker");
    // A planning edit made while paused is picked up on resume.
    h.card("Two");
    h.store.orchestrator_set_state("cli", &h.board, "running").unwrap();
    h.wait("both done after resume", |s| h.done_count(s) == 2);
    h.finish(handle);
}

#[test]
fn a_worker_that_keeps_exiting_unfinished_is_retried_then_detached_with_a_note() {
    let h = Harness::new();
    let key = h.card("Broken");
    let handle = h.start(1, 2, |_, _| {}); // exits at once without pausing or finishing
    h.wait("session detached", |s| {
        s.list_task_sessions(&h.board).unwrap().as_array().unwrap().is_empty()
            && s.poll_events(0, 1000).unwrap()["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "task_session_detached")
    });
    // Resumed once after the first failure, then given up on: two launches, not a loop.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(h.launches.lock().unwrap().len(), 2);
    let card = h.store.view_card(&key).unwrap();
    assert!(card.to_string().contains("session was detached"), "{card}");
    h.finish(handle);
}

#[test]
fn a_paused_worker_resumes_with_the_answer_in_its_briefing() {
    let h = Harness::new();
    let key = h.card("Needs a decision");
    let handle = h.start(1, 3, |launch, store| {
        if launch.resume {
            finish_card(launch, store);
            return;
        }
        let question = store
            .raise_question(
                &launch.actor,
                &launch.board_key,
                "engineering",
                "Which store?",
                "",
                &["SQLite".to_owned(), "Files".to_owned()],
                "SQLite",
                "One file, transactional.",
                &[launch.card_key.clone()],
            )
            .unwrap();
        store
            .pause_task_session(&launch.actor, &launch.card_key, question["key"].as_str())
            .unwrap();
    });
    h.wait("question raised", |s| {
        !s.list_questions(Some(&h.board), Some("open")).unwrap().as_array().unwrap().is_empty()
    });
    // While the question is open the orchestrator must not resume the worker.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(h.launches.lock().unwrap().len(), 1);
    let question = h.store.list_questions(Some(&h.board), Some("open")).unwrap()[0].clone();
    h.store
        .resolve_question(
            "human",
            question["key"].as_str().unwrap(),
            "SQLite",
            None,
            question["version"].as_i64().unwrap(),
            false,
        )
        .unwrap();
    h.wait("card done", |s| h.done_count(s) == 1);
    h.finish(handle);
    let launches = h.launches.lock().unwrap();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].0, key);
    assert!(launches[1].1, "second launch resumes the session");
    assert!(launches[1].2.contains("question_answered"), "answer delivered in the briefing");
}

#[test]
fn a_usage_limit_backs_the_board_off_without_failing_the_card() {
    let h = Harness::new();
    let key = h.card("Interrupted");
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 2;
    *h.limit_next.lock().unwrap() = Some(LimitHit { resets_at: Some(reset), kind: "five_hour".into() });
    // First run is cut off at once by the limit; the resumed run finishes the card.
    let handle = h.start(1, 3, |launch, store| {
        if launch.resume {
            finish_card(launch, store);
        }
    });
    h.wait("board backing off", |s| s.orchestrator_status(&h.board).unwrap()["backing_off"] == true);
    let status = h.store.orchestrator_status(&h.board).unwrap();
    assert!(status["backoff_reason"].as_str().unwrap().contains("five_hour"), "{status}");
    assert_eq!(h.launches.lock().unwrap().len(), 1);

    // Nothing starts while backing off, even though the card is waiting for resume.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(h.launches.lock().unwrap().len(), 1, "dispatched during backoff");

    h.wait("card done after the reset", |s| h.done_count(s) == 1);
    h.finish(handle);
    let launches = h.launches.lock().unwrap();
    assert_eq!(launches.len(), 2);
    assert!(launches[1].1, "the interrupted session is resumed, not restarted");
    let types = h.event_types();
    assert!(!types.contains(&"task_session_detached".to_owned()), "limit counted as a failure");
    let notes = h.store.view_card(&key).unwrap()["notes"].clone();
    assert!(notes.as_array().unwrap().is_empty(), "failure note added: {notes}");
}

#[test]
fn a_limit_without_a_reset_time_uses_the_fallback_backoff() {
    let h = Harness::new();
    h.card("Throttled");
    *h.limit_next.lock().unwrap() = Some(LimitHit { resets_at: None, kind: "api_429".into() });
    let handle = h.start_with(
        Options {
            holder: "test".into(),
            pool: Some(1),
            idle_wait: Duration::from_millis(40),
            limit_fallback: Duration::from_secs(3600),
            ..Options::default()
        },
        |_, _| {},
    );
    h.wait("board backing off", |s| s.orchestrator_status(&h.board).unwrap()["backing_off"] == true);
    let until = h.store.orchestrator_status(&h.board).unwrap()["backoff_until"].as_str().unwrap().to_owned();
    let until = chrono::DateTime::parse_from_rfc3339(&until).unwrap();
    assert!(until > chrono::Utc::now() + chrono::Duration::minutes(55), "{until}");
    h.finish(handle);
}

#[test]
fn repeated_instant_failures_stop_dispatch_instead_of_walking_the_backlog() {
    let h = Harness::new();
    for i in 0..5 {
        h.card(&format!("Card {i}"));
    }
    // Every worker dies immediately; the per-card limit is set high so only the guard acts.
    let handle = h.start_with(
        Options {
            holder: "test".into(),
            pool: Some(1),
            max_failures: 10,
            idle_wait: Duration::from_millis(40),
            max_fast_failures: 3,
            ..Options::default()
        },
        |_, _| {},
    );
    h.wait("board backing off", |s| s.orchestrator_status(&h.board).unwrap()["backing_off"] == true);
    let status = h.store.orchestrator_status(&h.board).unwrap();
    assert!(status["backoff_reason"].as_str().unwrap().contains("exited within"), "{status}");
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(h.launches.lock().unwrap().len(), 3, "kept launching after the guard tripped");
    assert!(!h.event_types().contains(&"task_session_detached".to_owned()));
    h.finish(handle);
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn a_human_note_reaches_the_running_worker_but_its_own_notes_do_not() {
    let h = Harness::new();
    let key = h.card("Live");
    let messages = Arc::clone(&h.messages);
    let handle = h.start(1, 3, move |launch, store| {
        // The worker annotating its own card is not a message to itself.
        store.add_card_note(&launch.actor, &launch.card_key, "working on it").unwrap();
        wait_until("a note to arrive", || !messages.lock().unwrap().is_empty());
        finish_card(launch, store);
    });
    h.wait("worker launched", |_| !h.launches.lock().unwrap().is_empty());
    h.store.add_card_note("human", &key, "please use SQLite").unwrap();
    h.wait("card done", |s| h.done_count(s) == 1);
    h.finish(handle);
    let messages = h.messages.lock().unwrap();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("please use SQLite") && messages[0].contains("human"), "{messages:?}");
}

#[test]
fn force_releasing_a_card_stops_its_worker_and_it_is_not_restarted() {
    let h = Harness::new();
    let key = h.card("Obsolete");
    let stops = Arc::clone(&h.stops);
    let handle = h.start(1, 3, move |_, _| wait_until("stop", || stops.load(Ordering::SeqCst) > 0));
    h.wait("worker launched", |_| !h.launches.lock().unwrap().is_empty());
    // Only a card's owner can cancel it (ADR 0002), so taking work away from a running worker
    // means force-releasing it, then replanning or cancelling it.
    let version = h.store.view_card(&key).unwrap()["version"].as_i64().unwrap();
    h.store.release_card("human", &key, version, Some("obsolete")).unwrap();
    wait_until("worker stopped", || h.stops.load(Ordering::SeqCst) == 1);
    std::thread::sleep(Duration::from_millis(300));
    h.finish(handle);
    assert_eq!(h.stops.load(Ordering::SeqCst), 1);
}

#[test]
fn a_held_session_stops_its_worker_and_resumes_only_after_release() {
    let h = Harness::new();
    let key = h.card("Takeover");
    let stops = Arc::clone(&h.stops);
    let handle = h.start(1, 3, move |launch, store| {
        if launch.resume {
            finish_card(launch, store);
        } else {
            wait_until("stop", || stops.load(Ordering::SeqCst) > 0);
        }
    });
    h.wait("worker launched", |_| !h.launches.lock().unwrap().is_empty());
    h.store.hold_task_session("ui", &key, "jarod").unwrap();
    wait_until("worker stopped", || h.stops.load(Ordering::SeqCst) == 1);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(h.launches.lock().unwrap().len(), 1, "resumed while a human held the session");
    h.store.release_task_session_hold("ui", &key).unwrap();
    h.wait("card done after release", |s| h.done_count(s) == 1);
    h.finish(handle);
    let launches = h.launches.lock().unwrap();
    assert_eq!(launches.len(), 2);
    assert!(launches[1].1, "released session is resumed, not restarted");
}

#[test]
fn humans_are_notified_once_per_event_and_not_for_history() {
    let h = Harness::new();
    let ask = |title: &str| {
        h.store
            .raise_question("agent:x", &h.board, "business", title, "", &["Yes".to_owned(), "No".to_owned()], "Yes", "Simplest.", &[])
            .unwrap();
    };
    ask("Old question"); // raised before the orchestrator ever ran: history, not news
    let handle = h.start(1, 3, |_, _| {});
    h.wait("lease taken", |s| s.orchestrator_status(&h.board).unwrap()["holder"].is_string());
    ask("Fresh question");
    wait_until("notification", || h.notices.lock().unwrap().iter().any(|(t, _)| t.contains("question")));
    h.finish(handle);

    // Raised while no orchestrator runs: delivered after restart, and only once.
    ask("Raised while away");
    h.stop.store(false, Ordering::SeqCst);
    let handle = h.start(1, 3, |_, _| {});
    wait_until("catch-up notification", || {
        h.notices.lock().unwrap().iter().any(|(_, body)| body.contains("Raised while away"))
    });
    std::thread::sleep(Duration::from_millis(200));
    h.finish(handle);
    let bodies: Vec<String> = h.notices.lock().unwrap().iter().map(|(_, b)| b.clone()).collect();
    assert!(!bodies.iter().any(|b| b.contains("Old question")), "{bodies:?}");
    assert_eq!(bodies.iter().filter(|b| b.contains("Fresh question")).count(), 1, "{bodies:?}");
    assert_eq!(bodies.iter().filter(|b| b.contains("Raised while away")).count(), 1, "{bodies:?}");
}

#[test]
fn workers_are_told_whether_they_get_a_worktree() {
    let h = Harness::new();
    let code = h.card("Code");
    let research = h.card("Research");
    let version = h.store.view_card(&research).unwrap()["version"].as_i64().unwrap();
    h.store.set_card_workspace("human", &research, "none", version).unwrap();
    let handle = h.start(2, 3, finish_card);
    h.wait("both done", |s| h.done_count(s) == 2);
    h.finish(handle);
    let launches = h.launches.lock().unwrap();
    let prompt = |key: &str| launches.iter().find(|l| l.0 == key).unwrap().2.clone();
    assert!(prompt(&code).contains("own git worktree"), "{}", prompt(&code));
    assert!(prompt(&code).contains("merge --ff-only"), "workers integrate themselves: {}", prompt(&code));
    assert!(!prompt(&code).contains("--review-required"), "review is opt-in, not the default");
    assert!(prompt(&code).contains("Write and Edit tools"));
    assert!(prompt(&research).contains("investigative"), "{}", prompt(&research));
    assert!(!prompt(&research).contains("own git worktree"));
}
