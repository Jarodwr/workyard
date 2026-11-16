//! The orchestrator loop for one board. It holds the board's lease, dispatches work through
//! the core's `orchestrator_dispatch`, starts and resumes workers through a `Runner`, and
//! repairs sessions whose worker exited without finishing or pausing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use workyard_core::{DEFAULT_LEASE_SECONDS, Result, Store};

use crate::runner::{Launch, Runner, Worker, WorkerState};

pub struct Options {
    pub holder: String,
    /// Maximum concurrent workers; stored on the board and kept across restarts when `None`.
    pub pool: Option<i64>,
    pub lease_seconds: i64,
    /// Consecutive exits without finishing or pausing before the session is detached.
    pub max_failures: u32,
    /// Longest the loop sleeps when nothing commits (it also polls worker exit).
    pub idle_wait: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            holder: "orchestrator".into(),
            pool: None,
            lease_seconds: DEFAULT_LEASE_SECONDS,
            max_failures: 3,
            idle_wait: Duration::from_secs(1),
        }
    }
}

pub fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".into())
}

pub struct Orchestrator<'a> {
    store: &'a Store,
    board: String,
    runner: Box<dyn Runner>,
    opts: Options,
    live: HashMap<String, Box<dyn Worker>>,
    failures: HashMap<String, u32>,
}

impl<'a> Orchestrator<'a> {
    pub fn new(store: &'a Store, board: &str, runner: Box<dyn Runner>, opts: Options) -> Self {
        Self {
            store,
            board: board.to_owned(),
            runner,
            opts,
            live: HashMap::new(),
            failures: HashMap::new(),
        }
    }

    /// Run until `stop` is set or the lease is lost. Fails fast with `orchestrator_conflict`
    /// when another orchestrator holds the board. Always stops workers and releases the lease.
    pub fn run(&mut self, stop: &AtomicBool) -> Result<()> {
        let lease = self.store.orchestrator_acquire(
            &self.board,
            &self.opts.holder,
            &hostname(),
            Some(i64::from(std::process::id())),
            self.opts.lease_seconds,
            self.opts.pool,
        )?;
        let actor = lease["actor"].as_str().unwrap_or_default().to_owned();
        let result = self.main_loop(&actor, stop);
        self.shutdown(&actor);
        result
    }

