//! The single rules and storage layer shared by Workyard's adapters.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

mod orchestrator;
pub use orchestrator::DEFAULT_LEASE_SECONDS;
mod phases;
mod projects;
mod sessions;

pub mod types;
use types::{
    Blocker, BlockerKind, BoardId, Card, CardId, DependencyTarget, LaneId, LaneKind, PhaseId,
    Priority, Progress, Question, QuestionId, QuestionKind, QuestionOptions, QuestionOutcome,
    QuestionStatus, ReviewGate, Version,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl Error {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: None,
            details: None,
        }
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Error::new("database_error", value.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::new("io_error", value.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::new("json_error", value.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

#[derive(Debug, Default, Clone)]
pub struct BoardFilter {
    pub lane: Option<String>,
    pub phase: Option<String>,
    pub agent_type: Option<String>,
    pub blocked_only: bool,
    pub text: Option<String>,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> Result<PathBuf> {
        if let Ok(path) = std::env::var("WORKYARD_DB") {
            return Ok(PathBuf::from(path));
        }
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| Error::new("config_error", "HOME is not set"))?;
            return Ok(PathBuf::from(home).join("Library/Application Support/workyard/workyard.db"));
        }
        #[cfg(not(target_os = "macos"))]
        {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                .ok_or_else(|| Error::new("config_error", "No app-data directory found"))?;
            Ok(base.join("workyard/workyard.db"))
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// File whose contents and mtime change after every committed write. Other processes
    /// (the desktop, `events --follow`) wait on it instead of polling the database.
    pub fn tick_path(&self) -> PathBuf {
        let mut path = self.path.clone().into_os_string();
        path.push(".tick");
        PathBuf::from(path)
    }

    /// Commit, then publish the latest event ID. The event log stays the source of truth;
    /// a missed or failed tick only delays a watcher until its fallback poll.
    pub(crate) fn commit(&self, tx: Transaction) -> Result<()> {
        let latest: i64 = tx.query_row("SELECT COALESCE(MAX(id),0) FROM events", [], |r| r.get(0))?;
        tx.commit()?;
        let _ = fs::write(self.tick_path(), latest.to_string());
        Ok(())
    }

    pub fn open(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut db = Connection::open(&self.path)?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.busy_timeout(Duration::from_secs(5))?;
        migrate(&mut db)?;
        Ok(db)
    }

    pub fn create_board(&self, actor: &str, name: &str, description: &str) -> Result<Value> {
        require_nonblank("name", name)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = uuid();
        let key = next_key(&tx, "board", "B")?;
        let now = now();
        tx.execute(
            "INSERT INTO boards(id,key,name,description,created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, key, name.trim(), description, now],
        )?;
        let mut lane_effects = Vec::new();
        for (position, (lane_name, kind)) in [
            ("Backlog", LaneKind::Backlog),
            ("In Progress", LaneKind::Active),
            ("Done", LaneKind::Done),
        ]
        .into_iter()
        .enumerate()
        {
            let lane_id = uuid();
            tx.execute(
                "INSERT INTO lanes(id,board_id,name,position,kind) VALUES (?1,?2,?3,?4,?5)",
                params![lane_id, id, lane_name, position as i64, kind.as_str()],
            )?;
            lane_effects.push(
                json!({"type":"lane_created","lane_id":lane_id,"after":lane_row(&tx,&lane_id)?}),
            );
        }
        let phase_id = uuid();
        tx.execute("INSERT INTO phases(id,board_id,name,position,created_at) VALUES (?1,?2,'Current work',0,?3)",params![phase_id,id,now])?;
        tx.execute(
            "UPDATE boards SET current_phase_id=?2 WHERE id=?1",
            params![id, phase_id],
        )?;
        lane_effects.push(json!({"type":"phase_created","phase_id":phase_id,"after":phases::phase_row(&tx,&phase_id)?}));
        let after = board_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "board_created",
            &id,
            "board",
            &id,
            None,
            Some(&after),
            &lane_effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn list_boards(&self) -> Result<Value> {
        let db = self.open()?;
        let mut stmt = db.prepare("SELECT id FROM boards ORDER BY created_at, key")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut boards = Vec::new();
        for id in ids {
            let mut board = board_row(&db, &id)?;
            let mut counts = Vec::new();
            let mut lanes = db.prepare(
                "SELECT id,name FROM lanes WHERE board_id=?1 AND archived=0 ORDER BY position",
            )?;
            for row in lanes.query_map([&id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (lane_id, name) = row?;
                let count: i64 = db.query_row(
                    "SELECT COUNT(*) FROM cards WHERE lane_id=?1 AND cancelled_at IS NULL",
                    [&lane_id],
                    |r| r.get(0),
                )?;
                counts.push(json!({"lane_id":lane_id,"name":name,"count":count}));
            }
            board["lanes"] = json!(counts);
            boards.push(board);
        }
        Ok(json!(boards))
    }

    pub fn set_board_archived(
        &self,
        actor: &str,
        board: &str,
        expected: i64,
        archived: bool,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_board(&tx, board)?;
        let before = board_row(&tx, &id)?;
        check_version(&before, expected)?;
        if before["archived"] == archived {
            return Err(Error::new(
                "validation_error",
                "board is already in that archive state",
            ));
        }
        tx.execute(
            "UPDATE boards SET archived=?2,version=version+1 WHERE id=?1",
            params![id, archived],
        )?;
        let after = board_row(&tx, &id)?;
        record(
            &tx,
            actor,
            if archived {
                "board_archived"
            } else {
                "board_unarchived"
            },
            &id,
            "board",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn set_lane_archived(
        &self,
        actor: &str,
        lane: &str,
        expected: i64,
        archived: bool,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_lane_global(&tx, lane)?;
        let before = lane_row(&tx, &id)?;
        check_version(&before, expected)?;
        let lane_kind = LaneKind::try_from(before["kind"].as_str().unwrap_or(""))?;
        if !archived || lane_kind.is_work() {
            return Err(Error::new(
                "fixed_lane",
                "the three work lanes are fixed; only an empty legacy lane may be archived",
            ));
        }
        if before["archived"] == archived {
            return Err(Error::new(
                "validation_error",
                "lane is already in that archive state",
            ));
        }
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        if archived {
            let count: i64 =
                tx.query_row("SELECT COUNT(*) FROM cards WHERE lane_id=?1", [&id], |r| {
                    r.get(0)
                })?;
            if count > 0 {
                return Err(Error::new(
                    "lane_not_empty",
                    "move its cards before archiving the lane",
                ));
            }
        }
        tx.execute(
            "UPDATE lanes SET archived=?2,version=version+1 WHERE id=?1",
            params![id, archived],
        )?;
        let after = lane_row(&tx, &id)?;
        record(
            &tx,
            actor,
            if archived {
                "lane_archived"
            } else {
                "lane_unarchived"
            },
            board_id,
            "lane",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn view_history(&self, entity: &str, since: i64, limit: i64) -> Result<Value> {
        if since < 0 || !(1..=1000).contains(&limit) {
            return Err(Error::new(
                "validation_error",
                "since must be nonnegative and limit must be 1–1000",
            ));
        }
        let db = self.open()?;
        if entity.starts_with("C-")
            || db.query_row(
                "SELECT EXISTS(SELECT 1 FROM cards WHERE id=?1)",
                [entity],
                |r| r.get::<_, bool>(0),
            )?
        {
            let card_id = resolve_card(&db, entity)?;
            return self.history_for_card_with_db(&db, &card_id, since, limit);
        }
        let board_id = resolve_board(&db, entity)?;
        let mut stmt=db.prepare("SELECT id,ts,actor,type,board_id,entity_type,entity_id,before_json,after_json,effects_json,note FROM events WHERE board_id=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = stmt
            .query_map(params![board_id, since, limit], event_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(json!(rows))
    }

    pub fn view_board(&self, board: &str) -> Result<Value> {
        let db = self.open()?;
        let board_id = resolve_board(&db, board)?;
        let mut output = board_row(&db, &board_id)?;
        let mut lanes = Vec::new();
        let mut stmt = db.prepare("SELECT id,name,kind,position,archived,version FROM lanes WHERE board_id=?1 AND archived=0 ORDER BY position,id")?;
        let rows = stmt.query_map([&board_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (lane_id, name, kind, position, archived, version) = row?;
            let mut cards = Vec::new();
            let mut card_stmt =
                db.prepare("SELECT id FROM cards WHERE lane_id=?1 ORDER BY position,id")?;
            for card in card_stmt.query_map([&lane_id], |r| r.get::<_, String>(0))? {
                let mut row = card_row(&db, &card?)?;
                annotate_claim_activity(&db, &mut row)?;
                cards.push(row);
            }
            lanes.push(json!({"id":lane_id,"name":name,"kind":kind,"position":position,"archived":archived!=0,"version":version,"cards":cards}));
        }
        output["lanes"] = json!(lanes);
        output["phases"] = self.list_phases(&board_id)?;
        Ok(output)
    }

    pub fn view_board_filtered(&self, board: &str, filter: &BoardFilter) -> Result<Value> {
        let mut output = self.view_board(board)?;
        let search = filter.text.as_ref().map(|s| s.to_lowercase());
        let phase_id = filter
            .phase
            .as_deref()
            .map(|phase| {
                phases::resolve_phase(&self.open()?, output["id"].as_str().unwrap(), Some(phase))
                    .map(|(id, _)| id)
            })
            .transpose()?;
        if let Some(lanes) = output["lanes"].as_array_mut() {
            lanes.retain(|lane| {
                filter.lane.as_ref().is_none_or(|name| {
                    lane["id"].as_str() == Some(name.as_str())
                        || lane["name"].as_str() == Some(name.as_str())
                })
            });
            for lane in lanes {
                if let Some(cards) = lane["cards"].as_array_mut() {
                    cards.retain(|card| {
                        phase_id
                            .as_ref()
                            .is_none_or(|id| card["phase_id"].as_str() == Some(id.as_str()))
                            && filter
                                .agent_type
                                .as_ref()
                                .is_none_or(|a| card["agent_type"].as_str() == Some(a.as_str()))
                            && (!filter.blocked_only || card["blocked"] == true)
                            && search.as_ref().is_none_or(|term| {
                                ["title", "description", "status_note"].iter().any(|field| {
                                    card[*field]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_lowercase()
                                        .contains(term)
                                })
                            })
                    });
                }
            }
        }
        Ok(output)
    }

    pub fn view_card(&self, card: &str) -> Result<Value> {
        let db = self.open()?;
        let card_id = resolve_card(&db, card)?;
        let mut output = card_row(&db, &card_id)?;
        annotate_claim_activity(&db, &mut output)?;
        let mut deps = Vec::new();
        let mut stmt = db.prepare("SELECT id,from_card_id,to_card_id,to_question_id,scope FROM dependencies WHERE from_card_id=?1 OR to_card_id=?1 ORDER BY created_at,id")?;
        for row in stmt.query_map([&card_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (id, from, to_card, to_question, scope) = row?;
            deps.push(json!({"id":id,"from_card_id":from,"to_card_id":to_card,"to_question_id":to_question,"scope":scope}));
        }
        output["dependencies"] = json!(deps);
        let mut changes = Vec::new();
        let mut stmt =
            db.prepare("SELECT id FROM card_changes WHERE card_id=?1 ORDER BY created_at,id")?;
        for change in stmt.query_map([&card_id], |r| r.get::<_, String>(0))? {
            changes.push(projects::change_row(&db, &change?)?);
        }
        output["changes"] = json!(changes);
        let mut notes = Vec::new();
        let mut stmt = db.prepare("SELECT id,card_id,actor,body,created_at FROM card_notes WHERE card_id=?1 ORDER BY created_at,id")?;
        for row in stmt.query_map([&card_id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"card_id":r.get::<_,String>(1)?,"actor":r.get::<_,String>(2)?,"body":r.get::<_,String>(3)?,"created_at":r.get::<_,String>(4)?})))? {
            notes.push(row?);
        }
        output["notes"] = json!(notes);
        output["session"] = json!(sessions::active_session(&db, &card_id)?);
        output["events"] = self.history_for_card_with_db(&db, &card_id, 0, 20)?;
        Ok(output)
    }

    pub fn read_card(&self, card: &str) -> Result<Card> {
        let db = self.open()?;
        let id = resolve_card(&db, card)?;
        card_snapshot(&db, &id)
    }

    pub fn read_question(&self, question: &str) -> Result<Question> {
        let db = self.open()?;
        let id = resolve_question(&db, question)?;
        question_snapshot(&db, &id)
    }

    /// Add a separate annotation without changing agent-owned card fields or version.
    pub fn add_card_note(&self, actor: &str, card: &str, body: &str) -> Result<Value> {
        require_nonblank("actor", actor)?;
        require_nonblank("body", body)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let card_id = resolve_card(&tx, card)?;
        let board_id: String =
            tx.query_row("SELECT board_id FROM cards WHERE id=?1", [&card_id], |r| {
                r.get(0)
            })?;
        check_board_active(&tx, &board_id)?;
        let note = json!({"id":uuid(),"card_id":card_id,"actor":actor,"body":body.trim(),"created_at":now()});
        tx.execute("INSERT INTO card_notes(id,board_id,card_id,actor,body,created_at) VALUES (?1,?2,?3,?4,?5,?6)", params![note["id"].as_str(),board_id,note["card_id"].as_str(),actor,note["body"].as_str(),note["created_at"].as_str()])?;
        record(
            &tx,
            actor,
            "card_note_added",
            &board_id,
            "card",
            &card_id,
            None,
            Some(&note),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(note)
    }

    pub fn create_card(
        &self,
        actor: &str,
        board: &str,
        title: &str,
        description: &str,
        priority: &str,
        lane: Option<&str>,
    ) -> Result<Value> {
        self.create_card_with_type(actor, board, title, description, priority, lane, "")
    }

    pub fn create_card_with_type(
        &self,
        actor: &str,
        board: &str,
        title: &str,
        description: &str,
        priority: &str,
        lane: Option<&str>,
        agent_type: &str,
    ) -> Result<Value> {
        self.create_card_in_phase(
            actor,
            board,
            title,
            description,
            priority,
            lane,
            agent_type,
            None,
        )
    }

    pub fn create_card_in_phase(
        &self,
        actor: &str,
        board: &str,
        title: &str,
        description: &str,
        priority: &str,
        lane: Option<&str>,
        agent_type: &str,
        phase: Option<&str>,
    ) -> Result<Value> {
        require_nonblank("title", title)?;
        validate_priority(priority)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let (phase_id, _) = phases::resolve_phase(&tx, &board_id, phase)?;
        phases::ensure_phase_open(&tx, &board_id, &phase_id)?;
        let lane_id = if let Some(lane) = lane {
            resolve_lane(&tx, &board_id, lane)?
        } else {
            tx.query_row("SELECT id FROM lanes WHERE board_id=?1 AND kind='backlog' AND archived=0 ORDER BY position LIMIT 1", [&board_id], |r| r.get(0)).optional()?.ok_or_else(|| Error::new("not_found", "board has no active backlog lane"))?
        };
        ensure_work_lane(&tx, &lane_id)?;
        let (_, current_position) = phases::current_phase(&tx, &board_id)?;
        let selected_position: i64 = tx.query_row(
            "SELECT position FROM phases WHERE id=?1",
            [&phase_id],
            |r| r.get(0),
        )?;
        let lane_kind: String =
            tx.query_row("SELECT kind FROM lanes WHERE id=?1", [&lane_id], |r| {
                r.get(0)
            })?;
        if selected_position > current_position && lane_kind != "backlog" {
            return Err(Error::new(
                "phase_planned",
                "future-phase cards must be created in Backlog",
            ));
        }
        let position: i64 = tx.query_row(
            "SELECT COUNT(*) FROM cards WHERE lane_id=?1",
            [&lane_id],
            |r| r.get(0),
        )?;
        let id = uuid();
        let key = next_key(&tx, "card", "C")?;
        let at = now();
        tx.execute("INSERT INTO cards(id,key,board_id,lane_id,position,title,description,priority,agent_type,created_at,updated_at,phase_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,?11)", params![id,key,board_id,lane_id,position,title.trim(),description,priority,agent_type.trim(),at,phase_id])?;
        let after = card_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "card_created",
            &board_id,
            "card",
            &id,
            None,
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn update_card(
        &self,
        actor: &str,
        card: &str,
        expected: impl Into<Option<i64>>,
        title: Option<&str>,
        description: Option<&str>,
        priority: Option<&str>,
        agent_type: Option<&str>,
    ) -> Result<Value> {
        if title.is_none() && description.is_none() && priority.is_none() && agent_type.is_none() {
            return Err(Error::new("validation_error", "no fields to update"));
        }
        if let Some(title) = title {
            require_nonblank("title", title)?;
        }
        if let Some(priority) = priority {
            validate_priority(priority)?;
        }
        let expected: Option<i64> = expected.into();
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, expected)?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        tx.execute("UPDATE cards SET title=COALESCE(?2,title), description=COALESCE(?3,description), priority=COALESCE(?4,priority), agent_type=COALESCE(?5,agent_type), updated_at=?6, version=version+1 WHERE id=?1", params![id,title.map(str::trim),description,priority,agent_type.map(str::trim),now()])?;
        let after = card_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "card_updated",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    /// Choose where the card's worker runs: `worktree` (its own git worktree and branch, the
    /// default) or `none` for investigative work that changes no repository.
    pub fn set_card_workspace(
        &self,
        actor: &str,
        card: &str,
        workspace: &str,
        expected: impl Into<Option<i64>>,
    ) -> Result<Value> {
        if !["worktree", "none"].contains(&workspace) {
            return Err(Error::new("validation_error", "workspace must be worktree or none"));
        }
        let expected: Option<i64> = expected.into();
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, expected)?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        if before["workspace"] == workspace {
            return Ok(before);
        }
        tx.execute(
            "UPDATE cards SET workspace=?2,updated_at=?3,version=version+1 WHERE id=?1",
            params![id, workspace, now()],
        )?;
        let after = card_row(&tx, &id)?;
        record(&tx, actor, "card_workspace_changed", board_id, "card", &id, Some(&before), Some(&after), &[], "")?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn claim_card(&self, actor: &str, card: &str, expected: i64) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let after = Self::claim_card_tx(&tx, actor, card, expected)?;
        self.commit(tx)?;
        Ok(after)
    }

    pub(crate) fn claim_card_tx(tx: &Transaction, actor: &str, card: &str, expected: i64) -> Result<Value> {
        require_nonblank("actor", actor)?;
        let id = resolve_card(&tx, card)?;
        let before = card_row(&tx, &id)?;
        check_version(&before, expected)?;
        if let Some(owner) = before["owner_actor"].as_str() {
            return Err(Error::new(
                "ownership_conflict",
                format!("card is owned by {owner}"),
            ));
        }
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        let lane_kind: String = tx.query_row(
            "SELECT kind FROM lanes WHERE id=?1",
            [before["lane_id"].as_str().unwrap()],
            |r| r.get(0),
        )?;
        if lane_kind != "backlog" {
            return Err(Error::new(
                "not_claimable",
                "only Backlog cards can be claimed",
            ));
        }
        if before["blocked"] == true {
            return Err(
                Error::new("blocked", "resolve start blockers before claiming work")
                    .details(json!({"blockers":before["blockers"]})),
            );
        }
        phases::ensure_schedulable(&tx, &before)?;
        tx.execute(
            "UPDATE cards SET owner_actor=?2,updated_at=?3,version=version+1 WHERE id=?1",
            params![id, actor, now()],
        )?;
        let after = card_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "card_claimed",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        Ok(after)
    }

    pub fn release_card(
        &self,
        actor: &str,
        card: &str,
        expected: impl Into<Option<i64>>,
        force_reason: Option<&str>,
    ) -> Result<Value> {
        let expected: Option<i64> = expected.into();
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = card_row(&tx, &id)?;
        match expected {
            Some(expected) => check_version(&before, expected)?,
            None if force_reason.is_none() && before["owner_actor"].as_str() == Some(actor) => {}
            None if force_reason.is_some() => {
                return Err(Error::new(
                    "version_required",
                    "force release needs the card's expected version",
                )
                .hint("Pass --expected-version")
                .details(json!({"current":before})));
            }
            None => return Err(version_required(&before, actor)),
        }
        let owner = before["owner_actor"]
            .as_str()
            .ok_or_else(|| Error::new("validation_error", "card is not claimed"))?;
        if force_reason.is_none() && owner != actor {
            return Err(owner_error(owner));
        }
        if let Some(reason) = force_reason {
            require_nonblank("reason", reason)?;
        }
        let board_id = before["board_id"].as_str().unwrap();
        phases::ensure_phase_open(&tx, board_id, before["phase_id"].as_str().unwrap())?;
        if before["cancelled_at"].is_string() {
            return Err(Error::new(
                "card_cancelled",
                "cancelled cards cannot change ownership",
            ));
        }
        tx.execute(
            "UPDATE cards SET owner_actor=NULL,updated_at=?2,version=version+1 WHERE id=?1",
            params![id, now()],
        )?;
        sessions::detach_for_card(&tx, &id)?;
        let after = card_row(&tx, &id)?;
        let kind = if force_reason.is_some() {
            "card_force_released"
        } else {
            "card_released"
        };
        record(
            &tx,
            actor,
            kind,
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &[],
            force_reason.unwrap_or(""),
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn transfer_card(
        &self,
        actor: &str,
        card: &str,
        expected: i64,
        new_owner: &str,
    ) -> Result<Value> {
        require_nonblank("new owner", new_owner)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = card_row(&tx, &id)?;
        check_version(&before, expected)?;
        if before["owner_actor"].as_str() != Some(actor) {
            return Err(owner_error(
                before["owner_actor"].as_str().unwrap_or("no one"),
            ));
        }
        if actor == new_owner {
            return Err(Error::new(
                "validation_error",
                "new owner must differ from current owner",
            ));
        }
        let board_id = before["board_id"].as_str().unwrap();
        phases::ensure_phase_open(&tx, board_id, before["phase_id"].as_str().unwrap())?;
        if before["cancelled_at"].is_string() {
            return Err(Error::new(
                "card_cancelled",
                "cancelled cards cannot change ownership",
            ));
        }
        tx.execute(
            "UPDATE cards SET owner_actor=?2,updated_at=?3,version=version+1 WHERE id=?1",
            params![id, new_owner, now()],
        )?;
        sessions::detach_for_card(&tx, &id)?;
        let after = card_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "card_transferred",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn move_card(
        &self,
        actor: &str,
        card: &str,
        destination: &str,
        position: Option<i64>,
        expected: impl Into<Option<i64>>,
    ) -> Result<Value> {
        let expected: Option<i64> = expected.into();
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let after = Self::move_card_tx(&tx, actor, card, destination, position, expected)?;
        self.commit(tx)?;
        Ok(after)
    }

    pub(crate) fn move_card_tx(tx: &Transaction, actor: &str, card: &str, destination: &str, position: Option<i64>, expected: Option<i64>) -> Result<Value> {
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, expected)?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        let to_lane = resolve_lane(&tx, board_id, destination)?;
        let to_kind: String =
            tx.query_row("SELECT kind FROM lanes WHERE id=?1", [&to_lane], |r| {
                r.get(0)
            })?;
        let to_kind = LaneKind::try_from(to_kind.as_str())?;
        if !to_kind.is_work() {
            return Err(Error::new(
                "fixed_lane",
                "cards can only move to Backlog, In Progress, or Done",
            ));
        }
        let from_lane = before["lane_id"].as_str().unwrap();
        if to_kind == LaneKind::Active && from_lane != to_lane {
            phases::ensure_schedulable(&tx, &before)?;
        }
        if to_kind == LaneKind::Done && from_lane != to_lane {
            let (_, current) = phases::current_phase(&tx, board_id)?;
            let phase: i64 = tx.query_row(
                "SELECT position FROM phases WHERE id=?1",
                [before["phase_id"].as_str().unwrap()],
                |r| r.get(0),
            )?;
            if phase > current && before["lane_id"] != to_lane {
                let from_kind: String =
                    tx.query_row("SELECT kind FROM lanes WHERE id=?1", [from_lane], |r| {
                        r.get(0)
                    })?;
                if from_kind != "active" {
                    return Err(Error::new(
                        "phase_planned",
                        "start next-phase work before completing it",
                    ));
                }
            }
        }
        if to_kind == LaneKind::Active && from_lane != to_lane && before["blocked"] == true {
            return Err(Error::new(
                "blocked",
                "card cannot move into an active lane while blocked",
            )
            .hint("Resolve its blockers first")
            .details(json!({"blockers":before["blockers"]})));
        }
        if to_kind == LaneKind::Done && before["can_finish"] == false {
            return Err(Error::new("finish_blocked", "card has unfinished completion requirements")
                .details(json!({"blockers":before["blockers"],"finish_blockers":before["finish_blockers"],"review_gates":before["review_gates"]})));
        }
        if to_kind == LaneKind::Done && from_lane != to_lane {
            let pending = projects::unintegrated_changes(&tx, &id)?;
            if !pending.is_empty() {
                return Err(Error::new("not_integrated", "card has changes that are not on their target branch yet")
                    .hint("Merge the main branch into your branch, run the tests, then fast-forward the target branch to your branch and try again")
                    .details(json!({"changes":pending})));
            }
        }
        let affected = dependents_of(&tx, &id)?;
        let old_states = capture_states(&tx, &affected)?;
        let mut source_ids = ordered_cards(&tx, from_lane)?;
        source_ids.retain(|x| x != &id);
        let mut target_ids = if from_lane == to_lane {
            source_ids.clone()
        } else {
            ordered_cards(&tx, &to_lane)?
        };
        let index = position
            .unwrap_or(target_ids.len() as i64)
            .clamp(0, target_ids.len() as i64) as usize;
        target_ids.insert(index, id.clone());
        if from_lane != to_lane {
            renumber(&tx, &source_ids, from_lane)?;
        }
        // Moving to a different lane must happen before renumbering the destination.
        tx.execute("UPDATE cards SET lane_id=?2,progress=CASE WHEN ?3='done' THEN 100 ELSE progress END,updated_at=?4,version=version+1 WHERE id=?1", params![id,to_lane,to_kind.as_str(),now()])?;
        if to_kind != LaneKind::Active {
            sessions::detach_for_card(&tx, &id)?;
        }
        renumber(&tx, &target_ids, &to_lane)?;
        let after = card_row(&tx, &id)?;
        let effects =
            changed_state_effects(&tx, &affected, &old_states, before["key"].as_str().unwrap())?;
        record(
            &tx,
            actor,
            "card_moved",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        Ok(after)
    }

    pub fn update_progress(
        &self,
        actor: &str,
        card: &str,
        progress: i64,
        note: Option<&str>,
        expected: impl Into<Option<i64>>,
    ) -> Result<Value> {
        let progress = i64::from(Progress::try_from(progress)?);
        let expected: Option<i64> = expected.into();
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, expected)?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        tx.execute("UPDATE cards SET progress=?2,status_note=COALESCE(?3,status_note),updated_at=?4,version=version+1 WHERE id=?1", params![id,progress,note,now()])?;
        let after = card_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "progress_updated",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn add_dependency(
        &self,
        actor: &str,
        card: &str,
        target: &str,
        expected: i64,
    ) -> Result<Value> {
        self.add_dependency_scoped(actor, card, target, "start", expected)
    }

    pub fn add_dependency_scoped(
        &self,
        actor: &str,
        card: &str,
        target: &str,
        scope: &str,
        expected: i64,
    ) -> Result<Value> {
        if !matches!(scope, "start" | "finish") {
            return Err(Error::new(
                "validation_error",
                "dependency scope must be start or finish",
            ));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, Some(expected))?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        let resolved_target = resolve_dependency_target(&tx, target)?;
        let (target_type, target_id, target_board, target_key) =
            if let DependencyTarget::Question { id: q } = resolved_target {
                let q = q.to_string();
                let (board, key): (String, String) = tx.query_row(
                    "SELECT board_id,key FROM questions WHERE id=?1",
                    [&q],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                ("question", q, board, key)
            } else {
                let DependencyTarget::Card { id: c } = resolved_target else {
                    unreachable!()
                };
                let c = c.to_string();
                let (board, key): (String, String) =
                    tx.query_row("SELECT board_id,key FROM cards WHERE id=?1", [&c], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?;
                ("card", c, board, key)
            };
        if target_board != board_id {
            return Err(Error::new(
                "cross_board_dependency",
                "dependency target belongs to another board",
            ));
        }
        if target_type == "card" {
            let cancelled: bool = tx.query_row(
                "SELECT cancelled_at IS NOT NULL FROM cards WHERE id=?1",
                [&target_id],
                |r| r.get(0),
            )?;
            if cancelled {
                return Err(Error::new(
                    "cancelled_prerequisite",
                    "cannot add a dependency on cancelled work",
                )
                .hint("Link a replacement card or remove the obsolete requirement"));
            }
        }
        phases::ensure_dependency_order(&tx, &id, &target_id, target_type == "question")?;
        if target_id == id {
            return Err(Error::new(
                "self_dependency",
                "card cannot depend on itself",
            ));
        }
        if target_type == "card" && reaches_card(&tx, &target_id, &id)? {
            return Err(Error::new(
                "dependency_cycle",
                "dependency would create a cycle",
            ));
        }
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM dependencies WHERE from_card_id=?1 AND (to_card_id=?2 OR to_question_id=?2))", params![id,target_id], |r| r.get(0))?;
        if exists {
            return Err(Error::new(
                "duplicate_dependency",
                "dependency already exists",
            ));
        }
        let old_states = capture_states(&tx, &[id.clone()])?;
        let dep_id = uuid();
        tx.execute("INSERT INTO dependencies(id,board_id,from_card_id,to_card_id,to_question_id,created_at,scope) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![dep_id,board_id,id,if target_type=="card" {Some(&target_id)} else {None},if target_type=="question" {Some(&target_id)} else {None},now(),scope])?;
        tx.execute(
            "UPDATE cards SET version=version+1,updated_at=?2 WHERE id=?1",
            params![id, now()],
        )?;
        let after = card_row(&tx, &id)?;
        let mut effects = changed_state_effects(&tx, &[id.clone()], &old_states, &target_key)?;
        effects.push(json!({"type":"dependency_linked","card_id":id,"dependency_id":dep_id,"target_type":target_type,"target_id":target_id,"target_key":target_key,"scope":scope}));
        record(
            &tx,
            actor,
            "dependency_added",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &effects,
            &target_key,
        )?;
        self.commit(tx)?;
        Ok(json!({"dependency_id":dep_id,"card":after}))
    }

    pub fn remove_dependency(
        &self,
        actor: &str,
        card: &str,
        target: &str,
        expected: i64,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &id, actor, Some(expected))?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        let target_id = match resolve_dependency_target(&tx, target)? {
            DependencyTarget::Card { id } => id.to_string(),
            DependencyTarget::Question { id } => id.to_string(),
        };
        let old_states = capture_states(&tx, &[id.clone()])?;
        let dependency_id: Option<String> = tx.query_row("SELECT id FROM dependencies WHERE from_card_id=?1 AND (to_card_id=?2 OR to_question_id=?2)", params![id,target_id], |r| r.get(0)).optional()?;
        let deleted = tx.execute("DELETE FROM dependencies WHERE from_card_id=?1 AND (to_card_id=?2 OR to_question_id=?2)", params![id,target_id])?;
        if deleted == 0 {
            return Err(Error::new("not_found", "dependency not found"));
        }
        tx.execute(
            "UPDATE cards SET version=version+1,updated_at=?2 WHERE id=?1",
            params![id, now()],
        )?;
        let after = card_row(&tx, &id)?;
        let mut effects = changed_state_effects(&tx, &[id.clone()], &old_states, target)?;
        effects.push(json!({"type":"dependency_unlinked","card_id":id,"dependency_id":dependency_id,"target_id":target_id,"target_key":target}));
        record(
            &tx,
            actor,
            "dependency_removed",
            board_id,
            "card",
            &id,
            Some(&before),
            Some(&after),
            &effects,
            target,
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn set_dependency_scope(
        &self,
        actor: &str,
        dependency: &str,
        scope: &str,
        expected: i64,
    ) -> Result<Value> {
        if !matches!(scope, "start" | "finish") {
            return Err(Error::new(
                "validation_error",
                "dependency scope must be start or finish",
            ));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (card_id, old_scope): (String, String) = tx
            .query_row(
                "SELECT from_card_id,scope FROM dependencies WHERE id=?1",
                [dependency],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::new("not_found", "dependency not found"))?;
        let before = guard_card(&tx, &card_id, actor, Some(expected))?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        if old_scope == scope {
            return Err(Error::new(
                "validation_error",
                "dependency already has that scope",
            ));
        }
        let old_states = capture_states(&tx, &[card_id.clone()])?;
        tx.execute(
            "UPDATE dependencies SET scope=?2 WHERE id=?1",
            params![dependency, scope],
        )?;
        tx.execute(
            "UPDATE cards SET version=version+1,updated_at=?2 WHERE id=?1",
            params![card_id, now()],
        )?;
        let after = card_row(&tx, &card_id)?;
        let mut effects = changed_state_effects(&tx, &[card_id.clone()], &old_states, dependency)?;
        effects.push(json!({"type":"dependency_scope_changed","dependency_id":dependency,"card_id":card_id,"before":old_scope,"after":scope}));
        record(
            &tx,
            actor,
            "dependency_scope_updated",
            board_id,
            "card",
            &card_id,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn raise_question(
        &self,
        actor: &str,
        board: &str,
        kind: &str,
        title: &str,
        body: &str,
        options: &[String],
        recommended_option: &str,
        recommendation_reason: &str,
        blocks: &[String],
    ) -> Result<Value> {
        self.raise_question_in_phase(
            actor,
            board,
            kind,
            title,
            body,
            options,
            recommended_option,
            recommendation_reason,
            blocks,
            None,
        )
    }

    pub fn raise_question_in_phase(
        &self,
        actor: &str,
        board: &str,
        kind: &str,
        title: &str,
        body: &str,
        options: &[String],
        recommended_option: &str,
        recommendation_reason: &str,
        blocks: &[String],
        phase: Option<&str>,
    ) -> Result<Value> {
        require_nonblank("title", title)?;
        let options = validate_question_options(options)?;
        validate_recommendation(&options, recommended_option, recommendation_reason)?;
        let kind = QuestionKind::try_from(kind)?.as_str();
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let (phase_id, phase_position) = phases::resolve_phase(&tx, &board_id, phase)?;
        phases::ensure_phase_open(&tx, &board_id, &phase_id)?;
        let mut card_ids = Vec::new();
        let mut seen = HashSet::new();
        for card in blocks {
            let id = resolve_card(&tx, card)?;
            let row = card_row(&tx, &id)?;
            if row["board_id"] != board_id {
                return Err(Error::new(
                    "cross_board_dependency",
                    "blocked card belongs to another board",
                ));
            }
            phases::ensure_phase_open(&tx, &board_id, row["phase_id"].as_str().unwrap())?;
            if row["cancelled_at"].is_string() {
                return Err(Error::new(
                    "card_cancelled",
                    "a question cannot block cancelled work",
                ));
            }
            let card_phase: i64 = tx.query_row(
                "SELECT position FROM phases WHERE id=?1",
                [row["phase_id"].as_str().unwrap()],
                |r| r.get(0),
            )?;
            if phase_position > card_phase {
                return Err(Error::new(
                    "future_dependency",
                    "a question cannot block a card in an earlier phase",
                ));
            }
            if let Some(owner) = row["owner_actor"].as_str() {
                if owner != actor {
                    return Err(owner_error(owner));
                }
            }
            if !seen.insert(id.clone()) {
                return Err(Error::new(
                    "duplicate_dependency",
                    "blocks list contains a duplicate card",
                ));
            }
            card_ids.push(id);
        }
        let old_states = capture_states(&tx, &card_ids)?;
        let id = uuid();
        let key = next_key(&tx, "question", "Q")?;
        tx.execute("INSERT INTO questions(id,key,board_id,kind,title,body,options_json,recommended_option,recommendation_reason,created_at,phase_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![id,key,board_id,kind,title.trim(),body,serde_json::to_string(&options)?,recommended_option.trim(),recommendation_reason.trim(),now(),phase_id])?;
        let mut link_effects = Vec::new();
        for card_id in &card_ids {
            let dependency_id = uuid();
            tx.execute("INSERT INTO dependencies(id,board_id,from_card_id,to_question_id,created_at) VALUES (?1,?2,?3,?4,?5)", params![dependency_id,board_id,card_id,id,now()])?;
            link_effects.push(json!({"type":"question_linked","card_id":card_id,"question_id":id,"question_key":key,"dependency_id":dependency_id}));
            tx.execute(
                "UPDATE cards SET version=version+1,updated_at=?2 WHERE id=?1",
                params![card_id, now()],
            )?;
        }
        let after = question_row(&tx, &id)?;
        let mut effects = changed_state_effects(&tx, &card_ids, &old_states, &key)?;
        effects.extend(link_effects);
        record(
            &tx,
            actor,
            "question_raised",
            &board_id,
            "question",
            &id,
            None,
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn update_question_options(
        &self,
        actor: &str,
        question: &str,
        expected: i64,
        options: &[String],
        recommended_option: &str,
        recommendation_reason: &str,
    ) -> Result<Value> {
        let options = validate_question_options(options)?;
        validate_recommendation(&options, recommended_option, recommendation_reason)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_question(&tx, question)?;
        let before = question_row(&tx, &id)?;
        check_version(&before, expected)?;
        if serde_json::from_value::<Question>(before.clone())?.status != QuestionStatus::Open {
            return Err(Error::new(
                "validation_error",
                "only open questions can change options",
            ));
        }
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        phases::ensure_phase_open(&tx, board_id, before["phase_id"].as_str().unwrap())?;
        tx.execute(
            "UPDATE questions SET options_json=?2,recommended_option=?3,recommendation_reason=?4,version=version+1 WHERE id=?1",
            params![id, serde_json::to_string(&options)?,recommended_option.trim(),recommendation_reason.trim()],
        )?;
        let after = question_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "question_options_updated",
            board_id,
            "question",
            &id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn resolve_question(
        &self,
        actor: &str,
        question: &str,
        answer: &str,
        note: Option<&str>,
        expected: i64,
        dropped: bool,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_question(&tx, question)?;
        let before = question_row(&tx, &id)?;
        check_version(&before, expected)?;
        let question: Question = serde_json::from_value(before.clone())?;
        if question.status != QuestionStatus::Open {
            return Err(Error::new("validation_error", "question is no longer open"));
        }
        let outcome = if dropped {
            QuestionOutcome::drop(answer)?
        } else {
            QuestionOutcome::answer(&question.options, answer, note)?
        };
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        let affected = cards_blocked_by_question(&tx, &id)?;
        let old_states = capture_states(&tx, &affected)?;
        let (status, answer, answer_note) = match &outcome {
            QuestionOutcome::Answered {
                selection,
                explanation,
            } => (
                QuestionStatus::Answered,
                selection.as_str(),
                explanation.as_str(),
            ),
            QuestionOutcome::Dropped { reason } => (QuestionStatus::Dropped, reason.as_str(), ""),
        };
        tx.execute(
            "UPDATE questions SET status=?2,answer=?3,answer_note=?4,answered_at=?5,version=version+1 WHERE id=?1",
            params![id, status.as_str(), answer, answer_note, now()],
        )?;
        let after = question_row(&tx, &id)?;
        let mut effects =
            changed_state_effects(&tx, &affected, &old_states, before["key"].as_str().unwrap())?;
        for card_id in &affected {
            effects.push(json!({"type":"question_resolved_for_card","card_id":card_id,"question_id":id,"question_key":before["key"]}));
        }
        record(
            &tx,
            actor,
            if dropped {
                "question_dropped"
            } else {
                "question_answered"
            },
            board_id,
            "question",
            &id,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn list_questions(&self, board: Option<&str>, status: Option<&str>) -> Result<Value> {
        let db = self.open()?;
        let board_id = board.map(|b| resolve_board(&db, b)).transpose()?;
        let mut stmt = db.prepare("SELECT id FROM questions WHERE (?1 IS NULL OR board_id=?1) AND (?2 IS NULL OR status=?2) ORDER BY created_at,key")?;
        let ids = stmt
            .query_map(params![board_id, status], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut result = Vec::new();
        for id in ids {
            let mut row = question_row(&db, &id)?;
            let mut blocked_cards = Vec::new();
            let mut cards = db.prepare("SELECT c.key FROM dependencies d JOIN cards c ON c.id=d.from_card_id WHERE d.to_question_id=?1 ORDER BY c.key")?;
            for card in cards.query_map([&id], |r| r.get::<_, String>(0))? {
                blocked_cards.push(card?);
            }
            row["blocks"] = json!(blocked_cards);
            if let Some(stakeholder_id) = row["stakeholder_id"].as_str() {
                row["stakeholder"] = db.query_row("SELECT name,role,contact FROM project_stakeholders WHERE id=?1", [stakeholder_id], |r| Ok(json!({"name":r.get::<_,String>(0)?,"role":r.get::<_,String>(1)?,"contact":r.get::<_,String>(2)?})))?;
            }
            result.push(row);
        }
        Ok(json!(result))
    }

    pub fn list_questions_filtered(
        &self,
        board: Option<&str>,
        status: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Value> {
        if let Some(kind) = kind {
            if !matches!(kind, "engineering" | "business") {
                return Err(Error::new(
                    "validation_error",
                    "kind must be engineering or business",
                ));
            }
        }
        let mut result = self.list_questions(board, status)?;
        if let (Some(kind), Some(items)) = (kind, result.as_array_mut()) {
            items.retain(|q| q["kind"].as_str() == Some(kind));
        }
        Ok(result)
    }

    pub fn next_ready_task(&self, board: &str, agent_type: Option<&str>) -> Result<Value> {
        let db = self.open()?;
        let board_id = resolve_board(&db, board)?;
        Self::next_ready_task_on(&db, &board_id, agent_type)
    }

    pub(crate) fn next_ready_task_on(
        db: &Connection,
        board_id: &str,
        agent_type: Option<&str>,
    ) -> Result<Value> {
        let board_id = board_id.to_owned();
        let (current_id, current_position) = phases::current_phase(&db, &board_id)?;
        let mut paused = db.prepare("SELECT c.id FROM cards c JOIN lanes l ON l.id=c.lane_id JOIN phases p ON p.id=c.phase_id JOIN task_sessions s ON s.card_id=c.id AND s.detached_at IS NULL WHERE c.board_id=?1 AND p.position BETWEEN ?2 AND ?2+1 AND l.kind='active' AND c.cancelled_at IS NULL AND c.owner_actor=s.owner_actor AND s.paused_at IS NOT NULL AND (?3 IS NULL OR c.agent_type=?3) ORDER BY p.position,c.position,c.key")?;
        let paused_ids = paused.query_map(params![board_id,current_position,agent_type], |row| row.get::<_,String>(0))?;
        let mut best_resume: Option<(i64, Value)> = None;
        for id in paused_ids {
            let row = card_row(&db, &id?)?;
            let session = sessions::active_session(&db, row["id"].as_str().unwrap())?.unwrap();
            if !sessions::resume_eligible(&db, &row, &session)? { continue; }
            let phase_position: i64 = db.query_row("SELECT position FROM phases WHERE id=?1", [row["phase_id"].as_str().unwrap()], |r| r.get(0))?;
            if best_resume.as_ref().is_none_or(|(position, current)| phase_position < *position || (phase_position == *position && row["blocking_impact"].as_i64() > current["blocking_impact"].as_i64())) {
                best_resume = Some((phase_position, row));
            }
        }
        if let Some((_, mut card)) = best_resume {
            card["dispatch_action"] = json!("resume");
            card["session"] = json!(sessions::active_session(&db, card["id"].as_str().unwrap())?);
            return Ok(card);
        }
        let selected_phase = if phases::current_queue_has_work(&db, &board_id)? {
            Some(current_id)
        } else {
            db.query_row(
                "SELECT id FROM phases WHERE board_id=?1 AND position=?2",
                params![board_id, current_position + 1],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        };
        let Some(selected_phase) = selected_phase else {
            return Ok(Value::Null);
        };
        let mut stmt = db.prepare("SELECT c.id FROM cards c JOIN lanes l ON l.id=c.lane_id WHERE c.phase_id=?1 AND l.kind='backlog' AND l.archived=0 AND c.owner_actor IS NULL AND c.cancelled_at IS NULL AND (?2 IS NULL OR c.agent_type=?2) ORDER BY c.position,c.key")?;
        let ids = stmt.query_map(params![selected_phase, agent_type], |r| {
            r.get::<_, String>(0)
        })?;
        let mut best: Option<Value> = None;
        for id in ids {
            let row = card_row(&db, &id?)?;
            if row["ready"] == true && row["owner_actor"].is_null() {
                if best.as_ref().is_none_or(|current| {
                    row["blocking_impact"].as_i64() > current["blocking_impact"].as_i64()
                }) {
                    best = Some(row);
                }
            }
        }
        if let Some(mut card) = best {
            card["dispatch_action"] = json!("claim");
            Ok(card)
        } else {
            Ok(Value::Null)
        }
    }

    pub fn poll_events(&self, since: i64, limit: i64) -> Result<Value> {
        if since < 0 || !(1..=1000).contains(&limit) {
            return Err(Error::new(
                "validation_error",
                "since must be nonnegative and limit must be 1–1000",
            ));
        }
        let db = self.open()?;
        let mut stmt = db.prepare("SELECT id,ts,actor,type,board_id,entity_type,entity_id,before_json,after_json,effects_json,note FROM events WHERE id>?1 ORDER BY id LIMIT ?2")?;
        let events = stmt
            .query_map(params![since, limit + 1], event_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let has_more = events.len() > limit as usize;
        let events: Vec<_> = events.into_iter().take(limit as usize).collect();
        let cursor = events
            .last()
            .and_then(|e| e["id"].as_i64())
            .unwrap_or(since);
        Ok(json!({"events":events,"cursor":cursor,"has_more":has_more}))
    }

    pub fn latest_event_id(&self) -> Result<i64> {
        let db = self.open()?;
        Ok(db.query_row("SELECT COALESCE(MAX(id),0) FROM events", [], |r| r.get(0))?)
    }

    fn history_for_card_with_db(
        &self,
        db: &Connection,
        card_id: &str,
        since: i64,
        limit: i64,
    ) -> Result<Value> {
        let mut stmt = db.prepare("SELECT id,ts,actor,type,board_id,entity_type,entity_id,before_json,after_json,effects_json,note FROM events WHERE id>?1 AND board_id=(SELECT board_id FROM cards WHERE id=?2) ORDER BY id DESC")?;
        let rows = stmt.query_map(params![since, card_id], event_from_row)?;
        let mut events = Vec::new();
        for row in rows {
            let event = row?;
            let touches = event["entity_id"] == card_id
                || event["effects"]
                    .as_array()
                    .is_some_and(|effects| effects.iter().any(|e| e["card_id"] == card_id));
            if touches {
                events.push(event);
                if events.len() >= limit as usize {
                    break;
                }
            }
        }
        Ok(json!(events))
    }
}

/// Newest schema this binary can open; keep in step with the last migration below.
const SCHEMA_VERSION: i64 = 15;

fn migrate(db: &mut Connection) -> Result<()> {
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version > SCHEMA_VERSION {
        return Err(Error::new(
            "schema_too_new",
            format!(
                "database schema is version {version} but this Workyard binary only supports up to version {SCHEMA_VERSION}; the binary is older than the database"
            ),
        )
        .hint("Rebuild or reinstall the workyard CLI from a current checkout, or point WORKYARD_DB at a database this binary created")
        .details(json!({"binary_schema_version": SCHEMA_VERSION, "database_schema_version": version, "older": "binary"})));
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version == 0 {
        tx.execute_batch(include_str!("../migrations/001.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (1,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 1")?;
        version = 1;
    }
    if version == 1 {
        tx.execute_batch(include_str!("../migrations/002.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (2,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 2")?;
        version = 2;
    }
    if version == 2 {
        tx.execute_batch(include_str!("../migrations/003.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (3,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 3")?;
        version = 3;
    }
    if version == 3 {
        tx.execute_batch(include_str!("../migrations/004.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (4,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 4")?;
        version = 4;
    }
    if version == 4 {
        tx.execute_batch(include_str!("../migrations/005.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (5,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 5")?;
        version = 5;
    }
    if version == 5 {
        tx.execute_batch(include_str!("../migrations/006.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (6,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 6")?;
        version = 6;
    }
    if version == 6 {
        tx.execute_batch(include_str!("../migrations/007.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (7,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 7")?;
        version = 7;
    }
    if version == 7 {
        tx.execute_batch(include_str!("../migrations/008.sql"))?;
        let mut stmt = tx.prepare("SELECT id FROM boards")?;
        let boards = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        for board_id in boards {
            let phase_id = uuid();
            tx.execute("INSERT INTO phases(id,board_id,name,position,created_at) VALUES (?1,?2,'Current work',0,?3)",params![phase_id,board_id,now()])?;
            tx.execute(
                "UPDATE boards SET current_phase_id=?2 WHERE id=?1",
                params![board_id, phase_id],
            )?;
            tx.execute(
                "UPDATE cards SET phase_id=?2 WHERE board_id=?1",
                params![board_id, phase_id],
            )?;
            tx.execute(
                "UPDATE questions SET phase_id=?2 WHERE board_id=?1",
                params![board_id, phase_id],
            )?;
        }
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (8,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 8")?;
        version = 8;
    }
    if version == 8 {
        tx.execute_batch(include_str!("../migrations/009.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (9,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 9")?;
        version = 9;
    }
    if version == 9 {
        tx.execute_batch(include_str!("../migrations/010.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (10,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 10")?;
        version = 10;
    }
    if version == 10 {
        tx.execute_batch(include_str!("../migrations/011.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (11,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 11")?;
        version = 11;
    }
    if version == 11 {
        tx.execute_batch(include_str!("../migrations/012.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (12,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 12")?;
        version = 12;
    }
    if version == 12 {
        tx.execute_batch(include_str!("../migrations/013.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (13,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 13")?;
        version = 13;
    }
    if version == 13 {
        tx.execute_batch(include_str!("../migrations/014.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (14,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 14")?;
        version = 14;
    }
    if version == 14 {
        tx.execute_batch(include_str!("../migrations/015.sql"))?;
        tx.execute(
            "INSERT INTO schema_version(version,applied_at) VALUES (15,?1)",
            [now()],
        )?;
        tx.execute_batch("PRAGMA user_version = 15")?;
    }
    tx.commit()?;
    Ok(())
}

fn uuid() -> String {
    Uuid::new_v4().to_string()
}
fn validate_question_options(options: &[String]) -> Result<QuestionOptions> {
    QuestionOptions::new(options)
}
fn validate_recommendation(
    options: &QuestionOptions,
    recommended: &str,
    reason: &str,
) -> Result<()> {
    if !options.iter().any(|option| option == recommended.trim()) {
        return Err(Error::new(
            "validation_error",
            "recommended option must be one of the question's options",
        ));
    }
    require_nonblank("recommendation reason", reason)?;
    if reason.trim().chars().count() > 280 {
        return Err(Error::new(
            "validation_error",
            "recommendation reason must be at most 280 characters",
        ));
    }
    Ok(())
}
fn now() -> String {
    Utc::now().to_rfc3339()
}
fn require_nonblank(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::new(
            "validation_error",
            format!("{field} cannot be blank"),
        ))
    } else {
        Ok(())
    }
}
fn next_key(tx: &Transaction<'_>, kind: &str, prefix: &str) -> Result<String> {
    tx.execute(
        "INSERT OR IGNORE INTO id_counters(kind,next_value) VALUES (?1,1)",
        [kind],
    )?;
    let n: i64 = tx.query_row(
        "SELECT next_value FROM id_counters WHERE kind=?1",
        [kind],
        |r| r.get(0),
    )?;
    tx.execute(
        "UPDATE id_counters SET next_value=next_value+1 WHERE kind=?1",
        [kind],
    )?;
    Ok(format!("{prefix}-{n}"))
}
fn resolve_board(db: &Connection, value: &str) -> Result<String> {
    db.query_row(
        "SELECT id FROM boards WHERE id=?1 OR key=?1",
        [value],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::new("not_found", format!("board {value} not found")))
}
fn resolve_card(db: &Connection, value: &str) -> Result<String> {
    db.query_row("SELECT id FROM cards WHERE id=?1 OR key=?1", [value], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or_else(|| Error::new("not_found", format!("card {value} not found")))
}
fn resolve_question(db: &Connection, value: &str) -> Result<String> {
    db.query_row(
        "SELECT id FROM questions WHERE id=?1 OR key=?1",
        [value],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::new("not_found", format!("question {value} not found")))
}
fn is_question_ref(db: &Connection, value: &str) -> Result<bool> {
    if value.starts_with("Q-") {
        return Ok(true);
    }
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM questions WHERE id=?1)",
        [value],
        |r| r.get(0),
    )?)
}
fn resolve_dependency_target(db: &Connection, value: &str) -> Result<DependencyTarget> {
    if is_question_ref(db, value)? {
        let id = resolve_question(db, value)?;
        Ok(DependencyTarget::Question {
            id: QuestionId::try_from(id.as_str())?,
        })
    } else {
        let id = resolve_card(db, value)?;
        Ok(DependencyTarget::Card {
            id: CardId::try_from(id.as_str())?,
        })
    }
}
fn resolve_lane(db: &Connection, board_id: &str, value: &str) -> Result<String> {
    db.query_row(
        "SELECT id FROM lanes WHERE board_id=?1 AND archived=0 AND (id=?2 OR name=?2)",
        params![board_id, value],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::new("not_found", format!("active lane {value} not found")))
}
fn ensure_work_lane(db: &Connection, lane_id: &str) -> Result<()> {
    let kind: String = db.query_row("SELECT kind FROM lanes WHERE id=?1", [lane_id], |r| {
        r.get(0)
    })?;
    if LaneKind::try_from(kind.as_str())?.is_work() {
        Ok(())
    } else {
        Err(Error::new(
            "fixed_lane",
            "cards can only be created in Backlog, In Progress, or Done",
        ))
    }
}
fn board_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row("SELECT id,key,name,description,created_at,archived,version,project_id,current_phase_id,review_required FROM boards WHERE id=?1", [id], |r| Ok(json!({"review_required":r.get::<_,i64>(9)?!=0,"id":r.get::<_,String>(0)?,"key":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"description":r.get::<_,String>(3)?,"created_at":r.get::<_,String>(4)?,"archived":r.get::<_,i64>(5)?!=0,"version":r.get::<_,i64>(6)?,"project_id":r.get::<_,Option<String>>(7)?,"current_phase_id":r.get::<_,Option<String>>(8)?})))?)
}
fn card_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(serde_json::to_value(card_snapshot(db, id)?)?)
}
fn card_snapshot(db: &Connection, id: &str) -> Result<Card> {
    let (id, key, board_id, lane_id, position, title, description, priority, progress, status_note, owner_actor, created_at, updated_at, version, agent_type) = db.query_row(
        "SELECT id,key,board_id,lane_id,position,title,description,priority,progress,status_note,owner_actor,created_at,updated_at,version,agent_type FROM cards WHERE id=?1",
        [id],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,i64>(4)?, r.get::<_,String>(5)?, r.get::<_,String>(6)?, r.get::<_,String>(7)?, r.get::<_,i64>(8)?, r.get::<_,String>(9)?, r.get::<_,Option<String>>(10)?, r.get::<_,String>(11)?, r.get::<_,String>(12)?, r.get::<_,i64>(13)?, r.get::<_,String>(14)?)),
    )?;
    let finish_blockers = blockers(db, &id, "finish")?;
    let blockers = blockers(db, &id, "start")?;
    let mut review_gates = Vec::new();
    let mut stmt = db.prepare("SELECT id,working_branch,commit_oid,approved_commit_oid,reviewer FROM card_changes WHERE card_id=?1 AND review_required=1 ORDER BY created_at,id")?;
    for row in stmt.query_map([&id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })? {
        let (change_id, working_branch, commit_oid, approved_commit_oid, reviewer) = row?;
        let approved =
            !commit_oid.is_empty() && approved_commit_oid.as_deref() == Some(commit_oid.as_str());
        review_gates.push(ReviewGate {
            change_id,
            working_branch,
            commit_oid,
            approved,
            reviewer,
        });
    }
    let blocked = !blockers.is_empty();
    let can_finish =
        !blocked && finish_blockers.is_empty() && review_gates.iter().all(|gate| gate.approved);
    let lane_kind: String =
        db.query_row("SELECT kind FROM lanes WHERE id=?1", [&lane_id], |r| {
            r.get(0)
        })?;
    let lane_kind = LaneKind::try_from(lane_kind.as_str())?;
    let open_question_count: i64 = db.query_row("SELECT COUNT(*) FROM dependencies d JOIN questions q ON q.id=d.to_question_id WHERE d.from_card_id=?1 AND q.status='open'", [&id], |r| r.get(0))?;
    let (phase_id, cancelled_at, cancellation_reason, cancelled_by, workspace): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) = db.query_row(
        "SELECT phase_id,cancelled_at,cancellation_reason,cancelled_by,workspace FROM cards WHERE id=?1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    Ok(Card {
        id: CardId::try_from(id.as_str())?,
        key,
        board_id: BoardId::try_from(board_id.as_str())?,
        lane_id: LaneId::try_from(lane_id.as_str())?,
        phase_id: PhaseId::try_from(phase_id.as_str())?,
        cancelled_at: cancelled_at.clone(),
        cancellation_reason,
        cancelled_by,
        position,
        title,
        description,
        priority: Priority::try_from(priority.as_str())?,
        progress: Progress::try_from(progress)?,
        status_note,
        owner_actor,
        created_at,
        updated_at,
        version: Version::try_from(version)?,
        agent_type,
        workspace,
        blocked,
        blockers,
        finish_blockers,
        review_gates,
        can_finish,
        ready: !blocked && lane_kind != LaneKind::Done && cancelled_at.is_none(),
        open_question_count,
        blocking_impact: blocking_impact(db, &id)?,
    })
}
/// Minutes of owner silence after which a claim on active work is flagged `stale`.
fn stale_after_minutes() -> i64 {
    std::env::var("WORKYARD_STALE_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|m| *m > 0)
        .unwrap_or(15)
}

/// Add `last_activity_at` and `stale` to a card read. Derived on read and never stored, so
/// event history does not capture a value that depends on the clock. A claim is stale when
/// the card is In Progress, its owner has written nothing for the threshold, and nothing
/// accounts for the silence: a paused session is waiting by design, and a connected
/// orchestrator watches its own workers and pauses the session of any that die.
fn annotate_claim_activity(db: &Connection, card: &mut Value) -> Result<()> {
    card["stale"] = json!(false);
    let (Some(owner), Some(id)) = (
        card["owner_actor"].as_str().map(str::to_owned),
        card["id"].as_str().map(str::to_owned),
    ) else {
        return Ok(());
    };
    let (owner, id) = (owner.as_str(), id.as_str());
    // Enough for a board to offer "open this agent in a terminal" without a per-card read.
    if let Some(session) = sessions::active_session(db, id)? {
        card["session"] = json!({
            "runner": session["runner"],
            "paused": session["paused_at"].is_string(),
            "held_by": session["held_by"],
            "waiting_integration": session["waiting_integration"],
            "activity": session_activity(db, session["id"].as_str().unwrap_or_default())?.0,
            "activity_at": session_activity(db, session["id"].as_str().unwrap_or_default())?.1,
        });
    }
    let lane_kind: String = db.query_row(
        "SELECT kind FROM lanes WHERE id=?1",
        [card["lane_id"].as_str().unwrap_or_default()],
        |r| r.get(0),
    )?;
    if lane_kind != "active" || card["cancelled_at"].is_string() {
        return Ok(());
    }
    let last_event: Option<String> = db.query_row(
        "SELECT MAX(ts) FROM events WHERE entity_id=?1 AND actor=?2",
        params![id, owner],
        |r| r.get(0),
    )?;
    let updated = card["updated_at"].as_str().unwrap_or_default().to_owned();
    let last = last_event.filter(|ts| *ts > updated).unwrap_or(updated);
    card["last_activity_at"] = json!(last);
    if let Some(session) = sessions::active_session(db, id)? {
        if session["paused_at"].is_string()
            || orchestrator::board_orchestrator_connected(db, card["board_id"].as_str().unwrap_or_default())?
        {
            return Ok(());
        }
    }
    if let Ok(last) = chrono::DateTime::parse_from_rfc3339(&last) {
        let silent = Utc::now().signed_duration_since(last);
        card["stale"] = json!(silent > chrono::Duration::minutes(stale_after_minutes()));
    }
    Ok(())
}
fn session_activity(db: &Connection, session_id: &str) -> Result<(String, Option<String>)> {
    Ok(db
        .query_row("SELECT activity,activity_at FROM task_sessions WHERE id=?1", [session_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .unwrap_or_default())
}
fn blocking_impact(db: &Connection, id: &str) -> Result<i64> {
    Ok(db.query_row(
        "WITH RECURSIVE held(id) AS (
            SELECT d.from_card_id FROM dependencies d
            JOIN cards source ON source.id=d.to_card_id
            JOIN lanes source_lane ON source_lane.id=source.lane_id
            WHERE d.to_card_id=?1 AND source_lane.kind!='done'
            UNION
            SELECT d.from_card_id FROM dependencies d JOIN held h ON d.to_card_id=h.id
            JOIN cards source ON source.id=h.id
            JOIN lanes source_lane ON source_lane.id=source.lane_id
            WHERE source_lane.kind!='done'
        ) SELECT COUNT(*) FROM held",
        [id],
        |r| r.get(0),
    )?)
}
fn blockers(db: &Connection, card_id: &str, scope: &str) -> Result<Vec<Blocker>> {
    let mut result = Vec::new();
    let mut stmt = db.prepare("SELECT c.id,c.key,c.title,c.cancelled_at IS NOT NULL FROM dependencies d JOIN cards c ON c.id=d.to_card_id JOIN lanes l ON l.id=c.lane_id WHERE d.from_card_id=?1 AND d.scope=?2 AND l.kind!='done' ORDER BY c.key")?;
    for row in stmt.query_map(params![card_id, scope], |r| {
        Ok(Blocker {
            kind: BlockerKind::Card,
            id: r.get(0)?,
            key: r.get(1)?,
            title: r.get(2)?,
            cancelled: r.get(3)?,
        })
    })? {
        result.push(row?);
    }
    let mut stmt = db.prepare("SELECT q.id,q.key,q.title FROM dependencies d JOIN questions q ON q.id=d.to_question_id WHERE d.from_card_id=?1 AND d.scope=?2 AND q.status='open' ORDER BY q.key")?;
    for row in stmt.query_map(params![card_id, scope], |r| {
        Ok(Blocker {
            kind: BlockerKind::Question,
            id: r.get(0)?,
            key: r.get(1)?,
            title: r.get(2)?,
            cancelled: false,
        })
    })? {
        result.push(row?);
    }
    Ok(result)
}
fn record(
    db: &Connection,
    actor: &str,
    kind: &str,
    board_id: &str,
    entity_type: &str,
    entity_id: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    effects: &[Value],
    note: &str,
) -> Result<()> {
    require_nonblank("actor", actor)?;
    if let (Some(epoch), false) = (orchestrator::actor_epoch(actor), board_id.is_empty()) {
        orchestrator::check_epoch(db, board_id, epoch)?;
    }
    db.execute("INSERT INTO events(ts,actor,type,board_id,entity_type,entity_id,before_json,after_json,effects_json,note) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![now(),actor,kind,if board_id.is_empty() {None} else {Some(board_id)},entity_type,entity_id,before.map(serde_json::to_string).transpose()?,after.map(serde_json::to_string).transpose()?,serde_json::to_string(effects)?,note])?;
    Ok(())
}
fn event_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let before: Option<String> = r.get(7)?;
    let after: Option<String> = r.get(8)?;
    let effects: String = r.get(9)?;
    Ok(
        json!({"id":r.get::<_,i64>(0)?,"ts":r.get::<_,String>(1)?,"actor":r.get::<_,String>(2)?,"type":r.get::<_,String>(3)?,"board_id":r.get::<_,Option<String>>(4)?,"entity_type":r.get::<_,String>(5)?,"entity_id":r.get::<_,String>(6)?,"before":before.and_then(|v| serde_json::from_str::<Value>(&v).ok()),"after":after.and_then(|v| serde_json::from_str::<Value>(&v).ok()),"effects":serde_json::from_str::<Value>(&effects).unwrap_or(json!([])),"note":r.get::<_,String>(10)?}),
    )
}

fn validate_priority(value: &str) -> Result<()> {
    types::Priority::try_from(value).map(|_| ())
}
fn resolve_lane_global(db: &Connection, value: &str) -> Result<String> {
    db.query_row("SELECT id FROM lanes WHERE id=?1", [value], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::new("not_found", format!("lane {value} not found; use its UUID")))
}
fn lane_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row("SELECT id,board_id,name,kind,position,archived,version FROM lanes WHERE id=?1",[id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"board_id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"kind":r.get::<_,String>(3)?,"position":r.get::<_,i64>(4)?,"archived":r.get::<_,i64>(5)?!=0,"version":r.get::<_,i64>(6)?})))?)
}
fn check_board_active(db: &Connection, id: &str) -> Result<()> {
    let archived: bool = db.query_row("SELECT archived FROM boards WHERE id=?1", [id], |r| {
        r.get(0)
    })?;
    if archived {
        Err(Error::new("board_archived", "board is archived"))
    } else {
        Ok(())
    }
}
fn check_version(row: &Value, expected: i64) -> Result<()> {
    let expected = Version::try_from(expected)?;
    if row["version"].as_i64() == Some(i64::from(expected)) {
        Ok(())
    } else {
        Err(
            Error::new("version_conflict", "entity changed since it was read")
                .hint("Read the entity again and retry with its current version")
                .details(json!({"current":row})),
        )
    }
}
fn owner_error(owner: &str) -> Error {
    Error::new("ownership_conflict", format!("card is owned by {owner}"))
        .hint("Ask the owner to edit or transfer the card, or use force-release for recovery")
}
/// An owner may omit the version: the claim already serializes their writes.
fn version_required(row: &Value, actor: &str) -> Error {
    match row["owner_actor"].as_str() {
        Some(owner) if owner != actor => owner_error(owner),
        _ => Error::new(
            "version_required",
            "expected version may be omitted only by the card's owner",
        )
        .hint("Pass --expected-version, or claim the card first")
        .details(json!({"current":row})),
    }
}
fn guard_card(db: &Connection, id: &str, actor: &str, expected: Option<i64>) -> Result<Value> {
    let row = card_row(db, id)?;
    match expected {
        Some(expected) => check_version(&row, expected)?,
        None if row["owner_actor"].as_str() == Some(actor) => {}
        None => return Err(version_required(&row, actor)),
    }
    phases::ensure_phase_open(
        db,
        row["board_id"].as_str().unwrap(),
        row["phase_id"].as_str().unwrap(),
    )?;
    if row["cancelled_at"].is_string() {
        return Err(Error::new(
            "card_cancelled",
            "cancelled cards cannot be edited",
        ));
    }
    if let Some(owner) = row["owner_actor"].as_str() {
        if owner != actor {
            return Err(owner_error(owner));
        }
    }
    Ok(row)
}
fn question_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(serde_json::to_value(question_snapshot(db, id)?)?)
}
fn question_snapshot(db: &Connection, id: &str) -> Result<Question> {
    let (id, key, board_id, kind, title, body, status, answer, owner, created_at, answered_at, version, options_json, answer_note, recommended_option, recommendation_reason, stakeholder_id) = db.query_row(
        "SELECT id,key,board_id,kind,title,body,status,answer,owner,created_at,answered_at,version,options_json,answer_note,recommended_option,recommendation_reason,stakeholder_id FROM questions WHERE id=?1",
        [id],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,String>(5)?, r.get::<_,String>(6)?, r.get::<_,String>(7)?, r.get::<_,String>(8)?, r.get::<_,String>(9)?, r.get::<_,Option<String>>(10)?, r.get::<_,i64>(11)?, r.get::<_,String>(12)?, r.get::<_,String>(13)?, r.get::<_,String>(14)?, r.get::<_,String>(15)?, r.get::<_,Option<String>>(16)?)),
    )?;
    let phase_id: String =
        db.query_row("SELECT phase_id FROM questions WHERE id=?1", [&id], |r| {
            r.get(0)
        })?;
    Ok(Question {
        id: QuestionId::try_from(id.as_str())?,
        key,
        board_id: BoardId::try_from(board_id.as_str())?,
        phase_id: PhaseId::try_from(phase_id.as_str())?,
        stakeholder_id,
        kind: QuestionKind::try_from(kind.as_str())?,
        title,
        body,
        status: QuestionStatus::try_from(status.as_str())?,
        answer,
        owner,
        created_at,
        answered_at,
        version: Version::try_from(version)?,
        options: serde_json::from_str(&options_json)?,
        answer_note,
        recommended_option,
        recommendation_reason,
    })
}
fn ordered_cards(db: &Connection, lane_id: &str) -> Result<Vec<String>> {
    let mut stmt = db.prepare("SELECT id FROM cards WHERE lane_id=?1 ORDER BY position,id")?;
    Ok(stmt
        .query_map([lane_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
fn renumber(db: &Connection, ids: &[String], lane_id: &str) -> Result<()> {
    for (index, id) in ids.iter().enumerate() {
        db.execute(
            "UPDATE cards SET position=?2 WHERE id=?1 AND lane_id=?3 AND position!=?2",
            params![id, index as i64, lane_id],
        )?;
    }
    Ok(())
}
fn cards_blocked_by_question(db: &Connection, question_id: &str) -> Result<Vec<String>> {
    let mut stmt = db.prepare(
        "SELECT from_card_id FROM dependencies WHERE to_question_id=?1 ORDER BY from_card_id",
    )?;
    Ok(stmt
        .query_map([question_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
fn dependents_of(db: &Connection, card_id: &str) -> Result<Vec<String>> {
    let mut stmt = db.prepare(
        "SELECT from_card_id FROM dependencies WHERE to_card_id=?1 ORDER BY from_card_id",
    )?;
    Ok(stmt
        .query_map([card_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
fn reaches_card(db: &Connection, start: &str, sought: &str) -> Result<bool> {
    let mut visited = HashSet::new();
    let mut stack = vec![start.to_owned()];
    while let Some(id) = stack.pop() {
        if id == sought {
            return Ok(true);
        }
        if !visited.insert(id.clone()) {
            continue;
        }
        let mut stmt = db.prepare(
            "SELECT to_card_id FROM dependencies WHERE from_card_id=?1 AND to_card_id IS NOT NULL",
        )?;
        for next in stmt.query_map([&id], |r| r.get::<_, String>(0))? {
            stack.push(next?);
        }
    }
    Ok(false)
}
fn capture_states(db: &Connection, ids: &[String]) -> Result<HashMap<String, Value>> {
    let mut states = HashMap::new();
    for id in ids {
        states.insert(id.clone(), card_row(db, id)?);
    }
    Ok(states)
}
fn changed_state_effects(
    db: &Connection,
    ids: &[String],
    before: &HashMap<String, Value>,
    cause_key: &str,
) -> Result<Vec<Value>> {
    let mut effects = Vec::new();
    for id in ids {
        let Some(old) = before.get(id) else {
            continue;
        };
        let new = card_row(db, id)?;
        let was_blocked = old["blocked"].as_bool().unwrap_or(false);
        let is_blocked = new["blocked"].as_bool().unwrap_or(false);
        if was_blocked != is_blocked {
            let kind = if is_blocked {
                "card_became_blocked"
            } else if new["ready"] == true {
                "card_became_ready"
            } else {
                "card_unblocked"
            };
            effects.push(json!({"type":kind,"card_id":id,"card_key":new["key"],"owner_actor":new["owner_actor"],"before":{"blocked":was_blocked,"ready":old["ready"]},"after":{"blocked":is_blocked,"ready":new["ready"]},"cause_key":cause_key}));
        }
        if old["can_finish"] != new["can_finish"] {
            effects.push(json!({"type":if new["can_finish"]==true {"card_can_finish"} else {"card_finish_blocked"},"card_id":id,"card_key":new["key"],"owner_actor":new["owner_actor"],"before":{"can_finish":old["can_finish"]},"after":{"can_finish":new["can_finish"]},"cause_key":cause_key}));
        }
    }
    Ok(effects)
}
