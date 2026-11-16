//! The orchestrator loop for one board. It holds the board's lease, dispatches work through
//! the core's `orchestrator_dispatch`, starts and resumes workers through a `Runner`, and
//! repairs sessions whose worker exited without finishing or pausing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use workyard_core::{DEFAULT_LEASE_SECONDS, Result, Store};

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::notify::{Notifier, Silent};
use crate::runner::{LimitHit, Launch, PermissionAnswer, PermissionAsk, Runner, Worker, WorkerState, Workspace};
use crate::worktree::{branch_for, check_main_checkout, current_branch, remove_merged, worktree_path};

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
    /// Times a card whose session was detached after repeated failures is handed back to the
    /// backlog for a fresh worker (per card, per run) before a human is asked instead.
    pub max_reclaims: u32,
    /// Least time between activity updates published for one card.
    pub activity_interval: Duration,
    /// `full` or `safe`: how much workers may do without asking. Recorded on the board.
    pub permissions: String,
    /// Run a long-lived supervisor agent that is told what needs judgement (stuck workers,
    /// questions, failures) and acts through the `workyard` CLI. Costs tokens, so off by default.
    pub supervisor: bool,
    /// Leave a finished card's worktree and branch in place instead of removing them.
    pub keep_worktrees: bool,
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
            max_reclaims: 2,
            activity_interval: Duration::from_secs(2),
            permissions: "full".into(),
            supervisor: false,
            keep_worktrees: false,
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
    /// The supervisor agent, when enabled, and how many times it was started.
    supervisor: Option<Live>,
    supervisor_starts: u32,
    /// Things the supervisor should hear about, sent in one message every few seconds.
    digest: std::cell::RefCell<Vec<String>>,
    digest_sent: Instant,
    failures: HashMap<String, u32>,
    fast_failures: u32,
    limit_streak: u32,
    notifier: Box<dyn Notifier>,
    board_id: String,
    /// Last board event this orchestrator has processed; persisted on the board so a restart
    /// neither repeats nor misses notifications and messages.
    cursor: i64,
    /// The activity text last published per card, and when.
    published: HashMap<String, (String, Instant)>,
    /// Denials already announced, so each distinct one notifies once.
    denials_seen: HashSet<String>,
    reclaims: HashMap<String, u32>,
    /// Cards moved to Done whose worktree and branch are still to be removed.
    cleanup: HashSet<String>,
    /// The branch the main checkout was on when the run started; workers merge into it.
    expected_branch: Option<String>,
    /// The main-checkout problem already announced, so each distinct one notifies once.
    checkout_problem: Option<String>,
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
            supervisor: None,
            supervisor_starts: 0,
            digest: std::cell::RefCell::new(Vec::new()),
            digest_sent: Instant::now(),
            failures: HashMap::new(),
            fast_failures: 0,
            limit_streak: 0,
            notifier: Box::new(Silent),
            board_id: String::new(),
            cursor: 0,
            published: HashMap::new(),
            denials_seen: HashSet::new(),
            reclaims: HashMap::new(),
            cleanup: HashSet::new(),
            expected_branch: None,
            checkout_problem: None,
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
        self.log_err(
            self.store.orchestrator_set_permissions(&actor, &self.board, &self.opts.permissions),
            &self.board,
        );
        // The first run on a board starts from now rather than replaying its whole history.
        self.cursor = lease["cursor"].as_i64().unwrap_or(0);
        if self.cursor == 0 {
            self.cursor = self.store.latest_event_id().unwrap_or(0);
            let _ = self.store.orchestrator_set_cursor(&actor, &self.board, self.cursor);
        }
        // Requests left by a previous orchestrator have no worker waiting on them.
        self.expire_permission_requests(&actor, None);
        self.expected_branch = self.runner.checkout().as_deref().and_then(current_branch);
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
            self.publish_activity(actor);
            for live in self.live.values_mut() {
                if live.worker.idle() {
                    live.worker.close_input();
                }
            }
            self.reap(actor);
            self.remove_finished_worktrees();
            self.supervise();
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
            if let WorkerState::Exited { success, limit } = live.worker.poll() {
                exited.push((key.clone(), live.started.elapsed(), success, limit));
            }
        }
        if exited.is_empty() {
            return;
        }
        let sessions = self
            .store
            .list_task_sessions(&self.board)
            .unwrap_or_else(|_| json!([]));
        for (key, ran_for, success, limit) in exited {
            self.live.remove(&key);
            self.expire_permission_requests(actor, Some(&key));
            self.set_activity(actor, &key, "");
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
                // Only a worker that died (non-zero exit) quickly points at something broken.
                // One that ended its turn cleanly without finishing is that card's problem
                // (counted per card below), not a reason to stop the whole board.
                if !success && ran_for < self.opts.fast_failure_window {
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
        self.tell_supervisor(format!("Dispatch is backing off until {}: {reason}", until.format("%H:%M UTC")));
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
        self.tell_supervisor(format!(
            "The worker on {key} {why} (failure {count} of {}). Its log is in the board's .logs directory.",
            self.opts.max_failures
        ));
        if count >= self.opts.max_failures {
            self.failures.remove(key);
            self.log_err(self.store.detach_task_session(actor, key), key);
            let reclaims = self.reclaims.entry(key.to_owned()).or_insert(0);
            if *reclaims < self.opts.max_reclaims {
                *reclaims += 1;
                let attempt = *reclaims;
                // Hand the card back so the next dispatch starts a fresh worker on it; its
                // branch and worktree are reused, so nothing the old worker did is lost.
                let reason = format!("the worker {why} {count} times; starting again with a fresh worker (attempt {attempt})");
                self.log_err(self.store.add_card_note(actor, key, &format!("Workyard: {reason}.")), key);
                self.log_err(self.store.orchestrator_reclaim(actor, &self.board, key, &reason), key);
            } else {
                self.notifier.notify("Workyard: a card needs you", &format!("{key}: the worker {why} {count} times"));
                self.log_err(
                    self.store.add_card_note(
                        actor,
                        key,
                        &format!(
                            "The worker {why} {count} times, and a fresh worker did no better, so its session was detached. A human should review this card."
                        ),
                    ),
                    key,
                );
            }
        } else {
            self.log_err(self.store.pause_task_session(actor, key, None), key);
        }
    }

    /// Whether the main checkout can take a worker's fast-forward. A problem is announced once
    /// (notification, log, supervisor) and dispatch waits; it is re-checked every pass, so work
    /// resumes as soon as the checkout is put right.
    fn main_checkout_ready(&mut self) -> bool {
        let Some(main) = self.runner.checkout() else { return true };
        match check_main_checkout(&main, self.expected_branch.as_deref()) {
            Ok(()) => {
                if self.checkout_problem.take().is_some() {
                    eprintln!("main checkout is fine again; dispatching resumes");
                    self.tell_supervisor("The main checkout is fine again; dispatch has resumed.".into());
                }
                true
            }
            Err(problem) => {
                if self.checkout_problem.as_deref() != Some(&problem) {
                    eprintln!("not dispatching: {problem}");
                    self.notifier.notify(
                        "Workyard: dispatch is waiting on the main checkout",
                        &format!("{problem}. Workers merge into it, so no new card starts until it is clean, on its branch and not mid-merge."),
                    );
                    self.tell_supervisor(format!("Dispatch is paused: {problem}."));
                    self.checkout_problem = Some(problem);
                }
                false
            }
        }
    }

    fn dispatch_all(&mut self, actor: &str, stop: &AtomicBool) -> Result<()> {
        if !self.main_checkout_ready() {
            return Ok(());
        }
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
                    let prompt = claim_prompt(&self.board, &key, &worker, workspace, &self.prompt_context(&key));
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
            allow_rules: self.store.permission_run_rules(key).unwrap_or_default(),
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
        let prompt = resume_prompt(&key, &pages, &self.prompt_context(&key));
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
                    self.handle_event(actor, event);
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

    /// Where this card's worker works, for its prompt: real paths when the runner has a
    /// repository, and whether the board wants human review of commits.
    fn prompt_context(&self, key: &str) -> PromptContext {
        let board = self.store.list_boards().ok().and_then(|boards| {
            let list = boards.as_array().cloned().or_else(|| boards["boards"].as_array().cloned())?;
            list.into_iter().find(|b| b["key"] == self.board.as_str())
        });
        let review = board.as_ref().is_some_and(|board| board["review_required"] == true);
        let rebase = board.as_ref().is_some_and(|board| board["integration"] == "rebase");
        let main = self.runner.checkout();
        PromptContext {
            review,
            rebase,
            branch: branch_for(key),
            worktree: main.as_ref().map(|m| worktree_path(m, key)),
            target: main.as_deref().and_then(current_branch),
            main,
        }
    }

    /// Show each live worker's current activity on its card, only when it changed and no
    /// more often than `activity_interval`. A refused tool is announced once and shown as
    /// "blocked" until the worker does something else.
    fn publish_activity(&mut self, actor: &str) {
        let mut updates = Vec::new();
        let mut asks = Vec::new();
        for (key, live) in self.live.iter_mut() {
            asks.extend(live.worker.take_permission_asks().into_iter().map(|ask| (key.clone(), ask)));
            for denial in live.worker.take_denials() {
                if self.denials_seen.insert(format!("{key}: {denial}")) {
                    self.notifier.notify("Workyard: a tool was refused", &format!("{key}: {denial}"));
                    if self.opts.supervisor {
                        self.digest.borrow_mut().push(format!("A tool was refused for the worker on {key}: {denial}"));
                    }
                }
                updates.push((key.clone(), format!("blocked: {denial}"), true));
            }
            if let Some(text) = live.worker.activity() {
                updates.push((key.clone(), text, false));
            }
        }
        for (key, ask) in asks {
            self.open_permission_request(actor, &key, &ask);
        }
        for (key, text, urgent) in updates {
            let due = match self.published.get(&key) {
                Some((last, at)) => *last != text && (urgent || at.elapsed() >= self.opts.activity_interval),
                None => true,
            };
            if due {
                self.set_activity(actor, &key, &text);
            }
        }
    }

    /// A worker is waiting for a human to approve a tool: record the request (a human decides
    /// it with `orch perms decide` or in the desktop app) and say so.
    fn open_permission_request(&mut self, actor: &str, key: &str, ask: &PermissionAsk) {
        let opened = self.store.permission_request_open(
            actor,
            &self.board,
            key,
            &ask.request_id,
            &ask.tool_use_id,
            &ask.tool_name,
            &ask.input,
            &ask.description,
        );
        match opened {
            Ok(_) => self.notifier.notify("Workyard: a tool needs your approval", &format!("{key}: {}", ask.description)),
            Err(error) => {
                // Nobody could decide it, so refuse rather than leave the worker blocked.
                eprintln!("{key}: {error}");
                let answer = PermissionAnswer::Deny { message: "Workyard could not record this request".into() };
                if let Some(live) = self.live.get_mut(key) {
                    let _ = live.worker.answer_permission(&ask.request_id, &answer);
                }
            }
        }
    }

    /// A human decided a request: answer the worker that is waiting on it.
    fn answer_permission(&mut self, request: &Value) {
        let (Some(key), Some(request_id)) = (request["card_key"].as_str(), request["request_id"].as_str()) else {
            return;
        };
        let answer = match request["status"].as_str() {
            Some("allowed_once") => PermissionAnswer::Allow { rule: None },
            Some("allowed_run") => PermissionAnswer::Allow {
                rule: request["run_rule"].as_str().filter(|r| !r.is_empty()).map(str::to_owned),
            },
            Some("denied") => PermissionAnswer::Deny {
                message: match request["reason"].as_str().filter(|r| !r.is_empty()) {
                    Some(reason) => format!("Denied by a human: {reason}"),
                    None => "Denied by a human".into(),
                },
            },
            _ => return,
        };
        if let Some(live) = self.live.get_mut(key) {
            if let Err(error) = live.worker.answer_permission(request_id, &answer) {
                eprintln!("{key}: could not answer the worker: {error}");
            }
        }
    }

    /// Close requests nobody can answer any more because their worker is gone.
    fn expire_permission_requests(&self, actor: &str, key: Option<&str>) {
        self.log_err(self.store.permission_requests_expire(actor, &self.board, key), key.unwrap_or(&self.board));
    }

    fn set_activity(&mut self, actor: &str, key: &str, text: &str) {
        self.published.insert(key.to_owned(), (text.to_owned(), Instant::now()));
        self.log_err(self.store.orchestrator_set_activity(actor, &self.board, key, text), key);
    }

    fn card_key(&self, card_id: &str) -> Option<String> {
        self.store.read_card(card_id).ok().map(|card| card.key)
    }

    fn in_done_lane(&self, lane_id: &str) -> bool {
        let Ok(board) = self.store.view_board(&self.board) else { return false };
        board["lanes"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|lane| lane["id"] == lane_id && lane["kind"] == "done")
    }

    /// Remove the worktree and branch of cards that are Done. It waits for the card's worker
    /// to exit, and never touches a held session, a dirty worktree, or a branch that is not
    /// on the main checkout's branch (`remove_merged` refuses those); what it removes or
    /// leaves is logged.
    fn remove_finished_worktrees(&mut self) {
        if self.cleanup.is_empty() {
            return;
        }
        let Some(main) = self.runner.checkout() else {
            self.cleanup.clear();
            return;
        };
        let held: HashSet<String> = self
            .store
            .list_task_sessions(&self.board)
            .ok()
            .and_then(|sessions| sessions.as_array().cloned())
            .into_iter()
            .flatten()
            .filter(|entry| !entry["session"]["held_by"].is_null())
            .filter_map(|entry| entry["card_key"].as_str().map(str::to_owned))
            .collect();
        for key in self.cleanup.clone() {
            if self.live.contains_key(&key) || held.contains(&key) {
                continue; // try again once the worker is gone or the hold is released
            }
            self.cleanup.remove(&key);
            let done = self.store.read_card(&key).is_ok_and(|card| self.in_done_lane(&card.lane_id.to_string()));
            if !done {
                continue; // moved back out of Done meanwhile
            }
            match remove_merged(&main, &key) {
                Ok(Some(what)) => eprintln!("{key}: removed {what}"),
                Ok(None) => {}
                Err(error) => eprintln!("{key}: kept its worktree and branch: {error}"),
            }
        }
    }

    fn stop_worker(&mut self, actor: &str, key: &str) {
        if let Some(mut live) = self.live.remove(key) {
            live.worker.stop();
            self.expire_permission_requests(actor, Some(key));
        }
    }

    fn handle_event(&mut self, actor: &str, event: &Value) {
        let after = &event["after"];
        let entity = event["entity_id"].as_str().unwrap_or_default();
        match event["type"].as_str().unwrap_or_default() {
            "question_raised" => {
                self.notifier.notify(
                    "Workyard: a question needs you",
                    &format!("{}: {}", after["key"].as_str().unwrap_or("?"), after["title"].as_str().unwrap_or("")),
                );
                self.tell_supervisor(format!(
                    "Question {} ({}) raised: {} Options: {}. Recommended: {}.",
                    after["key"].as_str().unwrap_or("?"),
                    after["kind"].as_str().unwrap_or("?"),
                    after["title"].as_str().unwrap_or(""),
                    after["options"],
                    after["recommended_option"].as_str().unwrap_or("none"),
                ));
            }
            "card_reclaimed" => {
                let key = self.card_key(entity).unwrap_or_default();
                self.tell_supervisor(format!("{key} was handed back to the backlog after its worker kept failing."));
            }
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
                    self.stop_worker(actor, &key);
                }
            }
            "card_moved" => {
                if !self.opts.keep_worktrees && self.in_done_lane(after["lane_id"].as_str().unwrap_or_default()) {
                    if let Some(key) = after["key"].as_str() {
                        self.cleanup.insert(key.to_owned());
                    }
                }
            }
            "permission_decided" => self.answer_permission(after),
            "task_session_held" => {
                let card_id = event["effects"][0]["card_id"].as_str().unwrap_or_default();
                if let Some(key) = self.card_key(card_id) {
                    self.stop_worker(actor, &key);
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

    /// Queue something for the supervisor agent; a no-op when none is configured.
    fn tell_supervisor(&self, line: String) {
        if self.opts.supervisor {
            self.digest.borrow_mut().push(line);
        }
    }

    /// Keep the supervisor agent running and hand it what happened, batched. The script keeps
    /// the lease and every process; the supervisor only talks to the board through the CLI.
    fn supervise(&mut self) {
        if !self.opts.supervisor {
            return;
        }
        if let Some(live) = self.supervisor.as_mut() {
            if matches!(live.worker.poll(), WorkerState::Exited { .. }) {
                self.supervisor = None;
            } else {
                // The supervisor has no card and no human to ask: refuse, rather than leave it
                // blocked on an answer that never comes.
                for ask in live.worker.take_permission_asks() {
                    let message = "The supervisor may only use its allowed tools".to_owned();
                    let _ = live.worker.answer_permission(&ask.request_id, &PermissionAnswer::Deny { message });
                }
            }
        }
        if self.supervisor.is_none() && self.supervisor_starts < 3 {
            self.supervisor_starts += 1;
            let session_id = uuid::Uuid::new_v4().to_string();
            let launch = Launch {
                board_key: self.board.clone(),
                card_key: "SUPERVISOR".into(),
                actor: format!("agent:supervisor-{}", self.board.to_lowercase()),
                session_id,
                prompt: supervisor_prompt(&self.board, &self.opts.permissions, self.prompt_context("").rebase),
                resume: false,
                workspace: Workspace::None,
                allow_rules: Vec::new(),
            };
            match self.runner.start(&launch) {
                Ok(worker) => self.supervisor = Some(Live { worker, started: Instant::now() }),
                Err(error) => eprintln!("could not start the supervisor: {error}"),
            }
            if self.supervisor.is_none() && self.supervisor_starts >= 3 {
                self.notifier.notify("Workyard", "The supervisor agent could not be kept running; continuing without it.");
            }
        }
        if self.digest_sent.elapsed() < Duration::from_secs(5) || self.digest.borrow().is_empty() {
            return;
        }
        let Some(live) = self.supervisor.as_mut() else { return };
        let lines: Vec<String> = std::mem::take(&mut *self.digest.borrow_mut());
        let message = format!("Board {} events needing your judgement:\n- {}", self.board, lines.join("\n- "));
        if live.worker.send(&message).is_ok() {
            self.digest_sent = Instant::now();
        } else {
            self.digest.borrow_mut().splice(0..0, lines);
        }
    }

    fn shutdown(&mut self, actor: &str) {
        let keys: Vec<String> = self.live.keys().cloned().collect();
        for (_, live) in self.live.iter_mut() {
            live.worker.stop();
        }
        self.live.clear();
        if let Some(mut supervisor) = self.supervisor.take() {
            supervisor.worker.stop();
        }
        self.expire_permission_requests(actor, None);
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

/// What a prompt needs to know about where the worker is and what finishing involves.
struct PromptContext {
    /// The board wants a human to approve commits before they are merged.
    review: bool,
    /// The board integrates by rebasing the card branch onto the target, not merging the target in.
    rebase: bool,
    branch: String,
    main: Option<PathBuf>,
    worktree: Option<PathBuf>,
    /// The branch checked out in the main checkout: where finished work lands.
    target: Option<String>,
}

fn integration_steps(key: &str, ctx: &PromptContext) -> String {
    let main = ctx
        .main
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "the first entry of `git worktree list`".into());
    let target = ctx.target.clone().unwrap_or_else(|| "main".into());
    let branch = &ctx.branch;
    let (bring, why) = if ctx.rebase {
        (
            format!("run `git rebase {target}` and resolve any conflicts yourself (`git rebase --continue`), never leaving a merge commit"),
            "This board keeps history linear: rebase, do not merge.",
        )
    } else {
        (format!("run `git merge {target}` and resolve any conflicts yourself"), "")
    };
    format!(
        "Finishing means your work is on {target}. {why} Loop: (1) in your worktree {bring}, \
then run the project's tests; (2) run `git -C {main} merge --ff-only {branch}`; if it refuses because {target} moved meanwhile, go back to (1). \
Then run `workyard task move {key} Done`; it is refused with `not_integrated` until your branch is on {target}. \
If the repository documents a merge queue or merge train (CONTRIBUTING, CLAUDE.md, docs/), follow that instead of step (2); if your work lands later, \
run `workyard orch session pause {key} --for-integration` and stop, and you will be resumed once it is on {target}."
    )
}

fn claim_prompt(board: &str, key: &str, actor: &str, workspace: Workspace, ctx: &PromptContext) -> String {
    let place = match workspace {
        Workspace::Worktree => {
            let home = match (&ctx.worktree, &ctx.main) {
                (Some(worktree), Some(main)) => format!(
                    "You work in your own git worktree {} on branch {} (already created; your shell starts there). The main checkout is {}; do not edit files in it, only merge into it as described below.",
                    worktree.display(),
                    ctx.branch,
                    main.display()
                ),
                _ => format!(
                    "You work in your own git worktree on its own branch (see `git branch --show-current`); the main checkout is the first entry of `git worktree list`, and you do not edit files in it."
                ),
            };
            let review = if ctx.review {
                format!(
                    "\nThis board requires human review of commits: once your work is committed and linked, run `workyard orch session pause {key} --for-review` and stop. \
You are resumed once a human approves it; only then integrate and finish."
                )
            } else {
                String::new()
            };
            format!(
                "{home}\nCommit your work in the worktree. Once you have commits, record them with `workyard task link-change {key} <repository-id> {}` \
(find the repository id with `workyard --json projects` then `workyard --json project <id>`), and run `workyard task update-change <change> <commit>` after each further commit.{review}\n{}",
                ctx.branch,
                integration_steps(key, ctx)
            )
        }
        Workspace::None => format!(
            "This card is investigative: do not modify any repository. Report what you find with `workyard task note {key} \"...\"` \
and keep the status note current."
        ),
    };
    format!(
        "You are working Workyard card {key} on board {board}. Your actor is {actor} (set in WORKYARD_ACTOR); \
the card is already claimed by you, In Progress, with this session attached.\n\
Create and change files with your Write and Edit tools, not shell heredocs: they are more reliable here. Use the `workyard` CLI (the workyard skill describes it). Read the card with `workyard --json card {key}`, \
do the work, and report honestly with `workyard task progress {key} <0-100> --note \"...\"`: after you have planned, after the main work, after the tests pass; never 100 before the work is integrated.\n\
{place}\n\
Messages that arrive while you work are notes a human left on this card; treat them as instructions about it.\n\
Do not raise a question for a mechanical blocker: a refused tool or command, a merge conflict you can resolve, a failing test you can fix. Work around it or fix it. \
Ask only for a real decision that a human has to make: `workyard task ask ... --blocks {key}`, then `workyard orch session pause {key} --waiting-question <Q-key>` and stop. You will be resumed with the answer.\n\
When finished, run `workyard task move {key} Done`. Do not work on other cards."
    )
}

fn resume_prompt(key: &str, pages: &[Value], ctx: &PromptContext) -> String {
    let events: Vec<&Value> = pages
        .iter()
        .flat_map(|page| page["events"].as_array().into_iter().flatten())
        .collect();
    format!(
        "Resume Workyard card {key}. You paused it earlier; it is In Progress and claimed by you again. \
Re-read it with `workyard --json card {key}`. These board events happened since you last saw it, \
including any answer to your question:\n{}\n\
Continue the work. If your work is not on the target branch yet, integrate it first. {}\n\
If you must wait again, pause (`workyard orch session pause {key} --waiting-question <Q-key>`{} or `--for-integration`; flags combine, and you resume only once every one holds) and stop. \
Do not raise a question for a mechanical blocker; fix it or work around it.",
        serde_json::to_string(&events).unwrap_or_default(),
        integration_steps(key, ctx),
        if ctx.review { ", `--for-review`" } else { "" },
    )
}

/// What the supervisor agent is told once at start. It is woken later by event digests.
fn supervisor_prompt(board: &str, permissions: &str, rebase: bool) -> String {
    let integration = if rebase {
        "Workers on this board integrate by rebasing onto the target, so a note about a conflict should say to rebase, not merge."
    } else {
        "Workers on this board integrate by merging the target into their branch."
    };
    format!(
        "You are the supervisor agent for Workyard board {board}. A script runs the board (leases, worker processes, \
limits, backoff); you do the thinking it cannot. You stay running. Messages arrive as lists of board events that \
need judgement: a worker that kept exiting unfinished, a question, a refused tool, a card handed back to the backlog, a backoff. \
Between messages do nothing and spend nothing.\n\
Use only the `workyard` CLI (read with `--json`: `board {board}`, `card <key>`, `questions {board}`, `events`). You may:\n\
- steer a worker with `workyard task note <card> \"...\"`: a note reaches a running worker at its next step. Say concretely how to get \
around the problem (use Write/Edit instead of a heredoc, a different command, resolve the conflict, run the tests another way).\n\
- answer an engineering question with `workyard admin answer` ONLY when its kind is engineering, it has a recommended option, and you \
agree with it. Every other question (business, stakeholder, a choice that costs money or changes scope) is for the human: do nothing, \
they are already notified.\n\
- note on a card what you learned when a card keeps failing, so the human can read it.\n\
You must NOT: approve changes or reviews, move cards between lanes, cancel or force-release cards, change code or files, run git, \
start or stop processes, or answer for a human. If something needs one of those, leave a note on the card saying what and why. \
Permissions for workers are `{permissions}`. {integration} Keep every action small and say in one line what you did and why.\n\
Reply 'ready' now."
    )
}

#[cfg(test)]
mod integration_prompt_tests {
    use super::*;

    fn ctx(rebase: bool) -> PromptContext {
        PromptContext { review: false, rebase, branch: "workyard/c-1".into(), main: None, worktree: None, target: Some("main".into()) }
    }

    #[test]
    fn prompt_follows_the_boards_integration() {
        let merge = integration_steps("C-1", &ctx(false));
        assert!(merge.contains("git merge main") && !merge.contains("git rebase"));
        let rebase = integration_steps("C-1", &ctx(true));
        assert!(rebase.contains("git rebase main") && !rebase.contains("git merge main"));
        assert!(rebase.contains("--ff-only") && rebase.contains("not_integrated"));
    }
}
