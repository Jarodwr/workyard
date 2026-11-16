use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use workyard_core::{BoardFilter, DEFAULT_LEASE_SECONDS, Error, Store};
use workyard_orchestrator::{ClaudeRunner, MacNotifier, Options, Orchestrator, Silent, hostname};

#[derive(Parser)]
#[command(name = "workyard", about = "Local kanban for work with AI agents")]
struct Cli {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[arg(long, global = true, env = "WORKYARD_ACTOR", default_value = "cli")]
    actor: String,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Work on a card: claim, report progress, ask questions, link code changes.
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Shape projects, boards, phases, and structure.
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    /// Orchestrator and operator controls: sessions and ownership recovery.
    Orch {
        #[command(subcommand)]
        command: OrchCommand,
    },
    /// Human and recovery actions: archive, resolve questions, dependency scope.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    Boards,
    Board {
        board: String,
        #[arg(long)]
        phase: Option<String>,
        #[arg(long)]
        lane: Option<String>,
        #[arg(long)]
        agent_type: Option<String>,
        #[arg(long)]
        blocked: bool,
        #[arg(long)]
        text: Option<String>,
    },
    Card {
        card: String,
    },
    Projects,
    Project {
        project: String,
    },
    Questions {
        board: Option<String>,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        kind: Option<String>,
    },
    Next {
        board: String,
        #[arg(long)]
        agent_type: Option<String>,
    },
    History {
        entity: String,
        #[arg(long, default_value_t = 0)]
        since: i64,
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    Events {
        #[arg(long, default_value_t = 0)]
        since: i64,
        #[arg(long, default_value_t = 100)]
        limit: i64,
        /// Keep running and print each new event as one JSON line, waking on every commit
        #[arg(long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum TaskCommand {
    Claim {
        card: String,
        #[arg(long)]
        expected_version: i64,
    },
    Release {
        card: String,
        #[arg(long, help = "Required unless you own the card")]
        expected_version: Option<i64>,
    },
    Move {
        card: String,
        lane: String,
        #[arg(long)]
        position: Option<i64>,
        #[arg(long, help = "Required unless you own the card")]
        expected_version: Option<i64>,
    },
    Progress {
        card: String,
        progress: i64,
        #[arg(long)]
        note: Option<String>,
        #[arg(long, help = "Required unless you own the card")]
        expected_version: Option<i64>,
    },
    Note {
        card: String,
        body: String,
    },
    Update {
        card: String,
        #[arg(long, help = "Required unless you own the card")]
        expected_version: Option<i64>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long)]
        agent_type: Option<String>,
    },
    AddCard {
        board: String,
        title: String,
        #[arg(long)]
        phase: Option<String>,
        #[arg(long, default_value = "")]
        desc: String,
        #[arg(long)]
        agent_type: Option<String>,
        #[arg(long)]
        lane: Option<String>,
        /// worktree (default: its own git worktree and branch) or none (investigative work)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Choose where a card's worker runs: worktree (default) or none for investigations.
    Workspace {
        card: String,
        workspace: String,
        #[arg(long, help = "Required unless you own the card")]
        expected_version: Option<i64>,
    },
    Depend {
        card: String,
        #[arg(long)]
        on: String,
        #[arg(long, default_value = "start")]
        scope: String,
        #[arg(long)]
        expected_version: i64,
    },
    Undepend {
        card: String,
        #[arg(long)]
        on: String,
        #[arg(long)]
        expected_version: i64,
    },
    Ask {
        board: String,
        title: String,
        #[arg(long)]
        phase: Option<String>,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "")]
        body: String,
        #[arg(
            long = "option",
            required = true,
            help = "Substantive choice; repeat 2-10 times. Other is added automatically"
        )]
        options: Vec<String>,
        #[arg(long)]
        recommend: String,
        #[arg(long)]
        why: String,
        #[arg(long, value_delimiter = ',')]
        blocks: Vec<String>,
    },
    QuestionOptions {
        question: String,
        #[arg(long)]
        expected_version: i64,
        #[arg(
            long = "option",
            required = true,
            help = "Substantive choice; repeat 2-10 times. Other is added automatically"
        )]
        options: Vec<String>,
        #[arg(long)]
        recommend: String,
        #[arg(long)]
        why: String,
    },
    LinkChange {
        card: String,
        repository: String,
        branch: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value = "")]
        commit: String,
        #[arg(long)]
        review_required: bool,
        #[arg(long)]
        expected_version: i64,
    },
    UpdateChange {
        change: String,
        commit: String,
        #[arg(long)]
        expected_version: i64,
    },
    RequireReview {
        change: String,
        #[arg(long)]
        required: bool,
        #[arg(long)]
        expected_version: i64,
        #[arg(long)]
        expected_card_version: i64,
    },
}

