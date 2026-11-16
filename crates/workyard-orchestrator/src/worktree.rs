//! Per-card git worktrees, created by Workyard rather than by the runtime. A worker gets its
//! own checkout and branch so parallel workers never share files, and because the runtime is
//! not told it is "in a worktree" it can still merge into the main checkout itself.
//!
//! Layout: `<main checkout>/.workyard/worktrees/<key>` on branch `workyard/<key>` (key in
//! lower case). The directory is kept out of git through the repository's `info/exclude`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Make sure the card's worktree exists and return its path. An existing one (a resumed
/// session, or a card handed to a fresh worker) is reused as it is, with its branch and
/// uncommitted files; a missing one is added on `workyard/<key>`, branching from the main
/// checkout's current commit, or from the existing branch if the card has been here before.
pub fn ensure_worktree(main: &Path, card_key: &str) -> io::Result<PathBuf> {
    let path = worktree_path(main, card_key);
    if path.join(".git").exists() {
        return Ok(path);
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
    Ok(path)
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

    #[test]
    fn a_directory_that_is_not_a_repository_is_reported() {
        let dir = std::env::temp_dir().join(format!("workyard-wt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(ensure_worktree(&dir, "C-1").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
