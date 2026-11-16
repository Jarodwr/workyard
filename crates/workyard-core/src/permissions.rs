//! Tool requests from workers in `safe` mode. The orchestrator opens a request when the
//! runtime asks the host whether a tool may run; a human decides it; the orchestrator reads
//! the decision event and answers the worker. Both steps are events.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::orchestrator::check_epoch_for;
use super::{
    Error, Result, Store, now, record, require_nonblank, resolve_board, resolve_card, uuid,
};

const COLUMNS: &str = "p.id,b.key,c.key,p.request_id,p.tool_use_id,p.tool_name,p.input_json,p.description,p.status,p.run_rule,p.decided_by,p.reason,p.created_at,p.decided_at";
const FROM: &str = "FROM permission_requests p JOIN cards c ON c.id=p.card_id JOIN boards b ON b.id=p.board_id";

fn row_json(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let input: String = r.get(6)?;
    Ok(json!({
        "id": r.get::<_, String>(0)?, "board_key": r.get::<_, String>(1)?, "card_key": r.get::<_, String>(2)?,
        "request_id": r.get::<_, String>(3)?, "tool_use_id": r.get::<_, String>(4)?,
        "tool_name": r.get::<_, String>(5)?, "input": serde_json::from_str::<Value>(&input).unwrap_or(Value::Null),
        "description": r.get::<_, String>(7)?, "status": r.get::<_, String>(8)?,
        "run_rule": r.get::<_, String>(9)?, "decided_by": r.get::<_, Option<String>>(10)?,
        "reason": r.get::<_, String>(11)?, "created_at": r.get::<_, String>(12)?,
        "decided_at": r.get::<_, Option<String>>(13)?,
    }))
}

fn request_row(db: &Connection, id: &str) -> Result<Value> {
    db.query_row(&format!("SELECT {COLUMNS} {FROM} WHERE p.id=?1"), [id], row_json)
        .optional()?
        .ok_or_else(|| Error::new("not_found", format!("permission request {id} not found")))
}

/// The rule an "allow for this run" decision grants, from the request itself: a Bash command
/// is allowed by its first word (`Bash(touch *)`), any other tool by name. Never wider than the
/// tool that asked.
pub fn default_run_rule(tool_name: &str, input: &Value) -> String {
    if tool_name == "Bash" {
        let first = input["command"].as_str().unwrap_or_default().split_whitespace().next().unwrap_or_default();
        if !first.is_empty() {
            return format!("Bash({first} *)");
        }
    }
    tool_name.to_owned()
}

impl Store {
    /// Record that a worker is waiting on a tool. Repeating the same runtime request returns
    /// the existing one. Orchestrator actor only.
    pub fn permission_request_open(
        &self,
        actor: &str,
        board: &str,
        card: &str,
        request_id: &str,
        tool_use_id: &str,
        tool_name: &str,
        input: &Value,
        description: &str,
    ) -> Result<Value> {
        require_nonblank("request id", request_id)?;
        require_nonblank("tool name", tool_name)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch_for(&tx, actor, &board_id)?;
        let card_id = resolve_card(&tx, card)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM permission_requests WHERE card_id=?1 AND request_id=?2",
                params![card_id, request_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return request_row(&tx, &id);
        }
        let id = uuid();
        let description: String = description.chars().take(300).collect();
        tx.execute(
            "INSERT INTO permission_requests(id,board_id,card_id,request_id,tool_use_id,tool_name,input_json,description,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, board_id, card_id, request_id, tool_use_id, tool_name, serde_json::to_string(input)?, description, now()],
        )?;
        let after = request_row(&tx, &id)?;
        record(&tx, actor, "permission_requested", &board_id, "permission_request", &id, None, Some(&after), &[json!({"card_id": card_id})], "")?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Requests on a board, newest first; `status` filters (for example `pending`).
    pub fn permission_requests(&self, board: &str, status: Option<&str>) -> Result<Value> {
        let db = self.open()?;
        let board_id = resolve_board(&db, board)?;
        let mut stmt = db.prepare(&format!(
            "SELECT {COLUMNS} {FROM} WHERE p.board_id=?1 AND (?2 IS NULL OR p.status=?2) ORDER BY p.created_at DESC, p.rowid DESC"
        ))?;
        let rows = stmt
            .query_map(params![board_id, status], row_json)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Value::Array(rows))
    }