#[derive(Subcommand)]
enum PlanCommand {
    CreateProject {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    CreateBoard {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    AttachBoard {
        board: String,
        project: String,
        #[arg(long)]
        expected_version: i64,
    },
    AddRepository {
        project: String,
        path: String,
        #[arg(long, default_value = "main")]
        target_branch: String,
        #[arg(long)]
        remote_url: Option<String>,
    },
    AddDocument {
        project: String,
        repository: String,
        path: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        title: String,
    },
    AddStakeholder {
        project: String,
        name: String,
        #[arg(long, default_value = "")]
        role: String,
        #[arg(long, default_value = "")]
        contact: String,
    },
    AssignStakeholder {
        question: String,
        stakeholder: String,
        #[arg(long)]
        expected_version: i64,
    },
    UnassignStakeholder {
        question: String,
        #[arg(long)]
        expected_version: i64,
    },
    Phase {
        #[command(subcommand)]
        command: PhaseCommand,
    },
    Cancel {
        card: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expected_version: i64,
    },
}

#[derive(Subcommand)]
enum OrchCommand {
    /// Run the orchestrator for a board in the foreground until Ctrl-C. Only one may hold
    /// a board; a second fails with orchestrator_conflict.
    Run {
        board: String,
        /// Maximum concurrent workers (kept on the board across restarts)
        #[arg(long)]
        pool: Option<i64>,
        /// Claude Code binary used to run workers headless
        #[arg(long, default_value = "claude")]
        claude_bin: String,
        /// Extra argument for every worker (repeatable); replaces the defaults when given
        #[arg(long = "claude-arg", allow_hyphen_values = true)]
        claude_args: Vec<String>,
        /// Directory workers run in, usually the project's repository
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Consecutive failed runs before a card's session is detached
        #[arg(long, default_value_t = 3)]
        max_failures: u32,
        /// Do not send desktop notifications
        #[arg(long)]
        no_notify: bool,
    },
    /// Show the board's orchestrator: state, cursor, pool size, and lease.
    Status { board: String },
    /// Stop dispatching new work. Running workers continue.
    Pause { board: String },
    /// Resume dispatching.
    Resume { board: String },
    /// Dispatch nothing new and let running workers finish.
    Drain { board: String },
    /// Set how many workers may run at once.
    Pool { board: String, size: i64 },
    /// Stop dispatching until a time (use the orchestrator actor); omit --until to clear.
    Backoff {
        board: String,
        /// RFC 3339 time, for example 2026-11-16T14:20:00+00:00
        #[arg(long)]
        until: Option<String>,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Take the board's lease. Fails with orchestrator_conflict while another holds it.
    /// Prints the actor (orch:<board>#<epoch>) to use for every later orchestrator command.
    Acquire {
        board: String,
        #[arg(long, default_value = "orchestrator")]
        holder: String,
        #[arg(long)]
        pid: Option<i64>,
        #[arg(long, default_value_t = DEFAULT_LEASE_SECONDS)]
        lease: i64,
        #[arg(long)]
        pool: Option<i64>,
    },
    /// Renew the lease (use the orchestrator actor). The reply carries the desired state.
    Heartbeat {
        board: String,
        #[arg(long, default_value_t = DEFAULT_LEASE_SECONDS)]
        lease: i64,
    },
    /// Release the lease (use the orchestrator actor).
    Release { board: String },
    /// Record the last event fully processed (use the orchestrator actor).
    Cursor { board: String, to: i64 },
    /// Pick work and start it atomically (use the orchestrator actor).
    Dispatch {
        board: String,
        #[arg(long)]
        worker: String,
        #[arg(long)]
        runner: String,
        #[arg(long)]
        session_id: String,
        #[arg(long)]
        agent_type: Option<String>,
    },
    Transfer {
        card: String,
        #[arg(long)]
        to_actor: String,
        #[arg(long)]
        expected_version: i64,
    },
    ForceRelease {
        card: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expected_version: i64,
    },
    /// Manage the agent session attached to an In Progress task.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    ArchiveBoard {
        board: String,
        #[arg(long)]
        expected_version: i64,
    },
    UnarchiveBoard {
        board: String,
        #[arg(long)]
        expected_version: i64,
    },
    #[command(about = "Archive an empty legacy Review or custom lane during cleanup")]
    ArchiveLane {
        lane: String,
        #[arg(long)]
        expected_version: i64,
    },
    ScopeDependency {
        dependency: String,
        scope: String,
        #[arg(long)]
        expected_version: i64,
    },
    Answer {
        question: String,
        answer: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        expected_version: i64,
    },
    Drop {
        question: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expected_version: i64,
    },
}

#[derive(Subcommand)]
enum PhaseCommand {
    List {
        board: String,
    },
    Add {
        board: String,
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Describe {
        board: String,
        phase: String,
        description: String,
        #[arg(long)]
        expected_version: i64,
    },
    Complete {
        board: String,
    },
    Advance {
        board: String,
    },
    Assign {
        item: String,
        phase: String,
        #[arg(long)]
        expected_version: i64,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    Attach {
        card: String,
        #[arg(long)]
        runner: String,
        #[arg(long)]
        session_id: String,
    },
    Pause {
        card: String,
        #[arg(long)]
        waiting_question: Option<String>,
    },
    Resume { card: String },
    /// Take the session over (for example in a terminal). It stays paused and is never offered
    /// for resume, and the orchestrator stops its worker, until you release the hold.
    Hold {
        card: String,
        #[arg(long, default_value = "human")]
        by: String,
    },
    /// Give a held session back; the orchestrator resumes it with everything it missed.
    ReleaseHold { card: String },
    Ack {
        card: String,
        #[arg(long)]
        through_event_id: i64,
    },
    Detach { card: String },
    Briefing {
        card: String,
        #[arg(long, default_value_t = 100)]
        limit: i64,
        #[arg(long)]
        after: Option<i64>,
    },
}

fn run(store: &Store, actor: &str, command: Command) -> workyard_core::Result<Value> {
    Ok(match command {
        Command::Task { command } => run_task(store, actor, command)?,
        Command::Plan { command } => run_plan(store, actor, command)?,
        Command::Orch { command } => run_orch(store, actor, command)?,
        Command::Admin { command } => run_admin(store, actor, command)?,
        Command::Boards => store.list_boards()?,
        Command::Board {
            board,
            phase,
            lane,
            agent_type,
            blocked,
            text,
        } => store.view_board_filtered(
            &board,
            &BoardFilter {
                lane,
                phase,
                agent_type,
                blocked_only: blocked,
                text,
            },
        )?,
        Command::Card { card } => store.view_card(&card)?,
        Command::Projects => store.list_projects()?,
        Command::Project { project } => store.view_project(&project)?,
        Command::Questions { board, open, kind } => store.list_questions_filtered(
            board.as_deref(),
            if open { Some("open") } else { None },
            kind.as_deref(),
        )?,
        Command::Next { board, agent_type } => {
            store.next_ready_task(&board, agent_type.as_deref())?
        }
        Command::History {
            entity,
            since,
            limit,
        } => store.view_history(&entity, since, limit)?,
        Command::Events { since, limit, .. } => store.poll_events(since, limit)?,

    })
}

fn run_task(store: &Store, actor: &str, command: TaskCommand) -> workyard_core::Result<Value> {
    Ok(match command {
        TaskCommand::Claim {
            card,
            expected_version,
        } => store.claim_card(actor, &card, expected_version)?,
        TaskCommand::Release {
            card,
            expected_version,
        } => store.release_card(actor, &card, expected_version, None)?,
        TaskCommand::Move {
            card,
            lane,
            position,
            expected_version,
        } => store.move_card(actor, &card, &lane, position, expected_version)?,
        TaskCommand::Progress {
            card,
            progress,
            note,
            expected_version,
        } => store.update_progress(actor, &card, progress, note.as_deref(), expected_version)?,
        TaskCommand::Note { card, body } => store.add_card_note(actor, &card, &body)?,
        TaskCommand::Update {
            card,
            expected_version,
            title,
            desc,
            agent_type,
        } => store.update_card(
            actor,
            &card,
            expected_version,
            title.as_deref(),
            desc.as_deref(),
            None,
            agent_type.as_deref(),
        )?,
        TaskCommand::AddCard {
            board,
            title,
            phase,
            desc,
            agent_type,
            lane,
            workspace,
        } => {
            let card = store.create_card_in_phase(
                actor,
                &board,
                &title,
                &desc,
                "P2",
                lane.as_deref(),
                agent_type.as_deref().unwrap_or(""),
                phase.as_deref(),
            )?;
            match workspace {
                Some(workspace) => store.set_card_workspace(
                    actor,
                    card["key"].as_str().unwrap_or_default(),
                    &workspace,
                    card["version"].as_i64(),
                )?,
                None => card,
            }
        }
        TaskCommand::Workspace {
            card,
            workspace,
            expected_version,
        } => store.set_card_workspace(actor, &card, &workspace, expected_version)?,
        TaskCommand::Depend {
            card,
            on,
            scope,
            expected_version,
        } => store.add_dependency_scoped(actor, &card, &on, &scope, expected_version)?,
        TaskCommand::Undepend {
            card,
            on,
            expected_version,
        } => store.remove_dependency(actor, &card, &on, expected_version)?,
        TaskCommand::Ask {
            board,
            title,
            phase,
            kind,
            body,
            options,
            recommend,
            why,
            blocks,
        } => store.raise_question_in_phase(
            actor,
            &board,
            &kind,
            &title,
            &body,
            &options,
            &recommend,
            &why,
            &blocks,
            phase.as_deref(),
        )?,
        TaskCommand::QuestionOptions {
            question,
            expected_version,
            options,
            recommend,
            why,
        } => store.update_question_options(
            actor,
            &question,
            expected_version,
            &options,
            &recommend,
            &why,
        )?,
        TaskCommand::LinkChange {
            card,
            repository,
            branch,
            target,
            commit,
            review_required,
            expected_version,
        } => store.link_card_change(
            actor,
            &card,
            &repository,
            &branch,
            target.as_deref(),
            &commit,
            review_required,
            expected_version,
        )?,
        TaskCommand::UpdateChange {
            change,
            commit,
            expected_version,
        } => store.update_card_change(actor, &change, &commit, expected_version)?,
        TaskCommand::RequireReview {
            change,
            required,
            expected_version,
            expected_card_version,
        } => store.set_change_review_required(
            actor,
            &change,
            required,
            expected_version,
            expected_card_version,
        )?,
    })
}

fn run_plan(store: &Store, actor: &str, command: PlanCommand) -> workyard_core::Result<Value> {
    Ok(match command {
        PlanCommand::CreateProject { name, description } => {
            store.create_project(actor, &name, &description)?
        }
        PlanCommand::CreateBoard { name, description } => {
            store.create_board(actor, &name, &description)?
        }
        PlanCommand::AttachBoard {
            board,
            project,
            expected_version,
        } => store.attach_board_to_project(actor, &board, &project, expected_version)?,
        PlanCommand::AddRepository {
            project,
            path,
            target_branch,
            remote_url,
        } => store.add_project_repository(
            actor,
            &project,
            &path,
            &target_branch,
            remote_url.as_deref(),
        )?,
        PlanCommand::AddDocument {
            project,
            repository,
            path,
            kind,
            title,
        } => store.add_project_document(actor, &project, &repository, &path, &kind, &title)?,
        PlanCommand::AddStakeholder {
            project,
            name,
            role,
            contact,
        } => store.add_project_stakeholder(actor, &project, &name, &role, &contact)?,
        PlanCommand::AssignStakeholder {
            question,
            stakeholder,
            expected_version,
        } => store.assign_question_stakeholder(actor, &question, &stakeholder, expected_version)?,
        PlanCommand::UnassignStakeholder {
            question,
            expected_version,
        } => store.unassign_question_stakeholder(actor, &question, expected_version)?,
        PlanCommand::Phase { command } => match command {
            PhaseCommand::List { board } => store.list_phases(&board)?,
            PhaseCommand::Add { board, name, description } => store.add_phase_with_description(actor, &board, &name, &description)?,
            PhaseCommand::Describe { board, phase, description, expected_version } => store.describe_phase(actor, &board, &phase, &description, expected_version)?,
            PhaseCommand::Complete { board } => store.complete_phase(actor, &board)?,
            PhaseCommand::Advance { board } => store.advance_phase(actor, &board)?,
            PhaseCommand::Assign {
                item,
                phase,
                expected_version,
            } => store.assign_phase(actor, &item, &phase, expected_version)?,
        },
        PlanCommand::Cancel {
            card,
            reason,
            expected_version,
        } => store.cancel_card(actor, &card, &reason, expected_version)?,
    })
}

fn run_orch(store: &Store, actor: &str, command: OrchCommand) -> workyard_core::Result<Value> {
    Ok(match command {
        OrchCommand::Run {
            board,
            pool,
            claude_bin,
            claude_args,
            cwd,
            max_failures,
            no_notify,
        } => run_orchestrator(store, &board, pool, claude_bin, claude_args, cwd, max_failures, !no_notify)?,
        OrchCommand::Status { board } => store.orchestrator_status(&board)?,
        OrchCommand::Pause { board } => store.orchestrator_set_state(actor, &board, "paused")?,
        OrchCommand::Resume { board } => store.orchestrator_set_state(actor, &board, "running")?,
        OrchCommand::Drain { board } => store.orchestrator_set_state(actor, &board, "draining")?,
        OrchCommand::Backoff { board, until, reason } => {
            store.orchestrator_set_backoff(actor, &board, until.as_deref(), &reason)?
        }
        OrchCommand::Pool { board, size } => store.orchestrator_set_pool(actor, &board, size)?,
        OrchCommand::Acquire {
            board,
            holder,
            pid,
            lease,
            pool,
        } => store.orchestrator_acquire(&board, &holder, &hostname(), pid, lease, pool)?,
        OrchCommand::Heartbeat { board, lease } => {
            store.orchestrator_heartbeat(actor, &board, lease)?
        }
        OrchCommand::Release { board } => store.orchestrator_release(actor, &board)?,
        OrchCommand::Cursor { board, to } => store.orchestrator_set_cursor(actor, &board, to)?,
        OrchCommand::Dispatch {
            board,
            worker,
            runner,
            session_id,
            agent_type,
        } => store.orchestrator_dispatch(
            actor,
            &board,
            &worker,
            &runner,
            &session_id,
            agent_type.as_deref(),
        )?,
        OrchCommand::Transfer {
            card,
            to_actor,
            expected_version,
        } => store.transfer_card(actor, &card, expected_version, &to_actor)?,
        OrchCommand::ForceRelease {
            card,
            reason,
            expected_version,
        } => store.release_card(actor, &card, expected_version, Some(&reason))?,
        OrchCommand::Session { command } => match command {
            SessionCommand::Attach { card, runner, session_id } => store.attach_task_session(actor, &card, &runner, &session_id)?,
            SessionCommand::Pause { card, waiting_question } => store.pause_task_session(actor, &card, waiting_question.as_deref())?,
            SessionCommand::Resume { card } => store.resume_task_session(actor, &card)?,
            SessionCommand::Hold { card, by } => store.hold_task_session(actor, &card, &by)?,
            SessionCommand::ReleaseHold { card } => store.release_task_session_hold(actor, &card)?,
            SessionCommand::Ack { card, through_event_id } => store.acknowledge_task_session(actor, &card, through_event_id)?,
            SessionCommand::Detach { card } => store.detach_task_session(actor, &card)?,
            SessionCommand::Briefing { card, limit, after } => store.task_session_briefing(&card, limit, after)?,
        },
    })
}

fn run_admin(store: &Store, actor: &str, command: AdminCommand) -> workyard_core::Result<Value> {
    Ok(match command {
        AdminCommand::ArchiveBoard {
            board,
            expected_version,
        } => store.set_board_archived(actor, &board, expected_version, true)?,
        AdminCommand::UnarchiveBoard {
            board,
            expected_version,
        } => store.set_board_archived(actor, &board, expected_version, false)?,
        AdminCommand::ArchiveLane {
            lane,
            expected_version,
        } => store.set_lane_archived(actor, &lane, expected_version, true)?,
        AdminCommand::ScopeDependency {
            dependency,
            scope,
            expected_version,
        } => store.set_dependency_scope(actor, &dependency, &scope, expected_version)?,
        AdminCommand::Answer {
            question,
            answer,
            note,
            expected_version,
        } => store.resolve_question(
            actor,
            &question,
            &answer,
            note.as_deref(),
            expected_version,
            false,
        )?,
        AdminCommand::Drop {
            question,
            reason,
            expected_version,
        } => store.resolve_question(actor, &question, &reason, None, expected_version, true)?,
    })
}

/// Claude Code arguments for headless workers. Workers may run the `workyard` CLI and edit
/// files; anything else needs the user to pass their own `--claude-arg` set.
fn default_claude_args() -> Vec<String> {
    ["--permission-mode", "acceptEdits", "--allowedTools", "Bash(workyard:*)"]
        .map(String::from)
        .to_vec()
}

fn run_orchestrator(
    store: &Store,
    board: &str,
    pool: Option<i64>,
    claude_bin: String,
    claude_args: Vec<String>,
    cwd: Option<PathBuf>,
    max_failures: u32,
    notify: bool,
) -> workyard_core::Result<Value> {
    let db = store.path().to_path_buf();
    let mut log_dir = db.clone().into_os_string();
    log_dir.push(".logs");
    let runner = ClaudeRunner {
        bin: claude_bin,
        args: if claude_args.is_empty() { default_claude_args() } else { claude_args },
        db,
        cwd,
        log_dir: PathBuf::from(log_dir),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    if let Err(error) = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst)) {
        eprintln!("warning: Ctrl-C will not stop workers or release the lease cleanly: {error}");
    }
    let options = Options {
        holder: format!("cli:{}", std::process::id()),
        pool,
        max_failures,
        ..Options::default()
    };
    eprintln!("Orchestrating {board}. Press Ctrl-C to stop; workers are paused and the lease released.");
    let notifier: Box<dyn workyard_orchestrator::Notifier> =
        if notify { Box::new(MacNotifier) } else { Box::new(Silent) };
    Orchestrator::new(store, board, Box::new(runner), options)
        .with_notifier(notifier)
        .run(&stop)?;
    Ok(json!({"stopped": true}))
}

/// Print each new event as one JSON line, waking on every commit (with a 2s safety poll).
/// The marker is read before the query, so a write landing in between is never missed.
fn follow_events(store: &Store, mut cursor: i64, limit: i64) -> workyard_core::Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    loop {
        let marker = store.tick_marker();
        let page = store.poll_events(cursor, limit)?;
        for event in page["events"].as_array().into_iter().flatten() {
            if writeln!(out, "{event}").and_then(|_| out.flush()).is_err() {
                return Ok(());
            }
        }
        cursor = page["cursor"].as_i64().unwrap_or(cursor);
        if page["has_more"] != true {
            store.wait_for_tick(&marker, std::time::Duration::from_secs(2));
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if let Command::Events {
        since,
        limit,
        follow: true,
    } = &cli.command
    {
        let path = match &cli.db {
            Some(path) => Ok(path.clone()),
            None => Store::default_path(),
        };
        if let Err(error) = path.and_then(|p| follow_events(&Store::new(p), *since, *limit)) {
            eprintln!("{error}");
            std::process::exit(exit_code(&error));
        }
        return;
    }
    let path = match cli.db {
        Some(path) => Ok(path),
        None => Store::default_path(),
    };
    let result = path.and_then(|path| run(&Store::new(path), &cli.actor, cli.command));
    match result {
        Ok(data) => {
            if cli.json {
                println!("{}", json!({"ok":true,"data":data}));
            } else {
                println!("{}", serde_json::to_string_pretty(&data).unwrap());
            }
        }
        Err(error) => {
            let code = exit_code(&error);
            if cli.json {
                println!("{}", json!({"ok":false,"error":error}));
            } else {
                eprintln!("{}", error);
                if let Some(hint) = &error.hint {
                    eprintln!("Hint: {hint}");
                }
            }
            std::process::exit(code);
        }
    }
}

fn exit_code(error: &Error) -> i32 {
    match error.code.as_str() {
        "version_conflict" | "ownership_conflict" => 3,
        _ => 2,
    }
}
