use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

use super::{
    Error, Result, Store, card_row, check_board_active, event_from_row, now, record,
    require_nonblank, resolve_card, resolve_question, uuid,
};

pub(crate) fn active_session(db: &Connection, card_id: &str) -> Result<Option<Value>> {
    let id: Option<String> = db
        .query_row(
            "SELECT id FROM task_sessions WHERE card_id=?1 AND detached_at IS NULL",
            [card_id],
            |row| row.get(0),
        )
        .optional()?;
    id.map(|id| session_row(db, &id)).transpose()
}

fn session_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row(
        "SELECT s.id,s.card_id,s.owner_actor,s.runner,s.external_session_id,s.attached_at,s.paused_at,s.waiting_question_id,q.key,s.last_delivered_event_id,s.detached_at FROM task_sessions s LEFT JOIN questions q ON q.id=s.waiting_question_id WHERE s.id=?1",
        [id],
        |row| Ok(json!({
            "id":row.get::<_,String>(0)?, "card_id":row.get::<_,String>(1)?,
            "owner_actor":row.get::<_,String>(2)?, "runner":row.get::<_,String>(3)?,
            "external_session_id":row.get::<_,String>(4)?, "attached_at":row.get::<_,String>(5)?,
            "paused_at":row.get::<_,Option<String>>(6)?, "waiting_question_id":row.get::<_,Option<String>>(7)?,
            "waiting_question_key":row.get::<_,Option<String>>(8)?,
            "last_delivered_event_id":row.get::<_,i64>(9)?, "detached_at":row.get::<_,Option<String>>(10)?,
        })),
    )?)
}

fn claimed_active_card(db: &Connection, card_id: &str) -> Result<Value> {
    let card = card_row(db, card_id)?;
    let board_id = card["board_id"].as_str().unwrap();
    check_board_active(db, board_id)?;
    if card["owner_actor"].is_null() || card["cancelled_at"].is_string() {
        return Err(Error::new(
            "session_not_attachable",
            "task must be claimed and unfinished",
        ));
    }
    let kind: String = db.query_row(
        "SELECT kind FROM lanes WHERE id=?1",
        [card["lane_id"].as_str().unwrap()],
        |row| row.get(0),
    )?;
    if kind != "active" {
        return Err(Error::new(
            "session_not_attachable",
            "task must be In Progress",
        ));
    }
    Ok(card)
}

