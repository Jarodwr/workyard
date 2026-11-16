use std::path::PathBuf;

mod doctor;
mod watch;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use workyard_core::{BoardFilter, DEFAULT_LEASE_SECONDS, Error, Store};
use workyard_orchestrator::{ClaudeRunner, MacNotifier, Options, Orchestrator, Silent, hostname};

#[derive(Parser)]
#[command(name = "workyard", version, about = "Local kanban for work with AI agents")]
struct Cli {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[arg(long, global = true, env = "WORKYARD_ACTOR", default_value = "cli")]
    actor: String,
    #[arg(long, global = true)]
    json: bool,
    /// Migrate the database even while an orchestrator started by an older schema is running
    /// (also WORKYARD_ALLOW_MIGRATE=1)
    #[arg(long, global = true)]
    allow_migrate: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check this installation: version, database, claude and git, the plugin, the orchestrator.
    /// Exits 1 when a check FAILs.
    Doctor,
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
    /// One-screen board summary: lanes, progress, who is running, open questions, orchestrator.
    Status { board: String },
    /// Block until a board needs attention or finishes; prints why and exits 0, or exits 1 on timeout.
    Wait {
        board: String,
        /// attention, done, or <n>-done (n more cards finished than when the wait began)
        #[arg(long, default_value = "attention")]
        until: String,
        /// Give up after this long, e.g. 90s, 30m, 2h (default: wait forever)
        #[arg(long)]
        timeout: Option<String>,
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
    #[command(about = "Store a markdown report on the card, replacing the previous one (max 256 KB)")]
    Report {
        card: String,
        #[arg(long, help = "Read the report from this file; without it, from stdin")]
        file: Option<PathBuf>,
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
        /// Require human approval of this change (the default follows the board's setting)
        #[arg(long, conflicts_with = "no_review")]
        review_required: bool,
        /// Do not require approval even if the board does
        #[arg(long)]
        no_review: bool,
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

#[derive(Clone, Copy, clap::ValueEnum)]
enum ReviewSetting {
    On,
    Off,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum IntegrationSetting {
    Merge,
    Rebase,
}

/// How much workers may do without asking.
#[derive(Clone, Copy, clap::ValueEnum)]
enum Permissions {
    /// Permission checks off: workers run anything. For local projects you trust.
    Full,
    /// Edits and an allowlist of commands only; anything else is denied.
    Safe,
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
    /// Turn human review of commits on or off for a board (off by default). Changes linked
    /// afterwards need approval in the desktop when it is on.
    BoardReview {
        board: String,
        /// `on` or `off`
        setting: ReviewSetting,
    },
    /// How many phases past the current one workers may be dispatched from: a number, or
    /// `all` (the default). 1 offers the next phase only once the current one has no ready work.
    BoardPhaseWindow { board: String, window: String },
    /// Set environment variables every worker of the board starts with (KEY=VALUE, repeatable
    /// arguments), for example `CARGO_TARGET_DIR=/tmp/shared-target`. Stored in the database in
    /// plain text: do not put secrets here. `orch run --worker-env` overrides them per run.
    BoardWorkerEnv {
        board: String,
        /// KEY=VALUE pairs to set
        pairs: Vec<String>,
        /// Remove a variable (repeatable)
        #[arg(long)]
        unset: Vec<String>,
    },
    /// How workers bring a card branch onto the target: `merge` the target into it (default;
    /// leaves a merge commit) or `rebase` it onto the target (linear history).
    BoardIntegration { board: String, setting: IntegrationSetting },
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
        /// Maximum concurrent workers, kept on the board across restarts (default 3 until you choose)
        #[arg(long)]
        pool: Option<i64>,
        /// Claude Code binary used to run workers headless
        #[arg(long, default_value = "claude")]
        claude_bin: String,
        /// Extra argument for every worker (repeatable); replaces the permission defaults when given
        #[arg(long = "claude-arg", allow_hyphen_values = true)]
        claude_args: Vec<String>,
        /// How much workers may do without asking: `full` turns permission checks off (the
        /// default, for local projects), `safe` allows edits and an allowlist of commands
        #[arg(long, value_enum, default_value = "full")]
        permissions: Permissions,
        /// With `--permissions safe`, one more allowed tool pattern, for example `Bash(make:*)`
        #[arg(long = "allow")]
        allow: Vec<String>,
        /// Also run a supervisor agent that is told what needs judgement and acts through the CLI
        /// (costs tokens while it works)
        #[arg(long)]
        supervisor: bool,
        /// The Workyard Claude plugin workers load (its skill and actor hook). Found next to the
        /// app or in a checkout when omitted; `WORKYARD_PLUGIN_DIR` also works
        #[arg(long)]
        plugin_dir: Option<PathBuf>,
        /// Directory workers run in, usually the project's repository
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Consecutive failed runs before a card's session is detached
        #[arg(long, default_value_t = 3)]
        max_failures: u32,
        /// Do not send desktop notifications
        #[arg(long)]
        no_notify: bool,
        /// Environment variable for every worker, KEY=VALUE (repeatable); overrides the board's
        /// defaults from `plan board-worker-env`. Only the names show in `orch status`. Not for secrets
        #[arg(long = "worker-env")]
        worker_env: Vec<String>,
        /// Keep each card's worktree and branch after it is Done and merged (they are removed
        /// by default)
        #[arg(long)]
        keep_worktrees: bool,
    },
    /// Restart a board's orchestrator on the current binary: drain, wait for workers to pause or
    /// finish, release the lease, then replace this process with `orch run <board> <args>`.
    Restart {
        board: String,
        /// Seconds to wait for workers before giving up
        #[arg(long, default_value_t = 600)]
        timeout: u64,
        /// Arguments for `orch run`, after `--` (for example `-- --pool 3 --cwd .`)
        #[arg(last = true, allow_hyphen_values = true)]
        run_args: Vec<String>,
    },
    /// Show the board's orchestrator: state, cursor, pool size, and lease.
    Status { board: String },
    /// Stop dispatching new work. Running workers continue.
    Pause { board: String },
    /// Resume dispatching.
    Resume { board: String },
    /// Dispatch nothing new and let running workers finish.
    Drain { board: String },
    /// Set how many workers may run at once (a board starts at 3).
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
    /// Tools workers asked to use in `--permissions safe`: list them and decide them. Deciding is
    /// for a human; the supervisor and workers are refused.
    Perms {
        #[command(subcommand)]
        command: PermsCommand,
    },
}

#[derive(Subcommand)]
enum PermsCommand {
    /// List requests on a board, newest first.
    List {
        board: String,
        /// Only requests in this status: pending, allowed_once, allowed_run, denied or expired
        #[arg(long)]
        status: Option<String>,
    },
    /// Decide a pending request. The worker carries on at once, without a restart.
    Decide {
        /// The request's id (from `orch perms list`)
        id: String,
        #[arg(value_enum)]
        decision: PermsDecision,
        /// With allow-run, the rule to grant for the rest of the run instead of the default
        /// (a Bash command's first word, `Bash(touch *)`; any other tool by name)
        #[arg(long)]
        rule: Option<String>,
        /// Shown to the worker when denying
        #[arg(long, default_value = "")]
        reason: String,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum PermsDecision {
    AllowOnce,
    AllowRun,
    Deny,
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
        /// Wait until a human approves your commit (and any finish dependency is met)
        #[arg(long)]
        for_review: bool,
        /// Wait until your changes are on their target branch (someone else merges them)
        #[arg(long)]
        for_integration: bool,
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
        Command::Doctor => unreachable!("handled in main"),
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
        Command::Status { board } => watch::snapshot(store, &board)?,
        Command::Wait { .. } => unreachable!("handled in main"),
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
        TaskCommand::Report { card, file } => {
            let body = match file {
                Some(path) => std::fs::read_to_string(&path).map_err(|e| {
                    Error::new("validation_error", format!("cannot read {}: {e}", path.display()))
                })?,
                None => {
                    let mut text = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                        .map_err(|e| Error::new("validation_error", format!("cannot read stdin: {e}")))?;
                    text
                }
            };
            store.set_card_report(actor, &card, &body)?
        }
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
            no_review,
            expected_version,
        } => store.link_card_change(
            actor,
            &card,
            &repository,
            &branch,
            target.as_deref(),
            &commit,
            if review_required { Some(true) } else if no_review { Some(false) } else { None },
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
        PlanCommand::BoardReview { board, setting } => {
            store.set_board_review_required(actor, &board, matches!(setting, ReviewSetting::On))?
        }
        PlanCommand::BoardIntegration { board, setting } => {
            let setting = match setting {
                IntegrationSetting::Merge => "merge",
                IntegrationSetting::Rebase => "rebase",
            };
            store.set_board_integration(actor, &board, setting)?
        }
        PlanCommand::BoardPhaseWindow { board, window } => {
            let window = workyard_core::parse_phase_window(&window)?;
            store.set_board_phase_window(actor, &board, window)?
        }
        PlanCommand::BoardWorkerEnv { board, pairs, unset } => {
            let pairs = pairs.iter().map(|p| workyard_core::parse_env_pair(p)).collect::<workyard_core::Result<Vec<_>>>()?;
            store.set_board_worker_env(actor, &board, &pairs, &unset)?
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

/// Drain, wait for the board's workers to pause or finish, release the lease the old
/// orchestrator holds, then re-exec this binary as `orch run`. Returns only on failure.
fn restart_orchestrator(store: &Store, actor: &str, board: &str, timeout: u64, run_args: &[String]) -> workyard_core::Result<Value> {
    store.orchestrator_set_state(actor, board, "draining")?;
    eprintln!("Draining {board}; waiting up to {timeout}s for workers to pause or finish.");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    while store.orchestrator_active_workers(board)? > 0 {
        if std::time::Instant::now() >= deadline {
            return Err(Error::new("restart_timeout", format!("workers on {board} did not pause or finish within {timeout}s"))
                .hint("The board is left draining. Retry with a longer --timeout, or `workyard orch resume` to undo"));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let status = store.orchestrator_status(board)?;
    if status["connected"] == true {
        let holder = format!("orch:{}#{}", status["board_key"].as_str().unwrap_or(board), status["epoch"]);
        store.orchestrator_release(&holder, board)?;
        eprintln!("Released the lease held by {}.", status["holder"].as_str().unwrap_or("the old orchestrator"));
    }
    store.orchestrator_set_state(actor, board, "running")?;
    let exe = std::env::current_exe().map_err(|e| Error::new("restart_failed", format!("cannot find this binary: {e}")))?;
    let mut command = std::process::Command::new(exe);
    if let Some(db) = std::env::args().skip_while(|a| a != "--db").nth(1) {
        command.args(["--db", &db]);
    }
    command.args(["orch", "run", board]).args(run_args);
    eprintln!("Starting the orchestrator on this binary.");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(Error::new("restart_failed", format!("could not re-exec: {}", command.exec())))
    }
    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|e| Error::new("restart_failed", e.to_string()))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn run_orch(store: &Store, actor: &str, command: OrchCommand) -> workyard_core::Result<Value> {
    Ok(match command {
        OrchCommand::Run {
            board,
            pool,
            claude_bin,
            claude_args,
            permissions,
            allow,
            supervisor,
            plugin_dir,
            cwd,
            max_failures,
            no_notify,
            worker_env,
            keep_worktrees,
        } => {
            let worker_env = worker_env.iter().map(|p| workyard_core::parse_env_pair(p)).collect::<workyard_core::Result<_>>()?;
            run_orchestrator(store, &board, pool, claude_bin, claude_args, permissions, allow, supervisor, plugin_dir, cwd, max_failures, !no_notify, keep_worktrees, worker_env)?
        }
        OrchCommand::Restart { board, timeout, run_args } => restart_orchestrator(store, actor, &board, timeout, &run_args)?,
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
        OrchCommand::Perms { command } => match command {
            PermsCommand::List { board, status } => store.permission_requests(&board, status.as_deref())?,
            PermsCommand::Decide { id, decision, rule, reason } => {
                let decision = match decision {
                    PermsDecision::AllowOnce => "allow_once",
                    PermsDecision::AllowRun => "allow_run",
                    PermsDecision::Deny => "deny",
                };
                store.permission_decide(actor, &id, decision, rule.as_deref(), &reason)?
            }
        },
        OrchCommand::Session { command } => match command {
            SessionCommand::Attach { card, runner, session_id } => store.attach_task_session(actor, &card, &runner, &session_id)?,
            SessionCommand::Pause { card, waiting_question, for_review, for_integration } => {
                store.pause_task_session_waiting(actor, &card, waiting_question.as_deref(), for_review, for_integration)?
            }
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

/// Tools workers may use in `safe` mode besides editing files.
const SAFE_COMMANDS: &[&str] = &[
    "workyard", "git", "node", "npm", "npx", "pnpm", "yarn", "ls", "cat", "mkdir", "pwd", "cargo", "python3",
];

/// Claude Code arguments for headless workers. `full` turns permission checks off; `safe`
/// allows file edits and an allowlist of commands, plus whatever `--allow` adds.
fn permission_args(permissions: Permissions, allow: &[String]) -> Vec<String> {
    match permissions {
        Permissions::Full => vec!["--dangerously-skip-permissions".into()],
        Permissions::Safe => {
            // `--permission-prompt-tool stdio` (hidden in --help) is what makes Claude Code ask the
            // host, over stdin/stdout, instead of refusing; `--permission-prompts host` alone is not
            // enough. The orchestrator answers each request once a human has decided it.
            let mut args: Vec<String> = ["--permission-mode", "acceptEdits", "--permission-prompts", "host", "--permission-prompt-tool", "stdio", "--allowedTools"]
                .map(String::from)
                .into();
            args.extend(SAFE_COMMANDS.iter().map(|c| format!("Bash({c}:*)")));
            args.extend(allow.iter().cloned());
            args
        }
    }
}

/// Where the Workyard plugin is: `WORKYARD_PLUGIN_DIR`, the app bundle's resources, a Homebrew
/// layout (`<prefix>/share/workyard/plugin`), or a checkout above the running binary
/// (`target/debug/workyard` inside the repository).
fn find_plugin_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("WORKYARD_PLUGIN_DIR") {
        return Some(PathBuf::from(dir));
    }
    plugin_dir_for_exe(&std::env::current_exe().ok()?)
}

/// The plugin directory for a given executable path. Homebrew links `bin/workyard` into the
/// prefix, so the real path (symlinks resolved) is tried first, then the path as given.
fn plugin_dir_for_exe(exe: &std::path::Path) -> Option<PathBuf> {
    let is_plugin = |dir: &PathBuf| dir.join(".claude-plugin/plugin.json").exists();
    let real = exe.canonicalize().ok();
    for exe in real.iter().map(|p| p.as_path()).chain([exe]) {
        let dir = exe.parent()?;
        let bundled = dir.parent()?.join("Resources/marketplace/plugin");
        let brew = dir.join("../share/workyard/plugin");
        for candidate in [bundled, brew] {
            if is_plugin(&candidate) {
                return Some(candidate.canonicalize().unwrap_or(candidate));
            }
        }
        if let Some(found) = exe.ancestors().map(|dir| dir.join("plugin")).find(is_plugin) {
            return Some(found);
        }
    }
    None
}

/// Copies this executable to `<db>.bin/workyard` (only when it differs) so that rebuilding the
/// running binary does not change the `workyard` workers see. Returns the directory.
fn stable_cli_copy(db: &std::path::Path) -> std::io::Result<PathBuf> {
    let mut dir = db.to_path_buf().into_os_string();
    dir.push(".bin");
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    let exe = std::env::current_exe()?;
    let target = dir.join("workyard");
    if std::fs::read(&exe)? != std::fs::read(&target).unwrap_or_default() {
        // Write beside it and rename, so a worker never execs a half-written file.
        let temp = dir.join(format!("workyard.{}.tmp", std::process::id()));
        std::fs::copy(&exe, &temp)?;
        std::fs::rename(&temp, &target)?;
    }
    Ok(dir)
}

fn run_orchestrator(
    store: &Store,
    board: &str,
    pool: Option<i64>,
    claude_bin: String,
    claude_args: Vec<String>,
    permissions: Permissions,
    allow: Vec<String>,
    supervisor: bool,
    plugin_dir: Option<PathBuf>,
    cwd: Option<PathBuf>,
    max_failures: u32,
    notify: bool,
    keep_worktrees: bool,
    worker_env: std::collections::BTreeMap<String, String>,
) -> workyard_core::Result<Value> {
    let db = store.path().to_path_buf();
    let mut log_dir = db.clone().into_os_string();
    log_dir.push(".logs");
    let mut args = if claude_args.is_empty() { permission_args(permissions, &allow) } else { claude_args };
    if let Some(plugin) = plugin_dir.or_else(find_plugin_dir) {
        args.extend(["--plugin-dir".into(), plugin.to_string_lossy().into_owned()]);
    } else {
        eprintln!("warning: no Workyard plugin found; workers will not have its skill (pass --plugin-dir)");
    }
    let bin_dir = stable_cli_copy(&db)
        .map_err(|e| eprintln!("warning: no stable copy of workyard for workers: {e}"))
        .ok();
    let runner = ClaudeRunner {
        bin: claude_bin,
        args,
        db,
        cwd,
        log_dir: PathBuf::from(log_dir),
        bin_dir,
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
        permissions: match permissions {
            Permissions::Full => "full".into(),
            Permissions::Safe => "safe".into(),
        },
        supervisor,
        keep_worktrees,
        worker_env,
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
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayVersion && std::env::args().any(|a| a == "--json") => {
            println!("{}", json!({"ok":true,"data":{"version":env!("CARGO_PKG_VERSION"),"schema_version":workyard_core::SCHEMA_VERSION}}));
            return;
        }
        Err(error) => error.exit(),
    };
    if let Command::Doctor = &cli.command {
        let path = cli.db.clone().map(Ok).unwrap_or_else(Store::default_path);
        std::process::exit(doctor::run(path, cli.json));
    }
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
        if let Err(error) = path.and_then(|p| follow_events(&Store::new(p).allow_migrate(cli.allow_migrate), *since, *limit)) {
            eprintln!("{error}");
            std::process::exit(exit_code(&error));
        }
        return;
    }
    let path = match cli.db {
        Some(path) => Ok(path),
        None => Store::default_path(),
    };
    if let Command::Wait { board, until, timeout } = &cli.command {
        let store = path.map(|p| Store::new(p).allow_migrate(cli.allow_migrate));
        let outcome = store.and_then(|store| {
            let until = watch::parse_until(until)?;
            let timeout = timeout.as_deref().map(watch::parse_duration).transpose()?;
            watch::wait(&store, board, &until, timeout)
        });
        match outcome {
            Ok(Some(why)) => {
                if cli.json {
                    println!("{}", json!({"ok":true,"data":{"met":true,"reason":why}}));
                } else {
                    println!("{why}");
                }
            }
            Ok(None) => {
                if cli.json {
                    println!("{}", json!({"ok":false,"error":{"code":"timeout","message":"timed out"}}));
                } else {
                    eprintln!("timed out");
                }
                std::process::exit(4);
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(exit_code(&error));
            }
        }
        return;
    }
    let text_status = !cli.json && matches!(cli.command, Command::Status { .. });
    let result = path.and_then(|path| run(&Store::new(path).allow_migrate(cli.allow_migrate), &cli.actor, cli.command));
    match result {
        Ok(data) if text_status => print!("{}", watch::render(&data)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_is_found_in_a_homebrew_layout_through_a_symlink() {
        let root = std::env::temp_dir().join(format!("workyard-brew-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cellar = root.join("Cellar/workyard/0.1.0");
        std::fs::create_dir_all(cellar.join("bin")).unwrap();
        std::fs::create_dir_all(cellar.join("share/workyard/plugin/.claude-plugin")).unwrap();
        std::fs::write(cellar.join("share/workyard/plugin/.claude-plugin/plugin.json"), "{}").unwrap();
        std::fs::write(cellar.join("bin/workyard"), "").unwrap();
        // <prefix>/bin/workyard -> ../Cellar/workyard/0.1.0/bin/workyard, as Homebrew links it.
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::os::unix::fs::symlink("../Cellar/workyard/0.1.0/bin/workyard", root.join("bin/workyard")).unwrap();
        let found = plugin_dir_for_exe(&root.join("bin/workyard")).expect("plugin found");
        assert_eq!(found, cellar.join("share/workyard/plugin").canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn full_permissions_turn_checks_off_and_safe_ones_allow_a_list() {
        assert_eq!(permission_args(Permissions::Full, &["Bash(make:*)".into()]), vec!["--dangerously-skip-permissions"]);
        let safe = permission_args(Permissions::Safe, &["Bash(make:*)".into()]);
        assert_eq!(&safe[..2], ["--permission-mode", "acceptEdits"]);
        assert!(safe.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]), "{safe:?}");
        assert!(safe.windows(2).any(|w| w == ["--permission-prompts", "host"]), "{safe:?}");
        for tool in ["Bash(workyard:*)", "Bash(git:*)", "Bash(npm:*)", "Bash(cargo:*)", "Bash(python3:*)", "Bash(make:*)"] {
            assert!(safe.iter().any(|a| a == tool), "{tool} missing from {safe:?}");
        }
        assert!(!safe.iter().any(|a| a == "--dangerously-skip-permissions"));
    }
}
