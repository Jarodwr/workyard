use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Component, Path};

use crate::{
    Error, Result, Store, card_row, check_board_active, check_version, guard_card, now,
    question_row, record, require_nonblank, resolve_board, resolve_card, resolve_question, uuid,
};

fn project_id(db: &Connection, value: &str) -> Result<String> {
    db.query_row(
        "SELECT id FROM projects WHERE id=?1 OR name=?1",
        [value],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::new("not_found", format!("project {value} not found")))
}
fn project_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row("SELECT id,name,description,created_at,version FROM projects WHERE id=?1", [id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"description":r.get::<_,String>(2)?,"created_at":r.get::<_,String>(3)?,"version":r.get::<_,i64>(4)?})))?)
}
fn repo_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row("SELECT id,project_id,path,target_branch,remote_url,created_at FROM project_repositories WHERE id=?1", [id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"project_id":r.get::<_,String>(1)?,"path":r.get::<_,String>(2)?,"target_branch":r.get::<_,String>(3)?,"remote_url":r.get::<_,Option<String>>(4)?,"created_at":r.get::<_,String>(5)?})))?)
}
pub(crate) fn change_row(db: &Connection, id: &str) -> Result<Value> {
    Ok(db.query_row("SELECT id,card_id,repository_id,working_branch,target_branch,commit_oid,review_required,approved_commit_oid,reviewer,version,created_at FROM card_changes WHERE id=?1", [id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"card_id":r.get::<_,String>(1)?,"repository_id":r.get::<_,String>(2)?,"working_branch":r.get::<_,String>(3)?,"target_branch":r.get::<_,String>(4)?,"commit_oid":r.get::<_,String>(5)?,"review_required":r.get::<_,i64>(6)?!=0,"approved_commit_oid":r.get::<_,Option<String>>(7)?,"reviewer":r.get::<_,Option<String>>(8)?,"version":r.get::<_,i64>(9)?,"created_at":r.get::<_,String>(10)?})))?)
}

impl Store {
    pub fn list_projects(&self) -> Result<Value> {
        let db = self.open()?;
        let mut stmt = db.prepare("SELECT id FROM projects ORDER BY name")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut projects = Vec::new();
        for id in ids {
            let mut project = project_row(&db, &id)?;
            project["board_count"] = json!(db.query_row(
                "SELECT COUNT(*) FROM boards WHERE project_id=?1",
                [&id],
                |r| r.get::<_, i64>(0)
            )?);
            projects.push(project);
        }
        Ok(json!(projects))
    }

