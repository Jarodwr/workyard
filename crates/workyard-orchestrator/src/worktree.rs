//! Per-card git worktrees, created by Workyard rather than by the runtime. A worker gets its
//! own checkout and branch so parallel workers never share files, and because the runtime is
//! not told it is "in a worktree" it can still merge into the main checkout itself.
//!
//! Layout: `<main checkout>/.workyard/worktrees/<key>` on branch `workyard/<key>` (key in
//! lower case). The directory is kept out of git through the repository's `info/exclude`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn branch_for(card_key: &str) -> String {
    format!("workyard/{}", card_key.to_lowercase())
}

pub fn worktree_path(main: &Path, card_key: &str) -> PathBuf {
    main.join(".workyard").join("worktrees").join(card_key.to_lowercase())
}

fn git(main: &Path, args: &[&str]) -> io::Result<(bool, String, String)> {
    let out = Command::new("git").arg("-C").arg(main).args(args).output()?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    ))
}

fn failed(what: &str, stderr: String) -> io::Error {
    io::Error::other(format!("{what}: {stderr}"))
}

/// The branch checked out in the main checkout, which is where finished work is merged.
pub fn current_branch(main: &Path) -> Option<String> {
    git(main, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .filter(|(ok, out, _)| *ok && !out.is_empty())
        .map(|(_, out, _)| out)
}

fn exclude_workyard_dir(main: &Path) -> io::Result<()> {
    let (ok, dir, err) = git(main, &["rev-parse", "--git-common-dir"])?;
    if !ok {
        return Err(failed("not a git repository", err));
    }
    let common = if Path::new(&dir).is_absolute() { PathBuf::from(dir) } else { main.join(dir) };
    let exclude = common.join("info").join("exclude");
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == ".workyard/") {
        return Ok(());
    }
    std::fs::create_dir_all(common.join("info"))?;
    let separator = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    std::fs::write(&exclude, format!("{existing}{separator}.workyard/\n"))
}

/// A repository may declare this script; it runs once, from the new worktree's root, to restore
/// what git ignores (dependency directories, staged binaries) that a bare checkout lacks.
pub const SETUP_SCRIPT: &str = "scripts/worktree-setup.sh";
const SETUP_TIMEOUT: Duration = Duration::from_secs(120);

/// Run the repository's optional setup script in a freshly created worktree with
/// `WORKYARD_MAIN_CHECKOUT` and `WORKYARD_CARD` set. `Ok(())` when there is no script or it
/// succeeded; `Err` describes a failure or timeout.
fn run_setup(main: &Path, worktree: &Path, card_key: &str, timeout: Duration) -> Result<(), String> {
    if !worktree.join(SETUP_SCRIPT).is_file() {
        return Ok(());
    }
    let mut child = Command::new("sh")
        .arg(SETUP_SCRIPT)
        .current_dir(worktree)
        .env("WORKYARD_MAIN_CHECKOUT", main)
        .env("WORKYARD_CARD", card_key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {SETUP_SCRIPT}: {e}"))?;
    // Drain stderr on a thread so a chatty script cannot block on a full pipe.
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = io::Read::read_to_string(pipe, &mut text);
        }
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                // A grandchild may still hold the pipe open; do not wait for it.
                return Err(format!("{SETUP_SCRIPT} timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => break Err(e),
        }
    };
    let text = reader.join().unwrap_or_default();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{SETUP_SCRIPT} failed ({status}): {}", text.trim())),
        Err(e) => Err(format!("{SETUP_SCRIPT}: {e}")),
    }
}

/// Make sure the card's worktree exists and return its path. An existing one (a resumed
/// session, or a card handed to a fresh worker) is reused as it is, with its branch and
/// uncommitted files; a missing one is added on `workyard/<key>`, branching from the main
/// checkout's current commit, or from the existing branch if the card has been here before.
/// A newly created worktree runs the repository's `scripts/worktree-setup.sh`, if it has one;
/// a failure there is logged and never stops the worker.
pub fn ensure_worktree(main: &Path, card_key: &str) -> io::Result<PathBuf> {
    ensure_worktree_noting(main, card_key).map(|(path, _)| path)
}

