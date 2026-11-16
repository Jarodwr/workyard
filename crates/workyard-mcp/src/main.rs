use std::process::Command;

use rmcp::{
    ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone)]
struct WorkyardServer {
    cli: String,
    actor: String,
    db: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BoardArg {
    /// Board key such as B-1, or UUID.
    board: String,
    #[serde(default)]
    phase: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CardArg {
    /// Card key such as C-1, or UUID.
    card: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SessionAttachArg {
    card: String,
    /// Agent runtime name, for example codex or claude.
    runner: String,
    /// Stable resume identifier supplied by that runtime.
    session_id: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SessionPauseArg {
    card: String,
    /// Question key or UUID when this session is waiting for a specific answer.
    waiting_question: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SessionAckArg {
    card: String,
    /// Cursor from the last briefing page successfully delivered to the agent.
    through_event_id: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SessionBriefingArg {
    card: String,
    /// Number of board events to scan, from 1 to 1000.
    limit: Option<i64>,
    /// Cursor from a preceding briefing page; omit for the first page.
    after: Option<i64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NoteArg {
    card: String,
    /// A short human or agent comment. Does not edit card fields or ownership.
    body: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct VersionedCardArg {
    card: String,
    /// Version returned by view_card or view_board.
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateBoardArg {
    name: String,
    #[serde(default)]
    description: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateCardArg {
    board: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    agent_type: Option<String>,
    #[serde(default)]
    lane: Option<String>,
    #[serde(default)]
    phase: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateCardArg {
    card: String,
    expected_version: i64,
    title: Option<String>,
    description: Option<String>,
    agent_type: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MoveArg {
    card: String,
    lane: String,
    expected_version: i64,
    position: Option<i64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProgressArg {
    card: String,
    progress: i64,
    expected_version: i64,
    note: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DependencyArg {
    card: String,
    on: String,
    /// start prevents beginning work; finish only prevents moving to Done.
    #[serde(default)]
    scope: Option<String>,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScopeDependencyArg {
    dependency: String,
    scope: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectArg {
    project: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateProjectArg {
    name: String,
    #[serde(default)]
    description: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AttachBoardArg {
    board: String,
    project: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddRepositoryArg {
    project: String,
    path: String,
    #[serde(default)]
    target_branch: Option<String>,
    #[serde(default)]
    remote_url: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddDocumentArg {
    project: String,
    repository: String,
    path: String,
    kind: String,
    title: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddStakeholderArg {
    project: String,
    name: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    contact: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AssignStakeholderArg {
    question: String,
    stakeholder: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UnassignStakeholderArg {
    question: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LinkChangeArg {
    card: String,
    repository: String,
    branch: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    review_required: bool,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateChangeArg {
    change: String,
    commit: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RequireReviewArg {
    change: String,
    required: bool,
    expected_version: i64,
    expected_card_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ApproveChangeArg {
    change: String,
    reviewer: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QuestionArg {
    board: String,
    kind: String,
    title: String,
    #[serde(default)]
    body: String,
    /// Two to ten distinct, concrete choices. Do not include Other; it is added automatically.
    options: Vec<String>,
    /// Exact text of the recommended option.
    recommend: String,
    /// Explain in one plain-language sentence why this choice is a good default.
    why: String,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    phase: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PhaseAddArg {
    board: String,
    name: String,
    #[serde(default)]
    description: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PhaseDescribeArg {
    board: String,
    phase: String,
    description: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PhaseAssignArg {
    item: String,
    phase: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CancelArg {
    card: String,
    reason: String,
    expected_version: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ResolveQuestionArg {
    question: String,
    expected_version: i64,
    answer: String,
    /// Optional rationale; required when selecting Other.
    note: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QuestionOptionsArg {
    question: String,
    expected_version: i64,
    options: Vec<String>,
    recommend: String,
    why: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DropQuestionArg {
    question: String,
    expected_version: i64,
    reason: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QuestionsArg {
    board: Option<String>,
    #[serde(default)]
    open: bool,
    kind: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NextArg {
    board: String,
    agent_type: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HistoryArg {
    entity: String,
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EventsArg {
    /// Last event ID already processed. Use zero to read from the beginning.
    #[serde(default)]
    since: Option<i64>,
    /// Maximum events in one page, from 1 to 1000.
    #[serde(default)]
    limit: Option<i64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TransferArg {
    card: String,
    expected_version: i64,
    to_actor: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ForceReleaseArg {
    card: String,
    expected_version: i64,
    reason: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BoardVersionArg {
    board: String,
    expected_version: i64,
}

impl WorkyardServer {
    fn call(&self, name: &str, args: Vec<String>) -> CallToolResult {
        let mut command = Command::new(&self.cli);
        command.args(["--json", "--actor", &self.actor]);
        if let Some(db) = &self.db {
            command.args(["--db", db]);
        }
        command.arg(name).args(args);
        match command.output() {
            Ok(output) => match serde_json::from_slice::<Value>(&output.stdout) {
                Ok(result) if result["ok"] == true => {
                    let mut response = CallToolResult::structured(result["data"].clone());
                    response
                        .content
                        .push(ContentBlock::text(format!("{name} succeeded")));
                    response
                }
                Ok(result) => {
                    let error = result["error"].clone();
                    let mut response = CallToolResult::error(vec![ContentBlock::text(format!(
                        "{}: {}",
                        error["code"].as_str().unwrap_or("error"),
                        error["message"].as_str().unwrap_or("command failed")
                    ))]);
                    response.structured_content = Some(error);
                    response
                }
                Err(_) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "{name} returned invalid JSON; stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))]),
            },
            Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                "failed to start Workyard CLI: {error}"
            ))]),
        }
    }
}

#[tool_router(server_handler)]
impl WorkyardServer {
    #[tool(
        description = "List local projects that contain boards, repositories, documents, and stakeholders."
    )]
    fn list_projects(&self) -> CallToolResult {
        self.call("projects", vec![])
    }
    #[tool(
        description = "Read one project's boards, local repositories, document references, and stakeholders."
    )]
    fn view_project(&self, Parameters(a): Parameters<ProjectArg>) -> CallToolResult {
        self.call("project", vec![a.project])
    }
    #[tool(description = "Create a project above one or more boards.")]
    fn create_project(&self, Parameters(a): Parameters<CreateProjectArg>) -> CallToolResult {
        self.call(
            "create-project",
            vec![a.name, "--description".into(), a.description],
        )
    }
    #[tool(description = "Link a board to a project using the board's current version.")]
    fn attach_board_to_project(&self, Parameters(a): Parameters<AttachBoardArg>) -> CallToolResult {
        self.call(
            "attach-board",
            vec![
                a.board,
                a.project,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(description = "Register a local Git repository. Remote URL is optional.")]
    fn add_project_repository(
        &self,
        Parameters(a): Parameters<AddRepositoryArg>,
    ) -> CallToolResult {
        let mut x = vec![
            a.project,
            a.path,
            "--target-branch".into(),
            a.target_branch.unwrap_or("main".into()),
        ];
        if let Some(remote) = a.remote_url {
            x.extend(["--remote-url".into(), remote]);
        }
        self.call("add-repository", x)
    }
    #[tool(
        description = "Reference an ADR, diagram, or documentation file in a registered repository."
    )]
    fn add_project_document(&self, Parameters(a): Parameters<AddDocumentArg>) -> CallToolResult {
        self.call(
            "add-document",
            vec![
                a.project,
                a.repository,
                a.path,
                "--kind".into(),
                a.kind,
                "--title".into(),
                a.title,
            ],
        )
    }
    #[tool(description = "Record a project stakeholder for routing business questions.")]
    fn add_project_stakeholder(
        &self,
        Parameters(a): Parameters<AddStakeholderArg>,
    ) -> CallToolResult {
        self.call(
            "add-stakeholder",
            vec![
                a.project,
                a.name,
                "--role".into(),
                a.role,
                "--contact".into(),
                a.contact,
            ],
        )
    }
    #[tool(
        description = "Optionally assign a project stakeholder to a business or engineering question; this does not send a message."
    )]
    fn assign_question_stakeholder(
        &self,
        Parameters(a): Parameters<AssignStakeholderArg>,
    ) -> CallToolResult {
        self.call(
            "assign-stakeholder",
            vec![
                a.question,
                a.stakeholder,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(description = "Remove a question's stakeholder assignment. The question stays open and can be answered normally.")]
    fn unassign_question_stakeholder(
        &self,
        Parameters(a): Parameters<UnassignStakeholderArg>,
    ) -> CallToolResult {
        self.call(
            "unassign-stakeholder",
            vec![a.question, "--expected-version".into(), a.expected_version.to_string()],
        )
    }
    #[tool(
        description = "Link a local code branch and commit to a card you own; optionally require review approval."
    )]
    fn link_card_change(&self, Parameters(a): Parameters<LinkChangeArg>) -> CallToolResult {
        let mut x = vec![
            a.card,
            a.repository,
            a.branch,
            "--commit".into(),
            a.commit.unwrap_or_default(),
            "--expected-version".into(),
            a.expected_version.to_string(),
        ];
        if let Some(target) = a.target {
            x.extend(["--target".into(), target]);
        }
        if a.review_required {
            x.push("--review-required".into());
        }
        self.call("link-change", x)
    }
    #[tool(
        description = "Update a linked change to a new commit, invalidating any approval of its old commit."
    )]
    fn update_card_change(&self, Parameters(a): Parameters<UpdateChangeArg>) -> CallToolResult {
        self.call(
            "update-change",
            vec![
                a.change,
                a.commit,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Add or remove the review approval gate on an existing linked change. The card owner supplies both current versions."
    )]
    fn set_change_review_required(
        &self,
        Parameters(a): Parameters<RequireReviewArg>,
    ) -> CallToolResult {
        let mut x = vec![
            a.change,
            "--expected-version".into(),
            a.expected_version.to_string(),
            "--expected-card-version".into(),
            a.expected_card_version.to_string(),
        ];
        if a.required {
            x.push("--required".into());
        }
        self.call("require-review", x)
    }
    #[tool(description = "Record review approval for the current commit of a linked code change.")]
    fn approve_card_change(&self, Parameters(a): Parameters<ApproveChangeArg>) -> CallToolResult {
        self.call(
            "approve-change",
            vec![
                a.change,
                a.reviewer,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "List local boards and per-lane card counts. Start here when the board ID is unknown."
    )]
    fn list_boards(&self) -> CallToolResult {
        self.call("boards", vec![])
    }
    #[tool(
        description = "Read an ordered board with card versions, progress, blockers, and ownership before planning or editing."
    )]
    fn view_board(&self, Parameters(a): Parameters<BoardArg>) -> CallToolResult {
        let mut x = vec![a.board];
        if let Some(phase) = a.phase {
            x.extend(["--phase".into(), phase]);
        }
        self.call("board", x)
    }
    #[tool(description = "List a board's ordered phases and identify the current one.")]
    fn list_phases(&self, Parameters(a): Parameters<BoardArg>) -> CallToolResult {
        self.call("phase", vec!["list".into(), a.board])
    }
    #[tool(description = "Add a future phase with a short description to a board. Phases are scheduled in creation order.")]
    fn add_phase(&self, Parameters(a): Parameters<PhaseAddArg>) -> CallToolResult {
        self.call("phase", vec!["add".into(), a.board, a.name, "--description".into(), a.description])
    }
    #[tool(description = "Set or update a phase description using its current version.")]
    fn describe_phase(&self, Parameters(a): Parameters<PhaseDescribeArg>) -> CallToolResult {
        self.call("phase", vec!["describe".into(),a.board,a.phase,a.description,"--expected-version".into(),a.expected_version.to_string()])
    }
    #[tool(description = "Explicitly complete and archive the current phase after all its work and questions are resolved; works for the final phase too.")]
    fn complete_phase(&self, Parameters(a): Parameters<BoardArg>) -> CallToolResult {
        self.call("phase", vec!["complete".into(), a.board])
    }
    #[tool(
        description = "Advance one phase when its cards and questions are resolved and cancelled prerequisites have been replanned."
    )]
    fn advance_phase(&self, Parameters(a): Parameters<BoardArg>) -> CallToolResult {
        self.call("phase", vec!["advance".into(), a.board])
    }
    #[tool(
        description = "Assign an unclaimed Backlog card or question to an open phase; dependency order is checked."
    )]
    fn assign_phase(&self, Parameters(a): Parameters<PhaseAssignArg>) -> CallToolResult {
        self.call(
            "phase",
            vec![
                "assign".into(),
                a.item,
                a.phase,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Cancel obsolete work with a reason. Cancellation does not satisfy dependents; replan them explicitly."
    )]
    fn cancel_card(&self, Parameters(a): Parameters<CancelArg>) -> CallToolResult {
        self.call(
            "cancel",
            vec![
                a.card,
                "--reason".into(),
                a.reason,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Read one card, its blockers, dependencies, owner, current version, and recent history before editing it."
    )]
    fn view_card(&self, Parameters(a): Parameters<CardArg>) -> CallToolResult {
        self.call("card", vec![a.card])
    }
    #[tool(description = "Attach a resumable agent runtime session to a claimed In Progress task. Session bookkeeping does not change the card version.")]
    fn attach_task_session(&self, Parameters(a): Parameters<SessionAttachArg>) -> CallToolResult {
        self.call("session", vec!["attach".into(), a.card, "--runner".into(), a.runner, "--session-id".into(), a.session_id])
    }
    #[tool(description = "Park an attached task session and free its worker slot. Optionally name the question whose answer should wake it, even when other blockers remain.")]
    fn pause_task_session(&self, Parameters(a): Parameters<SessionPauseArg>) -> CallToolResult {
        let mut args = vec!["pause".into(), a.card];
        if let Some(question) = a.waiting_question { args.extend(["--waiting-question".into(), question]); }
        self.call("session", args)
    }
    #[tool(description = "Read the task's current state and relevant events since its last delivered cursor before resuming its agent. Follow has_more and acknowledge only after delivery.")]
    fn task_session_briefing(&self, Parameters(a): Parameters<SessionBriefingArg>) -> CallToolResult {
        let mut args = vec!["briefing".into(), a.card, "--limit".into(), a.limit.unwrap_or(100).to_string()];
        if let Some(after) = a.after { args.extend(["--after".into(), after.to_string()]); }
        self.call("session", args)
    }
    #[tool(description = "Acknowledge a briefing cursor after its changes have been delivered to the attached agent session.")]
    fn acknowledge_task_session(&self, Parameters(a): Parameters<SessionAckArg>) -> CallToolResult {
        self.call("session", vec!["ack".into(), a.card, "--through-event-id".into(), a.through_event_id.to_string()])
    }
    #[tool(description = "Mark a paused task session running after delivering its briefing. Fails if its wait condition is unresolved.")]
    fn resume_task_session(&self, Parameters(a): Parameters<CardArg>) -> CallToolResult {
        self.call("session", vec!["resume".into(), a.card])
    }
    #[tool(description = "Detach a task's session when it cannot be resumed or is replaced. The task's owner and content stay unchanged.")]
    fn detach_task_session(&self, Parameters(a): Parameters<CardArg>) -> CallToolResult {
        self.call("session", vec!["detach".into(), a.card])
    }
    #[tool(
        description = "Append a comment to a card without changing its version or owner. Use for context that should not overwrite the owner's status note."
    )]
    fn add_card_note(&self, Parameters(a): Parameters<NoteArg>) -> CallToolResult {
        self.call("note", vec![a.card, a.body])
    }
    #[tool(description = "Create a board with default Backlog, In Progress, and Done lanes.")]
    fn create_board(&self, Parameters(a): Parameters<CreateBoardArg>) -> CallToolResult {
        self.call(
            "create-board",
            vec![a.name, "--description".into(), a.description],
        )
    }
    #[tool(
        description = "Create a card in a local board. Defaults to Backlog. Work impact is derived from unresolved dependencies."
    )]
    fn create_card(&self, Parameters(a): Parameters<CreateCardArg>) -> CallToolResult {
        let mut x = vec![a.board, a.title, "--desc".into(), a.description];
        if let Some(l) = a.lane {
            x.extend(["--lane".into(), l]);
        }
        if let Some(phase) = a.phase {
            x.extend(["--phase".into(), phase]);
        }
        if let Some(agent_type) = a.agent_type {
            x.extend(["--agent-type".into(), agent_type]);
        }
        self.call("add-card", x)
    }
    #[tool(
        description = "Claim an unowned card before starting work. Only the same actor can edit it until release or transfer."
    )]
    fn claim_card(&self, Parameters(a): Parameters<VersionedCardArg>) -> CallToolResult {
        self.call(
            "claim",
            vec![
                a.card,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(description = "Release a card you own after finishing or handing off work.")]
    fn release_card(&self, Parameters(a): Parameters<VersionedCardArg>) -> CallToolResult {
        self.call(
            "release",
            vec![
                a.card,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(description = "Transfer a card you own to another stable actor identity.")]
    fn transfer_card(&self, Parameters(a): Parameters<TransferArg>) -> CallToolResult {
        self.call(
            "transfer",
            vec![
                a.card,
                "--to-actor".into(),
                a.to_actor,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Update the fields of an unclaimed card or one you own. Pass the version you just read."
    )]
    fn update_card(&self, Parameters(a): Parameters<UpdateCardArg>) -> CallToolResult {
        let mut x = vec![
            a.card,
            "--expected-version".into(),
            a.expected_version.to_string(),
        ];
        for (flag, value) in [
            ("--title", a.title),
            ("--desc", a.description),
            ("--agent-type", a.agent_type),
        ] {
            if let Some(v) = value {
                x.extend([flag.into(), v]);
            }
        }
        self.call("update", x)
    }
    #[tool(
        description = "Move an unclaimed card or one you own to a lane. A blocked card cannot enter an active lane."
    )]
    fn move_card(&self, Parameters(a): Parameters<MoveArg>) -> CallToolResult {
        let mut x = vec![
            a.card,
            a.lane,
            "--expected-version".into(),
            a.expected_version.to_string(),
        ];
        if let Some(p) = a.position {
            x.extend(["--position".into(), p.to_string()]);
        }
        self.call("move", x)
    }
    #[tool(
        description = "Record meaningful progress on an unclaimed card or one you own. Include a short status note when possible."
    )]
    fn update_progress(&self, Parameters(a): Parameters<ProgressArg>) -> CallToolResult {
        let mut x = vec![
            a.card,
            a.progress.to_string(),
            "--expected-version".into(),
            a.expected_version.to_string(),
        ];
        if let Some(n) = a.note {
            x.extend(["--note".into(), n]);
        }
        self.call("progress", x)
    }
    #[tool(
        description = "Add a card/question dependency. Use scope=start to block beginning work or scope=finish to block moving to Done; rejects cycles."
    )]
    fn add_dependency(&self, Parameters(a): Parameters<DependencyArg>) -> CallToolResult {
        self.call(
            "depend",
            vec![
                a.card,
                "--on".into(),
                a.on,
                "--scope".into(),
                a.scope.unwrap_or("start".into()),
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(description = "Remove a card's dependency after rereading its version.")]
    fn remove_dependency(&self, Parameters(a): Parameters<DependencyArg>) -> CallToolResult {
        self.call(
            "undepend",
            vec![
                a.card,
                "--on".into(),
                a.on,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Change an existing dependency between start and finish scope in one event. The card owner supplies its current version."
    )]
    fn set_dependency_scope(
        &self,
        Parameters(a): Parameters<ScopeDependencyArg>,
    ) -> CallToolResult {
        self.call(
            "scope-dependency",
            vec![
                a.dependency,
                a.scope,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Raise an engineering or business question with 2 to 10 concrete answer options. Do not include Other; it is added automatically. Recommend one option and explain why in simple language. Optionally block cards you may edit."
    )]
    fn raise_question(&self, Parameters(a): Parameters<QuestionArg>) -> CallToolResult {
        let mut x = vec![
            a.board,
            a.title,
            "--kind".into(),
            a.kind,
            "--body".into(),
            a.body,
        ];
        for option in a.options {
            x.extend(["--option".into(), option]);
        }
        x.extend(["--recommend".into(), a.recommend, "--why".into(), a.why]);
        if !a.blocks.is_empty() {
            x.extend(["--blocks".into(), a.blocks.join(",")]);
        }
        if let Some(phase) = a.phase {
            x.extend(["--phase".into(), phase]);
        }
        self.call("ask", x)
    }
    #[tool(
        description = "Replace choices and recommendation on an open question after rereading its version. Supply 2 to 10 distinct substantive choices and a simple reason for the recommended choice. Do not include Other; it is added automatically."
    )]
    fn update_question_options(
        &self,
        Parameters(a): Parameters<QuestionOptionsArg>,
    ) -> CallToolResult {
        let mut x = vec![
            a.question,
            "--expected-version".into(),
            a.expected_version.to_string(),
        ];
        for option in a.options {
            x.extend(["--option".into(), option]);
        }
        x.extend(["--recommend".into(), a.recommend, "--why".into(), a.why]);
        self.call("question-options", x)
    }
    #[tool(
        description = "Answer an open question. One event reports all cards made ready as effects."
    )]
    fn answer_question(&self, Parameters(a): Parameters<ResolveQuestionArg>) -> CallToolResult {
        let mut x = vec![
            a.question,
            a.answer,
            "--expected-version".into(),
            a.expected_version.to_string(),
        ];
        if let Some(note) = a.note {
            x.extend(["--note".into(), note]);
        }
        self.call("answer", x)
    }
    #[tool(
        description = "Drop an open question with a reason; this also resolves its blocking links."
    )]
    fn drop_question(&self, Parameters(a): Parameters<DropQuestionArg>) -> CallToolResult {
        self.call(
            "drop",
            vec![
                a.question,
                "--reason".into(),
                a.reason,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "List engineering and business questions, optionally for one board or only open questions."
    )]
    fn list_questions(&self, Parameters(a): Parameters<QuestionsArg>) -> CallToolResult {
        let mut x = Vec::new();
        if let Some(b) = a.board {
            x.push(b);
        }
        if a.open {
            x.push("--open".into());
        }
        if let Some(kind) = a.kind {
            x.extend(["--kind".into(), kind]);
        }
        self.call("questions", x)
    }
    #[tool(
        description = "Select an eligible paused In Progress task to resume first; otherwise select a ready, unclaimed Backlog task to claim. Inspect dispatch_action in the response."
    )]
    fn next_ready_task(&self, Parameters(a): Parameters<NextArg>) -> CallToolResult {
        let mut x = vec![a.board];
        if let Some(s) = a.agent_type {
            x.extend(["--agent-type".into(), s]);
        }
        self.call("next", x)
    }
    #[tool(
        description = "Read events for a board or card, including the effects caused by each action."
    )]
    fn view_history(&self, Parameters(a): Parameters<HistoryArg>) -> CallToolResult {
        self.call(
            "history",
            vec![
                a.entity,
                "--since".into(),
                a.since.unwrap_or(0).to_string(),
                "--limit".into(),
                a.limit.unwrap_or(100).to_string(),
            ],
        )
    }
    #[tool(
        description = "Read the next page of committed events in ID order. Process each event once, then pass its returned cursor as since; repeat while has_more is true."
    )]
    fn poll_events(&self, Parameters(a): Parameters<EventsArg>) -> CallToolResult {
        self.call(
            "events",
            vec![
                "--since".into(),
                a.since.unwrap_or(0).to_string(),
                "--limit".into(),
                a.limit.unwrap_or(100).to_string(),
            ],
        )
    }
    #[tool(description = "Archive a board with its current version.")]
    fn archive_board(&self, Parameters(a): Parameters<BoardVersionArg>) -> CallToolResult {
        self.call(
            "archive-board",
            vec![
                a.board,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(description = "Restore an archived board with its current version.")]
    fn unarchive_board(&self, Parameters(a): Parameters<BoardVersionArg>) -> CallToolResult {
        self.call(
            "unarchive-board",
            vec![
                a.board,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
    #[tool(
        description = "Recovery only: release a card owned by a stopped agent and record the reason."
    )]
    fn force_release_card(&self, Parameters(a): Parameters<ForceReleaseArg>) -> CallToolResult {
        self.call(
            "force-release",
            vec![
                a.card,
                "--reason".into(),
                a.reason,
                "--expected-version".into(),
                a.expected_version.to_string(),
            ],
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_name = std::env::var("WORKYARD_MCP_CLIENT").unwrap_or_else(|_| "agent".into());
    let server = WorkyardServer {
        cli: std::env::var("WORKYARD_CLI").unwrap_or_else(|_| "workyard".into()),
        actor: format!("mcp:{client_name}"),
        db: std::env::var("WORKYARD_DB").ok(),
    };
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
