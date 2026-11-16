//! The orchestrator loop for one board. It holds the board's lease, dispatches work through
//! the core's `orchestrator_dispatch`, starts and resumes workers through a `Runner`, and
//! repairs sessions whose worker exited without finishing or pausing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use workyard_core::{DEFAULT_LEASE_SECONDS, Result, Store};

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::notify::{Notifier, Silent};
use crate::runner::{LimitHit, Launch, Runner, Worker, WorkerState, Workspace};

pub struct Options {
    pub holder: String,
    /// Maximum concurrent workers; stored on the board and kept across restarts when `None`.
    pub pool: Option<i64>,
    pub lease_seconds: i64,
    /// Consecutive exits without finishing or pausing before the session is detached.
    pub max_failures: u32,
    /// Longest the loop sleeps when nothing commits (it also polls worker exit).
    pub idle_wait: Duration,
    /// Added to a limit's reset time before dispatching again.
    pub limit_margin: Duration,
    /// Backoff when a limit gives no reset time.
    pub limit_fallback: Duration,
    /// A worker that fails within this long of starting counts as a fast failure.
    pub fast_failure_window: Duration,
    /// Consecutive fast failures (across all cards) that stop dispatch for a while.
    pub max_fast_failures: u32,
    pub fast_failure_backoff: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            holder: "orchestrator".into(),
            pool: None,
            lease_seconds: DEFAULT_LEASE_SECONDS,
            max_failures: 3,
            idle_wait: Duration::from_secs(1),
            limit_margin: Duration::from_secs(5),
            limit_fallback: Duration::from_secs(30 * 60),
            fast_failure_window: Duration::from_secs(20),
            max_fast_failures: 3,
            fast_failure_backoff: Duration::from_secs(10 * 60),
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
    live: HashMap<String, Live>,
    failures: HashMap<String, u32>,
    fast_failures: u32,
    limit_streak: u32,
    notifier: Box<dyn Notifier>,
    board_id: String,
    /// Last board event this orchestrator has processed; persisted on the board so a restart
    /// neither repeats nor misses notifications and messages.
    cursor: i64,
}

