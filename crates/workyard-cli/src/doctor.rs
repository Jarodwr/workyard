//! `workyard doctor`: checks that this installation can do its job.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use workyard_core::{Result, SCHEMA_VERSION, Store};

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Ok => "OK",
            Level::Warn => "WARN",
            Level::Fail => "FAIL",
        }
    }
}

struct Check {
    name: &'static str,
    level: Level,
    message: String,
}

fn check(name: &'static str, level: Level, message: impl Into<String>) -> Check {
    Check { name, level, message: message.into() }
}

/// Finds `name` in the directories of `PATH`, searched in order.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// First line of `<bin> --version`, or why it could not be read.
fn tool_version(bin: &Path) -> std::result::Result<String, String> {
    let output = Command::new(bin).arg("--version").output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("`--version` exited with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("").trim().to_owned())
}

fn tool_check(name: &'static str, missing: Level, hint: &str) -> Check {
    match which(name) {
        None => check(name, missing, format!("`{name}` not found on PATH; {hint}")),
        Some(path) => match tool_version(&path) {
            Ok(version) => check(name, Level::Ok, format!("{version} ({})", path.display())),
            Err(why) => check(name, Level::Warn, format!("found at {} but its version could not be read: {why}", path.display())),
        },
    }
}

fn orchestrator_checks(store: &Store) -> Result<Vec<Check>> {
    let mut checks = Vec::new();
    let boards = store.list_boards()?;
    let boards = boards.as_array().cloned().unwrap_or_default();
    if boards.is_empty() {
        checks.push(check("orchestrator", Level::Ok, "no boards yet"));
    }
    for board in boards {
        let Some(key) = board["key"].as_str() else { continue };
        let status = store.orchestrator_status(key)?;
        let live = status["holder"].is_string()
            && status["lease_expires_at"]
                .as_str()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .is_some_and(|t| t > chrono::Utc::now());
        let desired = status["desired_state"].as_str().unwrap_or("?");
        let message = if live {
            format!("{key}: running ({}, pid {}, {desired})", status["holder"].as_str().unwrap_or("?"), status["pid"])
        } else {
            format!("{key}: not running ({desired})")
        };
        checks.push(check("orchestrator", Level::Ok, message));
        if let Some(schema) = status["schema_version"].as_i64().filter(|s| live && *s != SCHEMA_VERSION) {
            checks.push(check("orchestrator", Level::Warn, format!("{key}: started by schema {schema}, this binary has {SCHEMA_VERSION}")));
        }
    }
    Ok(checks)
}

/// Runs the checks, prints them (text or JSON) and returns the process exit code.
pub fn run(db_path: Result<PathBuf>, json_output: bool) -> i32 {
    let mut checks = vec![
        check("version", Level::Ok, env!("CARGO_PKG_VERSION")),
        check("schema", Level::Ok, SCHEMA_VERSION.to_string()),
    ];
    let mut database = Value::Null;
    match db_path {
        Err(error) => checks.push(check("database", Level::Fail, error.to_string())),
        Ok(path) => {
            database = json!(path.display().to_string());
            if !path.exists() {
                checks.push(check("database", Level::Warn, format!("{} does not exist yet; it is created on first use", path.display())));
            } else {
                match orchestrator_checks(&Store::new(&path)) {
                    Ok(orch) => {
                        checks.push(check("database", Level::Ok, path.display().to_string()));
                        checks.extend(orch);
                    }
                    Err(error) => checks.push(check("database", Level::Fail, format!("{}: {error}", path.display()))),
                }
            }
        }
    }
    checks.push(tool_check("claude", Level::Fail, "install Claude Code, workers cannot run without it"));
    checks.push(tool_check("git", Level::Fail, "install git, workers use git worktrees"));
    match crate::find_plugin_dir() {
        Some(dir) if dir.join(".claude-plugin/plugin.json").exists() => checks.push(check("plugin", Level::Ok, dir.display().to_string())),
        Some(dir) => checks.push(check("plugin", Level::Warn, format!("{} has no .claude-plugin/plugin.json", dir.display()))),
        None => checks.push(check("plugin", Level::Warn, "plugin directory not found; set WORKYARD_PLUGIN_DIR or pass --plugin-dir to `orch run`")),
    }

    let failed = checks.iter().any(|c| c.level == Level::Fail);
    if json_output {
        let items: Vec<Value> = checks.iter().map(|c| json!({"name": c.name, "status": c.level.label(), "message": c.message})).collect();
        println!(
            "{}",
            json!({"ok": !failed, "data": {"version": env!("CARGO_PKG_VERSION"), "schema_version": SCHEMA_VERSION, "database": database, "checks": items}})
        );
    } else {
        for c in &checks {
            println!("{:<4}  {:<12} {}", c.level.label(), c.name, c.message);
        }
    }
    i32::from(failed)
}
