//! Claude Code backend: `claude -p` runs headless on the user's existing login, so no API key
//! is needed. A new worker uses `--session-id`; a resumed one uses `--resume`.
//!
//! Workers run with `--input-format stream-json --output-format stream-json --verbose`.
//!
//! Output: every line is appended to the worker's log and read as JSON, so a usage limit is
//! recognised from structured fields (`rate_limit_event`, `result.api_error_status`), never
//! from message text.
//!
//! Input: the first prompt and any later message are JSON user messages on stdin. Claude Code
//! picks a message up at the next tool boundary of a running turn (or as the next turn), which
//! is how a human's note reaches a worker without restarting it. The process stays alive after
//! a turn until stdin closes, so the engine closes it once the worker is idle.
//!
//! Workspace: a card that needs a worktree runs in `<repo>/.workyard/worktrees/<key>` on branch
//! `workyard/<key>`, created by Workyard (see `worktree`) and reused when the session resumes.
//! The runtime is deliberately not given `--worktree`: its isolation would stop the worker
//! merging into the main checkout, which is part of finishing a card.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::runner::{LimitHit, Launch, Runner, Worker, WorkerState, Workspace};
use crate::worktree::ensure_worktree;

pub struct ClaudeRunner {
    pub bin: String,
    /// Extra arguments, for example permission settings and allowed tools. Do not pass the
    /// input or output format, or `--worktree`: the runner owns them.
    pub args: Vec<String>,
    pub db: PathBuf,
    pub cwd: Option<PathBuf>,
    /// Each worker's output is appended to `<log_dir>/<card>-<session>.log`.
    pub log_dir: PathBuf,
    /// Directory holding a stable copy of `workyard`, put first on each worker's PATH. Without
    /// it, workers use the directory of the running executable, which a rebuild can replace.
    pub bin_dir: Option<PathBuf>,
}

/// The PATH a worker gets: `dir` first, then whatever PATH the orchestrator has.
fn worker_path(dir: PathBuf, existing: Option<std::ffi::OsString>) -> Option<std::ffi::OsString> {
    let mut paths = vec![dir];
    if let Some(existing) = existing {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).ok()
}

/// Does this stream-json line say the run hit a usage limit?
///
/// - `rate_limit_event` with `rate_limit_info.status == "rejected"`: the subscription window
///   is exhausted; `resetsAt` (Unix seconds) and `rateLimitType` say when and which.
///   `allowed` and `allowed_warning` are not limits.
/// - the final `result` with `api_error_status == 429`, or an assistant message whose
///   `error` is `rate_limit`: a rate limit with no reset time.
pub fn limit_signal(line: &Value) -> Option<LimitHit> {
    match line["type"].as_str()? {
        "rate_limit_event" => {
            let info = &line["rate_limit_info"];
            (info["status"] == "rejected").then(|| LimitHit {
                resets_at: info["resetsAt"].as_f64().map(|t| t as i64),
                kind: info["rateLimitType"].as_str().unwrap_or("usage").to_owned(),
            })
        }
        "result" if line["api_error_status"] == 429 => Some(LimitHit {
            resets_at: None,
            kind: "api_429".into(),
        }),
        "assistant" if line["error"] == "rate_limit" || line["message"]["error"] == "rate_limit" => {
            Some(LimitHit {
                resets_at: None,
                kind: "rate_limit".into(),
            })
        }
        _ => None,
    }
}

/// Keep the most informative hit: one that knows its reset time beats one that does not.
fn merge(slot: &mut Option<LimitHit>, hit: LimitHit) {
    match slot {
        Some(existing) if existing.resets_at.is_some() || hit.resets_at.is_none() => {}
        _ => *slot = Some(hit),
    }
}

/// How long after a `result` with nothing following it a worker counts as idle. A queued
/// message starts its next turn at once, so any later event resets the clock.
const IDLE_AFTER: Duration = Duration::from_millis(1500);

struct Activity {
    last_event: Instant,
    last_was_result: bool,
    /// What the worker last did, and how many tools it has used this run.
    doing: String,
    tools: u32,
    denials: Vec<String>,
}