struct Live {
    worker: Box<dyn Worker>,
    started: Instant,
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
            fast_failures: 0,
            limit_streak: 0,
            notifier: Box::new(Silent),
            board_id: String::new(),
            cursor: 0,
        }
    }

    /// Send desktop notifications for things that need a human.
    pub fn with_notifier(mut self, notifier: Box<dyn Notifier>) -> Self {
        self.notifier = notifier;
        self
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
        self.board_id = lease["board_id"].as_str().unwrap_or_default().to_owned();
        // The first run on a board starts from now rather than replaying its whole history.
        self.cursor = lease["cursor"].as_i64().unwrap_or(0);
        if self.cursor == 0 {
            self.cursor = self.store.latest_event_id().unwrap_or(0);
            let _ = self.store.orchestrator_set_cursor(&actor, &self.board, self.cursor);
        }
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
            self.pump_events(actor)?;
            for live in self.live.values_mut() {
                if live.worker.idle() {
                    live.worker.close_input();
                }
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
    /// (finished) is normal; a usage limit backs the whole board off; anything else is a
    /// failure of that card.
    fn reap(&mut self, actor: &str) {
        let mut exited = Vec::new();
        for (key, live) in self.live.iter_mut() {
            if let WorkerState::Exited { limit, .. } = live.worker.poll() {
                exited.push((key.clone(), live.started.elapsed(), limit));
            }
        }
        if exited.is_empty() {
            return;
        }
        let sessions = self
            .store
            .list_task_sessions(&self.board)
            .unwrap_or_else(|_| json!([]));
        for (key, ran_for, limit) in exited {
            self.live.remove(&key);
            let session = sessions
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["card_key"] == key.as_str());
            let unpaused = session.is_some_and(|entry| entry["session"]["paused_at"].is_null());
            if let Some(limit) = limit {
                // Not the card's fault: keep its session resumable and wait the limit out.
                self.limit_hit(actor, &limit);
                if unpaused {
                    self.log_err(self.store.pause_task_session(actor, &key, None), &key);
                }
            } else if unpaused {
                self.limit_streak = 0;
                if ran_for < self.opts.fast_failure_window {
                    self.fast_failure(actor);
                } else {
                    self.fast_failures = 0;
                }
                self.worker_failed(actor, &key, "exited without finishing or pausing");
            } else {
                self.limit_streak = 0;
                self.fast_failures = 0;
                self.failures.remove(&key);
            }
        }
    }

    /// Stop dispatching until the limit resets, using the reset time the runtime reported or,
    /// failing that, a doubling fallback.
    fn limit_hit(&mut self, actor: &str, limit: &LimitHit) {
        let until = match limit.resets_at.and_then(|t| DateTime::from_timestamp(t, 0)) {
            Some(reset) if reset > Utc::now() => {
                self.limit_streak = 0;
                reset + ChronoDuration::from_std(self.opts.limit_margin).unwrap_or_default()
            }
            _ => {
                self.limit_streak += 1;
                let factor = 1u32 << self.limit_streak.saturating_sub(1).min(3);
                Utc::now()
                    + ChronoDuration::from_std(self.opts.limit_fallback * factor).unwrap_or_default()
            }
        };
        self.set_backoff(actor, until, &format!("Claude usage limit ({})", limit.kind));
    }

    /// Several workers in a row dying within seconds points at something broken (auth,
    /// permissions, a bad flag), not at one card. Stop and let a human look rather than
    /// walk the backlog failing every card.
    fn fast_failure(&mut self, actor: &str) {
        self.fast_failures += 1;
        if self.fast_failures >= self.opts.max_fast_failures {
            let n = self.fast_failures;
            self.fast_failures = 0;
            let until = Utc::now()
                + ChronoDuration::from_std(self.opts.fast_failure_backoff).unwrap_or_default();
            self.set_backoff(
                actor,
                until,
                &format!(
                    "{n} workers in a row exited within {}s without finishing; check the worker logs",
                    self.opts.fast_failure_window.as_secs()
                ),
            );
        }
    }

    fn set_backoff(&self, actor: &str, until: DateTime<Utc>, reason: &str) {
        self.notifier.notify(
            "Workyard is waiting",
            &format!("{reason}. Dispatch resumes after {}.", until.with_timezone(&chrono::Local).format("%H:%M")),
        );
        self.log_err(
            self.store
                .orchestrator_set_backoff(actor, &self.board, Some(&until.to_rfc3339()), reason),
            &self.board,
        );
    }

    /// Pause the session so it can be resumed, or after repeated failures detach it and
    /// leave a note, so a broken card cannot keep the orchestrator in a restart loop.
    fn worker_failed(&mut self, actor: &str, key: &str, why: &str) {
        let count = self.failures.entry(key.to_owned()).or_insert(0);
        *count += 1;
        let count = *count;
        if count >= self.opts.max_failures {
            self.failures.remove(key);
            self.notifier.notify("Workyard: a card needs you", &format!("{key}: the worker {why} {count} times"));
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
                    let workspace = Workspace::from_card(&next["card"]);
                    let prompt = claim_prompt(&self.board, &key, &worker, workspace);
                    self.launch(actor, &key, &worker, &session_id, prompt, false, workspace);
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
        workspace: Workspace,
    ) -> bool {
        let launch = Launch {
            board_key: self.board.clone(),
            card_key: key.to_owned(),
            actor: worker_actor.to_owned(),
            session_id: session_id.to_owned(),
            prompt,
            resume,
            workspace,
        };
        match self.runner.start(&launch) {
            Ok(worker) => {
                self.live.insert(
                    key.to_owned(),
                    Live {
                        worker,
                        started: Instant::now(),
                    },
                );
                true
            }
            Err(error) => {
                eprintln!("could not start worker for {key}: {error}");
                self.fast_failure(actor);
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
        let workspace = Workspace::from_card(card);
        if !self.launch(actor, &key, &owner, &session_id, prompt, true, workspace) {
            return false;
        }
        for page in &pages {
            if let Some(cursor) = page["cursor"].as_i64() {
                self.log_err(self.store.acknowledge_task_session(actor, &key, cursor), &key);
            }
        }
        true
    }

    /// Read board events since the cursor: notify a human about what needs them, hand notes
    /// to the worker on that card, and stop workers whose card was cancelled, taken away, or
    /// taken over. The cursor is stored on the board so a restart carries on where it left off.
    fn pump_events(&mut self, actor: &str) -> Result<()> {
        loop {
            let page = self.store.poll_events(self.cursor, 200)?;
            for event in page["events"].as_array().into_iter().flatten() {
                if event["board_id"] == self.board_id.as_str() {
                    self.handle_event(event);
                }
            }
            let next = page["cursor"].as_i64().unwrap_or(self.cursor);
            if next > self.cursor {
                self.cursor = next;
                self.log_err(self.store.orchestrator_set_cursor(actor, &self.board, next), &self.board);
            }
            if page["has_more"] != true {
                return Ok(());
            }
        }
    }

    fn card_key(&self, card_id: &str) -> Option<String> {
        self.store.read_card(card_id).ok().map(|card| card.key)
    }

    fn stop_worker(&mut self, key: &str) {
        if let Some(mut live) = self.live.remove(key) {
            live.worker.stop();
        }
    }

    fn handle_event(&mut self, event: &Value) {
        let after = &event["after"];
        let entity = event["entity_id"].as_str().unwrap_or_default();
        match event["type"].as_str().unwrap_or_default() {
            "question_raised" => self.notifier.notify(
                "Workyard: a question needs you",
                &format!("{}: {}", after["key"].as_str().unwrap_or("?"), after["title"].as_str().unwrap_or("")),
            ),
            "card_note_added" => {
                let author = after["actor"].as_str().unwrap_or_default();
                let Ok(card) = self.store.read_card(entity) else { return };
                // Notes by the worker itself or by the orchestrator are not messages to it.
                if author.starts_with("orch:") || Some(author) == card.owner_actor.as_deref() {
                    return;
                }
                if let Some(live) = self.live.get_mut(&card.key) {
                    let message = format!(
                        "New note on {} from {author}: {}",
                        card.key,
                        after["body"].as_str().unwrap_or_default()
                    );
                    // If it cannot be delivered the note still reaches the worker in its
                    // briefing when the session next resumes.
                    let _ = live.worker.send(&message);
                }
            }
            "card_cancelled" | "card_force_released" | "card_transferred" => {
                if let Some(key) = self.card_key(entity) {
                    self.stop_worker(&key);
                }
            }
            "task_session_held" => {
                let card_id = event["effects"][0]["card_id"].as_str().unwrap_or_default();
                if let Some(key) = self.card_key(card_id) {
                    self.stop_worker(&key);
                }
            }
            "card_change_linked" | "card_change_updated" => {
                let commit = after["commit_oid"].as_str().unwrap_or_default();
                if after["review_required"] == true && !commit.is_empty() && after["approved_commit_oid"].as_str() != Some(commit) {
                    let key = after["card_id"].as_str().and_then(|id| self.card_key(id)).unwrap_or_default();
                    self.notifier.notify("Workyard: review needed", &format!("{key} has a commit waiting for your approval"));
                }
            }
            _ => {}
        }
    }

    fn shutdown(&mut self, actor: &str) {
        let keys: Vec<String> = self.live.keys().cloned().collect();
        for (_, live) in self.live.iter_mut() {
            live.worker.stop();
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

fn claim_prompt(board: &str, key: &str, actor: &str, workspace: Workspace) -> String {
    let place = match workspace {
        Workspace::Worktree => format!(
            "You work in your own git worktree on its own branch (see `git branch --show-current`); never touch another checkout. \
Commit your work there. Once you have commits, record them with `workyard task link-change {key} <repository-id> <your-branch> --review-required` \
(find the repository id with `workyard --json projects` then `workyard --json project <id>`), and run \
`workyard task update-change <change> <commit>` after each further commit. A human approves the commit in the desktop app; \
you cannot move the card to Done until they do."
        ),
        Workspace::None => format!(
            "This card is investigative: do not modify any repository. Report what you find with `workyard task note {key} \"...\"` \
and keep the status note current."
        ),
    };
    format!(
        "You are working Workyard card {key} on board {board}. Your actor is {actor} (set in WORKYARD_ACTOR); \
the card is already claimed by you, In Progress, with this session attached.\n\
Use the `workyard` CLI (the workyard skill describes it). Read the card with `workyard --json card {key}`, \
do the work, and report with `workyard task progress {key} <0-100> --note \"...\"`.\n\
{place}\n\
Messages that arrive while you work are notes a human left on this card; treat them as instructions about it.\n\
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
