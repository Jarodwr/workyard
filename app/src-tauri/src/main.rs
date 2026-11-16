use serde_json::Value;
use tauri::Emitter;
use workyard_core::Store;

fn store() -> Result<Store, String> {
    Store::default_path()
        .map(Store::new)
        .map_err(|e| e.to_string())
}
fn wrap(value: workyard_core::Result<Value>) -> Result<Value, String> {
    value.map_err(|e| serde_json::to_string(&e).unwrap_or_else(|_| e.to_string()))
}

#[tauri::command]
fn list_projects() -> Result<Value, String> {
    wrap(store()?.list_projects())
}
#[tauri::command]
fn view_project(project: String) -> Result<Value, String> {
    wrap(store()?.view_project(&project))
}
#[tauri::command]
fn list_boards() -> Result<Value, String> {
    wrap(store()?.list_boards())
}
#[tauri::command]
fn view_board(board: String) -> Result<Value, String> {
    wrap(store()?.view_board(&board))
}
#[tauri::command]
fn view_card(card: String) -> Result<Value, String> {
    wrap(store()?.view_card(&card))
}
#[tauri::command]
fn add_card_note(card: String, body: String) -> Result<Value, String> {
    wrap(store()?.add_card_note("ui", &card, &body))
}
#[tauri::command]
fn latest_event_id() -> Result<i64, String> {
    store()?.latest_event_id().map_err(|e| e.to_string())
}
#[tauri::command]
fn poll_events(since: i64, limit: i64) -> Result<Value, String> {
    wrap(store()?.poll_events(since, limit))
}
#[tauri::command]
fn list_questions() -> Result<Value, String> {
    wrap(store()?.list_questions(None, None))
}
#[tauri::command]
fn answer_question(
    question: String,
    answer: String,
    note: String,
    expected_version: i64,
) -> Result<Value, String> {
    wrap(store()?.resolve_question(
        "ui",
        &question,
        &answer,
        Some(&note),
        expected_version,
        false,
    ))
}
#[tauri::command]
fn assign_question_stakeholder(question: String, stakeholder: String, expected_version: i64) -> Result<Value, String> {
    wrap(store()?.assign_question_stakeholder("ui", &question, &stakeholder, expected_version))
}
#[tauri::command]
fn unassign_question_stakeholder(question: String, expected_version: i64) -> Result<Value, String> {
    wrap(store()?.unassign_question_stakeholder("ui", &question, expected_version))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The project repository for a board: where a worker's worktree lives.
fn repository_for_board(store: &Store, board_id: &str) -> Option<String> {
    let boards = store.list_boards().ok()?;
    let project = boards
        .as_array()?
        .iter()
        .find(|board| board["id"] == board_id)?["project_id"]
        .as_str()?
        .to_owned();
    let project = store.view_project(&project).ok()?;
    project["repositories"][0]["path"].as_str().map(str::to_owned)
}

/// Take a card's agent session over in the default terminal. The session is held first, which
/// pauses it and makes the orchestrator stop its worker and leave the session alone until
/// the hold is released, so two processes never drive one session.
#[tauri::command]
fn open_in_terminal(card: String) -> Result<Value, String> {
    let store = store()?;
    let detail = store.view_card(&card).map_err(|e| e.to_string())?;
    let session = detail["session"].clone();
    let session_id = session["external_session_id"]
        .as_str()
        .ok_or("this card has no agent session to open")?
        .to_owned();
    if session["runner"] != "claude" {
        return Err("only Claude Code sessions can be opened in a terminal".into());
    }
    let who = std::env::var("USER").unwrap_or_else(|_| "human".into());
    let held = wrap(store.hold_task_session("ui", &card, &who))?;
    let key = detail["key"].as_str().unwrap_or_default().to_lowercase();
    let repo = repository_for_board(&store, detail["board_id"].as_str().unwrap_or_default())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    let worktree = if detail["workspace"] == "none" { String::new() } else { format!(" --worktree {}", shell_quote(&key)) };
    let script = format!(
        "#!/bin/zsh\ncd {repo} || exit 1\nexport WORKYARD_DB={db}\nexport WORKYARD_ACTOR={actor}\n\
echo 'Workyard: you are driving {key}. Exit Claude when done, then choose Release hold in Workyard.'\n\
claude --resume {session}{worktree}\n",
        repo = shell_quote(&repo),
        db = shell_quote(&store.path().to_string_lossy()),
        actor = shell_quote(detail["owner_actor"].as_str().unwrap_or("human")),
        key = key,
        session = shell_quote(&session_id),
        worktree = worktree,
    );
    // A .command file opens in whichever terminal is the user's default for such files.
    let path = std::env::temp_dir().join(format!("workyard-{key}.command"));
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::process::Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?;
    Ok(held)
}
/// Start the board's orchestrator as a detached `workyard orch run`, in the project's repository,
/// so it keeps running after the desktop closes. A login shell supplies the user's PATH, which a
/// GUI app does not inherit, so `claude` can be found. One orchestrator per board still holds:
/// a second start fails inside `orch run` and its reason is returned here.
#[tauri::command]
fn start_orchestrator(board: String) -> Result<Value, String> {
    use std::io::{Read, Seek, SeekFrom};
    let store = store()?;
    let view = store.view_board(&board).map_err(|e| e.to_string())?;
    let key = view["key"].as_str().unwrap_or(&board).to_owned();
    let status = store.orchestrator_status(&key).map_err(|e| e.to_string())?;
    if status["connected"] == true {
        return Err(format!("{key} already has a running orchestrator ({})", status["holder"].as_str().unwrap_or("unknown")));
    }
    let repo = repository_for_board(&store, view["id"].as_str().unwrap_or_default());
    let bin = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("workyard")))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workyard".into());
    let mut logs = store.path().as_os_str().to_owned();
    logs.push(".logs");
    let logs = std::path::PathBuf::from(logs);
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log_path = logs.join(format!("orchestrator-{}.log", key.to_lowercase()));
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| e.to_string())?;
    let start = log.metadata().map(|m| m.len()).unwrap_or(0);
    let mut command = std::process::Command::new("/bin/zsh");
    command
        .args(["-l", "-c", "exec \"$0\" \"$@\"", &bin, "orch", "run", &key])
        .env("WORKYARD_DB", store.path())
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    if let Some(repo) = &repo {
        command.args(["--cwd", repo]);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0); // not tied to this app's process group or terminal
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(Some(exit)) = child.try_wait() {
            // It stopped at once: the log says why (another orchestrator, claude not found, ...).
            let mut tail = String::new();
            if let Ok(mut file) = std::fs::File::open(&log_path) {
                let _ = file.seek(SeekFrom::Start(start));
                let _ = file.read_to_string(&mut tail);
            }
            return Err(format!("the orchestrator exited ({exit}): {}", tail.trim()));
        }
        if store.orchestrator_status(&key).map(|s| s["connected"] == true).unwrap_or(false) {
            break;
        }
    }
    std::thread::spawn(move || {
        let _ = child.wait(); // reap it if it ends while the desktop is open
    });
    wrap(store.orchestrator_status(&key))
}