/// What one stream-json line tells us about the worker's current activity.
#[derive(Debug, PartialEq)]
pub enum Observed {
    /// A tool call, described briefly ("Edit app.mjs", "Bash: node --test").
    Tool(String),
    Thinking,
    /// A tool request the runtime refused.
    Denied(String),
    /// The worker finished a turn.
    TurnDone,
}

const DENIAL_PHRASES: &[&str] = &[
    "requires approval",
    "isolated in the worktree",
    "was blocked",
    "Contains brace with quote character",
];

fn clip(text: &str, n: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= n {
        one_line
    } else {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    }
}

fn describe_tool(name: &str, input: &Value) -> String {
    let file = input["file_path"].as_str().or(input["path"].as_str()).map(|p| {
        std::path::Path::new(p).file_name().and_then(|f| f.to_str()).unwrap_or(p).to_owned()
    });
    match name {
        "Bash" => format!("Bash: {}", clip(input["command"].as_str().unwrap_or(""), 60)),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => match file {
            Some(file) => format!("{name} {file}"),
            None => name.to_owned(),
        },
        "Grep" | "Glob" => format!("{name} {}", clip(input["pattern"].as_str().unwrap_or(""), 40)),
        other => other.to_owned(),
    }
}

/// Read one stream-json line for activity. Structured fields only: tool_use blocks in
/// assistant messages, is_error tool_result blocks in user messages, and the final result.
pub fn observe(line: &Value) -> Vec<Observed> {
    let mut seen = Vec::new();
    match line["type"].as_str().unwrap_or_default() {
        "assistant" => {
            for block in line["message"]["content"].as_array().into_iter().flatten() {
                match block["type"].as_str().unwrap_or_default() {
                    "tool_use" => seen.push(Observed::Tool(describe_tool(
                        block["name"].as_str().unwrap_or("tool"),
                        &block["input"],
                    ))),
                    "thinking" | "text" => seen.push(Observed::Thinking),
                    _ => {}
                }
            }
        }
        "user" => {
            for block in line["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] != "tool_result" || block["is_error"] != true {
                    continue;
                }
                let text = match &block["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" "),
                    _ => String::new(),
                };
                if DENIAL_PHRASES.iter().any(|phrase| text.contains(phrase)) {
                    seen.push(Observed::Denied(clip(&text, 140)));
                }
            }
        }
        "result" => seen.push(Observed::TurnDone),
        _ => {}
    }
    seen
}

fn apply(activity: &mut Activity, seen: Vec<Observed>) {
    for item in seen {
        match item {
            Observed::Tool(text) => {
                activity.tools += 1;
                activity.doing = text;
            }
            Observed::Thinking => activity.doing = "thinking".into(),
            Observed::Denied(text) => activity.denials.push(text),
            Observed::TurnDone => activity.doing = "idle".into(),
        }
    }
}

struct ChildWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    activity: Arc<Mutex<Activity>>,
    reader_done: Arc<AtomicBool>,
    limit: Arc<Mutex<Option<LimitHit>>>,
    exited_at: Option<Instant>,
}

impl ChildWorker {
    fn limit(&self) -> Option<LimitHit> {
        self.limit.lock().ok().and_then(|slot| slot.clone())
    }
}

impl Worker for ChildWorker {
    fn poll(&mut self) -> WorkerState {
        match self.child.try_wait() {
            Ok(None) => WorkerState::Running,
            Ok(Some(status)) => {
                // Give the reader a moment to drain the pipe so the final lines are seen.
                let since = *self.exited_at.get_or_insert_with(Instant::now);
                if !self.reader_done.load(Ordering::SeqCst) && since.elapsed() < Duration::from_secs(2) {
                    return WorkerState::Running;
                }
                WorkerState::Exited {
                    success: status.success(),
                    limit: self.limit(),
                }
            }
            Err(_) => WorkerState::Exited {
                success: false,
                limit: self.limit(),
            },
        }
    }

