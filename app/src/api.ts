import { invoke } from "@tauri-apps/api/core";

export type Blocker = { type: "card" | "question"; id: string; key: string; title: string; cancelled?: boolean };
/** The session summary on a board card. Fields after `held_by` are newer and may be missing. */
export type CardSession = {
  runner: string;
  paused: boolean;
  held_by: string | null;
  waiting_review?: boolean;
  waiting_integration?: boolean;
  waiting_question_key?: string | null;
  /** What the worker is doing right now, as published by the orchestrator. */
  activity?: string;
  activity_at?: string | null;
  attached_at?: string | null;
  /** Tool calls the worker has made this run, and seconds since it attached. */
  tool_uses?: number;
  elapsed_secs?: number | null;
  paused_at?: string | null;
};
export type Card = {
  id: string;
  key: string;
  board_id: string;
  lane_id: string;
  phase_id: string;
  cancelled_at?: string | null;
  cancellation_reason?: string | null;
  position: number;
  title: string;
  description: string;
  priority: "P0" | "P1" | "P2" | "P3";
  blocking_impact: number;
  progress: number;
  status_note: string;
  agent_type: string;
  owner_actor: string | null;
  version: number;
  blocked: boolean;
  ready: boolean;
  blockers: Blocker[];
  finish_blockers?: Blocker[];
  cancelled_prerequisites?: Blocker[];
  review_gates?: { change_id: string; working_branch: string; commit_oid: string; approved: boolean; reviewer: string | null }[];
  can_finish?: boolean;
  open_question_count: number;
  workspace?: "worktree" | "none";
  session?: CardSession | null;
  stale?: boolean;
  last_activity_at?: string;
};
export type Lane = { id: string; name: string; kind: string; position: number; version: number; cards: Card[] };
export type Phase = { id: string; board_id: string; name: string; description: string; version: number; closed_at: string | null; position: number; status: "closed" | "current" | "future" };
export type Board = { id: string; key: string; name: string; description: string; version: number; archived: boolean; review_required?: boolean; integration?: "merge" | "rebase"; project_id?: string | null; current_phase_id?: string | null; phases: Phase[]; lanes: Lane[] };
export type ProjectListItem = { id: string; name: string; description: string; version: number; board_count: number };
export type RepositoryInfo = { path: string; branch: string; dirty: boolean };
export type Project = ProjectListItem & {
  boards: { id: string; key: string; name: string; description: string; archived: boolean }[];
  repositories: { id: string; path: string; target_branch: string; remote_url: string | null }[];
  documents: { id: string; repository_id: string; path: string; kind: "adr" | "diagram" | "documentation"; title: string }[];
  stakeholders: { id: string; name: string; role: string; contact: string }[];
  changes: (CardChange & { card_key: string; card_title: string })[];
};
export type BoardListItem = Omit<Board, "lanes" | "phases"> & { lanes: { lane_id: string; name: string; count: number }[]; phases?: Phase[] };
export type EventEffect = { type: string; card_id?: string; card_key?: string; cause_key?: string };
export type BoardEvent = { id: number; type: string; board_id: string | null; entity_type: string; entity_id: string; effects: EventEffect[] };
export type EventPage = { events: BoardEvent[]; cursor: number; has_more: boolean };
export type Question = { id: string; key: string; board_id: string; phase_id: string; stakeholder_id?: string | null; stakeholder?: { name: string; role: string; contact: string }; kind: "engineering" | "business"; title: string; body: string; options: string[]; recommended_option: string; recommendation_reason: string; status: string; answer: string; answer_note: string; version: number; blocks: string[] };
export type Dependency = { id: string; from_card_id: string; to_card_id: string | null; to_question_id: string | null; scope?: "start" | "finish" };
export type CardChange = { id: string; version: number; repository_id: string; working_branch: string; target_branch: string; commit_oid: string; review_required: boolean; approved_commit_oid: string | null; reviewer: string | null };
export type Orchestrator = { board_key: string; desired_state: "running" | "paused" | "draining"; pool_size: number; connected: boolean; holder: string | null; epoch: number; heartbeat_at: string | null; backing_off: boolean; backoff_until: string | null; backoff_reason: string; permissions?: "full" | "safe" };
/** A tool a safe-mode worker is waiting on a human to approve. */
export type PermissionRequest = { id: string; board_key: string; card_key: string; request_id: string; tool_name: string; input: Record<string, unknown>; description: string; status: "pending" | "allowed_once" | "allowed_run" | "denied" | "expired"; run_rule: string; created_at: string };
export type PermissionDecision = "allow_once" | "allow_run" | "deny";
/** One readable line of a worker's output, parsed from its stream-json log. */
export type AgentLogRow = { kind: "text" | "tool" | "result" | "error" | "user"; text: string; tool?: string };
export type CardNote = { id: string; card_id: string; actor: string; body: string; created_at: string };
export type TaskSession = { id: string; card_id: string; owner_actor: string; runner: string; external_session_id: string; attached_at: string; paused_at: string | null; waiting_question_id: string | null; waiting_question_key: string | null; last_delivered_event_id: number; detached_at: string | null; held_by?: string | null };
export type CardReport = { body: string; actor: string; version: number; updated_at: string };
export type CardDetail = Card & { dependencies: Dependency[]; changes: CardChange[]; notes: CardNote[]; report: CardReport | null; session: TaskSession | null; events: { id: number; type: string; actor: string; ts: string }[] };

