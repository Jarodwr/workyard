//! Runtime backends. A runner starts a worker agent for one card and reports when it exits.
//! Agents report their work through board commands, which stay durable events; the process
//! handle only supplies the ephemeral "is it still running" signal.

/// Where a worker runs. Mirrors the card's `workspace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workspace {
    /// Its own git worktree and branch, so parallel workers never share a checkout.
    Worktree,
    /// Investigative work that changes no repository.
    None,
}

impl Workspace {
    pub fn from_card(card: &serde_json::Value) -> Self {
        if card["workspace"] == "none" { Self::None } else { Self::Worktree }
    }
}

/// Everything a runner needs to start or resume one worker.
pub struct Launch {
    pub board_key: String,
    pub card_key: String,
    /// The worker's actor. It owns the card and must be reused when the session resumes.
    pub actor: String,
    /// The runtime session ID recorded with `session attach`.
    pub session_id: String,
    pub prompt: String,
    /// Continue an existing runtime session instead of starting a new one.
    pub resume: bool,
    pub workspace: Workspace,
}

/// A usage or rate limit the worker ran into, read from the runtime's structured output.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitHit {
    /// When the limit resets, as Unix seconds, if the runtime said.
    pub resets_at: Option<i64>,
    /// The runtime's name for the limit, for example `five_hour`.
    pub kind: String,
}

pub enum WorkerState {
    Running,
    /// `limit` is set when the worker stopped because of a limit rather than a fault.
    Exited { success: bool, limit: Option<LimitHit> },
}

pub trait Worker: Send {
    fn poll(&mut self) -> WorkerState;
    /// Stop the worker. Called on shutdown; must be safe to call after it exited.
    fn stop(&mut self);
    /// Hand a running worker a message. The runtime delivers it at its next opportunity (for
    /// Claude Code, the next tool boundary or turn). Errors when the worker cannot take input.
    fn send(&mut self, _message: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "worker takes no input"))
    }
    /// True once the worker has finished its work for now and gone quiet. The engine then
    /// closes its input so the process can exit.
    fn idle(&mut self) -> bool {
        false
    }
    /// Close the worker's input. Safe to call repeatedly.
    fn close_input(&mut self) {}
    /// A short description of what the worker is doing right now ("Edit app.mjs (12 tools)"),
    /// derived from the runtime's own output. `None` when the runtime cannot say.
    fn activity(&mut self) -> Option<String> {
        None
    }
    /// Tool requests the runtime refused since the last call (a permission denial, a blocked
    /// command), each as a short description. The engine tells a human about new ones.
    fn take_denials(&mut self) -> Vec<String> {
        Vec::new()
    }
}

pub trait Runner: Send {
    /// Recorded as the session's runner, and used in worker actor names.
    fn name(&self) -> &str;
    fn start(&self, launch: &Launch) -> std::io::Result<Box<dyn Worker>>;
    /// The main checkout workers integrate into, when the runner works inside a repository.
    /// Worker prompts use it to name real paths and branches.
    fn checkout(&self) -> Option<std::path::PathBuf> {
        None
    }
}
