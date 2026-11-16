//! Claude Code backend: `claude -p` runs headless on the user's existing login, so no API key
//! is needed. A new worker uses `--session-id`; a resumed one uses `--resume`.
//!
//! Workers run with `--output-format stream-json --verbose`. Every line is appended to the
//! worker's log, and each is read as JSON so a usage limit is recognised from structured
//! fields (`rate_limit_event`, `result.api_error_status`), never from message text.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::runner::{LimitHit, Launch, Runner, Worker, WorkerState};

pub struct ClaudeRunner {
    pub bin: String,
    /// Extra arguments, for example permission settings and allowed tools. Do not pass
    /// `--output-format`: the runner needs `stream-json`.
    pub args: Vec<String>,
    pub db: PathBuf,
    pub cwd: Option<PathBuf>,
    /// Each worker's output is appended to `<log_dir>/<card>-<session>.log`.
    pub log_dir: PathBuf,
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

struct ChildWorker {
    child: Child,
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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Runner for ClaudeRunner {
    fn name(&self) -> &str {
        "claude"
    }

    fn start(&self, launch: &Launch) -> std::io::Result<Box<dyn Worker>> {
        fs::create_dir_all(&self.log_dir)?;
        let mut log = OpenOptions::new().create(true).append(true).open(self.log_dir.join(format!(
            "{}-{}.log",
            launch.card_key,
            &launch.session_id[..launch.session_id.len().min(8)]
        )))?;
        let mut command = Command::new(&self.bin);
        command
            .arg("-p")
            .arg(&launch.prompt)
            .args(["--output-format", "stream-json", "--verbose"])
            .arg(if launch.resume { "--resume" } else { "--session-id" })
            .arg(&launch.session_id)
            .args(&self.args)
            .env("WORKYARD_ACTOR", &launch.actor)
            .env("WORKYARD_DB", &self.db)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log.try_clone()?);
        // The worker must find the same `workyard` binary the orchestrator is running from.
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)) {
            let mut paths = vec![dir];
            if let Some(existing) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&existing));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                command.env("PATH", joined);
            }
        }
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let limit = Arc::new(Mutex::new(None));
        let reader_done = Arc::new(AtomicBool::new(false));
        {
            let (limit, done) = (Arc::clone(&limit), Arc::clone(&reader_done));
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = writeln!(log, "{line}");
                    if let Some(hit) = serde_json::from_str::<Value>(&line).ok().as_ref().and_then(limit_signal) {
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
    fn a_hit_with_a_reset_time_wins_over_one_without() {
        let mut slot = None;
        merge(&mut slot, LimitHit { resets_at: None, kind: "api_429".into() });
        merge(&mut slot, LimitHit { resets_at: Some(5), kind: "five_hour".into() });
        merge(&mut slot, LimitHit { resets_at: None, kind: "api_429".into() });
        assert_eq!(slot, Some(LimitHit { resets_at: Some(5), kind: "five_hour".into() }));
    }
}