export const api = {
  listProjects: () => invoke<ProjectListItem[]>("list_projects"),
  viewProject: (project: string) => invoke<Project>("view_project", { project }),
  listBoards: () => invoke<BoardListItem[]>("list_boards"),
  viewBoard: (board: string) => invoke<Board>("view_board", { board }),
  viewCard: (card: string) => invoke<CardDetail>("view_card", { card }),
  addCardNote: (card: string, body: string) => invoke<CardNote>("add_card_note", { card, body }),
  dbPath: () => invoke<string>("db_path"),
  latestEventId: () => invoke<number>("latest_event_id"),
  pollEvents: (since: number, limit = 100) => invoke<EventPage>("poll_events", { since, limit }),
  listQuestions: () => invoke<Question[]>("list_questions"),
  answerQuestion: (question: Question, answer: string, note: string) => invoke<Question>("answer_question", { question: question.key, answer, note, expectedVersion: question.version }),
  assignQuestionStakeholder: (question: Question, stakeholder: string) => invoke<Question>("assign_question_stakeholder", { question: question.key, stakeholder, expectedVersion: question.version }),
  unassignQuestionStakeholder: (question: Question) => invoke<Question>("unassign_question_stakeholder", { question: question.key, expectedVersion: question.version }),
  openInTerminal: (card: string) => invoke<TaskSession>("open_in_terminal", { card }),
  openPlanner: (board: string) => invoke<{ board: string }>("open_planner", { board }),
  openProjectPlanner: (project: string) => invoke<{ project: string }>("open_project_planner", { project }),
  inspectRepository: (path: string) => invoke<RepositoryInfo>("inspect_repository", { path }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  createProjectFromRepository: (name: string, description: string, path: string, targetBranch: string) => invoke<Project>("create_project_from_repository", { name, description, path, targetBranch }),
  startOrchestrator: (board: string) => invoke<Orchestrator>("start_orchestrator", { board, supervisor: getSupervisorPreference(board) }),
  stopOrchestrator: (board: string) => invoke<Orchestrator>("stop_orchestrator", { board }),
  releaseSessionHold: (card: string) => invoke<TaskSession>("release_session_hold", { card }),
  holdSession: (card: string) => invoke<TaskSession>("hold_session", { card }),
  agentLogTail: (card: string, limit = 60) => invoke<AgentLogRow[]>("agent_log_tail", { card, limit }),
  setBoardIntegration: (board: string, integration: "merge" | "rebase") => invoke<Board>("set_board_integration", { board, integration }),
  setBoardReviewRequired: (board: string, required: boolean) => invoke<Board>("set_board_review_required", { board, required }),
  approveChange: (change: CardChange) => invoke<CardChange>("approve_change", { change: change.id, expectedVersion: change.version }),
  orchestratorStatus: (board: string) => invoke<Orchestrator>("orchestrator_status", { board }),
  setOrchestratorState: (board: string, state: Orchestrator["desired_state"]) => invoke<Orchestrator>("set_orchestrator_state", { board, state }),
  pendingPermissions: (board: string) => invoke<PermissionRequest[]>("pending_permissions", { board }),
  decidePermission: (request: string, decision: PermissionDecision) => invoke<PermissionRequest>("decide_permission", { request, decision }),
};

export const isDemo = !("__TAURI_INTERNALS__" in window);

let demoBoard: Board = {
  id: "demo-board", key: "B-1", name: "Workyard demo", description: "Animation fixture", version: 1, archived: false, project_id: "demo-project",
  current_phase_id: "demo-phase", phases: [{ id: "demo-phase", board_id: "demo-board", name: "Current work", description: "Build the local command and event foundation.", version: 1, closed_at: null, position: 0, status: "current" }, { id: "demo-phase-2", board_id: "demo-board", name: "Integration", description: "Connect agents, questions, and the desktop view.", version: 1, closed_at: null, position: 1, status: "future" }],
  lanes: [
    { id: "backlog", name: "Backlog", kind: "backlog", position: 0, version: 1, cards: [
      { id: "demo-1", key: "C-1", board_id: "demo-board", phase_id: "demo-phase", lane_id: "backlog", position: 0, title: "Build the event stream", description: "One event per action, with effects.", priority: "P1", blocking_impact: 0, progress: 30, status_note: "Schema is ready", agent_type: "Engineering", owner_actor: null, version: 1, blocked: false, ready: true, blockers: [], open_question_count: 0 },
      { id: "demo-2", key: "C-2", board_id: "demo-board", phase_id: "demo-phase-2", lane_id: "backlog", position: 1, title: "Wire the desktop board", description: "Keep card state stable across lane moves.", priority: "P2", blocking_impact: 0, progress: 0, status_note: "", agent_type: "Frontend", owner_actor: null, version: 1, blocked: false, ready: true, blockers: [], finish_blockers: [{type:"card",id:"demo-1",key:"C-1",title:"Build the event stream"}], can_finish:false, open_question_count: 0 },
    ] },
    { id: "active", name: "In Progress", kind: "active", position: 1, version: 1, cards: [
      { id: "demo-3", key: "C-3", board_id: "demo-board", phase_id: "demo-phase", lane_id: "active", position: 0, title: "Storage module", description: "Persist todos.", priority: "P1", blocking_impact: 0, progress: 0, status_note: "", agent_type: "Engineering", owner_actor: "agent:claude-demo3", version: 1, blocked: false, ready: true, blockers: [], open_question_count: 0, can_finish: true, session: { runner: "claude", paused: false, held_by: null, activity: "running node --test", activity_at: new Date().toISOString(), attached_at: new Date(Date.now() - 4 * 60_000).toISOString() } },
      { id: "demo-4", key: "C-4", board_id: "demo-board", phase_id: "demo-phase", lane_id: "active", position: 1, title: "Wire up the UI", description: "Connect model and storage.", priority: "P1", blocking_impact: 0, progress: 100, status_note: "", agent_type: "Frontend", owner_actor: "agent:claude-demo4", version: 1, blocked: false, ready: true, blockers: [], open_question_count: 0, can_finish: false, review_gates: [{ change_id: "demo-change", working_branch: "workyard/c-4", commit_oid: "84f9584", approved: false, reviewer: null }], session: { runner: "claude", paused: true, held_by: null, waiting_review: true, attached_at: new Date(Date.now() - 20 * 60_000).toISOString() } },
    ] },
    { id: "done", name: "Done", kind: "done", position: 2, version: 1, cards: [] },
  ],
};
const demoRuntimeBoard: Board = structuredClone(demoBoard);
demoRuntimeBoard.id = "demo-runtime";
demoRuntimeBoard.key = "B-2";
demoRuntimeBoard.name = "Agent runtime";
demoRuntimeBoard.description = "Orchestrator events, sessions, and handoffs";
demoRuntimeBoard.current_phase_id = "demo-runtime-phase";
demoRuntimeBoard.phases = [{ id: "demo-runtime-phase", board_id: demoRuntimeBoard.id, name: "Current work", description: "Define session ownership and handoffs.", version: 1, closed_at: null, position: 0, status: "current" }];
demoRuntimeBoard.lanes = demoRuntimeBoard.lanes.map((lane) => ({ ...lane, id: `runtime-${lane.id}`, cards: [] }));
const demoResearchBoard: Board = structuredClone(demoRuntimeBoard);
demoResearchBoard.id = "demo-research";
demoResearchBoard.key = "B-3";
demoResearchBoard.name = "Research backlog";
demoResearchBoard.description = "Investigations without code delivery";
demoResearchBoard.project_id = "demo-field-notes";
demoResearchBoard.current_phase_id = "demo-research-phase";
demoResearchBoard.phases = [{ id: "demo-research-phase", board_id: demoResearchBoard.id, name: "Current work", description: "Investigate local-first onboarding needs.", version: 1, closed_at: null, position: 0, status: "current" }];
demoResearchBoard.lanes = demoResearchBoard.lanes.map((lane) => ({ ...lane, id: `research-${lane.kind}`, cards: [] }));
let demoEvents: BoardEvent[] = [];
let demoNextId = 3;
let demoQuestions: Question[] = [
  { id: "demo-q-1", key: "Q-1", board_id: "demo-board", phase_id: "demo-phase", stakeholder_id: "demo-person", stakeholder: { name: "Product lead", role: "Business decisions", contact: "Local demo contact" }, kind: "business", title: "Which rollout should we prioritize?", body: "Choose the initial audience for Workyard.", options: ["Solo developers", "Small teams", "Other"], recommended_option: "Solo developers", recommendation_reason: "The local workflow is ready for individuals first.", status: "open", answer: "", answer_note: "", version: 1, blocks: [] },
  { id: "demo-q-2", key: "Q-2", board_id: "demo-runtime", phase_id: "demo-runtime-phase", stakeholder_id: "demo-engineering", stakeholder: { name: "Engineering lead", role: "Architecture review", contact: "Local demo contact" }, kind: "engineering", title: "How should we detect a stalled orchestrator?", body: "Choose the initial session signal.", options: ["Heartbeat timeout", "Process exit", "Other"], recommended_option: "Heartbeat timeout", recommendation_reason: "It reflects whether the session is responding.", status: "open", answer: "", answer_note: "", version: 1, blocks: [] },
  { id: "demo-q-3", key: "Q-3", board_id: "demo-board", phase_id: "demo-phase", stakeholder_id: null, kind: "engineering", title: "Which event fields should the graph use?", body: "This can be resolved by the agent without a stakeholder.", options: ["Current event payload", "Expanded graph payload", "Other"], recommended_option: "Current event payload", recommendation_reason: "It already provides the dependency IDs.", status: "open", answer: "", answer_note: "", version: 1, blocks: [] },
];
let demoDependencies: Dependency[] = [{id:"demo-dep-1",from_card_id:"demo-2",to_card_id:"demo-1",to_question_id:null,scope:"finish"}];
let demoNotes: CardNote[] = [];
let demoOrchestrator: Orchestrator = { board_key: "B-1", desired_state: "running", pool_size: 2, connected: true, holder: "cli:demo", epoch: 1, heartbeat_at: null, backing_off: false, backoff_until: null, backoff_reason: "", permissions: "full" };
const clone = <T,>(value: T): T => structuredClone(value);
function demoFindCard(id: string): Card {
  const card = demoBoard.lanes.flatMap((lane) => lane.cards).find((c) => c.id === id || c.key === id);
  if (!card) throw new Error(`Card ${id} not found`);
  return card;
}
function demoEvent(type: string, entityId: string) {
  demoEvents.push({ id: demoEvents.length + 1, type, board_id: demoBoard.id, entity_type: "card", entity_id: entityId, effects: [] });
}
const supervisorKey = (board: string) => `workyard.supervisor.${board}`;
/** Whether to start the board's orchestrator with `--supervisor`; kept per board in the browser. */
export function getSupervisorPreference(board: string): boolean {
  try { return localStorage.getItem(supervisorKey(board)) === "1"; } catch { return false; }
}
export function setSupervisorPreference(board: string, on: boolean) {
  try { localStorage.setItem(supervisorKey(board), on ? "1" : "0"); } catch { /* storage unavailable: the choice lasts until reload */ }
}
const demoApi = {
  openInTerminal: async (_card: string): Promise<TaskSession> => { throw new Error("Opening a terminal needs the desktop app"); },
  openProjectPlanner: async (_project: string): Promise<{ project: string }> => { throw new Error("Opening a planner needs the desktop app"); },
  inspectRepository: async (_path: string): Promise<RepositoryInfo> => { throw new Error("Creating a project needs the desktop app"); },
  pickFolder: async (): Promise<string | null> => null,
  createProjectFromRepository: async (_name: string, _description: string, _path: string, _branch: string): Promise<Project> => { throw new Error("Creating a project needs the desktop app"); },
  openPlanner: async (_board: string): Promise<{ board: string }> => { throw new Error("Opening a planner needs the desktop app"); },
  startOrchestrator: async (_board: string): Promise<Orchestrator> => { throw new Error("Starting an orchestrator needs the desktop app"); },
  stopOrchestrator: async (_board: string): Promise<Orchestrator> => { throw new Error("Stopping an orchestrator needs the desktop app"); },
  releaseSessionHold: async (_card: string): Promise<TaskSession> => { throw new Error("Releasing a hold needs the desktop app"); },
  holdSession: async (_card: string): Promise<TaskSession> => { throw new Error("Stopping an agent needs the desktop app"); },
  pendingPermissions: async (_board: string): Promise<PermissionRequest[]> => [],
  decidePermission: async (_request: string, _decision: PermissionDecision): Promise<PermissionRequest> => { throw new Error("Deciding a request needs the desktop app"); },
  agentLogTail: async (card: string, _limit = 60): Promise<AgentLogRow[]> => [
    { kind: "user", text: `You are working Workyard card ${card} on board B-1.` },
    { kind: "text", text: "I'll read the card, then write the storage module and its tests." },
    { kind: "tool", tool: "Bash", text: "workyard --json card " + card },
    { kind: "tool", tool: "Write", text: "storage.mjs" },
    { kind: "tool", tool: "Bash", text: "node --test" },
    { kind: "result", text: "ℹ tests 10  ℹ pass 10  ℹ fail 0" },
  ],
  setBoardIntegration: async (board: string, integration: "merge" | "rebase"): Promise<Board> => { const target = [demoBoard, demoRuntimeBoard, demoResearchBoard].find((item) => item.key === board || item.id === board) ?? demoBoard; target.integration = integration; demoEvent("board_integration_changed", target.id); return clone(target); },
  setBoardReviewRequired: async (board: string, required: boolean): Promise<Board> => { const target = [demoBoard, demoRuntimeBoard, demoResearchBoard].find((item) => item.key === board || item.id === board) ?? demoBoard; target.review_required = required; demoEvent("board_review_changed", target.id); return clone(target); },
  approveChange: async (change: CardChange): Promise<CardChange> => ({ ...clone(change), approved_commit_oid: change.commit_oid, reviewer: "demo", version: change.version + 1 }),
  orchestratorStatus: async (board: string): Promise<Orchestrator> => ({ ...clone(demoOrchestrator), board_key: board }),
  setOrchestratorState: async (board: string, state: Orchestrator["desired_state"]): Promise<Orchestrator> => { demoOrchestrator = { ...demoOrchestrator, desired_state: state }; demoEvent("orchestrator_state_changed", board); return { ...clone(demoOrchestrator), board_key: board }; },
  listProjects: async (): Promise<ProjectListItem[]> => [{ id: "demo-project", name: "Workyard", description: "Local agent workspace", version: 1, board_count: 2 }, { id: "demo-field-notes", name: "Field Notes", description: "A non-code research project", version: 1, board_count: 1 }],
  viewProject: async (project: string): Promise<Project> => project === "demo-field-notes" ? { id: "demo-field-notes", name: "Field Notes", description: "A non-code research project", version: 1, board_count: 1, boards: [{ id: demoResearchBoard.id, key: demoResearchBoard.key, name: demoResearchBoard.name, description: demoResearchBoard.description, archived: false }], repositories: [], documents: [], stakeholders: [], changes: [] } : { id: "demo-project", name: "Workyard", description: "Local agent workspace", version: 1, board_count: 2, boards: [demoBoard,demoRuntimeBoard].map((board)=>({id:board.id,key:board.key,name:board.name,description:board.description,archived:false})), repositories: [{ id: "demo-repo", path: "~/Development/workyard", target_branch: "main", remote_url: null }], documents: [{ id: "demo-adr", repository_id: "demo-repo", path: "docs/adr/0001-board-phases-and-scheduling.md", kind: "adr", title: "Board phases and scheduling" }, { id: "demo-diagram", repository_id: "demo-repo", path: "docs/architecture.md", kind: "diagram", title: "Workyard architecture" }, { id: "demo-doc", repository_id: "demo-repo", path: "docs/SPEC.md", kind: "documentation", title: "Build spec" }], stakeholders: [{ id: "demo-person", name: "Product lead", role: "Business decisions", contact: "Local demo contact" }, { id: "demo-engineering", name: "Engineering lead", role: "Architecture review", contact: "Local demo contact" }], changes: [] },
  listBoards: async (): Promise<BoardListItem[]> => [demoBoard,demoRuntimeBoard,demoResearchBoard].map((board) => ({ ...clone(board), lanes: board.lanes.map((lane) => ({ lane_id: lane.id, name: lane.name, count: lane.cards.length })) })),
  viewBoard: async (board: string): Promise<Board> => clone([demoBoard,demoRuntimeBoard,demoResearchBoard].find((item)=>item.key===board || item.id===board) ?? demoBoard),
  viewCard: async (card: string): Promise<CardDetail> => ({ ...clone(demoFindCard(card)), dependencies: clone(demoDependencies.filter((d) => d.from_card_id === demoFindCard(card).id || d.to_card_id === demoFindCard(card).id)), changes: [], report: null, notes: clone(demoNotes.filter((n) => n.card_id === demoFindCard(card).id)), session: null, events: [] }),
  addCardNote: async (card: string, body: string): Promise<CardNote> => { const note = { id: `demo-note-${demoNotes.length + 1}`, card_id: demoFindCard(card).id, actor: "ui", body: body.trim(), created_at: new Date().toISOString() }; demoNotes.push(note); demoEvent("card_note_added", note.card_id); return clone(note); },
  createBoard: async (name: string): Promise<Board> => { demoBoard.name = name; return clone(demoBoard); },
  createCard: async (_board: string, title: string): Promise<Card> => {
    const card: Card = { ...clone(demoBoard.lanes[0].cards[0]), id: `demo-${demoNextId}`, key: `C-${demoNextId++}`, title, position: demoBoard.lanes[0].cards.length, progress: 0, status_note: "", version: 1 };
    demoBoard.lanes[0].cards.push(card); demoEvent("card_created", card.id); return clone(card);
  },
  moveCard: async (card: Card, lane: string): Promise<Card> => {
    const target = demoBoard.lanes.find((l) => l.id === lane || l.name === lane)!;
    const current = demoFindCard(card.id);
    demoBoard.lanes.find((l) => l.id === current.lane_id)!.cards = demoBoard.lanes.find((l) => l.id === current.lane_id)!.cards.filter((c) => c.id !== current.id);
    current.lane_id = target.id; current.position = target.cards.length; current.version += 1;
    if (target.kind === "done") current.progress = 100;
    if (current.id === "demo-1") {
      const dependent=demoFindCard("demo-2");
      dependent.finish_blockers=target.kind === "done" ? [] : [{type:"card",id:"demo-1",key:"C-1",title:"Build the event stream"}];
      dependent.can_finish=target.kind === "done";
    }
    target.cards.push(current); demoEvent("card_moved", current.id); return clone(current);
  },
  updateCard: async (card: Card, title: string): Promise<Card> => { const c = demoFindCard(card.id); c.title = title; c.version++; demoEvent("card_updated", c.id); return clone(c); },
  claimCard: async (card: Card): Promise<Card> => { const c = demoFindCard(card.id); c.owner_actor = "ui"; c.version++; demoEvent("card_claimed", c.id); return clone(c); },
  releaseCard: async (card: Card): Promise<Card> => { const c = demoFindCard(card.id); c.owner_actor = null; c.version++; demoEvent("card_released", c.id); return clone(c); },
  transferCard: async (card: Card, toActor: string): Promise<Card> => { const c = demoFindCard(card.id); c.owner_actor = toActor; c.version++; demoEvent("card_transferred", c.id); return clone(c); },
  forceReleaseCard: async (card: Card, _reason: string): Promise<Card> => { const c = demoFindCard(card.id); c.owner_actor = null; c.version++; demoEvent("card_force_released", c.id); return clone(c); },
  updateProgress: async (card: Card, progress: number, note: string): Promise<Card> => { const c = demoFindCard(card.id); c.progress = progress; c.status_note = note; c.version++; demoEvent("progress_updated", c.id); return clone(c); },
  dbPath: async (): Promise<string> => "demo",
  latestEventId: async (): Promise<number> => demoEvents.length,
  pollEvents: async (since: number, limit = 100): Promise<EventPage> => {
    const events = demoEvents.filter((e) => e.id > since);
    const page = events.slice(0, limit);
    return { events: clone(page), cursor: page.at(-1)?.id ?? since, has_more: events.length > limit };
  },
  listQuestions: async (): Promise<Question[]> => clone(demoQuestions),
  raiseQuestion: async (_board: string, kind: "engineering" | "business", title: string, blocks: string[]): Promise<Question> => {
    const question: Question = { id: `demo-q-${demoQuestions.length + 1}`, key: `Q-${demoQuestions.length + 1}`, board_id: demoBoard.id, phase_id: "demo-phase", kind, title, body: "", options: ["Yes", "No", "Other"], recommended_option: "Other", recommendation_reason: "Describe the answer that best fits this question.", status: "open", answer: "", answer_note: "", version: 1, blocks };
    demoQuestions.push(question);
    for (const key of blocks) { const card = demoFindCard(key); card.blocked = true; card.ready = false; card.open_question_count++; card.blockers.push({ id: question.id, key: question.key, title, type: "question" }); card.version++; demoDependencies.push({ id: `demo-dep-${demoDependencies.length + 1}`, from_card_id: card.id, to_card_id: null, to_question_id: question.id }); }
    demoEvent("question_raised", question.id);
    return clone(question);
  },
  answerQuestion: async (question: Question, answer: string, note: string): Promise<Question> => {
    const found = demoQuestions.find((q) => q.id === question.id)!;
    found.status = "answered"; found.answer = answer; found.answer_note = note; found.version++;
    for (const key of found.blocks) { const card = demoFindCard(key); card.blockers = card.blockers.filter((b) => b.id !== found.id); card.blocked = card.blockers.length > 0; card.ready = !card.blocked; card.open_question_count--; }
    demoEvent("question_answered", found.id);
    return clone(found);
  },
  assignQuestionStakeholder: async (question: Question, stakeholder: string): Promise<Question> => {
    const found = demoQuestions.find((item) => item.id === question.id)!;
    found.stakeholder_id = stakeholder;
    found.stakeholder = stakeholder === "demo-person" ? { name: "Product lead", role: "Business decisions", contact: "Local demo contact" } : undefined;
    found.version++;
    demoEvent("question_stakeholder_assigned", found.id);
    return clone(found);
  },
  unassignQuestionStakeholder: async (question: Question): Promise<Question> => {
    const found = demoQuestions.find((item) => item.id === question.id)!;
    found.stakeholder_id = null;
    found.stakeholder = undefined;
    found.version++;
    demoEvent("question_stakeholder_unassigned", found.id);
    return clone(found);
  },
  addDependency: async (card: Card, target: string): Promise<unknown> => { const c = demoFindCard(card.id); const targetCard = target.startsWith("C-") ? demoFindCard(target) : null; const question = target.startsWith("Q-") ? demoQuestions.find((q) => q.key === target) : null; demoDependencies.push({ id: `demo-dep-${demoDependencies.length + 1}`, from_card_id: c.id, to_card_id: targetCard?.id ?? null, to_question_id: question?.id ?? null }); if (targetCard?.ready || question?.status === "open") { c.blocked = true; c.ready = false; c.blockers.push({ type: targetCard ? "card" : "question", id: targetCard?.id ?? question!.id, key: target, title: targetCard?.title ?? question!.title }); } c.version++; demoEvent("dependency_added", c.id); return clone(c); },
  removeDependency: async (card: Card, target: string): Promise<unknown> => { const c = demoFindCard(card.id); const targetId = target.startsWith("C-") ? demoFindCard(target).id : demoQuestions.find((q) => q.key === target)?.id; demoDependencies = demoDependencies.filter((d) => !(d.from_card_id === c.id && (d.to_card_id === targetId || d.to_question_id === targetId))); c.blockers = c.blockers.filter((b) => b.id !== targetId); c.blocked = c.blockers.length > 0; c.ready = !c.blocked; c.version++; demoEvent("dependency_removed", c.id); return clone(c); },
};

export const client: typeof api = isDemo ? demoApi : api;
export function simulateExternalMove() {
  if (!isDemo) return;
  const card = demoFindCard("C-1");
  const current = demoBoard.lanes.find((l) => l.id === card.lane_id)!;
  const next = demoBoard.lanes[(demoBoard.lanes.indexOf(current) + 1) % demoBoard.lanes.length];
  void demoApi.moveCard(card, next.id);
}
