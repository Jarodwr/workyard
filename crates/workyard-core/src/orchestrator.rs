//! One orchestrator session per board: desired state, event cursor, pool size, and a lease.
//!
//! The lease is held by at most one process. Every takeover increments `epoch`, a fencing
//! token carried in the orchestrator's actor (`orch:<board>#<epoch>`). `record` rejects any
//! write whose actor carries a stale epoch, so a suspended orchestrator cannot act after
//! it was replaced. Lease expiry alone is not the guarantee.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use super::{
    Error, Result, Store, check_board_active, now, record, require_nonblank, resolve_board,
};

pub const DEFAULT_LEASE_SECONDS: i64 = 30;

/// Epoch carried by an orchestrator actor such as `orch:B-1#3`; `None` for any other actor.
pub(crate) fn actor_epoch(actor: &str) -> Option<i64> {
    actor.strip_prefix("orch:")?.rsplit_once('#')?.1.parse().ok()
}

/// Reject a write from an orchestrator that no longer holds the board's lease.
pub(crate) fn check_epoch(db: &Connection, board_id: &str, epoch: i64) -> Result<()> {
    let current: Option<(i64, Option<String>)> = db
        .query_row(
            "SELECT epoch,holder FROM orchestrator_sessions WHERE board_id=?1",
            [board_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match current {
        Some((current, Some(_))) if current == epoch => Ok(()),
        _ => Err(Error::new(
            "stale_epoch",
            "this orchestrator no longer holds the board's lease",
        )
        .hint("Stop. Another orchestrator took over, or the lease was released")),
    }
}

fn epoch_of(actor: &str) -> Result<i64> {
    actor_epoch(actor).ok_or_else(|| {
        Error::new(
            "validation_error",
            "actor must be an orchestrator actor (orch:<board>#<epoch>) returned by orch acquire",
        )
    })
}

fn ensure_row(db: &Connection, board_id: &str) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO orchestrator_sessions(board_id,updated_at) VALUES (?1,?2)",
        params![board_id, now()],
    )?;
    Ok(())
}

fn pid_alive(pid: i64) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(true)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// A lease is live while unexpired and, when the holder ran on this host, while its pid exists.
fn lease_live(row: &Value, host: &str) -> bool {
    let Some(expires) = row["lease_expires_at"].as_str() else {
        return false;
    };
    let unexpired = DateTime::parse_from_rfc3339(expires)
        .map(|t| t.with_timezone(&Utc) > Utc::now())
        .unwrap_or(false);
    if !unexpired {
        return false;
    }
    match (row["host"].as_str(), row["pid"].as_i64()) {
        (Some(h), Some(pid)) if h == host => pid_alive(pid),
        _ => true,
    }
}

fn session_row(db: &Connection, board_id: &str) -> Result<Value> {
    let key: String = db.query_row("SELECT key FROM boards WHERE id=?1", [board_id], |r| r.get(0))?;
    let row = db
        .query_row(
            "SELECT desired_state,cursor,pool_size,holder,host,pid,epoch,heartbeat_at,lease_expires_at,backoff_until,backoff_reason FROM orchestrator_sessions WHERE board_id=?1",
            [board_id],
            |r| {
                Ok(json!({
                    "desired_state": r.get::<_, String>(0)?, "cursor": r.get::<_, i64>(1)?,
                    "pool_size": r.get::<_, i64>(2)?, "holder": r.get::<_, Option<String>>(3)?,
                    "host": r.get::<_, Option<String>>(4)?, "pid": r.get::<_, Option<i64>>(5)?,
                    "epoch": r.get::<_, i64>(6)?, "heartbeat_at": r.get::<_, Option<String>>(7)?,
                    "lease_expires_at": r.get::<_, Option<String>>(8)?,
                    "backoff_until": r.get::<_, Option<String>>(9)?,
                    "backoff_reason": r.get::<_, String>(10)?,
                }))
            },
        )
        .optional()?
        .unwrap_or_else(|| {
            json!({"desired_state":"running","cursor":0,"pool_size":1,"holder":null,"host":null,"pid":null,"epoch":0,"heartbeat_at":null,"lease_expires_at":null,"backoff_until":null,"backoff_reason":""})
        });
    let mut row = row;
    let epoch = row["epoch"].as_i64().unwrap_or(0);
    row["board_id"] = json!(board_id);
    row["board_key"] = json!(key);
    row["connected"] = json!(row["holder"].is_string() && lease_live(&row, ""));
    row["backing_off"] = json!(row["backoff_until"]
        .as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t.with_timezone(&Utc) > Utc::now()));
    row["actor"] = if row["holder"].is_string() {
        json!(format!("orch:{key}#{epoch}"))
    } else {
        Value::Null
    };
    Ok(row)
}

