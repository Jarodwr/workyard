//! Limits on what particular actors may do, enforced in the core rather than by prompt.

use crate::types::{Question, QuestionKind, QuestionStatus};
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
        .hint("Steer workers with `task note`; leave approvals, lane moves, cancellations, releases and business questions to a human"));
    }
    Ok(())
}

/// The supervisor may only answer an open engineering question, and only with its recommended option.
pub(crate) fn check_supervisor_answer(
    actor: &str,
    question: &Question,
    answer: &str,
    dropped: bool,
) -> Result<()> {
    if !is_supervisor(actor) {
        return Ok(());
    }
    let hint = "Answer only engineering questions that have a recommended option, with exactly that option; leave the rest to a human";
    if dropped {
        return Err(Error::new(
            "forbidden_for_supervisor",
            "the supervisor may not drop a question",
        )
        .hint(hint));
    }
    if question.kind != QuestionKind::Engineering || question.status != QuestionStatus::Open {
        return Err(Error::new(
            "forbidden_for_supervisor",
            "the supervisor may only answer engineering questions",
        )
        .hint(hint));
    }
    if question.recommended_option.trim().is_empty() {
        return Err(Error::new(
            "forbidden_for_supervisor",
            "the question has no recommended option for the supervisor to confirm",
        )
        .hint(hint));
    }
    if answer.trim() != question.recommended_option.trim() {
        return Err(Error::new(
            "forbidden_for_supervisor",
            "the supervisor's answer must equal the question's recommended option",
        )
        .hint(hint));
    }
    Ok(())
}
