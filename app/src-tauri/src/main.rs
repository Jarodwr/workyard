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
fn db_path() -> Result<String, String> {
    Ok(store()?.path().to_string_lossy().into_owned())
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
fn pending_permissions(board: String) -> Result<Value, String> {
    wrap(store()?.permission_requests(&board, Some("pending")))
}
#[tauri::command]
fn decide_permission(request: String, decision: String) -> Result<Value, String> {
    wrap(store()?.permission_decide("ui", &request, &decision, None, ""))
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
/// Post to a question's thread as the human at the desktop; it wakes a worker paused on it.
#[tauri::command]
fn discuss_question(question: String, body: String) -> Result<Value, String> {
    wrap(store()?.post_question_message("ui", &question, &body))
}
#[tauri::command]
fn question_thread(question: String) -> Result<Value, String> {
    wrap(store()?.list_question_messages(&question))
}
/// The human has looked at the thread: clear its unread agent replies.
#[tauri::command]
fn mark_question_read(question: String) -> Result<i64, String> {
    store()?.mark_question_messages_read(&question).map_err(|e| e.to_string())
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

/// The directory holding the `workyard` CLI the desktop ships with (next to this executable),
/// or None when there is none, in which case the terminal's own PATH is used.
fn bundled_bin_dir(exe: &std::path::Path) -> Option<String> {
    let dir = exe.parent()?;
    dir.join("workyard").exists().then(|| dir.to_string_lossy().into_owned())
}

/// Shell lines that export WORKYARD_BIN for a terminal script, so the plugin hook finds the CLI.
fn bin_export_line(exe: Option<&std::path::Path>) -> String {
    match exe.and_then(bundled_bin_dir) {
        Some(dir) => format!("export WORKYARD_BIN={}\n", shell_quote(&dir)),
        None => "export WORKYARD_BIN=\"$(dirname \"$(command -v workyard)\" 2>/dev/null)\"\n".into(),
    }
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
    // A worker runs in `<repo>/.workyard/worktrees/<key>`; resume there so Claude Code finds the session.
    let worktree_dir = std::path::Path::new(&repo).join(".workyard/worktrees").join(&key);
    let repo = if detail["workspace"] != "none" && worktree_dir.is_dir() { worktree_dir.to_string_lossy().into_owned() } else { repo };
    let script = format!(
        "#!/bin/zsh\ncd {repo} || exit 1\nexport WORKYARD_DB={db}\nexport WORKYARD_ACTOR={actor}\n{bin}\
echo 'Workyard: you are driving {key}. Exit Claude when done, then choose Release hold in Workyard.'\n\
claude --resume {session}\n",
        bin = bin_export_line(std::env::current_exe().ok().as_deref()),
        repo = shell_quote(&repo),
        db = shell_quote(&store.path().to_string_lossy()),
        actor = shell_quote(detail["owner_actor"].as_str().unwrap_or("human")),
        key = key,
        session = shell_quote(&session_id),
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
fn start_orchestrator(board: String, supervisor: Option<bool>) -> Result<Value, String> {
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
    if supervisor == Some(true) {
        command.arg("--supervisor");
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
            if exit.success() {
                break; // a board with no work left stops by itself: not a failure
            }
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
        "#!/bin/zsh\ncd {repo} || exit 1\nexport WORKYARD_DB={db}\nexport WORKYARD_ACTOR={actor}\n{bin}\
echo 'Workyard planner for {key}. Cards are not started from here; run the orchestrator for that.'\n\
claude --append-system-prompt \"$(cat {system})\" {first}\n",
        bin = bin_export_line(std::env::current_exe().ok().as_deref()),
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
/// What the new-project dialog needs to know about a folder: that it is a git repository, its
/// current branch (the default target) and whether the working tree is dirty.
fn git_repository_info(path: &str) -> Result<Value, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Choose the repository folder.".into());
    }
    if !std::path::Path::new(path).is_dir() {
        return Err(format!("{path} is not a folder."));
    }
    let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(path).args(args).output();
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).map_err(|e| format!("could not run git: {e}"))?;
    if !branch.status.success() {
        let why = String::from_utf8_lossy(&branch.stderr);
        return Err(if why.contains("not a git repository") {
            format!("{path} is not a git repository.")
        } else {
            format!("{path} has no commits yet; make a first commit so workers have a branch to merge into.")
        });
    }
    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_owned();
    if branch == "HEAD" {
        return Err("The repository is on a detached HEAD; check out a branch first.".into());
    }
    let root = git(&["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| path.to_owned());
    let dirty = git(&["status", "--porcelain"]).map(|out| !out.stdout.is_empty()).unwrap_or(false);
    Ok(serde_json::json!({"path": root, "branch": branch, "dirty": dirty}))
}

#[tauri::command]
fn inspect_repository(path: String) -> Result<Value, String> {
    git_repository_info(&path)
}

/// A native folder picker without a new dependency: AppleScript on macOS. None when cancelled.
#[tauri::command]
fn pick_folder() -> Result<Option<String>, String> {
    if !cfg!(target_os = "macos") {
        return Err("A folder picker is not available here; type the path.".into());
    }
    let out = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (choose folder with prompt \"Choose the repository folder\")"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(None); // cancelled
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().trim_end_matches('/').to_owned();
    Ok((!path.is_empty()).then_some(path))
}

/// Create a project and its repository as one step; the project is removed again if the
/// repository cannot be added.
#[tauri::command]
fn create_project_from_repository(name: String, description: String, path: String, target_branch: String) -> Result<Value, String> {
    let info = git_repository_info(&path)?;
    let root = info["path"].as_str().unwrap_or(&path).to_owned();
    let store = store()?;
    let project = store.create_project("ui", &name, &description).map_err(|e| e.to_string())?;
    let id = project["id"].as_str().unwrap_or_default().to_owned();
    if let Err(error) = store.add_project_repository("ui", &id, &root, &target_branch, None) {
        let _ = store.open().and_then(|db| db.execute("DELETE FROM projects WHERE id=?1", [id.as_str()]).map_err(Into::into));
        return Err(format!("could not add the repository, the project was not created: {error}"));
    }
    wrap(store.view_project(&id))
}

/// `agent:planner-<slug>` needs a name made of lowercase letters, digits and dashes.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() { "project".into() } else { out }
}

/// The planner's opening message for a project: where it is, what boards it has, what to do.
fn project_planner_message(name: &str, id: &str, repo: &str, branch: &str, boards: &[(String, String)]) -> String {
    let mut text = format!(
        "I want to plan work for the Workyard project \"{name}\" (id {id}). Its repository is {repo} and the target branch is {branch}.\n"
    );
    if boards.is_empty() {
        text.push_str(&format!(
            "The project has no board yet. Ask me what I want built. Then create a board with `workyard plan create-board`, \
attach it with `workyard plan attach-board <board> {id}`, and add phases, cards and dependencies as the planner skill describes.\n"
        ));
    } else {
        text.push_str("The project has these boards:\n");
        for (key, board) in boards {
            text.push_str(&format!("- {key}: {board}\n"));
        }
        text.push_str(&format!(
            "Ask me whether to extend one of them or create another (`workyard plan create-board`, then `workyard plan attach-board <board> {id}`).\n"
        ));
    }
    text.push_str("Do not claim or start any card.");
    text
}

/// Open a planner agent in the default terminal for a whole project, started in its repository.
#[tauri::command]
fn open_project_planner(project: String) -> Result<Value, String> {
    const SKILL: &str = include_str!("../../../plugin/skills/workyard-planner/SKILL.md");
    let store = store()?;
    let view = store.view_project(&project).map_err(|e| e.to_string())?;
    let name = view["name"].as_str().unwrap_or_default().to_owned();
    let id = view["id"].as_str().unwrap_or(&project).to_owned();
    let repo = view["repositories"][0]["path"].as_str().ok_or("this project has no repository")?.to_owned();
    let branch = view["repositories"][0]["target_branch"].as_str().unwrap_or("main").to_owned();
    let boards: Vec<(String, String)> = view["boards"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|board| board["archived"] != true)
                .map(|board| (board["key"].as_str().unwrap_or_default().to_owned(), board["name"].as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let instructions = SKILL.splitn(3, "---").nth(2).unwrap_or(SKILL).trim().to_owned();
    let tag = slug(&name);
    let dir = std::env::temp_dir();
    let system = dir.join(format!("workyard-plan-project-{tag}.md"));
    std::fs::write(&system, instructions).map_err(|e| e.to_string())?;
    let first = project_planner_message(&name, &id, &repo, &branch, &boards);
    let script = format!(
        "#!/bin/zsh\ncd {repo} || exit 1\nexport WORKYARD_DB={db}\nexport WORKYARD_ACTOR={actor}\n{bin}\
echo 'Workyard planner for {title}. Cards are not started from here; run the orchestrator for that.'\n\
claude --append-system-prompt \"$(cat {system})\" {first}\n",
        bin = bin_export_line(std::env::current_exe().ok().as_deref()),
        repo = shell_quote(&repo),
        db = shell_quote(&store.path().to_string_lossy()),
        actor = shell_quote(&format!("agent:planner-{tag}")),
        system = shell_quote(&system.to_string_lossy()),
        first = shell_quote(&first),
        title = name.replace('\'', ""),
    );
    let path = dir.join(format!("workyard-plan-project-{tag}.command"));
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::process::Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"project": id}))
}

#[tauri::command]
fn release_session_hold(card: String) -> Result<Value, String> {
    wrap(store()?.release_task_session_hold("ui", &card))
}


/// Stop an agent without opening a terminal: hold its session, which pauses it, makes the
/// orchestrator stop its worker, and keeps it from being resumed until the hold is released.
#[tauri::command]
fn hold_session(card: String) -> Result<Value, String> {
    let who = std::env::var("USER").unwrap_or_else(|_| "human".into());
    wrap(store()?.hold_task_session("ui", &card, &who))
}

/// Choose how workers bring a card branch onto the target: `merge` or `rebase`.
#[tauri::command]
fn set_board_integration(board: String, integration: String) -> Result<Value, String> {
    wrap(store()?.set_board_integration("ui", &board, &integration))
}

/// Require (or stop requiring) human review of the code changes on a board's cards.
#[tauri::command]
fn set_board_review_required(board: String, required: bool) -> Result<Value, String> {
    wrap(store()?.set_board_review_required("ui", &board, required))
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// What a tool call acted on, in a few words: the command, the file, or the pattern.
fn tool_summary(input: &Value) -> String {
    for field in ["command", "file_path", "path", "pattern", "url", "description", "prompt"] {
        if let Some(text) = input[field].as_str() {
            return clip(text, 160);
        }
    }
    clip(&input.to_string(), 160)
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Turn one stream-json line from a worker's log into readable rows. Anything unrecognised
/// (system events, usage, thinking) is dropped.
fn log_rows(line: &str) -> Vec<Value> {
    let Ok(event) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    let row = |kind: &str, text: String, tool: Option<&str>| {
        let mut row = serde_json::json!({"kind": kind, "text": text});
        if let Some(tool) = tool {
            row["tool"] = Value::String(tool.to_owned());
        }
        row
    };
    let mut rows = Vec::new();
    match event["type"].as_str().unwrap_or_default() {
        "assistant" => {
            for part in event["message"]["content"].as_array().into_iter().flatten() {
                match part["type"].as_str().unwrap_or_default() {
                    "text" => {
                        let text = clip(part["text"].as_str().unwrap_or_default(), 600);
                        if !text.is_empty() {
                            rows.push(row("text", text, None));
                        }
                    }
                    "tool_use" => rows.push(row(
                        "tool",
                        tool_summary(&part["input"]),
                        Some(part["name"].as_str().unwrap_or("tool")),
                    )),
                    _ => {}
                }
            }
        }
        "user" => match &event["message"]["content"] {
            Value::String(text) => rows.push(row("user", clip(text, 300), None)),
            Value::Array(parts) => {
                for part in parts {
                    match part["type"].as_str().unwrap_or_default() {
                        "tool_result" => {
                            let text = clip(&result_text(&part["content"]), 300);
                            if part["is_error"] == true {
                                rows.push(row("error", text, None));
                            } else if !text.is_empty() {
                                rows.push(row("result", text, None));
                            }
                        }
                        "text" => rows.push(row("user", clip(part["text"].as_str().unwrap_or_default(), 300), None)),
                        _ => {}
                    }
                }
            }
            _ => {}
        },
        "result" => {
            let text = clip(event["result"].as_str().unwrap_or_default(), 400);
            if event["is_error"] == true {
                rows.push(row("error", if text.is_empty() { "The worker ended with an error".into() } else { text }, None));
            } else if !text.is_empty() {
                rows.push(row("text", text, None));
            }
        }
        "rate_limit_event" if event["rate_limit_info"]["status"] == "rejected" => {
            rows.push(row("error", "Usage limit reached".into(), None));
        }
        _ => {}
    }
    rows
}

/// The newest worker log for a card: `<db>.logs/<CARD>-<session>.log`.
fn newest_log(store: &Store, card: &str) -> Option<std::path::PathBuf> {
    let mut dir = store.path().as_os_str().to_owned();
    dir.push(".logs");
    let prefix = format!("{card}-");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(&prefix) && name.ends_with(".log")
        })
        .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok())
        .map(|entry| entry.path())
}

/// The last `limit` readable rows of a card's worker log. Only the end of the file is read,
/// since a long run's log is large.
#[tauri::command]
fn agent_log_tail(card: String, limit: usize) -> Result<Value, String> {
    use std::io::{Read, Seek, SeekFrom};
    if card.is_empty() || !card.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("not a card key".into());
    }
    let store = store()?;
    let Some(path) = newest_log(&store, &card.to_uppercase()) else {
        return Ok(Value::Array(Vec::new()));
    };
    const TAIL_BYTES: u64 = 256 * 1024;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // the first line is cut off
    }
    let mut rows: Vec<Value> = lines.flat_map(log_rows).collect();
    let keep = limit.clamp(1, 500);
    if rows.len() > keep {
        rows.drain(..rows.len() - keep);
    }
    Ok(Value::Array(rows))
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
            db_path,
            latest_event_id,
            poll_events,
            list_questions,
            answer_question,
            discuss_question,
            question_thread,
            mark_question_read,
            assign_question_stakeholder,
            unassign_question_stakeholder,
            approve_change,
            open_in_terminal,
            open_planner,
            open_project_planner,
            inspect_repository,
            pick_folder,
            create_project_from_repository,
            start_orchestrator,
            stop_orchestrator,
            release_session_hold,
            hold_session,
            agent_log_tail,
            set_board_review_required,
            set_board_integration,
            orchestrator_status,
            set_orchestrator_state,
            pending_permissions,
            decide_permission
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Workyard");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("workyard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C").arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .status().unwrap().success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn repository_info_reports_branch_and_dirtiness_and_refuses_non_repositories() {
        let dir = temp_dir("repo-info");
        assert!(git_repository_info(&dir.to_string_lossy()).unwrap_err().contains("not a git repository"));
        assert!(git_repository_info("/no/such/folder/here").unwrap_err().contains("not a folder"));
        git(&dir, &["init", "-q", "-b", "trunk"]);
        assert!(git_repository_info(&dir.to_string_lossy()).unwrap_err().contains("no commits"));
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        let info = git_repository_info(&dir.to_string_lossy()).unwrap();
        assert_eq!(info["branch"], "trunk");
        assert_eq!(info["dirty"], false);
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        assert_eq!(git_repository_info(&dir.to_string_lossy()).unwrap()["dirty"], true);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn slugs_and_the_planner_message() {
        assert_eq!(slug("My  Field Notes!"), "my-field-notes");
        assert_eq!(slug("***"), "project");
        let none = project_planner_message("Demo", "p1", "/r", "main", &[]);
        assert!(none.contains("no board yet") && none.contains("attach-board <board> p1") && none.contains("/r") && none.contains("main"));
        let some = project_planner_message("Demo", "p1", "/r", "main", &[("B-1".into(), "Core".into())]);
        assert!(some.contains("- B-1: Core") && some.contains("extend one"));
        assert!(some.ends_with("Do not claim or start any card."));
    }

    #[test]
    fn bin_export_uses_the_bundled_dir_or_falls_back_to_path() {
        let dir = std::env::temp_dir().join(format!("workyard-bin-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app");
        assert!(bin_export_line(Some(&exe)).contains("command -v workyard"));
        std::fs::write(dir.join("workyard"), "").unwrap();
        assert_eq!(bin_export_line(Some(&exe)), format!("export WORKYARD_BIN={}\n", shell_quote(&dir.to_string_lossy())));
        assert!(bin_export_line(None).contains("command -v workyard"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tool_calls_results_and_errors_become_rows() {
        let call = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Running the tests"},{"type":"tool_use","name":"Bash","input":{"command":"node --test"}}]}}"#;
        let rows = log_rows(call);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["kind"], "text");
        assert_eq!(rows[1]["tool"], "Bash");
        assert_eq!(rows[1]["text"], "node --test");

        let denied = r#"{"type":"user","message":{"content":[{"type":"tool_result","is_error":true,"content":"requires approval"}]}}"#;
        assert_eq!(log_rows(denied)[0]["kind"], "error");
        let ok = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"pass 10"}]}]}}"#;
        assert_eq!(log_rows(ok)[0]["text"], "pass 10");
    }

    #[test]
    fn noise_and_garbage_are_dropped_and_long_text_is_clipped() {
        assert!(log_rows(r#"{"type":"system","subtype":"init"}"#).is_empty());
        assert!(log_rows("not json").is_empty());
        let long = format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#, "x".repeat(2000));
        assert!(log_rows(&long)[0]["text"].as_str().unwrap().chars().count() <= 601);
    }
}