    /// Decide a pending request: `allow_once`, `allow_run` (also allows what `rule` covers for
    /// the rest of the run; defaults to `default_run_rule`) or `deny`. A human decides: neither
    /// the supervisor nor a worker may.
    pub fn permission_decide(&self, actor: &str, id: &str, decision: &str, rule: Option<&str>, reason: &str) -> Result<Value> {
        crate::actor_policy::forbid_supervisor(actor, "decide a worker's permission request")?;
        if actor.starts_with("agent:") || actor.starts_with("orch:") {
            return Err(Error::new("forbidden_for_agents", "only a human may decide a permission request")
                .hint("Run `workyard orch perms decide` as yourself, or use the desktop app"));
        }
        let status = match decision {
            "allow_once" => "allowed_once",
            "allow_run" => "allowed_run",
            "deny" => "denied",
            _ => return Err(Error::new("validation_error", "decision must be allow_once, allow_run or deny")),
        };
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = request_row(&tx, id)?;
        if before["status"] != "pending" {
            return Err(Error::new("not_pending", format!("request is already {}", before["status"].as_str().unwrap_or("decided")))
                .details(json!({"current": before})));
        }
        let run_rule = match (status, rule.map(str::trim).filter(|r| !r.is_empty())) {
            ("allowed_run", Some(rule)) => rule.to_owned(),
            ("allowed_run", None) => default_run_rule(before["tool_name"].as_str().unwrap_or_default(), &before["input"]),
            _ => String::new(),
        };
        tx.execute(
            "UPDATE permission_requests SET status=?2,run_rule=?3,decided_by=?4,reason=?5,decided_at=?6 WHERE id=?1",
            params![id, status, run_rule, actor, reason, now()],
        )?;
        let after = request_row(&tx, id)?;
        let board_id = resolve_board(&tx, before["board_key"].as_str().unwrap_or_default())?;
        let card_id = resolve_card(&tx, before["card_key"].as_str().unwrap_or_default())?;
        record(&tx, actor, "permission_decided", &board_id, "permission_request", id, Some(&before), Some(&after), &[json!({"card_id": card_id})], reason)?;
        self.commit(tx)?;
        Ok(after)
    }

    /// The worker that was waiting is gone, so nobody can answer: close its pending requests
    /// as expired. Orchestrator actor only.
    pub fn permission_requests_expire(&self, actor: &str, board: &str, card: Option<&str>) -> Result<i64> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch_for(&tx, actor, &board_id)?;
        let card_id = card.map(|c| resolve_card(&tx, c)).transpose()?;
        let ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM permission_requests WHERE board_id=?1 AND status='pending' AND (?2 IS NULL OR card_id=?2)",
            )?;
            stmt.query_map(params![board_id, card_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for id in &ids {
            let before = request_row(&tx, id)?;
            tx.execute("UPDATE permission_requests SET status='expired',decided_at=?2 WHERE id=?1", params![id, now()])?;
            let after = request_row(&tx, id)?;
            record(&tx, actor, "permission_expired", &board_id, "permission_request", id, Some(&before), Some(&after), &[], "worker is gone")?;
        }
        if !ids.is_empty() {
            self.commit(tx)?;
        }
        Ok(ids.len() as i64)
    }

    /// Rules granted "for this run" on a card, to hand a respawned worker (the runtime keeps
    /// them only in the process that was asked).
    pub fn permission_run_rules(&self, card: &str) -> Result<Vec<String>> {
        let db = self.open()?;
        let card_id = resolve_card(&db, card)?;
        let mut stmt = db.prepare(
            "SELECT DISTINCT run_rule FROM permission_requests WHERE card_id=?1 AND status='allowed_run' AND run_rule!='' ORDER BY run_rule",
        )?;
        Ok(stmt.query_map([card_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
}
