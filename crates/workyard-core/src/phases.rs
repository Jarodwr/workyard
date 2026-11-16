use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::{
    Error, Result, Store, board_row, card_row, check_board_active, check_version, guard_card, now,
    question_row, record, require_nonblank, resolve_board, resolve_card, resolve_question, uuid,
};

pub(crate) fn phase_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row("SELECT id,board_id,name,position,created_at,description,version,closed_at FROM phases WHERE id=?1", [id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"board_id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"position":r.get::<_,i64>(3)?,"created_at":r.get::<_,String>(4)?,"description":r.get::<_,String>(5)?,"version":r.get::<_,i64>(6)?,"closed_at":r.get::<_,Option<String>>(7)?})))?)
}

pub(crate) fn current_phase(db: &Connection, board_id: &str) -> Result<(String, i64)> {
    db.query_row("SELECT p.id,p.position FROM boards b JOIN phases p ON p.id=b.current_phase_id WHERE b.id=?1", [board_id], |r| Ok((r.get(0)?,r.get(1)?))).map_err(Into::into)
}

pub(crate) fn resolve_phase(
    db: &Connection,
    board_id: &str,
    value: Option<&str>,
) -> Result<(String, i64)> {
    if let Some(value) = value {
        db.query_row(
            "SELECT id,position FROM phases WHERE board_id=?1 AND (id=?2 OR name=?2)",
            params![board_id, value],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            Error::new(
                "not_found",
                format!("phase {value} not found on this board"),
            )
        })
    } else {
        current_phase(db, board_id)
    }
}