    fn stop(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn send(&mut self, message: &str) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "input closed"))?;
        write_user_message(stdin, message)?;
        if let Ok(mut activity) = self.activity.lock() {
            activity.last_was_result = false;
            activity.last_event = Instant::now();
        }
        Ok(())
    }

    fn idle(&mut self) -> bool {
        self.activity
            .lock()
            .is_ok_and(|a| a.last_was_result && a.last_event.elapsed() >= IDLE_AFTER)
    }

    fn close_input(&mut self) {
        self.stdin = None;
    }

    fn activity(&mut self) -> Option<String> {
        let a = self.activity.lock().ok()?;
        if a.doing.is_empty() {
            return None;
        }
        Some(if a.tools > 0 { format!("{} ({} tool uses)", a.doing, a.tools) } else { a.doing.clone() })
    }

    fn take_denials(&mut self) -> Vec<String> {
        self.activity.lock().map(|mut a| std::mem::take(&mut a.denials)).unwrap_or_default()
    }
}

fn write_user_message(stdin: &mut ChildStdin, text: &str) -> std::io::Result<()> {
    let message = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
    });
    writeln!(stdin, "{message}")?;
    stdin.flush()
}

impl Runner for ClaudeRunner {
    fn name(&self) -> &str {
        "claude"
    }

    fn checkout(&self) -> Option<PathBuf> {
        let dir = match &self.cwd {
            Some(dir) => dir.clone(),
            None => std::env::current_dir().ok()?,
        };
        std::fs::canonicalize(dir).ok()
    }

