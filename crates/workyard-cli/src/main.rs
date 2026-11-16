use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use workyard_core::{BoardFilter, Error, Store};

#[derive(Parser)]
#[command(name = "workyard", about = "Local kanban for work with AI agents")]
struct Cli {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[arg(long, global = true, default_value = "cli")]
    actor: String,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage the agent session attached to an In Progress task.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
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
    Projects,
    Project {
        project: String,
    },
    CreateProject {
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
    ApproveChange {
        change: String,
        reviewer: String,
        #[arg(long)]
        expected_version: i64,
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
    Note {
        card: String,
        body: String,
    },
    CreateBoard {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
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
    },
    Update {
        card: String,
        #[arg(long)]
        expected_version: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long)]
        agent_type: Option<String>,
    },
    Claim {
        card: String,
        #[arg(long)]
        expected_version: i64,
    },
    Release {
        card: String,
        #[arg(long)]
        expected_version: i64,
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
    Move {
        card: String,
        lane: String,
        #[arg(long)]
        position: Option<i64>,
        #[arg(long)]
        expected_version: i64,
    },
    Progress {
        card: String,
        progress: i64,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        expected_version: i64,
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
    ScopeDependency {
        dependency: String,
        scope: String,
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
        Command::Session { command } => match command {
            SessionCommand::Attach { card, runner, session_id } => store.attach_task_session(actor, &card, &runner, &session_id)?,
            SessionCommand::Pause { card, waiting_question } => store.pause_task_session(actor, &card, waiting_question.as_deref())?,
            SessionCommand::Resume { card } => store.resume_task_session(actor, &card)?,
            SessionCommand::Ack { card, through_event_id } => store.acknowledge_task_session(actor, &card, through_event_id)?,
            SessionCommand::Detach { card } => store.detach_task_session(actor, &card)?,
            SessionCommand::Briefing { card, limit, after } => store.task_session_briefing(&card, limit, after)?,
        },
        Command::Phase { command } => match command {
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
        Command::Cancel {
            card,
            reason,
            expected_version,
        } => store.cancel_card(actor, &card, &reason, expected_version)?,
        Command::Projects => store.list_projects()?,
        Command::Project { project } => store.view_project(&project)?,
        Command::CreateProject { name, description } => {
            store.create_project(actor, &name, &description)?
        }
        Command::AttachBoard {
            board,
            project,
            expected_version,
        } => store.attach_board_to_project(actor, &board, &project, expected_version)?,
        Command::AddRepository {
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
        Command::AddDocument {
            project,
            repository,
            path,
            kind,
            title,
        } => store.add_project_document(actor, &project, &repository, &path, &kind, &title)?,
        Command::AddStakeholder {
            project,
            name,
            role,
            contact,
        } => store.add_project_stakeholder(actor, &project, &name, &role, &contact)?,
        Command::AssignStakeholder {
            question,
            stakeholder,
            expected_version,
        } => store.assign_question_stakeholder(actor, &question, &stakeholder, expected_version)?,
        Command::UnassignStakeholder {
            question,
            expected_version,
        } => store.unassign_question_stakeholder(actor, &question, expected_version)?,
        Command::LinkChange {
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
        Command::UpdateChange {
            change,
            commit,
            expected_version,
        } => store.update_card_change(actor, &change, &commit, expected_version)?,
        Command::RequireReview {
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
        Command::ApproveChange {
            change,
            reviewer,
            expected_version,
        } => store.approve_card_change(actor, &change, &reviewer, expected_version)?,
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
        Command::Note { card, body } => store.add_card_note(actor, &card, &body)?,
        Command::CreateBoard { name, description } => {
            store.create_board(actor, &name, &description)?
        }
        Command::ArchiveBoard {
            board,
            expected_version,
        } => store.set_board_archived(actor, &board, expected_version, true)?,
        Command::UnarchiveBoard {
            board,
            expected_version,
        } => store.set_board_archived(actor, &board, expected_version, false)?,
        Command::ArchiveLane {
            lane,
            expected_version,
        } => store.set_lane_archived(actor, &lane, expected_version, true)?,
        Command::AddCard {
            board,
            title,
            phase,
            desc,
            agent_type,
            lane,
        } => store.create_card_in_phase(
            actor,
            &board,
            &title,
            &desc,
            "P2",
            lane.as_deref(),
            agent_type.as_deref().unwrap_or(""),
            phase.as_deref(),
        )?,
        Command::Update {
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
        Command::Claim {
            card,
            expected_version,
        } => store.claim_card(actor, &card, expected_version)?,
        Command::Release {
            card,
            expected_version,
        } => store.release_card(actor, &card, expected_version, None)?,
        Command::Transfer {
            card,
            to_actor,
            expected_version,
        } => store.transfer_card(actor, &card, expected_version, &to_actor)?,
        Command::ForceRelease {
            card,
            reason,
            expected_version,
        } => store.release_card(actor, &card, expected_version, Some(&reason))?,
        Command::Move {
            card,
            lane,
            position,
            expected_version,
        } => store.move_card(actor, &card, &lane, position, expected_version)?,
        Command::Progress {
            card,
            progress,
            note,
            expected_version,
        } => store.update_progress(actor, &card, progress, note.as_deref(), expected_version)?,
        Command::Depend {
            card,
            on,
            scope,
            expected_version,
        } => store.add_dependency_scoped(actor, &card, &on, &scope, expected_version)?,
        Command::Undepend {
            card,
            on,
            expected_version,
        } => store.remove_dependency(actor, &card, &on, expected_version)?,
        Command::ScopeDependency {
            dependency,
            scope,
            expected_version,
        } => store.set_dependency_scope(actor, &dependency, &scope, expected_version)?,
        Command::Ask {
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
        Command::QuestionOptions {
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
        Command::Answer {
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
        Command::Drop {
            question,
            reason,
            expected_version,
        } => store.resolve_question(actor, &question, &reason, None, expected_version, true)?,
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
        Command::Events { since, limit } => store.poll_events(since, limit)?,
    })
}

fn main() {
    let cli = Cli::parse();
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