fn require_current_owner(card: &Value, session: &Value) -> Result<()> {
    if card["owner_actor"] != session["owner_actor"] {
        Err(Error::new(
            "session_owner_changed",
            "the task owner changed; detach this session before continuing",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn resume_eligible(db: &Connection, card: &Value, session: &Value) -> Result<bool> {
    if session["paused_at"].is_null()
        || session["detached_at"].is_string()
        || card["owner_actor"] != session["owner_actor"]
    {
        return Ok(false);
    }
    if let Some(question_id) = session["waiting_question_id"].as_str() {
        let status: String = db.query_row(
            "SELECT status FROM questions WHERE id=?1",
            [question_id],
            |row| row.get(0),
        )?;
        Ok(status != "open")
    } else {
        Ok(card["ready"] == true)
    }
}

pub(crate) fn detach_for_card(db: &Connection, card_id: &str) -> Result<()> {
    db.execute("UPDATE task_sessions SET detached_at=?2,paused_at=NULL,waiting_question_id=NULL WHERE card_id=?1 AND detached_at IS NULL", params![card_id,now()])?;
    Ok(())
}

impl Store {
    pub fn attach_task_session(
        &self,
        actor: &str,
        card: &str,
        runner: &str,
        external_session_id: &str,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let after = Self::attach_task_session_tx(&tx, actor, card, runner, external_session_id)?;
        self.commit(tx)?;
        Ok(after)
    }

    pub(crate) fn attach_task_session_tx(tx: &Transaction, actor: &str, card: &str, runner: &str, external_session_id: &str) -> Result<Value> {
        require_nonblank("actor", actor)?;
        require_nonblank("runner", runner)?;
        require_nonblank("session ID", external_session_id)?;
        let card_id = resolve_card(&tx, card)?;
        let card = claimed_active_card(&tx, &card_id)?;
        if active_session(&tx, &card_id)?.is_some() {
            return Err(Error::new(
                "session_exists",
                "task already has an attached session",
            ));
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_sessions WHERE runner=?1 AND external_session_id=?2)",
            params![runner.trim(), external_session_id.trim()],
            |row| row.get(0),
        )?;
        if exists {
            return Err(Error::new(
                "session_exists",
                "runner session ID is already recorded",
            ));
        }
        let cursor: i64 = tx.query_row("SELECT COALESCE(MAX(id),0) FROM events", [], |row| {
            row.get(0)
        })?;
        let id = uuid();
        tx.execute("INSERT INTO task_sessions(id,card_id,owner_actor,runner,external_session_id,attached_at,last_delivered_event_id) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![id,card_id,card["owner_actor"].as_str().unwrap(),runner.trim(),external_session_id.trim(),now(),cursor])?;
        let after = session_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "task_session_attached",
            card["board_id"].as_str().unwrap(),
            "task_session",
            &id,
            None,
            Some(&after),
            &[json!({"type":"task_session_attached","card_id":card_id})],
            "",
        )?;
        Ok(after)
    }

    pub fn pause_task_session(
        &self,
        actor: &str,
        card: &str,
        waiting_question: Option<&str>,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let card_id = resolve_card(&tx, card)?;
        let card = claimed_active_card(&tx, &card_id)?;
        let before = active_session(&tx, &card_id)?
            .ok_or_else(|| Error::new("session_not_found", "task has no attached session"))?;
        require_current_owner(&card, &before)?;
        if before["paused_at"].is_string() {
            return Err(Error::new(
                "session_already_paused",
                "task session is already paused",
            ));
        }
        let question_id = waiting_question
            .map(|key| resolve_question(&tx, key))
            .transpose()?;
        if let Some(id) = &question_id {
            let board_id: String =
                tx.query_row("SELECT board_id FROM questions WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
            if board_id != card["board_id"] {
                return Err(Error::new(
                    "cross_board_question",
                    "waiting question belongs to another board",
                ));
            }
        }
        tx.execute(
            "UPDATE task_sessions SET paused_at=?2,waiting_question_id=?3 WHERE id=?1",
            params![before["id"].as_str().unwrap(), now(), question_id],
        )?;
        let after = session_row(&tx, before["id"].as_str().unwrap())?;
        record(
            &tx,
            actor,
            "task_session_paused",
            card["board_id"].as_str().unwrap(),
            "task_session",
            before["id"].as_str().unwrap(),
            Some(&before),
            Some(&after),
            &[json!({"type":"task_session_paused","card_id":card_id})],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn resume_task_session(&self, actor: &str, card: &str) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let card_id = resolve_card(&tx, card)?;
        let card = claimed_active_card(&tx, &card_id)?;
        let before = active_session(&tx, &card_id)?
            .ok_or_else(|| Error::new("session_not_found", "task has no attached session"))?;
        require_current_owner(&card, &before)?;
        if !resume_eligible(&tx, &card, &before)? {
            return Err(Error::new(
                "session_not_ready",
                "task session is not paused on a resolved wait",
            ));
        }
        tx.execute(
            "UPDATE task_sessions SET paused_at=NULL,waiting_question_id=NULL WHERE id=?1",
            [before["id"].as_str().unwrap()],
        )?;
        let after = session_row(&tx, before["id"].as_str().unwrap())?;
        record(
            &tx,
            actor,
            "task_session_resumed",
            card["board_id"].as_str().unwrap(),
            "task_session",
            before["id"].as_str().unwrap(),
            Some(&before),
            Some(&after),
            &[json!({"type":"task_session_resumed","card_id":card_id})],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn acknowledge_task_session(
        &self,
        actor: &str,
        card: &str,
        through_event_id: i64,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let card_id = resolve_card(&tx, card)?;
        let card = claimed_active_card(&tx, &card_id)?;
        let before = active_session(&tx, &card_id)?
            .ok_or_else(|| Error::new("session_not_found", "task has no attached session"))?;
        require_current_owner(&card, &before)?;
        let current = before["last_delivered_event_id"].as_i64().unwrap();
        let latest: i64 = tx.query_row("SELECT COALESCE(MAX(id),0) FROM events", [], |row| {
            row.get(0)
        })?;
        if through_event_id < current || through_event_id > latest {
            return Err(Error::new(
                "invalid_event_cursor",
                "acknowledged event ID must advance from the session cursor and not exceed the latest event",
            ));
        }
        if through_event_id == current {
            return Ok(before);
        }
        tx.execute(
            "UPDATE task_sessions SET last_delivered_event_id=?2 WHERE id=?1",
            params![before["id"].as_str().unwrap(), through_event_id],
        )?;
        let after = session_row(&tx, before["id"].as_str().unwrap())?;
        record(
            &tx,
            actor,
            "task_session_acknowledged",
            card["board_id"].as_str().unwrap(),
            "task_session",
            before["id"].as_str().unwrap(),
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn detach_task_session(&self, actor: &str, card: &str) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let card_id = resolve_card(&tx, card)?;
        let card = card_row(&tx, &card_id)?;
        let before = active_session(&tx, &card_id)?
            .ok_or_else(|| Error::new("session_not_found", "task has no attached session"))?;
        detach_for_card(&tx, &card_id)?;
        let after = session_row(&tx, before["id"].as_str().unwrap())?;
        record(
            &tx,
            actor,
            "task_session_detached",
            card["board_id"].as_str().unwrap(),
            "task_session",
            before["id"].as_str().unwrap(),
            Some(&before),
            Some(&after),
            &[json!({"type":"task_session_detached","card_id":card_id})],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Attached sessions for a board's cards with the card state an orchestrator needs to
    /// reconcile its workers: owner, whether the card was cancelled, and the session row.
    pub fn list_task_sessions(&self, board: &str) -> Result<Value> {
        let db = self.open()?;
        let board_id = super::resolve_board(&db, board)?;
        let mut stmt = db.prepare("SELECT s.id,c.id FROM task_sessions s JOIN cards c ON c.id=s.card_id WHERE c.board_id=?1 AND s.detached_at IS NULL ORDER BY c.key")?;
        let ids = stmt
            .query_map([&board_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (session_id, card_id) in ids {
            let card = card_row(&db, &card_id)?;
            out.push(json!({
                "card_key": card["key"],
                "owner_actor": card["owner_actor"],
                "cancelled": card["cancelled_at"].is_string(),
                "session": session_row(&db, &session_id)?,
            }));
        }
        Ok(Value::Array(out))
    }

    pub fn task_session_briefing(
        &self,
        card: &str,
        limit: i64,
        after_event_id: Option<i64>,
    ) -> Result<Value> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::new("validation_error", "limit must be 1–1000"));
        }
        let db = self.open()?;
        let card_id = resolve_card(&db, card)?;
        let card = card_row(&db, &card_id)?;
        let session = active_session(&db, &card_id)?
            .ok_or_else(|| Error::new("session_not_found", "task has no attached session"))?;
        let mut related = std::collections::HashSet::new();
        if let Some(id) = session["waiting_question_id"].as_str() {
            related.insert(id.to_owned());
        }
        let mut stmt =
            db.prepare("SELECT to_card_id,to_question_id FROM dependencies WHERE from_card_id=?1")?;
        for row in stmt.query_map([&card_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })? {
            let (target_card, target_question) = row?;
            if let Some(id) = target_card.or(target_question) {
                related.insert(id);
            }
        }
        let delivered = session["last_delivered_event_id"].as_i64().unwrap();
        let since = after_event_id.unwrap_or(delivered);
        let latest: i64 = db.query_row("SELECT COALESCE(MAX(id),0) FROM events", [], |row| {
            row.get(0)
        })?;
        if since < delivered || since > latest {
            return Err(Error::new(
                "invalid_event_cursor",
                "briefing cursor must be between the session's delivered cursor and the latest event",
            ));
        }
        let mut stmt = db.prepare("SELECT id,ts,actor,type,board_id,entity_type,entity_id,before_json,after_json,effects_json,note FROM events WHERE board_id=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = stmt
            .query_map(
                params![card["board_id"].as_str().unwrap(), since, limit + 1],
                event_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let has_more = rows.len() > limit as usize;
        let mut cursor = since;
        let mut events = Vec::new();
        for event in rows.into_iter().take(limit as usize) {
            cursor = event["id"].as_i64().unwrap();
            if event["entity_id"] == card_id
                || related.contains(event["entity_id"].as_str().unwrap_or(""))
                || event["effects"].as_array().is_some_and(|effects| {
                    effects.iter().any(|effect| effect["card_id"] == card_id)
                })
            {
                events.push(event);
            }
        }
        Ok(
            json!({"card":card,"session":session,"events":events,"cursor":cursor,"has_more":has_more}),
        )
    }
}