    fn start(&self, launch: &Launch) -> std::io::Result<Box<dyn Worker>> {
        fs::create_dir_all(&self.log_dir)?;
        let mut log = OpenOptions::new().create(true).append(true).open(self.log_dir.join(format!(
            "{}-{}.log",
            launch.card_key,
            &launch.session_id[..launch.session_id.len().min(8)]
        )))?;
        let mut command = Command::new(&self.bin);
        command.arg("-p").args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
        command
            .arg(if launch.resume { "--resume" } else { "--session-id" })
            .arg(&launch.session_id)
            .args(&self.args)
            .env("WORKYARD_ACTOR", &launch.actor)
            .env("WORKYARD_DB", &self.db)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log.try_clone()?);
        // The worker must find a `workyard` that a rebuild of the orchestrator cannot replace.
        let dir = self
            .bin_dir
            .clone()
            .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)));
        if let Some(joined) = dir.and_then(|dir| worker_path(dir, std::env::var_os("PATH"))) {
            command.env("PATH", joined);
        }
        match (launch.workspace, &self.cwd) {
            // The worker's own checkout; the main one stays free for it to merge into.
            (Workspace::Worktree, cwd) => {
                let main = match cwd {
                    Some(dir) => dir.clone(),
                    None => std::env::current_dir()?,
                };
                command.current_dir(ensure_worktree(&main, &launch.card_key)?);
            }
            (Workspace::None, Some(cwd)) => {
                command.current_dir(cwd);
            }
            (Workspace::None, None) => {}
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut stdin = child.stdin.take().expect("stdin is piped");
        write_user_message(&mut stdin, &launch.prompt)?;
        let limit = Arc::new(Mutex::new(None));
        let reader_done = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(Mutex::new(Activity {
            last_event: Instant::now(),
            last_was_result: false,
            doing: String::new(),
            tools: 0,
            denials: Vec::new(),
        }));
        {
            let (limit, done, activity) = (Arc::clone(&limit), Arc::clone(&reader_done), Arc::clone(&activity));
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = writeln!(log, "{line}");
                    let parsed = serde_json::from_str::<Value>(&line).ok();
                    if let Ok(mut a) = activity.lock() {
                        a.last_event = Instant::now();
                        a.last_was_result = parsed.as_ref().is_some_and(|v| v["type"] == "result");
                        if let Some(v) = &parsed {
                            apply(&mut a, observe(v));
                        }
                    }
                    if let Some(hit) = parsed.as_ref().and_then(limit_signal) {
                        if let Ok(mut slot) = limit.lock() {
                            merge(&mut slot, hit);
                        }
                    }
                }
                done.store(true, Ordering::SeqCst);
            });
        }
        Ok(Box::new(ChildWorker {
            child,
            stdin: Some(stdin),
            activity,
            reader_done,
            limit,
            exited_at: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn worker_path_starts_with_the_stable_copy() {
        let path = worker_path(PathBuf::from("/stable/bin"), Some("/usr/bin:/bin".into())).unwrap();
        let parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(parts[0], PathBuf::from("/stable/bin"));
        assert_eq!(parts[1..], [PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    }

    #[test]
    fn healthy_events_are_not_limits() {
        // Captured from a real run: an `allowed` event carries the window, not a limit.
        let allowed = json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1794826200,"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"utilization":0.05,"resetsAt":1794826200}}}});
        let warning = json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":1794826200,"rateLimitType":"five_hour"}});
        let done = json!({"type":"result","subtype":"success","is_error":false,"api_error_status":null,"terminal_reason":"completed"});
        for line in [allowed, warning, done, json!({"type":"assistant"}), json!({"no":"type"})] {
            assert_eq!(limit_signal(&line), None, "{line}");
        }
    }

    #[test]
    fn a_rejected_window_reports_when_it_resets() {
        let rejected = json!({"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1794826200,"rateLimitType":"five_hour"}});
        assert_eq!(
            limit_signal(&rejected),
            Some(LimitHit { resets_at: Some(1794826200), kind: "five_hour".into() })
        );
    }

    #[test]
    fn a_429_result_or_assistant_error_is_a_limit_without_a_reset_time() {
        let result = json!({"type":"result","subtype":"success","is_error":true,"api_error_status":429,"terminal_reason":"api_error"});
        let assistant = json!({"type":"assistant","error":"rate_limit"});
        for line in [result, assistant] {
            let hit = limit_signal(&line).expect("limit");
            assert_eq!(hit.resets_at, None);
        }
        // Other API errors are faults, not limits.
        assert_eq!(limit_signal(&json!({"type":"result","api_error_status":500})), None);
    }

    #[test]
    fn tool_calls_become_short_descriptions() {
        let edit = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/repo/src/app.mjs"}}]}});
        assert_eq!(observe(&edit), vec![Observed::Tool("Edit app.mjs".into())]);
        let bash = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"node --test"}}]}});
        assert_eq!(observe(&bash), vec![Observed::Tool("Bash: node --test".into())]);
        let text = json!({"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}});
        assert_eq!(observe(&text), vec![Observed::Thinking]);
        assert_eq!(observe(&json!({"type":"result","subtype":"success"})), vec![Observed::TurnDone]);
        let mut a = Activity { last_event: Instant::now(), last_was_result: false, doing: String::new(), tools: 0, denials: vec![] };
        apply(&mut a, observe(&edit));
        apply(&mut a, observe(&bash));
        assert_eq!((a.doing.as_str(), a.tools), ("Bash: node --test", 2));
    }

    #[test]
    fn refused_tools_are_recognised_from_error_results_only() {
        let denied = json!({"type":"user","message":{"content":[{"type":"tool_result","is_error":true,"content":"Claude requested permissions to use Bash, but you haven't granted it yet. This command requires approval"}]}});
        assert!(matches!(observe(&denied).as_slice(), [Observed::Denied(t)] if t.contains("requires approval")));
        let blocked = json!({"type":"user","message":{"content":[{"type":"tool_result","is_error":true,"content":[{"type":"text","text":"This session is isolated in the worktree"}]}]}});
        assert!(matches!(observe(&blocked).as_slice(), [Observed::Denied(_)]));
        // The same words in a successful result (a file that mentions them) are not a denial.
        let fine = json!({"type":"user","message":{"content":[{"type":"tool_result","content":"the docs say it requires approval"}]}});
        assert!(observe(&fine).is_empty());
    }

    #[test]
    fn a_hit_with_a_reset_time_wins_over_one_without() {
        let mut slot = None;
        merge(&mut slot, LimitHit { resets_at: None, kind: "api_429".into() });
        merge(&mut slot, LimitHit { resets_at: Some(5), kind: "five_hour".into() });
        merge(&mut slot, LimitHit { resets_at: None, kind: "api_429".into() });
        assert_eq!(slot, Some(LimitHit { resets_at: Some(5), kind: "five_hour".into() }));
    }
}
