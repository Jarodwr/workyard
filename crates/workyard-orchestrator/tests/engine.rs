use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use workyard_core::Store;
use workyard_orchestrator::{Launch, Options, Orchestrator, Runner, Worker, WorkerState, hostname};

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
}
struct FakeWorker(Arc<AtomicBool>);
impl Worker for FakeWorker {
    fn poll(&mut self) -> WorkerState {
        if self.0.load(Ordering::SeqCst) {
            WorkerState::Exited { success: true }
        } else {
            WorkerState::Running
        }
    }
    fn stop(&mut self) {}
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
        };
        std::thread::spawn(move || {
            behavior(&launch, &store);
            running.fetch_sub(1, Ordering::SeqCst);
            flag.store(true, Ordering::SeqCst);
        });
        Ok(Box::new(FakeWorker(done)))
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
        let runner = FakeRunner {
            db: self.db.clone(),
            behavior: Arc::new(behavior),
            running: Arc::clone(&self.running),
            max_running: Arc::clone(&self.max_running),
            launches: Arc::clone(&self.launches),
        };
        let (db, board, stop) = (self.db.clone(), self.board.clone(), Arc::clone(&self.stop));
        std::thread::spawn(move || {
            let store = Store::new(db);
            let options = Options {
                holder: "test".into(),
                pool: Some(pool),
                max_failures,
                idle_wait: Duration::from_millis(40),
                ..Options::default()
            };
            Orchestrator::new(&store, &board, Box::new(runner), options).run(&stop)
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