/// Ask the board's orchestrator to stop. It pauses its workers and releases the lease on the
/// way out, exactly as on Ctrl-C. Only possible for one running on this machine.
#[tauri::command]
fn stop_orchestrator(board: String) -> Result<Value, String> {
    let store = store()?;
    let status = store.orchestrator_status(&board).map_err(|e| e.to_string())?;
    if status["connected"] != true {
        return wrap(Ok(status));
    }
    if status["host"].as_str() != Some(workyard_orchestrator::hostname().as_str()) {
        return Err("that orchestrator runs on another machine".into());
    }
    let pid = status["pid"].as_i64().ok_or("the orchestrator did not record a process id")?;
    std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|e| e.to_string())?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if store.orchestrator_status(&board).map(|s| s["connected"] != true).unwrap_or(false) {
            break;
        }
    }
    wrap(store.orchestrator_status(&board))
}

/// Open a planner agent in the default terminal, pointed at one board. It gets the planner skill
/// as extra system prompt (so the plugin need not be installed) and acts as its own actor.
#[tauri::command]
fn open_planner(board: String) -> Result<Value, String> {
    const SKILL: &str = include_str!("../../../plugin/skills/workyard-planner/SKILL.md");
    let store = store()?;
    let view = store.view_board(&board).map_err(|e| e.to_string())?;
    let key = view["key"].as_str().unwrap_or(&board).to_owned();
    let name = view["name"].as_str().unwrap_or_default().to_owned();
    // Drop the skill's frontmatter; the rest is plain instructions.
    let instructions = SKILL.splitn(3, "---").nth(2).unwrap_or(SKILL).trim().to_owned();
    let repo = repository_for_board(&store, view["id"].as_str().unwrap_or_default())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    let dir = std::env::temp_dir();
    let system = dir.join(format!("workyard-plan-{}.md", key.to_lowercase()));
    std::fs::write(&system, instructions).map_err(|e| e.to_string())?;
    let first = format!(
        "I want to plan or change the work on board {key} ({name}). Read it first with `workyard --json board {key}` \
and its project with `workyard --json projects`, summarise what is there in a few lines, then ask me what to change. \
Do not claim or start any card."
    );
    let script = format!(
        "#!/bin/zsh\ncd {repo} || exit 1\nexport WORKYARD_DB={db}\nexport WORKYARD_ACTOR={actor}\n\
echo 'Workyard planner for {key}. Cards are not started from here; run the orchestrator for that.'\n\
claude --append-system-prompt \"$(cat {system})\" {first}\n",
        repo = shell_quote(&repo),
        db = shell_quote(&store.path().to_string_lossy()),
        actor = shell_quote(&format!("agent:planner-{}", key.to_lowercase())),
        system = shell_quote(&system.to_string_lossy()),
        first = shell_quote(&first),
        key = key,
    );
    let path = dir.join(format!("workyard-plan-{}.command", key.to_lowercase()));
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::process::Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"board": key}))
}
#[tauri::command]
fn release_session_hold(card: String) -> Result<Value, String> {
    wrap(store()?.release_task_session_hold("ui", &card))
}