    fn main_loop(&mut self, actor: &str, stop: &AtomicBool) -> Result<()> {
        self.reconcile(actor);
        let renew_every = Duration::from_secs((self.opts.lease_seconds / 3).max(1) as u64);
        let mut last_heartbeat = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            // Read the marker first so a commit during this pass wakes the next wait.
            let marker = self.store.tick_marker();
            if last_heartbeat.elapsed() >= renew_every {
                self.store
                    .orchestrator_heartbeat(actor, &self.board, self.opts.lease_seconds)?;
                last_heartbeat = Instant::now();
            }
            self.reap(actor);
            let status = self.store.orchestrator_status(&self.board)?;
            if status["desired_state"] == "running" {
                self.dispatch_all(actor, stop)?;
            }
            self.store.wait_for_tick(&marker, self.opts.idle_wait);
        }
        Ok(())
    }

    /// After a restart, any unpaused session has no worker of ours. Pause it so `next`
    /// offers it for resume rather than leaving it looking busy.
    fn reconcile(&mut self, actor: &str) {
        let Ok(sessions) = self.store.list_task_sessions(&self.board) else {
            return;
        };
        for entry in sessions.as_array().into_iter().flatten() {
            if entry["session"]["paused_at"].is_null() {
                let key = entry["card_key"].as_str().unwrap_or_default();
                self.log_err(self.store.pause_task_session(actor, key, None), key);
            }
        }
    }

    /// Handle workers that exited. An exit with the session paused (waiting) or gone
    /// (finished) is normal; anything else counts as a failure.
    fn reap(&mut self, actor: &str) {
        let mut exited = Vec::new();
        for (key, worker) in self.live.iter_mut() {
            if matches!(worker.poll(), WorkerState::Exited { .. }) {
                exited.push(key.clone());
            }
        }
        if exited.is_empty() {
            return;
        }
        let sessions = self
            .store
            .list_task_sessions(&self.board)
            .unwrap_or_else(|_| json!([]));
        for key in exited {
            self.live.remove(&key);
            let session = sessions
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["card_key"] == key.as_str());
            match session {
                Some(entry) if entry["session"]["paused_at"].is_null() => {
                    self.worker_failed(actor, &key, "exited without finishing or pausing");
                }
                _ => {
                    self.failures.remove(&key);
                }
            }
        }
    }

    /// Pause the session so it can be resumed, or after repeated failures detach it and
    /// leave a note, so a broken card cannot keep the orchestrator in a restart loop.
    fn worker_failed(&mut self, actor: &str, key: &str, why: &str) {
        let count = self.failures.entry(key.to_owned()).or_insert(0);
        *count += 1;
        let count = *count;
        if count >= self.opts.max_failures {
            self.failures.remove(key);
            self.log_err(self.store.detach_task_session(actor, key), key);
            self.log_err(
                self.store.add_card_note(
                    actor,
                    key,
                    &format!(
                        "The worker {why} {count} times, so its session was detached. A human should review this card."
                    ),
                ),
                key,
            );
        } else {
            self.log_err(self.store.pause_task_session(actor, key, None), key);
        }
    }

    fn dispatch_all(&mut self, actor: &str, stop: &AtomicBool) -> Result<()> {
        while !stop.load(Ordering::SeqCst) {
            let session_id = uuid::Uuid::new_v4().to_string();
            let worker = format!("agent:{}-{}", self.runner.name(), &session_id[..8]);
            let next = self.store.orchestrator_dispatch(
                actor,
                &self.board,
                &worker,
                self.runner.name(),
                &session_id,
                None,
            )?;
            match next["dispatch"].as_str() {
                Some("claim") => {
                    let key = next["card"]["key"].as_str().unwrap_or_default().to_owned();
                    let prompt = claim_prompt(&self.board, &key, &worker);
                    self.launch(actor, &key, &worker, &session_id, prompt, false);
                }
                Some("resume") => {
                    if !self.resume(actor, &next["card"]) {
                        break;
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// Start a worker. A failure to start counts like a failed run.
    fn launch(
        &mut self,
        actor: &str,
        key: &str,
        worker_actor: &str,
        session_id: &str,
        prompt: String,
        resume: bool,
    ) -> bool {
        let launch = Launch {
            board_key: self.board.clone(),
            card_key: key.to_owned(),
            actor: worker_actor.to_owned(),
            session_id: session_id.to_owned(),
            prompt,
            resume,
        };
        match self.runner.start(&launch) {
            Ok(worker) => {
                self.live.insert(key.to_owned(), worker);
                true
            }
            Err(error) => {
                eprintln!("could not start worker for {key}: {error}");
                self.worker_failed(actor, key, "could not be started");
                false
            }
        }
    }

    /// Follow the documented resume protocol: read every briefing page, win the dispatch by
    /// resuming the session, start the worker, and only then acknowledge each page.
    fn resume(&mut self, actor: &str, card: &Value) -> bool {
        let key = card["key"].as_str().unwrap_or_default().to_owned();
        if self.live.contains_key(&key) {
            return false; // the previous process is still exiting; try again next pass
        }
        let owner = card["owner_actor"].as_str().unwrap_or_default().to_owned();
        let session_id = card["session"]["external_session_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let mut pages = Vec::new();
        let mut after = None;
        loop {
            let Ok(page) = self.store.task_session_briefing(&key, 100, after) else {
                return false;
            };
            let more = page["has_more"] == true;
            after = page["cursor"].as_i64();
            pages.push(page);
            if !more {
                break;
            }
        }
        if self.store.resume_task_session(actor, &key).is_err() {
            return false; // another dispatcher won, or the wait is no longer resolved
        }
        let prompt = resume_prompt(&key, &pages);
        if !self.launch(actor, &key, &owner, &session_id, prompt, true) {
            return false;
        }
        for page in &pages {
            if let Some(cursor) = page["cursor"].as_i64() {
                self.log_err(self.store.acknowledge_task_session(actor, &key, cursor), &key);
            }
        }
        true
    }

    fn shutdown(&mut self, actor: &str) {
        let keys: Vec<String> = self.live.keys().cloned().collect();
        for (_, worker) in self.live.iter_mut() {
            worker.stop();
        }
        self.live.clear();
        for key in keys {
            // Resumable later; ignore "already paused" or "no session".
            let _ = self.store.pause_task_session(actor, &key, None);
        }
        let _ = self.store.orchestrator_release(actor, &self.board);
    }

    fn log_err<T>(&self, result: Result<T>, key: &str) {
        if let Err(error) = result {
            eprintln!("{key}: {error}");
        }
    }
}

fn claim_prompt(board: &str, key: &str, actor: &str) -> String {
    format!(
        "You are working Workyard card {key} on board {board}. Your actor is {actor} (set in WORKYARD_ACTOR); \
the card is already claimed by you, In Progress, with this session attached.\n\
Use the `workyard` CLI (the workyard skill describes it). Read the card with `workyard --json card {key}`, \
do the work, and report with `workyard task progress {key} <0-100> --note \"...\"`.\n\
If you need a human decision, raise it with `workyard task ask ... --blocks {key}`, then run \
`workyard orch session pause {key} --waiting-question <Q-key>` and stop. You will be resumed with the answer.\n\
When finished, run `workyard task move {key} Done`. Do not work on other cards."
    )
}

fn resume_prompt(key: &str, pages: &[Value]) -> String {
    let events: Vec<&Value> = pages
        .iter()
        .flat_map(|page| page["events"].as_array().into_iter().flatten())
        .collect();
    format!(
        "Resume Workyard card {key}. You paused it earlier; it is In Progress and claimed by you again. \
Re-read it with `workyard --json card {key}`. These board events happened since you last saw it, \
including any answer to your question:\n{}\n\
Continue the work. Finish with `workyard task move {key} Done`, or pause again if you must wait.",
        serde_json::to_string(&events).unwrap_or_default()
    )
}