/// True while a live orchestrator holds the board's lease.
pub(crate) fn board_orchestrator_connected(db: &Connection, board_id: &str) -> Result<bool> {
    Ok(session_row(db, board_id)?["connected"] == true)
}

fn active_workers(db: &Connection, board_id: &str) -> Result<i64> {
    Ok(db.query_row(
        "SELECT COUNT(*) FROM task_sessions s JOIN cards c ON c.id=s.card_id WHERE c.board_id=?1 AND s.detached_at IS NULL AND s.paused_at IS NULL",
        [board_id],
        |r| r.get(0),
    )?)
}

impl Store {
    pub fn orchestrator_status(&self, board: &str) -> Result<Value> {
        let db = self.open()?;
        let board_id = resolve_board(&db, board)?;
        session_row(&db, &board_id)
    }

    /// Take the board's lease. Check and claim happen in one immediate transaction, so two
    /// simultaneous callers cannot both win. A takeover increments the epoch.
    pub fn orchestrator_acquire(
        &self,
        board: &str,
        holder: &str,
        host: &str,
        pid: Option<i64>,
        lease_seconds: i64,
        pool_size: Option<i64>,
    ) -> Result<Value> {
        require_nonblank("holder", holder)?;
        if !(5..=3600).contains(&lease_seconds) {
            return Err(Error::new("validation_error", "lease must be 5–3600 seconds"));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        ensure_row(&tx, &board_id)?;
        let before = session_row(&tx, &board_id)?;
        if before["holder"].is_string() && lease_live(&before, host) {
            return Err(Error::new(
                "orchestrator_conflict",
                format!(
                    "board {} already has a running orchestrator ({})",
                    before["board_key"].as_str().unwrap_or(board),
                    before["holder"].as_str().unwrap_or("unknown")
                ),
            )
            .hint("Stop that orchestrator, or wait for its lease to expire")
            .details(json!({"current":before})));
        }
        let epoch = before["epoch"].as_i64().unwrap_or(0) + 1;
        let expires = (Utc::now() + Duration::seconds(lease_seconds)).to_rfc3339();
        tx.execute(
            "UPDATE orchestrator_sessions SET holder=?2,host=?3,pid=?4,epoch=?5,heartbeat_at=?6,lease_expires_at=?7,pool_size=COALESCE(?8,pool_size),updated_at=?6 WHERE board_id=?1",
            params![board_id, holder.trim(), host, pid, epoch, now(), expires, pool_size],
        )?;
        let after = session_row(&tx, &board_id)?;
        let actor = after["actor"].as_str().unwrap().to_owned();
        record(&tx, &actor, "orchestrator_acquired", &board_id, "orchestrator", &board_id, Some(&before), Some(&after), &[], "")?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Renew the lease. The reply carries `desired_state`, so a running orchestrator learns
    /// about pause and drain here. Heartbeats write no event and publish no tick.
    pub fn orchestrator_heartbeat(&self, actor: &str, board: &str, lease_seconds: i64) -> Result<Value> {
        let epoch = epoch_of(actor)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch(&tx, &board_id, epoch)?;
        let expires = (Utc::now() + Duration::seconds(lease_seconds)).to_rfc3339();
        tx.execute(
            "UPDATE orchestrator_sessions SET heartbeat_at=?2,lease_expires_at=?3 WHERE board_id=?1",
            params![board_id, now(), expires],
        )?;
        let row = session_row(&tx, &board_id)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn orchestrator_release(&self, actor: &str, board: &str) -> Result<Value> {
        let epoch = epoch_of(actor)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch(&tx, &board_id, epoch)?;
        let before = session_row(&tx, &board_id)?;
        // Record while the lease is still held: `record` fences on it.
        let mut preview = before.clone();
        for field in ["holder", "host", "pid", "heartbeat_at", "lease_expires_at", "actor"] {
            preview[field] = Value::Null;
        }
        preview["connected"] = json!(false);
        record(&tx, actor, "orchestrator_released", &board_id, "orchestrator", &board_id, Some(&before), Some(&preview), &[], "")?;
        tx.execute(
            "UPDATE orchestrator_sessions SET holder=NULL,host=NULL,pid=NULL,lease_expires_at=NULL,heartbeat_at=NULL,updated_at=?2 WHERE board_id=?1",
            params![board_id, now()],
        )?;
        let after = session_row(&tx, &board_id)?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Record the last event the orchestrator has fully processed. Never moves backwards.
    pub fn orchestrator_set_cursor(&self, actor: &str, board: &str, cursor: i64) -> Result<Value> {
        let epoch = epoch_of(actor)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch(&tx, &board_id, epoch)?;
        tx.execute(
            "UPDATE orchestrator_sessions SET cursor=MAX(cursor,?2) WHERE board_id=?1",
            params![board_id, cursor.max(0)],
        )?;
        let row = session_row(&tx, &board_id)?;
        tx.commit()?;
        Ok(row)
    }

    /// Pause, resume, or drain. Callable by anyone; pausing only stops new dispatches.
    pub fn orchestrator_set_state(&self, actor: &str, board: &str, state: &str) -> Result<Value> {
        if !["running", "paused", "draining"].contains(&state) {
            return Err(Error::new("validation_error", "state must be running, paused, or draining"));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        ensure_row(&tx, &board_id)?;
        let before = session_row(&tx, &board_id)?;
        if before["desired_state"] == state {
            return Ok(before);
        }
        tx.execute(
            "UPDATE orchestrator_sessions SET desired_state=?2,updated_at=?3 WHERE board_id=?1",
            params![board_id, state, now()],
        )?;
        let after = session_row(&tx, &board_id)?;
        record(&tx, actor, "orchestrator_state_changed", &board_id, "orchestrator", &board_id, Some(&before), Some(&after), &[], "")?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Stop dispatching until `until` (RFC 3339), recording why, or clear the backoff with
    /// `None`. A later time replaces an earlier one; an earlier time never shortens a backoff
    /// already in force, so two workers reporting different limits cannot undo each other.
    pub fn orchestrator_set_backoff(
        &self,
        actor: &str,
        board: &str,
        until: Option<&str>,
        reason: &str,
    ) -> Result<Value> {
        let epoch = epoch_of(actor)?;
        let new_until = until
            .map(|t| {
                DateTime::parse_from_rfc3339(t)
                    .map(|t| t.with_timezone(&Utc))
                    .map_err(|_| Error::new("validation_error", "until must be an RFC 3339 time"))
            })
            .transpose()?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch(&tx, &board_id, epoch)?;
        let before = session_row(&tx, &board_id)?;
        let current = before["backoff_until"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .filter(|t| *t > Utc::now());
        let (stored, why) = match (new_until, current) {
            (None, _) => (None, ""),
            (Some(new), Some(old)) if old >= new => (Some(old), before["backoff_reason"].as_str().unwrap_or("")),
            (Some(new), _) => (Some(new), reason),
        };
        if stored == current && why == before["backoff_reason"].as_str().unwrap_or("") {
            return Ok(before);
        }
        tx.execute(
            "UPDATE orchestrator_sessions SET backoff_until=?2,backoff_reason=?3,updated_at=?4 WHERE board_id=?1",
            params![board_id, stored.map(|t| t.to_rfc3339()), why, now()],
        )?;
        let after = session_row(&tx, &board_id)?;
        record(&tx, actor, "orchestrator_backoff_changed", &board_id, "orchestrator", &board_id, Some(&before), Some(&after), &[], why)?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn orchestrator_set_pool(&self, actor: &str, board: &str, size: i64) -> Result<Value> {
        if !(1..=64).contains(&size) {
            return Err(Error::new("validation_error", "pool size must be 1–64"));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        ensure_row(&tx, &board_id)?;
        let before = session_row(&tx, &board_id)?;
        tx.execute(
            "UPDATE orchestrator_sessions SET pool_size=?2,updated_at=?3 WHERE board_id=?1",
            params![board_id, size, now()],
        )?;
        let after = session_row(&tx, &board_id)?;
        record(&tx, actor, "orchestrator_pool_changed", &board_id, "orchestrator", &board_id, Some(&before), Some(&after), &[], "")?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Pick the next unit of work and, for a new Backlog task, start it in one transaction:
    /// claim for `worker`, move to In Progress, and attach the runtime session. A paused task
    /// is only returned (`dispatch: resume`); the orchestrator must still read its briefing
    /// pages and call `session resume`. Returns `dispatch: null` with a reason when nothing
    /// should start (paused, draining, pool full, or no ready work).
    pub fn orchestrator_dispatch(
        &self,
        actor: &str,
        board: &str,
        worker: &str,
        runner: &str,
        external_session_id: &str,
        agent_type: Option<&str>,
    ) -> Result<Value> {
        let epoch = epoch_of(actor)?;
        require_nonblank("worker", worker)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_epoch(&tx, &board_id, epoch)?;
        let session = session_row(&tx, &board_id)?;
        let idle = |reason: &str| json!({"dispatch":null,"reason":reason});
        if session["backing_off"] == true {
            return Ok(json!({"dispatch":null,"reason":"backing_off","until":session["backoff_until"],"why":session["backoff_reason"]}));
        }
        if session["desired_state"] != "running" {
            return Ok(idle(session["desired_state"].as_str().unwrap_or("paused")));
        }
        if active_workers(&tx, &board_id)? >= session["pool_size"].as_i64().unwrap_or(1) {
            return Ok(idle("pool_full"));
        }
        let next = Store::next_ready_task_on(&tx, &board_id, agent_type)?;
        if next.is_null() {
            return Ok(idle("no_work"));
        }
        if next["dispatch_action"] == "resume" {
            return Ok(json!({"dispatch":"resume","card":next}));
        }
        let key = next["key"].as_str().unwrap().to_owned();
        let claimed = Store::claim_card_tx(&tx, worker, &key, next["version"].as_i64().unwrap())?;
        let moved = Store::move_card_tx(&tx, worker, &key, "In Progress", None, claimed["version"].as_i64())?;
        let attached = Store::attach_task_session_tx(&tx, worker, &key, runner, external_session_id)?;
        record(
            &tx,
            actor,
            "orchestrator_dispatched",
            &board_id,
            "card",
            moved["id"].as_str().unwrap(),
            None,
            Some(&json!({"worker":worker,"runner":runner,"session_id":external_session_id})),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(json!({"dispatch":"claim","card":moved,"session":attached,"worker_actor":worker}))
    }

    /// Latest tick marker; pass it to `wait_for_tick` after reading events so a write that
    /// lands in between is not missed.
    pub fn tick_marker(&self) -> String {
        match std::fs::metadata(self.tick_path()) {
            Ok(m) => format!(
                "{:?}:{}",
                m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()),
                std::fs::read_to_string(self.tick_path()).unwrap_or_default()
            ),
            Err(_) => String::new(),
        }
    }

    /// Wait until the tick changes or `timeout` passes; returns the marker to wait on next.
    /// Waking spuriously is harmless: callers always re-read the event log.
    pub fn wait_for_tick(&self, since: &str, timeout: std::time::Duration) -> String {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let marker = self.tick_marker();
            if marker != since || std::time::Instant::now() >= deadline {
                return marker;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