/// A human approves the current commit of a linked change. Agents have no command for this.
#[tauri::command]
fn approve_change(change: String, expected_version: i64) -> Result<Value, String> {
    let reviewer = std::env::var("USER").unwrap_or_else(|_| "reviewer".into());
    wrap(store()?.approve_card_change("ui", &change, &reviewer, expected_version))
}
#[tauri::command]
fn orchestrator_status(board: String) -> Result<Value, String> {
    wrap(store()?.orchestrator_status(&board))
}
#[tauri::command]
fn set_orchestrator_state(board: String, state: String) -> Result<Value, String> {
    wrap(store()?.orchestrator_set_state("ui", &board, &state))
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Wake the UI on every committed write (the core touches `<db>.tick`), plus a
            // slow fallback every two seconds. The UI then reads the event log, which stays
            // the source of truth, so a missed tick only delays an update.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let Ok(store) = store() else { return };
                let mut marker = store.tick_marker();
                loop {
                    marker = store.wait_for_tick(&marker, std::time::Duration::from_secs(2));
                    let _ = handle.emit("workyard-tick", ());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            view_project,
            list_boards,
            view_board,
            view_card,
            add_card_note,
            latest_event_id,
            poll_events,
            list_questions,
            answer_question,
            assign_question_stakeholder,
            unassign_question_stakeholder,
            approve_change,
            open_in_terminal,
            open_planner,
            start_orchestrator,
            stop_orchestrator,
            release_session_hold,
            orchestrator_status,
            set_orchestrator_state
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Workyard");
}