pub(crate) fn ensure_phase_open(db: &Connection, board_id: &str, phase_id: &str) -> Result<()> {
    let (_, current) = current_phase(db, board_id)?;
    let phase: (i64, Option<String>) = db
        .query_row(
            "SELECT position,closed_at FROM phases WHERE id=?1 AND board_id=?2",
            params![phase_id, board_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::new("not_found", "phase does not belong to this board"))?;
    if phase.0 < current || phase.1.is_some() {
        Err(Error::new(
            "phase_closed",
            "completed phases cannot receive new work or structural changes",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_dependency_order(
    db: &Connection,
    dependent_id: &str,
    prerequisite_id: &str,
    prerequisite_is_question: bool,
) -> Result<()> {
    let dependent: i64 = db.query_row(
        "SELECT p.position FROM cards c JOIN phases p ON p.id=c.phase_id WHERE c.id=?1",
        [dependent_id],
        |r| r.get(0),
    )?;
    let sql = if prerequisite_is_question {
        "SELECT p.position FROM questions q JOIN phases p ON p.id=q.phase_id WHERE q.id=?1"
    } else {
        "SELECT p.position FROM cards c JOIN phases p ON p.id=c.phase_id WHERE c.id=?1"
    };
    let prerequisite: i64 = db.query_row(sql, [prerequisite_id], |r| r.get(0))?;
    if prerequisite > dependent {
        Err(Error::new(
            "future_dependency",
            "a card cannot depend on work in a later phase",
        )
        .hint("Move the prerequisite into this phase or move the dependent to a later phase"))
    } else {
        Ok(())
    }
}

/// How many phases past the current one may be worked early; `None` is unlimited.
pub(crate) fn phase_window(db: &Connection, board_id: &str) -> Result<Option<i64>> {
    Ok(db.query_row("SELECT phase_window FROM boards WHERE id=?1", [board_id], |r| r.get(0))?)
}

/// Whether work `distance` phases past the current one may be dispatched. A window of 1 is
/// the original rule: the next phase only once the current phase has no ready work.
pub(crate) fn phase_offered(distance: i64, window: Option<i64>, current_has_work: bool) -> bool {
    match window {
        _ if distance <= 0 => true,
        Some(1) => distance == 1 && !current_has_work,
        Some(w) => distance <= w,
        None => true,
    }
}

/// `all` is unlimited; otherwise a phase count of at least 1.
pub fn parse_phase_window(text: &str) -> Result<Option<i64>> {
    if text == "all" {
        return Ok(None);
    }
    text.parse::<i64>().ok().filter(|w| *w >= 1).map(Some).ok_or_else(|| {
        Error::new("validation_error", "phase window must be a number of at least 1, or all")
    })
}

/// Keys of ready Backlog cards the dispatcher is not offering because of the phase window.
pub(crate) fn held_back_by_phase(db: &Connection, board_id: &str) -> Result<Vec<String>> {
    let (_, current) = current_phase(db, board_id)?;
    let window = phase_window(db, board_id)?;
    let current_has_work = current_queue_has_work(db, board_id)?;
    let mut stmt = db.prepare("SELECT c.id,p.position FROM cards c JOIN lanes l ON l.id=c.lane_id JOIN phases p ON p.id=c.phase_id WHERE c.board_id=?1 AND p.position>?2 AND p.closed_at IS NULL AND l.kind='backlog' AND l.archived=0 AND c.owner_actor IS NULL AND c.cancelled_at IS NULL ORDER BY p.position,c.position,c.key")?;
    let rows = stmt.query_map(params![board_id, current], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut keys = Vec::new();
    for row in rows {
        let (id, position) = row?;
        if phase_offered(position - current, window, current_has_work) {
            continue;
        }
        let card = card_row(db, &id)?;
        if card["ready"] == true {
            keys.push(card["key"].as_str().unwrap_or_default().to_owned());
        }
    }
    Ok(keys)
}

pub(crate) fn current_queue_has_work(db: &Connection, board_id: &str) -> Result<bool> {
    let (phase_id, _) = current_phase(db, board_id)?;
    let mut stmt=db.prepare("SELECT c.id FROM cards c JOIN lanes l ON l.id=c.lane_id WHERE c.phase_id=?1 AND l.kind='backlog' AND c.owner_actor IS NULL AND c.cancelled_at IS NULL ORDER BY c.position")?;
    for row in stmt.query_map([phase_id], |r| r.get::<_, String>(0))? {
        if card_row(db, &row?)?["blocked"] == false {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_phase_complete(db: &Connection, phase_id: &str) -> Result<()> {
    let unfinished:i64=db.query_row("SELECT COUNT(*) FROM cards c JOIN lanes l ON l.id=c.lane_id WHERE c.phase_id=?1 AND c.cancelled_at IS NULL AND l.kind!='done'",[phase_id],|r|r.get(0))?;
    let questions:i64=db.query_row("SELECT COUNT(*) FROM questions WHERE phase_id=?1 AND status='open'",[phase_id],|r|r.get(0))?;
    let broken:i64=db.query_row("SELECT COUNT(*) FROM dependencies d JOIN cards prerequisite ON prerequisite.id=d.to_card_id JOIN cards dependent ON dependent.id=d.from_card_id WHERE prerequisite.phase_id=?1 AND prerequisite.cancelled_at IS NOT NULL AND dependent.cancelled_at IS NULL",[phase_id],|r|r.get(0))?;
    if unfinished > 0 || questions > 0 || broken > 0 {
        return Err(Error::new("phase_incomplete","finish or cancel current work, resolve its questions, and repair cancelled prerequisites before completing the phase").details(json!({"unfinished_cards":unfinished,"open_questions":questions,"cancelled_prerequisites":broken})));
    }
    Ok(())
}

pub(crate) fn ensure_schedulable(db: &Connection, card: &Value) -> Result<()> {
    if card["cancelled_at"].is_string() {
        return Err(Error::new(
            "card_cancelled",
            "cancelled work cannot be started",
        ));
    }
    let board_id = card["board_id"].as_str().unwrap();
    let phase_id = card["phase_id"]
        .as_str()
        .ok_or_else(|| Error::new("database_error", "card has no phase"))?;
    let (_, current) = current_phase(db, board_id)?;
    let phase: (i64, Option<String>) = db.query_row("SELECT position,closed_at FROM phases WHERE id=?1", [phase_id], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    if phase.0 < current || phase.1.is_some() {
        return Err(Error::new(
            "phase_closed",
            "card belongs to a completed phase",
        ));
    }
    let window = phase_window(db, board_id)?;
    if window.is_some_and(|w| phase.0 > current + w) {
        return Err(Error::new(
            "phase_planned",
            "this phase is beyond the board's phase window",
        ));
    }
    if window == Some(1) && phase.0 == current + 1 && current_queue_has_work(db, board_id)? {
        return Err(Error::new(
            "current_phase_queue_not_empty",
            "ready work remains in the current phase",
        ));
    }
    Ok(())
}

impl Store {
    /// Set how many phases past the current one the dispatcher may offer work from
    /// (`None` is unlimited; 1 is the original next-phase-only rule).
    pub fn set_board_phase_window(&self, actor: &str, board: &str, window: Option<i64>) -> Result<Value> {
        if window.is_some_and(|w| w < 1) {
            return Err(Error::new("validation_error", "phase window must be at least 1, or all"));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let before = board_row(&tx, &board_id)?;
        if before["phase_window"] == json!(window) {
            return Ok(before);
        }
        tx.execute(
            "UPDATE boards SET phase_window=?2,version=version+1 WHERE id=?1",
            params![board_id, window],
        )?;
        let after = board_row(&tx, &board_id)?;
        record(&tx, actor, "board_phase_window_changed", &board_id, "board", &board_id, Some(&before), Some(&after), &[], "")?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn list_phases(&self, board: &str) -> Result<Value> {
        let db = self.open()?;
        let board_id = resolve_board(&db, board)?;
        let (_, current) = current_phase(&db, &board_id)?;
        let mut stmt = db.prepare("SELECT id FROM phases WHERE board_id=?1 ORDER BY position")?;
        let mut result = Vec::new();
        for id in stmt.query_map([board_id], |r| r.get::<_, String>(0))? {
            let mut phase = phase_row(&db, &id?)?;
            let position = phase["position"].as_i64().unwrap();
            phase["status"] = json!(if position < current || phase["closed_at"].is_string() {
                "closed"
            } else if position == current {
                "current"
            } else {
                "future"
            });
            result.push(phase);
        }
        Ok(json!(result))
    }

    pub fn add_phase(&self, actor: &str, board: &str, name: &str) -> Result<Value> {
        self.add_phase_with_description(actor, board, name, "")
    }

    pub fn add_phase_with_description(&self, actor: &str, board: &str, name: &str, description: &str) -> Result<Value> {
        require_nonblank("phase name", name)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM phases WHERE board_id=?1 AND lower(name)=lower(?2))",
            params![board_id, name.trim()],
            |r| r.get(0),
        )?;
        if exists {
            return Err(Error::new(
                "duplicate_phase",
                "phase name already exists on this board",
            ));
        }
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position),-1)+1 FROM phases WHERE board_id=?1",
            [&board_id],
            |r| r.get(0),
        )?;
        let id = uuid();
        tx.execute(
            "INSERT INTO phases(id,board_id,name,position,created_at,description) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, board_id, name.trim(), position, now(), description.trim()],
        )?;
        tx.execute(
            "UPDATE boards SET version=version+1 WHERE id=?1",
            [&board_id],
        )?;
        let after = phase_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "phase_added",
            &board_id,
            "phase",
            &id,
            None,
            Some(&after),
            &[
                json!({"type":"board_phase_added","board_id":board_id,"board_version":board_row(&tx,&board_id)?["version"]}),
            ],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn describe_phase(&self, actor: &str, board: &str, phase: &str, description: &str, expected: i64) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let (phase_id, _) = resolve_phase(&tx, &board_id, Some(phase))?;
        ensure_phase_open(&tx, &board_id, &phase_id)?;
        let before = phase_row(&tx, &phase_id)?;
        check_version(&before, expected)?;
        tx.execute("UPDATE phases SET description=?2,version=version+1 WHERE id=?1", params![phase_id,description.trim()])?;
        tx.execute("UPDATE boards SET version=version+1 WHERE id=?1", [&board_id])?;
        let after = phase_row(&tx, &phase_id)?;
        record(&tx,actor,"phase_described",&board_id,"phase",&phase_id,Some(&before),Some(&after),&[],"")?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn complete_phase(&self, actor: &str, board: &str) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let (phase_id, _) = current_phase(&tx, &board_id)?;
        let before = phase_row(&tx, &phase_id)?;
        if before["closed_at"].is_string() {
            return Err(Error::new("phase_closed", "current phase is already completed"));
        }
        ensure_phase_complete(&tx, &phase_id)?;
        tx.execute("UPDATE phases SET closed_at=?2,version=version+1 WHERE id=?1",params![phase_id,now()])?;
        tx.execute("UPDATE boards SET version=version+1 WHERE id=?1",[&board_id])?;
        let after=phase_row(&tx,&phase_id)?;
        record(&tx,actor,"phase_completed",&board_id,"phase",&phase_id,Some(&before),Some(&after),&[json!({"type":"phase_archived","phase_id":phase_id})],"")?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn advance_phase(&self, actor: &str, board: &str) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let before = board_row(&tx, &board_id)?;
        let (current_id, current_position) = current_phase(&tx, &board_id)?;
        let next_id: String = tx
            .query_row(
                "SELECT id FROM phases WHERE board_id=?1 AND position=?2",
                params![board_id, current_position + 1],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::new("phase_not_found", "add a next phase before advancing"))?;
        ensure_phase_complete(&tx, &current_id)?;
        tx.execute("UPDATE phases SET closed_at=COALESCE(closed_at,?2),version=version+CASE WHEN closed_at IS NULL THEN 1 ELSE 0 END WHERE id=?1",params![current_id,now()])?;
        tx.execute(
            "UPDATE boards SET current_phase_id=?2,version=version+1 WHERE id=?1",
            params![board_id, next_id],
        )?;
        let after = board_row(&tx, &board_id)?;
        record(
            &tx,
            actor,
            "phase_advanced",
            &board_id,
            "board",
            &board_id,
            Some(&before),
            Some(&after),
            &[json!({"type":"phase_archived","phase_id":current_id}),json!({"type":"phase_current_changed","from":current_id,"to":next_id})],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn assign_phase(
        &self,
        actor: &str,
        item: &str,
        phase: &str,
        expected: i64,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let is_question = item.starts_with("Q-")
            || tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM questions WHERE id=?1)",
                [item],
                |r| r.get::<_, bool>(0),
            )?;
        let id = if is_question {
            resolve_question(&tx, item)?
        } else {
            resolve_card(&tx, item)?
        };
        let before = if is_question {
            question_row(&tx, &id)?
        } else {
            guard_card(&tx, &id, actor, Some(expected))?
        };
        if is_question {
            check_version(&before, expected)?;
        }
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        let old_phase = before["phase_id"].as_str().unwrap();
        ensure_phase_open(&tx, board_id, old_phase)?;
        let (new_phase, new_position) = resolve_phase(&tx, board_id, Some(phase))?;
        ensure_phase_open(&tx, board_id, &new_phase)?;
        if old_phase == new_phase {
            return Err(Error::new(
                "validation_error",
                "item is already in that phase",
            ));
        }
        let old_position: i64 = tx.query_row(
            "SELECT position FROM phases WHERE id=?1",
            [old_phase],
            |r| r.get(0),
        )?;
        if is_question {
            let mut stmt=tx.prepare("SELECT p.position FROM dependencies d JOIN cards c ON c.id=d.from_card_id JOIN phases p ON p.id=c.phase_id WHERE d.to_question_id=?1")?;
            for row in stmt.query_map([&id], |r| r.get::<_, i64>(0))? {
                if new_position > row? {
                    return Err(Error::new(
                        "future_dependency",
                        "question would move after a card that depends on it",
                    ));
                }
            }
            tx.execute(
                "UPDATE questions SET phase_id=?2,version=version+1 WHERE id=?1",
                params![id, new_phase],
            )?;
        } else {
            if before["cancelled_at"].is_string() {
                return Err(Error::new(
                    "card_cancelled",
                    "cancelled cards cannot change phase",
                ));
            }
            let lane: String = tx.query_row(
                "SELECT kind FROM lanes WHERE id=?1",
                [before["lane_id"].as_str().unwrap()],
                |r| r.get(0),
            )?;
            if lane != "backlog" || before["owner_actor"].is_string() {
                return Err(Error::new(
                    "phase_assignment_conflict",
                    "only unclaimed Backlog cards can change phase",
                ));
            }
            let mut stmt=tx.prepare("SELECT p.position FROM dependencies d LEFT JOIN cards c ON c.id=d.to_card_id LEFT JOIN questions q ON q.id=d.to_question_id JOIN phases p ON p.id=COALESCE(c.phase_id,q.phase_id) WHERE d.from_card_id=?1")?;
            for row in stmt.query_map([&id], |r| r.get::<_, i64>(0))? {
                if row? > new_position {
                    return Err(Error::new(
                        "future_dependency",
                        "card would move before a prerequisite",
                    ));
                }
            }
            let mut stmt=tx.prepare("SELECT p.position FROM dependencies d JOIN cards c ON c.id=d.from_card_id JOIN phases p ON p.id=c.phase_id WHERE d.to_card_id=?1")?;
            for row in stmt.query_map([&id], |r| r.get::<_, i64>(0))? {
                if new_position > row? {
                    return Err(Error::new(
                        "future_dependency",
                        "card would move after a dependent",
                    ));
                }
            }
            tx.execute(
                "UPDATE cards SET phase_id=?2,version=version+1,updated_at=?3 WHERE id=?1",
                params![id, new_phase, now()],
            )?;
        }
        let after = if is_question {
            question_row(&tx, &id)?
        } else {
            card_row(&tx, &id)?
        };
        record(
            &tx,
            actor,
            "phase_assigned",
            board_id,
            if is_question { "question" } else { "card" },
            &id,
            Some(&before),
            Some(&after),
            &[],
            &format!("from phase {old_position} to {new_position}"),
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn cancel_card(
        &self,
        actor: &str,
        card: &str,
        reason: &str,
        expected: i64,
    ) -> Result<Value> {
        crate::actor_policy::forbid_supervisor(actor, "cancel a card")?;
        require_nonblank("reason", reason)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, Some(expected))?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        ensure_phase_open(&tx, board_id, before["phase_id"].as_str().unwrap())?;
        if before["cancelled_at"].is_string() {
            return Err(Error::new("card_cancelled", "card is already cancelled"));
        }
        let lane: String = tx.query_row(
            "SELECT kind FROM lanes WHERE id=?1",
            [before["lane_id"].as_str().unwrap()],
            |r| r.get(0),
        )?;
        if lane == "done" {
            return Err(Error::new(
                "validation_error",
                "completed work cannot be cancelled",
            ));
        }
        let affected = crate::dependents_of(&tx, &id)?;
        let old = crate::capture_states(&tx, &affected)?;
        tx.execute("UPDATE cards SET cancelled_at=?2,cancellation_reason=?3,cancelled_by=?4,version=version+1,updated_at=?2 WHERE id=?1",params![id,now(),reason.trim(),actor])?;
        super::sessions::detach_for_card(&tx, &id)?;
        let after = card_row(&tx, &id)?;
        let mut effects =
            crate::changed_state_effects(&tx, &affected, &old, before["key"].as_str().unwrap())?;
        for dependent in affected {
            effects.push(json!({"type":"prerequisite_cancelled","card_id":dependent,"prerequisite_id":id,"owner_actor":old[&dependent]["owner_actor"]}));
        }
        record(
            &tx,
            actor,
            "card_cancelled",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &effects,
            reason.trim(),
        )?;
        self.commit(tx)?;
        Ok(after)
    }
}
