//! Claude Code backend: `claude -p` runs headless on the user's existing Claude Code login,
//! so no API key is needed. A new worker uses `--session-id`; a resumed one uses `--resume`.

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::runner::{Launch, Runner, Worker, WorkerState};

pub struct ClaudeRunner {
    pub bin: String,
    /// Extra arguments, for example permission settings and allowed tools.
    pub args: Vec<String>,
    pub db: PathBuf,
    pub cwd: Option<PathBuf>,
    /// Each worker's output is appended to `<log_dir>/<card>-<session>.log`.
    pub log_dir: PathBuf,
}

struct ChildWorker(Child);

impl Worker for ChildWorker {
    fn poll(&mut self) -> WorkerState {
        match self.0.try_wait() {
            Ok(None) => WorkerState::Running,
            Ok(Some(status)) => WorkerState::Exited {
                success: status.success(),
            },
            Err(_) => WorkerState::Exited { success: false },
        }
    }

    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Runner for ClaudeRunner {
    fn name(&self) -> &str {
        "claude"
    }

    fn start(&self, launch: &Launch) -> std::io::Result<Box<dyn Worker>> {
        fs::create_dir_all(&self.log_dir)?;
        let log = OpenOptions::new().create(true).append(true).open(self.log_dir.join(format!(
            "{}-{}.log",
            launch.card_key,
            &launch.session_id[..launch.session_id.len().min(8)]
        )))?;
        let mut command = Command::new(&self.bin);
        command
            .arg("-p")
            .arg(&launch.prompt)
            .arg(if launch.resume { "--resume" } else { "--session-id" })
            .arg(&launch.session_id)
            .args(&self.args)
            .env("WORKYARD_ACTOR", &launch.actor)
            .env("WORKYARD_DB", &self.db)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
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
        Ok(Box::new(ChildWorker(command.spawn()?)))
    }
}