/// Like [`ensure_worktree`], also returning the setup failure (if any) so a caller can note it.
pub fn ensure_worktree_noting(main: &Path, card_key: &str) -> io::Result<(PathBuf, Option<String>)> {
    let path = worktree_path(main, card_key);
    if path.join(".git").exists() {
        return Ok((path, None));
    }
    exclude_workyard_dir(main)?;
    let _ = git(main, &["worktree", "prune"]);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let branch = branch_for(card_key);
    let target = path.to_string_lossy().into_owned();
    let exists = git(main, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")])?.0;
    let (ok, _, err) = if exists {
        git(main, &["worktree", "add", &target, &branch])?
    } else {
        git(main, &["worktree", "add", "-b", &branch, &target, "HEAD"])?
    };
    if !ok {
        return Err(failed(&format!("could not create the worktree for {card_key}"), err));
    }
    let note = run_setup(main, &path, card_key, SETUP_TIMEOUT).err();
    if let Some(note) = &note {
        eprintln!("{card_key}: worktree setup: {note}");
    }
    Ok((path, note))
}

/// Remove a finished card's worktree and delete its branch. Safe by construction: nothing is
/// forced, so a dirty or locked worktree and a branch that is not merged into the main
/// checkout's branch are left alone (`Ok(None)` when there is nothing to remove, `Err` with
/// the reason when something was left behind). `Ok(Some(..))` says what was removed.
pub fn remove_merged(main: &Path, card_key: &str) -> io::Result<Option<String>> {
    let path = worktree_path(main, card_key);
    let branch = branch_for(card_key);
    let has_branch = git(main, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")])?.0;
    let has_worktree = path.join(".git").exists();
    if !has_branch && !has_worktree {
        return Ok(None);
    }
    // `branch -d` only checks against HEAD or the upstream, so check the target explicitly.
    if has_branch && !git(main, &["merge-base", "--is-ancestor", &branch, "HEAD"])?.0 {
        return Err(io::Error::other(format!("{branch} is not merged into the main checkout's branch")));
    }
    let mut removed = Vec::new();
    if has_worktree {
        let (ok, _, err) = git(main, &["worktree", "remove", &path.to_string_lossy()])?;
        if !ok {
            return Err(failed(&format!("left the worktree {}", path.display()), err));
        }
        removed.push(format!("worktree {}", path.display()));
    }
    if has_branch {
        let (ok, _, err) = git(main, &["branch", "-d", &branch])?;
        if !ok {
            return Err(failed(&format!("left the branch {branch}"), err));
        }
        removed.push(format!("branch {branch}"));
    }
    Ok(Some(removed.join(" and ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("workyard-wt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@example.invalid"],
            vec!["config", "user.name", "T"],
            vec!["commit", "-q", "--allow-empty", "-m", "first"],
        ] {
            assert!(git(&dir, &args).unwrap().0, "{args:?}");
        }
        dir
    }

    #[test]
    fn creates_a_worktree_on_its_own_branch_and_keeps_it_out_of_git() {
        let main = repo();
        let path = ensure_worktree(&main, "C-4").unwrap();
        assert_eq!(path, main.join(".workyard/worktrees/c-4"));
        assert_eq!(current_branch(&path).as_deref(), Some("workyard/c-4"));
        assert_eq!(current_branch(&main).as_deref(), Some("main"));
        let (_, status, _) = git(&main, &["status", "--porcelain"]).unwrap();
        assert_eq!(status, "", "the worktree directory must not show up as untracked");
        // A second call reuses it, including work in progress.
        std::fs::write(path.join("wip.txt"), "x").unwrap();
        assert_eq!(ensure_worktree(&main, "C-4").unwrap(), path);
        assert!(path.join("wip.txt").exists());
        // The exclude entry is written once.
        let exclude = std::fs::read_to_string(main.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches(".workyard/").count(), 1);
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_card_that_comes_back_gets_its_branch_and_commits_again() {
        let main = repo();
        let path = ensure_worktree(&main, "C-5").unwrap();
        std::fs::write(path.join("a.txt"), "a").unwrap();
        assert!(git(&path, &["add", "-A"]).unwrap().0);
        assert!(git(&path, &["commit", "-q", "-m", "work"]).unwrap().0);
        // The directory is lost (a deleted checkout); the branch with the work remains.
        assert!(git(&main, &["worktree", "remove", "--force", &path.to_string_lossy()]).unwrap().0);
        let again = ensure_worktree(&main, "C-5").unwrap();
        assert!(again.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_workers_branch_can_be_fast_forwarded_into_main_from_outside_its_worktree() {
        let main = repo();
        let path = ensure_worktree(&main, "C-6").unwrap();
        std::fs::write(path.join("b.txt"), "b").unwrap();
        assert!(git(&path, &["add", "-A"]).unwrap().0);
        assert!(git(&path, &["commit", "-q", "-m", "work"]).unwrap().0);
        assert!(git(&main, &["merge", "--ff-only", "workyard/c-6"]).unwrap().0);
        assert!(main.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&main);
    }

    /// Commit a setup script into the repository so new worktrees check it out.
    fn commit_setup(main: &Path, body: &str) {
        std::fs::create_dir_all(main.join("scripts")).unwrap();
        std::fs::write(main.join(SETUP_SCRIPT), body).unwrap();
        assert!(git(main, &["add", "-A"]).unwrap().0);
        assert!(git(main, &["commit", "-q", "-m", "setup"]).unwrap().0);
    }

    #[test]
    fn a_new_worktree_runs_the_setup_script_once_with_its_environment() {
        let main = repo();
        commit_setup(&main, "echo \"$WORKYARD_CARD $WORKYARD_MAIN_CHECKOUT\" >> setup-marker\n");
        let (path, note) = ensure_worktree_noting(&main, "C-7").unwrap();
        assert_eq!(note, None);
        let marker = std::fs::read_to_string(path.join("setup-marker")).unwrap();
        assert_eq!(marker.trim(), format!("C-7 {}", main.display()));
        // Reuse does not run it again.
        ensure_worktree(&main, "C-7").unwrap();
        let marker = std::fs::read_to_string(path.join("setup-marker")).unwrap();
        assert_eq!(marker.lines().count(), 1);
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_failing_setup_script_is_reported_but_the_worktree_is_still_returned() {
        let main = repo();
        commit_setup(&main, "echo broken >&2\nexit 3\n");
        let (path, note) = ensure_worktree_noting(&main, "C-8").unwrap();
        assert!(path.join(".git").exists());
        let note = note.expect("failure is reported");
        assert!(note.contains("broken"), "{note}");
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_setup_script_that_hangs_is_stopped() {
        let main = repo();
        let path = ensure_worktree(&main, "C-9").unwrap();
        std::fs::create_dir_all(path.join("scripts")).unwrap();
        std::fs::write(path.join(SETUP_SCRIPT), "sleep 30\n").unwrap();
        let err = run_setup(&main, &path, "C-9", Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        let _ = std::fs::remove_dir_all(&main);
    }

    fn commit_in(path: &Path, file: &str) {
        std::fs::write(path.join(file), file).unwrap();
        assert!(git(path, &["add", "-A"]).unwrap().0);
        assert!(git(path, &["commit", "-q", "-m", file]).unwrap().0);
    }

    #[test]
    fn a_merged_cards_worktree_and_branch_are_removed() {
        let main = repo();
        let path = ensure_worktree(&main, "C-10").unwrap();
        commit_in(&path, "a.txt");
        assert!(git(&main, &["merge", "--ff-only", "workyard/c-10"]).unwrap().0);
        let said = remove_merged(&main, "C-10").unwrap().unwrap();
        assert!(said.contains("worktree") && said.contains("branch workyard/c-10"), "{said}");
        assert!(!path.exists());
        assert!(!git(&main, &["rev-parse", "--verify", "-q", "refs/heads/workyard/c-10"]).unwrap().0);
        assert!(main.join("a.txt").exists());
        // Nothing left: a second call is a no-op.
        assert_eq!(remove_merged(&main, "C-10").unwrap(), None);
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_dirty_worktree_is_kept() {
        let main = repo();
        let path = ensure_worktree(&main, "C-11").unwrap();
        commit_in(&path, "a.txt");
        assert!(git(&main, &["merge", "--ff-only", "workyard/c-11"]).unwrap().0);
        std::fs::write(path.join("scratch.txt"), "unsaved").unwrap();
        assert!(remove_merged(&main, "C-11").is_err());
        assert!(path.join("scratch.txt").exists());
        assert!(git(&main, &["rev-parse", "--verify", "-q", "refs/heads/workyard/c-11"]).unwrap().0);
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_branch_that_is_not_on_main_is_kept() {
        let main = repo();
        let path = ensure_worktree(&main, "C-12").unwrap();
        commit_in(&path, "a.txt");
        let err = remove_merged(&main, "C-12").unwrap_err().to_string();
        assert!(err.contains("not merged"), "{err}");
        assert!(path.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&main);
    }

    #[test]
    fn a_directory_that_is_not_a_repository_is_reported() {
        let dir = std::env::temp_dir().join(format!("workyard-wt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(ensure_worktree(&dir, "C-1").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
