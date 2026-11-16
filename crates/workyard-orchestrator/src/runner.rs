//! Runtime backends. A runner starts a worker agent for one card and reports when it exits.
//! Agents report their work through board commands, which stay durable events; the process
//! handle only supplies the ephemeral "is it still running" signal.

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
}

pub trait Runner: Send {
    /// Recorded as the session's runner, and used in worker actor names.
    fn name(&self) -> &str;
    fn start(&self, launch: &Launch) -> std::io::Result<Box<dyn Worker>>;
}