    pub fn view_project(&self, project: &str) -> Result<Value> {
        let db = self.open()?;
        let id = project_id(&db, project)?;
        let mut result = project_row(&db, &id)?;
        let mut boards = Vec::new();
        let mut stmt = db.prepare("SELECT id,key,name,description,archived FROM boards WHERE project_id=?1 ORDER BY created_at")?;
        for row in stmt.query_map([&id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"key":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"description":r.get::<_,String>(3)?,"archived":r.get::<_,i64>(4)?!=0})))? { boards.push(row?); }
        result["boards"] = json!(boards);
        let mut repos = Vec::new();
        let mut stmt =
            db.prepare("SELECT id FROM project_repositories WHERE project_id=?1 ORDER BY path")?;
        for row in stmt.query_map([&id], |r| r.get::<_, String>(0))? {
            repos.push(repo_row(&db, &row?)?);
        }
        result["repositories"] = json!(repos);
        let mut docs = Vec::new();
        let mut stmt = db.prepare("SELECT id,repository_id,path,kind,title FROM project_documents WHERE project_id=?1 ORDER BY kind,title")?;
        for row in stmt.query_map([&id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"repository_id":r.get::<_,String>(1)?,"path":r.get::<_,String>(2)?,"kind":r.get::<_,String>(3)?,"title":r.get::<_,String>(4)?})))? { docs.push(row?); }
        result["documents"] = json!(docs);
        let mut people = Vec::new();
        let mut stmt = db.prepare("SELECT id,name,role,contact FROM project_stakeholders WHERE project_id=?1 ORDER BY name")?;
        for row in stmt.query_map([&id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"role":r.get::<_,String>(2)?,"contact":r.get::<_,String>(3)?})))? { people.push(row?); }
        result["stakeholders"] = json!(people);
        let mut changes = Vec::new();
        let mut stmt = db.prepare("SELECT cc.id,c.key,c.title FROM card_changes cc JOIN cards c ON c.id=cc.card_id JOIN boards b ON b.id=c.board_id WHERE b.project_id=?1 ORDER BY cc.created_at DESC")?;
        for row in stmt.query_map([&id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (change_id, card_key, card_title) = row?;
            let mut change = change_row(&db, &change_id)?;
            change["card_key"] = json!(card_key);
            change["card_title"] = json!(card_title);
            changes.push(change);
        }
        result["changes"] = json!(changes);
        Ok(result)
    }

    pub fn create_project(&self, actor: &str, name: &str, description: &str) -> Result<Value> {
        require_nonblank("name", name)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = uuid();
        tx.execute(
            "INSERT INTO projects(id,name,description,created_at) VALUES (?1,?2,?3,?4)",
            params![id, name.trim(), description, now()],
        )?;
        let after = project_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "project_created",
            "",
            "project",
            &id,
            None,
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn attach_board_to_project(
        &self,
        actor: &str,
        board: &str,
        project: &str,
        expected: i64,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let board_id = resolve_board(&tx, board)?;
        check_board_active(&tx, &board_id)?;
        let project_id = project_id(&tx, project)?;
        let before = super::board_row(&tx, &board_id)?;
        check_version(&before, expected)?;
        tx.execute(
            "UPDATE boards SET project_id=?2,version=version+1 WHERE id=?1",
            params![board_id, project_id],
        )?;
        let after = super::board_row(&tx, &board_id)?;
        record(
            &tx,
            actor,
            "board_project_changed",
            &board_id,
            "board",
            &board_id,
            Some(&before),
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn add_project_repository(
        &self,
        actor: &str,
        project: &str,
        path: &str,
        target_branch: &str,
        remote_url: Option<&str>,
    ) -> Result<Value> {
        require_nonblank("path", path)?;
        require_nonblank("target branch", target_branch)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project_id = project_id(&tx, project)?;
        let existing: i64 = tx.query_row(
            "SELECT COUNT(*) FROM project_repositories WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )?;
        if existing > 0 {
            return Err(Error::new("repository_exists", "this project already has a repository"));
        }
        let id = uuid();
        tx.execute("INSERT INTO project_repositories(id,project_id,path,target_branch,remote_url,created_at) VALUES (?1,?2,?3,?4,?5,?6)",params![id,project_id,path.trim(),target_branch.trim(),remote_url,now()])?;
        let after = repo_row(&tx, &id)?;
        record(
            &tx,
            actor,
            "repository_added",
            "",
            "repository",
            &id,
            None,
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn add_project_document(
        &self,
        actor: &str,
        project: &str,
        repository: &str,
        path: &str,
        kind: &str,
        title: &str,
    ) -> Result<Value> {
        require_nonblank("path", path)?;
        require_nonblank("title", title)?;
        if Path::new(path).is_absolute()
            || Path::new(path)
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(Error::new(
                "validation_error",
                "document path must stay relative to its repository",
            ));
        }
        if !matches!(kind, "adr" | "diagram" | "documentation") {
            return Err(Error::new(
                "validation_error",
                "document kind must be adr, diagram, or documentation",
            ));
        }
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project_id = project_id(&tx, project)?;
        let repo_project: String = tx
            .query_row(
                "SELECT project_id FROM project_repositories WHERE id=?1",
                [repository],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::new("not_found", "repository not found"))?;
        if repo_project != project_id {
            return Err(Error::new(
                "cross_project",
                "repository belongs to another project",
            ));
        }
        let id = uuid();
        tx.execute("INSERT INTO project_documents(id,project_id,repository_id,path,kind,title,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![id,project_id,repository,path.trim(),kind,title.trim(),now()])?;
        let after = json!({"id":id,"project_id":project_id,"repository_id":repository,"path":path.trim(),"kind":kind,"title":title.trim()});
        record(
            &tx,
            actor,
            "project_document_added",
            "",
            "document",
            &id,
            None,
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn add_project_stakeholder(
        &self,
        actor: &str,
        project: &str,
        name: &str,
        role: &str,
        contact: &str,
    ) -> Result<Value> {
        require_nonblank("name", name)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project_id = project_id(&tx, project)?;
        let id = uuid();
        tx.execute("INSERT INTO project_stakeholders(id,project_id,name,role,contact,created_at) VALUES (?1,?2,?3,?4,?5,?6)",params![id,project_id,name.trim(),role.trim(),contact.trim(),now()])?;
        let after = json!({"id":id,"project_id":project_id,"name":name.trim(),"role":role.trim(),"contact":contact.trim()});
        record(
            &tx,
            actor,
            "stakeholder_added",
            "",
            "stakeholder",
            &id,
            None,
            Some(&after),
            &[],
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn assign_question_stakeholder(
        &self,
        actor: &str,
        question: &str,
        stakeholder: &str,
        expected: i64,
    ) -> Result<Value> {
        self.set_question_stakeholder(actor, question, Some(stakeholder), expected)
    }

    pub fn unassign_question_stakeholder(
        &self,
        actor: &str,
        question: &str,
        expected: i64,
    ) -> Result<Value> {
        self.set_question_stakeholder(actor, question, None, expected)
    }

    fn set_question_stakeholder(
        &self,
        actor: &str,
        question: &str,
        stakeholder: Option<&str>,
        expected: i64,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = resolve_question(&tx, question)?;
        let before = question_row(&tx, &id)?;
        check_version(&before, expected)?;
        let board_id = before["board_id"].as_str().unwrap();
        crate::phases::ensure_phase_open(&tx, board_id, before["phase_id"].as_str().unwrap())?;
        if let Some(stakeholder) = stakeholder {
            let project_id: Option<String> = tx.query_row(
                "SELECT project_id FROM boards WHERE id=?1",
                [board_id],
                |r| r.get(0),
            )?;
            let stakeholder_project: String = tx
                .query_row(
                    "SELECT project_id FROM project_stakeholders WHERE id=?1",
                    [stakeholder],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| Error::new("not_found", "stakeholder not found"))?;
            if project_id.as_deref() != Some(stakeholder_project.as_str()) {
                return Err(Error::new(
                    "cross_project",
                    "stakeholder is not in the board's project",
                ));
            }
        } else if before["stakeholder_id"].is_null() {
            return Err(Error::new("validation_error", "question is already unassigned"));
        }
        tx.execute(
            "UPDATE questions SET stakeholder_id=?2,version=version+1 WHERE id=?1",
            params![id, stakeholder],
        )?;
        let after = question_row(&tx, &id)?;
        record(
            &tx,
            actor,
            if stakeholder.is_some() { "question_stakeholder_assigned" } else { "question_stakeholder_unassigned" },
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

    pub fn link_card_change(
        &self,
        actor: &str,
        card: &str,
        repository: &str,
        branch: &str,
        target: Option<&str>,
        commit: &str,
        review_required: bool,
        expected: i64,
    ) -> Result<Value> {
        require_nonblank("branch", branch)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let card_id = resolve_card(&tx, card)?;
        let before = guard_card(&tx, &card_id, actor, Some(expected))?;
        let board_id = before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        crate::phases::ensure_phase_open(&tx, board_id, before["phase_id"].as_str().unwrap())?;
        let project_id: Option<String> = tx.query_row(
            "SELECT project_id FROM boards WHERE id=?1",
            [board_id],
            |r| r.get(0),
        )?;
        let (repo_project, default_target): (String, String) = tx
            .query_row(
                "SELECT project_id,target_branch FROM project_repositories WHERE id=?1",
                [repository],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::new("not_found", "repository not found"))?;
        if project_id.as_deref() != Some(repo_project.as_str()) {
            return Err(Error::new(
                "cross_project",
                "repository is not in the card's project",
            ));
        }
        let target = target.unwrap_or(&default_target);
        require_nonblank("target branch", target)?;
        let id = uuid();
        tx.execute("INSERT INTO card_changes(id,card_id,repository_id,working_branch,target_branch,commit_oid,review_required,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![id,card_id,repository,branch.trim(),target.trim(),commit.trim(),review_required,now()])?;
        tx.execute(
            "UPDATE cards SET version=version+1,updated_at=?2 WHERE id=?1",
            params![card_id, now()],
        )?;
        let after = card_row(&tx, &card_id)?;
        let change = change_row(&tx, &id)?;
        let old = HashMap::from([(card_id.clone(), before.clone())]);
        let mut effects = super::changed_state_effects(&tx, &[card_id.clone()], &old, &id)?;
        effects.push(json!({"type":"change_linked","change":change}));
        record(
            &tx,
            actor,
            "card_change_linked",
            board_id,
            "card",
            &card_id,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(change)
    }

    pub fn update_card_change(
        &self,
        actor: &str,
        change: &str,
        commit: &str,
        expected: i64,
    ) -> Result<Value> {
        require_nonblank("commit", commit)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = change_row(&tx, change)?;
        check_version(&before, expected)?;
        let card_id = before["card_id"].as_str().unwrap();
        let card = card_row(&tx, card_id)?;
        let board_id = card["board_id"].as_str().unwrap();
        crate::phases::ensure_phase_open(&tx, board_id, card["phase_id"].as_str().unwrap())?;
        if card["cancelled_at"].is_string() {
            return Err(Error::new(
                "card_cancelled",
                "cancelled cards cannot change linked code",
            ));
        }
        if card["owner_actor"]
            .as_str()
            .is_some_and(|owner| owner != actor)
        {
            return Err(super::owner_error(card["owner_actor"].as_str().unwrap()));
        }
        tx.execute("UPDATE card_changes SET commit_oid=?2,approved_commit_oid=NULL,reviewer=NULL,version=version+1 WHERE id=?1",params![change,commit.trim()])?;
        let after = change_row(&tx, change)?;
        let old = HashMap::from([(card_id.to_owned(), card.clone())]);
        let mut effects = super::changed_state_effects(&tx, &[card_id.to_owned()], &old, change)?;
        effects.push(json!({"type":"review_pending","card_id":card_id}));
        record(
            &tx,
            actor,
            "card_change_updated",
            board_id,
            "change",
            change,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn set_change_review_required(
        &self,
        actor: &str,
        change: &str,
        required: bool,
        expected_change: i64,
        expected_card: i64,
    ) -> Result<Value> {
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = change_row(&tx, change)?;
        check_version(&before, expected_change)?;
        let card_id = before["card_id"].as_str().unwrap();
        let card_before = guard_card(&tx, card_id, actor, Some(expected_card))?;
        let board_id = card_before["board_id"].as_str().unwrap();
        check_board_active(&tx, board_id)?;
        if before["review_required"] == required {
            return Err(Error::new(
                "validation_error",
                "review requirement is already in that state",
            ));
        }
        tx.execute("UPDATE card_changes SET review_required=?2,approved_commit_oid=NULL,reviewer=NULL,version=version+1 WHERE id=?1",params![change,required])?;
        tx.execute(
            "UPDATE cards SET version=version+1,updated_at=?2 WHERE id=?1",
            params![card_id, now()],
        )?;
        let after = change_row(&tx, change)?;
        let old = HashMap::from([(card_id.to_owned(), card_before.clone())]);
        let effects = super::changed_state_effects(&tx, &[card_id.to_owned()], &old, change)?;
        record(
            &tx,
            actor,
            "change_review_requirement_updated",
            board_id,
            "change",
            change,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }

    pub fn approve_card_change(
        &self,
        actor: &str,
        change: &str,
        reviewer: &str,
        expected: i64,
    ) -> Result<Value> {
        require_nonblank("reviewer", reviewer)?;
        let mut db = self.open()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = change_row(&tx, change)?;
        check_version(&before, expected)?;
        if before["review_required"] != true {
            return Err(Error::new(
                "validation_error",
                "change does not require review",
            ));
        }
        let commit = before["commit_oid"].as_str().unwrap();
        require_nonblank("commit", commit)?;
        let card_id = before["card_id"].as_str().unwrap();
        let board_id: String =
            tx.query_row("SELECT board_id FROM cards WHERE id=?1", [card_id], |r| {
                r.get(0)
            })?;
        let card_before = card_row(&tx, card_id)?;
        crate::phases::ensure_phase_open(
            &tx,
            &board_id,
            card_before["phase_id"].as_str().unwrap(),
        )?;
        if card_before["cancelled_at"].is_string() {
            return Err(Error::new(
                "card_cancelled",
                "cancelled cards cannot receive review approval",
            ));
        }
        tx.execute("UPDATE card_changes SET approved_commit_oid=?2,reviewer=?3,version=version+1 WHERE id=?1",params![change,commit,reviewer.trim()])?;
        let after = change_row(&tx, change)?;
        let old = HashMap::from([(card_id.to_owned(), card_before)]);
        let mut effects = super::changed_state_effects(&tx, &[card_id.to_owned()], &old, change)?;
        effects.push(json!({"type":"review_approved","card_id":card_id}));
        record(
            &tx,
            actor,
            "card_review_approved",
            &board_id,
            "change",
            change,
            Some(&before),
            Some(&after),
            &effects,
            "",
        )?;
        self.commit(tx)?;
        Ok(after)
    }
}
