//! Integration gate, review default, waiting for integration, activity, and reclaim, with a
//! real git repository where the behaviour depends on git.
use std::path::{Path, PathBuf};
use workyard_core::Store;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

struct World {
    _dir: TempDir,
    store: Store,
    board: String,
    repo_path: PathBuf,
    repo_id: String,
}

fn world() -> World {
    let dir = std::env::temp_dir().join(format!("workyard-int-{}", uuid::Uuid::new_v4()));
    let repo_path = dir.join("repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    git(&repo_path, &["init", "-q", "-b", "main"]);
    git(&repo_path, &["config", "user.email", "t@example.invalid"]);
    git(&repo_path, &["config", "user.name", "T"]);
    git(&repo_path, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let store = Store::new(dir.join("test.db"));
    let board = store.create_board("cli", "Project", "").unwrap()["key"].as_str().unwrap().to_owned();
    let project = store.create_project("agent", "Code", "").unwrap();
    let pid = project["id"].as_str().unwrap().to_owned();
    store.attach_board_to_project("agent", &board, &pid, 1).unwrap();
    let repo = store
        .add_project_repository("agent", &pid, repo_path.to_str().unwrap(), "main", None)
        .unwrap();
    World { _dir: TempDir(dir), store, board, repo_path, repo_id: repo["id"].as_str().unwrap().to_owned() }
}

impl World {
    fn version(&self, key: &str) -> i64 {
        self.store.view_card(key).unwrap()["version"].as_i64().unwrap()
    }

    /// An In Progress card owned by `agent:w` with a session, and a change on `branch`.
    fn working_card(&self, title: &str, branch: &str, review: impl Into<Option<bool>>) -> (String, String) {
        let card = self.store.create_card("cli", &self.board, title, "", "P2", None).unwrap();
        let key = card["key"].as_str().unwrap().to_owned();
        self.store.claim_card("agent:w", &key, self.version(&key)).unwrap();
        self.store.move_card("agent:w", &key, "In Progress", None, self.version(&key)).unwrap();
        self.store.attach_task_session("agent:w", &key, "claude", &format!("s-{key}")).unwrap();
        let change = self
            .store
            .link_card_change("agent:w", &key, &self.repo_id, branch, None, "", review, self.version(&key))
            .unwrap();
        (key, change["id"].as_str().unwrap().to_owned())
    }

    fn branch_with_commit(&self, branch: &str, file: &str) {
        git(&self.repo_path, &["checkout", "-q", "-b", branch]);
        std::fs::write(self.repo_path.join(file), file).unwrap();
        git(&self.repo_path, &["add", "-A"]);
        git(&self.repo_path, &["commit", "-q", "-m", file]);
        git(&self.repo_path, &["checkout", "-q", "main"]);
    }
}

#[test]
fn done_requires_the_branch_to_be_on_the_target_branch() {
    let w = world();
    w.branch_with_commit("feature", "a.txt");
    let (key, _) = w.working_card("Feature", "feature", false);
    let err = w.store.move_card("agent:w", &key, "Done", None, w.version(&key)).unwrap_err();
    assert_eq!(err.code, "not_integrated");
    let details = err.details.unwrap().to_string();
    assert!(details.contains("feature") && details.contains("not on main"), "{details}");

    git(&w.repo_path, &["merge", "--ff-only", "-q", "feature"]);
    w.store.move_card("agent:w", &key, "Done", None, w.version(&key)).unwrap();
}

#[test]
fn a_branch_that_moved_on_after_a_merge_is_not_integrated_until_merged_again() {
    let w = world();
    w.branch_with_commit("feature", "a.txt");
    git(&w.repo_path, &["merge", "--ff-only", "-q", "feature"]);
    // More work lands on the branch after the first merge.
    git(&w.repo_path, &["checkout", "-q", "feature"]);
    std::fs::write(w.repo_path.join("b.txt"), "b").unwrap();
    git(&w.repo_path, &["add", "-A"]);
    git(&w.repo_path, &["commit", "-q", "-m", "b"]);
    git(&w.repo_path, &["checkout", "-q", "main"]);
    let (key, _) = w.working_card("Feature", "feature", false);
    assert_eq!(
        w.store.move_card("agent:w", &key, "Done", None, w.version(&key)).unwrap_err().code,
        "not_integrated"
    );
    git(&w.repo_path, &["merge", "--ff-only", "-q", "feature"]);
    w.store.move_card("agent:w", &key, "Done", None, w.version(&key)).unwrap();
}

#[test]
fn a_deleted_branch_falls_back_to_the_recorded_commit_and_a_missing_repository_blocks() {
    let w = world();
    w.branch_with_commit("gone", "g.txt");
    let tip = {
        let out = std::process::Command::new("git").arg("-C").arg(&w.repo_path).args(["rev-parse", "gone"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    // The change records the commit, then the branch is deleted without being merged.
    let card = w.store.create_card("cli", &w.board, "Gone", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap().to_owned();
    w.store.claim_card("agent:w", &key, w.version(&key)).unwrap();
    w.store.move_card("agent:w", &key, "In Progress", None, w.version(&key)).unwrap();
    w.store.link_card_change("agent:w", &key, &w.repo_id, "gone", None, &tip, false, w.version(&key)).unwrap();
    git(&w.repo_path, &["branch", "-q", "-D", "gone"]);
    assert_eq!(
        w.store.move_card("agent:w", &key, "Done", None, w.version(&key)).unwrap_err().code,
        "not_integrated"
    );
    git(&w.repo_path, &["merge", "--ff-only", "-q", &tip]);
    w.store.move_card("agent:w", &key, "Done", None, w.version(&key)).unwrap();

    // A repository whose directory has gone cannot be verified, so the card cannot finish.
    let w2 = world();
    let (key2, _) = w2.working_card("Lost", "feature", false);
    std::fs::remove_dir_all(&w2.repo_path).unwrap();
    let err = w2.store.move_card("agent:w", &key2, "Done", None, w2.version(&key2)).unwrap_err();
    assert_eq!(err.code, "not_integrated");
    assert!(err.details.unwrap().to_string().contains("not readable"));
}

#[test]
fn a_card_without_changes_is_not_gated() {
    let w = world();
    let card = w.store.create_card("cli", &w.board, "Research", "", "P2", None).unwrap();
    let key = card["key"].as_str().unwrap();
    w.store.claim_card("agent:w", key, w.version(key)).unwrap();
    w.store.move_card("agent:w", key, "In Progress", None, w.version(key)).unwrap();
    w.store.move_card("agent:w", key, "Done", None, w.version(key)).unwrap();
}

#[test]
fn review_follows_the_board_unless_the_change_says_otherwise() {
    let w = world();
    let (plain, plain_change) = w.working_card("Plain", "p", None);
    assert_eq!(w.store.view_card(&plain).unwrap()["review_gates"].as_array().unwrap().len(), 0, "off by default {plain_change}");
    assert_eq!(w.store.set_board_review_required("human", &w.board, true).unwrap()["review_required"], true);
    let (reviewed, _) = w.working_card("Reviewed", "r", None);
    assert_eq!(w.store.view_card(&reviewed).unwrap()["review_gates"].as_array().unwrap().len(), 1);
    let (waived, _) = w.working_card("Waived", "w", false);
    assert_eq!(w.store.view_card(&waived).unwrap()["review_gates"].as_array().unwrap().len(), 0);
    assert_eq!(w.store.view_board(&w.board).unwrap()["review_required"], true);
}

#[test]
fn a_session_waiting_for_integration_resumes_only_once_the_work_is_on_the_target() {
    let w = world();
    w.branch_with_commit("feature", "a.txt");
    let (key, _) = w.working_card("Feature", "feature", false);
    w.store.pause_task_session_for_integration("agent:w", &key).unwrap();
    let session = w.store.view_card(&key).unwrap()["session"].clone();
    assert!(session.is_null() || session["waiting_integration"] != false, "{session}");
    // Not offered while the branch is not merged...
    assert!(w.store.next_ready_task(&w.board, None).unwrap().is_null());
    assert_eq!(w.store.resume_task_session("orchestrator", &key).unwrap_err().code, "session_not_ready");
    // ...and offered, ahead of the backlog, once it is.
    git(&w.repo_path, &["merge", "--ff-only", "-q", "feature"]);
    let next = w.store.next_ready_task(&w.board, None).unwrap();
    assert_eq!(next["key"], key.as_str());
    assert_eq!(next["dispatch_action"], "resume");
    w.store.resume_task_session("orchestrator", &key).unwrap();
}

#[test]
fn waiting_for_one_thing_at_a_time() {
    let w = world();
    let (key, _) = w.working_card("Feature", "feature", false);
    let err = w.store.pause_task_session_waiting("agent:w", &key, None, true, true).unwrap_err();
    assert_eq!(err.code, "validation_error");
}

#[test]
fn activity_and_permissions_and_reclaim_are_fenced_by_the_lease() {
    let w = world();
    let (key, _) = w.working_card("Feature", "feature", false);
    let lease = w.store.orchestrator_acquire(&w.board, "run", "h", None, 30, None).unwrap();
    let actor = lease["actor"].as_str().unwrap().to_owned();
    assert_eq!(lease["permissions"], "full");
    assert_eq!(w.store.orchestrator_set_permissions(&actor, &w.board, "safe").unwrap()["permissions"], "safe");
    assert_eq!(w.store.orchestrator_set_permissions(&actor, &w.board, "reckless").unwrap_err().code, "validation_error");

    let events = w.store.latest_event_id().unwrap();
    w.store.orchestrator_set_activity(&actor, &w.board, &key, "Edit app.mjs (3 tool uses)").unwrap();
    assert_eq!(w.store.latest_event_id().unwrap(), events, "activity is not an event");
    let board = w.store.view_board(&w.board).unwrap().to_string();
    assert!(board.contains("Edit app.mjs (3 tool uses)") && board.contains("activity_at"), "{board}");
    // The same text again writes nothing; clearing works.
    w.store.orchestrator_set_activity(&actor, &w.board, &key, "Edit app.mjs (3 tool uses)").unwrap();
    w.store.orchestrator_set_activity(&actor, &w.board, &key, "").unwrap();
    assert!(!w.store.view_board(&w.board).unwrap().to_string().contains("Edit app.mjs"));

    // Reclaim puts the card back in the backlog, unowned, with no session.
    let moved = w.store.orchestrator_reclaim(&actor, &w.board, &key, "test").unwrap();
    assert!(moved["owner_actor"].is_null());
    let card = w.store.view_card(&key).unwrap();
    assert!(card["session"].is_null());
    assert_eq!(w.store.next_ready_task(&w.board, None).unwrap()["key"], key.as_str());

    // A stale epoch can do none of it.
    w.store.orchestrator_release(&actor, &w.board).unwrap();
    w.store.orchestrator_acquire(&w.board, "run-2", "h", None, 30, None).unwrap();
    let err = w.store.orchestrator_set_activity(&actor, &w.board, &key, "x").unwrap_err();
    assert_eq!(err.code, "stale_epoch");
}
