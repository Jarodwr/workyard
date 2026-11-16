//! Limits on what particular actors may do, enforced in the core rather than by prompt.

use crate::{Error, Result};

/// The supervisor agent's actor is `agent:supervisor-<board>`.
pub(crate) fn is_supervisor(actor: &str) -> bool {
    actor.starts_with("agent:supervisor-")
}

/// Reject `what` when the supervisor attempts it.
pub(crate) fn forbid_supervisor(actor: &str, what: &str) -> Result<()> {
    if is_supervisor(actor) {
        return Err(Error::new(
            "forbidden_for_supervisor",
            format!("the supervisor may not {what}"),
        )
        .hint("Leave the decision to a human: add a note starting `NEEDS HUMAN:` that states the problem and the options you see. You may add notes, hold or resume sessions, and `orch reclaim` a dead worker"));
    }
    Ok(())
}
