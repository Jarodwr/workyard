//! Conversation about an open question. Humans and agents can talk in a question's thread; only a
//! human answering or dropping the question resolves it. A human message wakes the worker that is
//! paused waiting on the question (see `sessions::resume_eligible`) so it can reply.

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

use super::{
    Error, Result, Store, check_board_active, now, record, require_nonblank, resolve_question,
    uuid,
};

/// Longest message body, in characters.
pub const MAX_MESSAGE_CHARS: usize = 8000;

/// Humans are everyone who is not an agent; the supervisor (`agent:supervisor-*`) is an agent.
pub fn is_human_actor(actor: &str) -> bool {
    !actor.trim().starts_with("agent:")
}

fn message_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, String>(0)?,
        "question_id": r.get::<_, String>(1)?,
        "question_key": r.get::<_, String>(2)?,
        "author": r.get::<_, String>(3)?,
        "author_is_human": r.get::<_, i64>(4)? != 0,
        "body": r.get::<_, String>(5)?,
        "created_at": r.get::<_, String>(6)?,
        "read_by_human": r.get::<_, i64>(7)? != 0,
    }))
}

const MESSAGE_COLUMNS: &str = "m.id,m.question_id,q.key,m.author,m.author_is_human,m.body,m.created_at,m.read_by_human";

pub(crate) fn thread(db: &Connection, question_id: &str) -> Result<Vec<Value>> {
    let mut stmt = db.prepare(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM question_messages m JOIN questions q ON q.id=m.question_id WHERE m.question_id=?1 ORDER BY m.seq"
    ))?;
    Ok(stmt
        .query_map([question_id], message_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// (message count, agent messages a human has not looked at) for the question list.
pub(crate) fn counts(db: &Connection, question_id: &str) -> Result<(i64, i64)> {
    Ok(db.query_row(
        "SELECT COUNT(*), COALESCE(SUM(author_is_human=0 AND read_by_human=0),0) FROM question_messages WHERE question_id=?1",
        [question_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// Whether a human posted in the thread after the event the session last had delivered.
pub(crate) fn human_message_after(db: &Connection, question_id: &str, event_id: i64) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM question_messages WHERE question_id=?1 AND author_is_human=1 AND event_id>?2)",
        params![question_id, event_id],
        |r| r.get(0),
    )?)
}

impl Store {
    /// Post to the thread of an open question. A human message wakes the worker paused on the
    /// question; the question itself stays open.
    pub fn post_question_message(&self, actor: &str, question: &str, body: &str) -> Result<Value> {
        require_nonblank("actor", actor)?;
        require_nonblank("message", body)?;
        let body = body.trim();
        if body.chars().count() > MAX_MESSAGE_CHARS {
            return Err(Error::new(
                "validation_error",
                format!("message must be at most {MAX_MESSAGE_CHARS} characters"),
            ));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let message = Self::post_question_message_tx(&tx, actor, question, body)?;
        self.commit(tx)?;
        Ok(message)
    }

    fn post_question_message_tx(tx: &Transaction, actor: &str, question: &str, body: &str) -> Result<Value> {
        let id = resolve_question(tx, question)?;
        let (key, board_id, status): (String, String, String) = tx.query_row(
            "SELECT key,board_id,status FROM questions WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if status != "open" {
            return Err(Error::new(
                "validation_error",
                "question is no longer open; its thread is closed",
            ));
        }
        check_board_active(tx, &board_id)?;
        let message_id = uuid();
        let human = is_human_actor(actor);
        let created_at = now();
        let message = json!({
            "id": message_id, "question_id": id, "question_key": key, "author": actor.trim(),
            "author_is_human": human, "body": body, "created_at": created_at, "read_by_human": human,
        });
        record(
            tx,
            actor,
            "question_message_posted",
            &board_id,
            "question",
            &id,
            None,
            Some(&message),
            &[],
            "",
        )?;
        let event_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO question_messages(id,question_id,board_id,author,author_is_human,body,created_at,event_id,read_by_human) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![message_id, id, board_id, actor.trim(), human, body, created_at, event_id, human],
        )?;
        Ok(message)
    }

    /// The thread of a question, oldest first.
    pub fn list_question_messages(&self, question: &str) -> Result<Value> {
        let db = self.open()?;
        let id = resolve_question(&db, question)?;
        Ok(json!(thread(&db, &id)?))
    }

    /// The question with its thread under `messages`.
    pub fn view_question(&self, question: &str) -> Result<Value> {
        let db = self.open()?;
        let id = resolve_question(&db, question)?;
        let mut row = serde_json::to_value(super::question_snapshot(&db, &id)?)?;
        row["messages"] = json!(thread(&db, &id)?);
        Ok(row)
    }

    /// A human has looked at the thread: clear its unread agent messages. Returns how many were
    /// cleared. Not an event, so it does not wake anyone.
    pub fn mark_question_messages_read(&self, question: &str) -> Result<i64> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_question(&tx, question)?;
        let changed = tx.execute(
            "UPDATE question_messages SET read_by_human=1 WHERE question_id=?1 AND read_by_human=0",
            [&id],
        )?;
        self.commit(tx)?;
        Ok(changed as i64)
    }
}
