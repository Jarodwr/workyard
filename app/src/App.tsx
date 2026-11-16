import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, FolderKanban, LayoutDashboard, UserRound, X } from "lucide-react";
import {
  client,
  isDemo,
  type Board,
  type BoardEvent,
  type BoardListItem,
  type Orchestrator,
  type CardDetail as Detail,
  type Question,
  type Project,
  type ProjectListItem,
} from "@/api";
import { BoardView, type BoardSection } from "@/components/workspace/board-view";
import { ProjectOverview } from "@/components/workspace/project-view";
import { StakeholderView } from "@/components/workspace/stakeholder-view";
import { CardDetail } from "@/components/workspace/card-detail";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { loadViewState, restoreTarget, saveViewState } from "@/lib/view-state";

function errorMessage(error: unknown): string {
  if (typeof error === "string") {
    try {
      return JSON.parse(error).message ?? error;
    } catch {
      return error;
    }
  }
  return error instanceof Error ? error.message : String(error);
}

export default function App() {
  const unassignedId = "__unassigned__";
  const [boards, setBoards] = useState<BoardListItem[]>([]);
  const [projects, setProjects] = useState<ProjectListItem[]>([]);
  const [project, setProject] = useState<Project | null>(null);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [selectedStakeholderId, setSelectedStakeholderId] = useState<string | null>(null);
  const [board, setBoard] = useState<Board | null>(null);
  const [boardKey, setBoardKey] = useState<string | null>(null);
  const [section, setSection] = useState<BoardSection>("dependencies");
  const [phaseId, setPhaseId] = useState<string | null>(null);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [cues, setCues] = useState<Record<string, string>>({});
  const [orchestrator, setOrchestrator] = useState<Orchestrator | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  // The database path keys what is remembered; empty until the first read finishes.
  const [scope, setScope] = useState("");
  const cursor = useRef(0);
  const boardRef = useRef<Board | null>(null);
  boardRef.current = board;

  const refresh = useCallback(
    async (key: string | null, selectedProject?: string | null) => {
      const [nextBoards, nextQuestions, nextBoard, nextProjects, nextProject, nextOrchestrator] = await Promise.all([
        client.listBoards(),
        client.listQuestions(),
        key ? client.viewBoard(key) : Promise.resolve(null),
        client.listProjects(),
        selectedProject && selectedProject !== unassignedId ? client.viewProject(selectedProject) : Promise.resolve(null),
        key ? client.orchestratorStatus(key) : Promise.resolve(null),
      ]);
      setBoards(nextBoards);
      setProjects(nextProjects);
      if (nextProject) setProject(nextProject);
      setQuestions(nextQuestions);
      if (nextBoard) setBoard(nextBoard);
      setOrchestrator(nextOrchestrator);
    },
    [],
  );

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        cursor.current = await client.latestEventId();
        const path = await client.dbPath().catch(() => "");
        await refresh(null);
        if (path && active) {
          const saved = loadViewState(path);
          const [knownProjects, knownBoards] = await Promise.all([client.listProjects(), client.listBoards()]);
          const target = restoreTarget(saved, knownProjects.map((item) => item.id), knownBoards, unassignedId);
          if (active) {
            setProjectId(target.projectId);
            setBoardKey(target.boardKey);
            if (target.boardKey) {
              setSection(saved.section ?? "dependencies");
              setPhaseId(saved.phaseId ?? null);
            }
            setScope(path);
          }
        }
      } catch (e) {
        if (active) setError(errorMessage(e));
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [refresh]);

  useEffect(() => {
    if (!scope || loading) return;
    saveViewState(scope, { projectId, boardKey, section, phaseId });
  }, [scope, loading, projectId, boardKey, section, phaseId]);

  useEffect(() => {
    setOrchestrator(null);
    if (!boardKey) {
      setBoard(null);
      return;
    }
    let active = true;
    void client
      .orchestratorStatus(boardKey)
      .then((value) => {
        if (active) setOrchestrator(value);
      })
      .catch((e) => {
        if (active) setError(errorMessage(e));
      });
    void client
      .viewBoard(boardKey)
      .then((value) => {
        if (active) setBoard(value);
      })
      .catch((e) => {
        if (active) setError(errorMessage(e));
      });
    return () => {
      active = false;
    };
  }, [boardKey]);

  useEffect(() => {
    if (!projectId || projectId === unassignedId) return;
    let active = true;
    void client.viewProject(projectId).then((value) => { if (active) setProject(value); }).catch((e) => { if (active) setError(errorMessage(e)); });
    return () => { active = false; };
  }, [projectId]);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    let active = true;
    void client
      .viewCard(selectedId)
      .then((value) => {
        if (active) setDetail(value);
      })
      .catch((e) => {
        if (active) setError(errorMessage(e));
      });
    return () => {
      active = false;
    };
  }, [selectedId, board]);

  useEffect(() => {
    let active = true;
    let busy = false;
    const poll = async () => {
      if (busy || loading) return;
      busy = true;
      try {
        const events: BoardEvent[] = [];
        let more = false;
        do {
          const page = await client.pollEvents(cursor.current);
          cursor.current = page.cursor;
          events.push(...page.events);
          more = page.has_more;
        } while (more && active);
        // Lease changes (an orchestrator exiting, its lease expiring) write no events, so
        // read the status on every wake rather than only when something was written.
        if (boardKey) {
          const next = await client.orchestratorStatus(boardKey);
          if (active) setOrchestrator((previous) => (JSON.stringify(previous) === JSON.stringify(next) ? previous : next));
        }
        if (!active) return;
        if (events.length === 0) {
          // A worker's live activity is stored without a board event, so a board with running
          // sessions is reread on every wake, and only replaced when something changed.
          const current = boardRef.current;
          if (boardKey && current?.key === boardKey && current.lanes.some((lane) => lane.cards.some((card) => card.session && !card.session.paused))) {
            const next = await client.viewBoard(boardKey);
            if (active) setBoard((previous) => (JSON.stringify(previous) === JSON.stringify(next) ? previous : next));
          }
          return;
        }
        await refresh(boardKey, projectId);
        if (!active) return;
        if (events.length <= 10) {
          const nextCues: Record<string, string> = {};
          for (const event of events) {
            if (event.entity_type === "card")
              nextCues[event.entity_id] = "changed";
            for (const effect of event.effects) {
              if (!effect.card_id) continue;
              if (["card_became_ready", "card_unblocked", "card_can_finish"].includes(effect.type))
                nextCues[effect.card_id] = "ready";
              else if (["card_became_blocked", "card_finish_blocked"].includes(effect.type))
                nextCues[effect.card_id] = "blocked";
              else if (!nextCues[effect.card_id])
                nextCues[effect.card_id] = "changed";
            }
          }
          setCues(nextCues);
          window.setTimeout(() => setCues({}), 1300);
        }
      } catch (e) {
        if (active) setError(errorMessage(e));
      } finally {
        busy = false;
      }
    };
    // The desktop wakes on the core's commit tick (and every two seconds regardless);
    // the demo has no backend, so it keeps its fast poll.
    const fallback = window.setInterval(() => {
      void poll();
    }, isDemo ? 250 : 3000);
    let unlisten: (() => void) | undefined;
    if (!isDemo) {
      void listen("workyard-tick", () => void poll()).then((stop) => {
        if (active) unlisten = stop;
        else stop();
      });
    }
    return () => {
      active = false;
      window.clearInterval(fallback);
      unlisten?.();
    };
  }, [boardKey, projectId, refresh, loading]);

  const selected =
    board?.lanes
      .flatMap((lane) => lane.cards)
      .find((card) => card.id === selectedId) ?? null;
  const selectedStakeholder = project && project.id === projectId
    ? project.stakeholders.find((person) => person.id === selectedStakeholderId)
    : undefined;
  const closeDetail = useCallback(() => setSelectedId(null), []);

  async function mutate(
    action: () => Promise<unknown>,
    key = boardKey,
  ): Promise<boolean> {
    try {
      setError("");
      await action();
      await refresh(key,projectId);
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    }
  }

  return (
    <div className="flex h-screen min-h-[600px] overflow-hidden bg-background text-foreground">
      {projectId!==null && <aside className="flex w-[238px] shrink-0 flex-col border-r bg-card/75">
        <div className="min-h-0 flex-1 overflow-y-auto px-3 py-5">
            <button className="mb-5 flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left hover:bg-accent" onClick={() => {setProjectId(null);setBoardKey(null);setSelectedStakeholderId(null);closeDetail();}} aria-label="Back to projects">
              <ArrowLeft className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 truncate text-sm font-semibold">{projectId===unassignedId ? "Unassigned boards" : project?.id===projectId ? project.name : projects.find((item)=>item.id===projectId)?.name ?? "Project"}</span>
            </button>
            {projectId!==unassignedId && <nav aria-label="Project sections" className="space-y-0.5">
              <SectionButton icon={<FolderKanban />} label="Overview" active={!boardKey && !selectedStakeholderId} onClick={() => {setBoardKey(null);setSelectedStakeholderId(null);closeDetail();}} />
            </nav>}
            {projectId!==unassignedId && <>
              <div className="mb-2 mt-6 flex items-center justify-between px-3"><span className="text-[10px] font-semibold uppercase tracking-[.14em] text-muted-foreground">Stakeholders</span><span className="text-[10px] text-muted-foreground">{project?.id===projectId ? project.stakeholders.length : 0}</span></div>
              <nav aria-label="Project stakeholders" className="space-y-1">
                {(project?.id===projectId ? project.stakeholders : []).map((person)=>{
                  const pending=questions.filter((question)=>question.stakeholder_id===person.id && question.status==="open" && project?.boards.some((board)=>board.id===question.board_id)).length;
                  return <NavButton key={person.id} icon={<UserRound />} label={person.name} count={pending} showZero active={!boardKey && selectedStakeholderId===person.id} onClick={()=>{setBoardKey(null);setSelectedStakeholderId(person.id);closeDetail();}} />;
                })}
                {project?.id===projectId && project.stakeholders.length===0 && <p className="px-3 py-2 text-xs text-muted-foreground">No stakeholders yet.</p>}
              </nav>
            </>}
            <div className="mb-2 mt-6 flex items-center justify-between px-3"><span className="text-[10px] font-semibold uppercase tracking-[.14em] text-muted-foreground">Boards</span><span className="text-[10px] text-muted-foreground">{boards.filter((item)=>projectId===unassignedId ? !item.project_id : item.project_id===projectId).length}</span></div>
            <nav aria-label="Project boards" className="space-y-1">
              {boards.filter((item)=>projectId===unassignedId ? !item.project_id : item.project_id===projectId).map((item)=>{
                const active=boardKey===item.key;
                return <NavButton key={item.id} icon={<LayoutDashboard />} label={item.name} active={active} count={item.lanes.reduce((n,lane)=>n+lane.count,0)} onClick={()=>{setBoardKey(item.key);setSelectedStakeholderId(null);setSection("dependencies");setPhaseId(null);closeDetail();}} />;
              })}
              {boards.every((item)=>projectId===unassignedId ? !!item.project_id : item.project_id!==projectId) && <p className="px-3 py-2 text-xs text-muted-foreground">No boards in this project yet.</p>}
            </nav>
        </div>
        <div className="border-t p-4">
          <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
            <span className="size-1.5 rounded-full bg-primary" /> Local SQLite ·{" "}
            {isDemo ? "Demo" : "Desktop"}
          </div>
        </div>
      </aside>}

      <div className="flex min-w-0 flex-1 flex-col">
        {error && (
          <div
            role="alert"
            className="mx-7 mt-4 flex items-center justify-between rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-2.5 text-xs text-destructive"
          >
            <span>{error}</span>
            <Button
              size="icon"
              variant="ghost"
              className="size-7"
              aria-label="Dismiss error"
              onClick={() => setError("")}
            >
              <X />
            </Button>
          </div>
        )}
        <main className="min-h-0 flex-1 overflow-hidden p-3">
          {loading ? (
            <div className="py-20 text-center text-sm text-muted-foreground">
              Loading Workyard…
            </div>
          ) : projectId===null ? (
            <div className="h-full overflow-auto p-4">
              <h1 className="text-2xl font-bold tracking-tight">Projects</h1>
              <p className="mt-1 text-sm text-muted-foreground">Choose a project to see its context and boards.</p>
              <div className="mt-6 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                {projects.map((item)=><button key={item.id} className="rounded-xl border bg-card p-5 text-left transition-colors hover:border-primary/50 hover:bg-accent/40" onClick={()=>{setProjectId(item.id);setSelectedStakeholderId(null);setBoardKey(null);}}><FolderKanban className="size-5 text-primary" /><h2 className="mt-3 text-base font-semibold">{item.name}</h2><p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{item.description || "No description yet."}</p><p className="mt-4 text-xs text-muted-foreground">{item.board_count} {item.board_count===1 ? "board" : "boards"}</p></button>)}
                {boards.some((item)=>!item.project_id) && <button className="rounded-xl border bg-card p-5 text-left transition-colors hover:border-primary/50 hover:bg-accent/40" onClick={()=>{setProjectId(unassignedId);setBoardKey(null);}}><FolderKanban className="size-5 text-muted-foreground" /><h2 className="mt-3 text-base font-semibold">Unassigned boards</h2><p className="mt-1 text-xs text-muted-foreground">Boards waiting to be attached to a project.</p></button>}
              </div>
            </div>
          ) : boardKey && board?.key===boardKey ? (
            <BoardView
              key={board.id}
              board={board}
              questions={questions.filter((question) => question.board_id === board.id)}
              stakeholders={project && project.id===board.project_id ? project.stakeholders : []}
              view={section}
              phaseId={phaseId}
              scope={scope}
              cues={cues}
              orchestrator={orchestrator}
              onOrchestratorState={(state) => mutate(() => client.setOrchestratorState(board.key, state))}
              onStartOrchestrator={() => void mutate(() => client.startOrchestrator(board.key))}
              onStopOrchestrator={() => void mutate(() => client.stopOrchestrator(board.key))}
              onPlan={() => void mutate(() => client.openPlanner(board.key))}
              onOpenInTerminal={(card) => mutate(() => client.openInTerminal(card.key))}
              onReleaseHold={(card) => mutate(() => client.releaseSessionHold(card.key))}
              onHold={(card) => mutate(() => client.holdSession(card.key))}
              onAddNote={(card, body) => mutate(() => client.addCardNote(card.key, body))}
              onReviewRequired={(required) => void mutate(() => client.setBoardReviewRequired(board.key, required))}
              onIntegration={(integration) => void mutate(() => client.setBoardIntegration(board.key, integration))}
              onOpen={setSelectedId}
              onAnswer={(question, answer, note) => mutate(() => client.answerQuestion(question, answer, note))}
              onAssignStakeholder={(question, stakeholderId) => mutate(() => stakeholderId ? client.assignQuestionStakeholder(question, stakeholderId) : client.unassignQuestionStakeholder(question))}
              onOpenPhase={(selectedPhaseId)=>{setPhaseId(selectedPhaseId);setSection("kanban");closeDetail();}}
              onBackToGraph={()=>{setSection("dependencies");setPhaseId(null);closeDetail();}}
            />
          ) : boardKey ? (
            <div className="p-8 text-sm text-muted-foreground">Loading board…</div>
          ) : projectId===unassignedId ? (
            <div className="p-8 text-sm text-muted-foreground">Select an unassigned board from the sidebar.</div>
          ) : project && project.id===projectId && selectedStakeholder ? (
            <StakeholderView project={project} stakeholder={selectedStakeholder} questions={questions} onAnswer={(question, answer, note) => mutate(() => client.answerQuestion(question, answer, note))} onAssignStakeholder={(question, stakeholderId) => mutate(() => stakeholderId ? client.assignQuestionStakeholder(question, stakeholderId) : client.unassignQuestionStakeholder(question))} />
          ) : project?.id===projectId ? (
            <ProjectOverview project={project} onOpenBoard={(key) => {setBoardKey(key);setSelectedStakeholderId(null);setSection("dependencies");setPhaseId(null);}} />
          ) : (
            <div className="p-8 text-sm text-muted-foreground">Loading project…</div>
          )}
        </main>
      </div>

      <CardDetail
        card={selected}
        detail={detail}
        board={board}
        questions={questions.filter((question) => question.board_id === board?.id)}
        onClose={closeDetail}
        onAddNote={(card, body) => mutate(() => client.addCardNote(card.key, body))}
        onApproveChange={(change) => mutate(() => client.approveChange(change))}
        onOpenInTerminal={(card) => mutate(() => client.openInTerminal(card.key))}
        onReleaseHold={(card) => mutate(() => client.releaseSessionHold(card.key))}
      />
    </div>
  );
}

function NavButton({
  icon,
  label,
  count,
  showZero = false,
  active,
  onClick,
}: {
  icon: React.ReactElement;
  label: string;
  count: number;
  showZero?: boolean;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-9 w-full items-center gap-2.5 rounded-md px-3 text-left text-xs font-medium transition-colors hover:bg-accent hover:text-accent-foreground",
        active ? "bg-accent text-accent-foreground" : "text-muted-foreground",
      )}
      onClick={onClick}
    >
      {<span className="[&_svg]:size-4">{icon}</span>}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {(count > 0 || showZero) && (
        <span className="rounded-md bg-background/70 px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">
          {count}
        </span>
      )}
    </button>
  );
}

function SectionButton({
  icon,
  label,
  count = 0,
  active,
  tone,
  onClick,
}: {
  icon: React.ReactElement;
  label: string;
  count?: number;
  active: boolean;
  tone?: "business" | "engineering";
  onClick: () => void;
}) {
  return (
    <button
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-xs transition-colors hover:bg-accent",
        active ? "bg-accent font-semibold text-foreground" : "text-muted-foreground",
        active && tone === "business" && "bg-amber-500/10 text-amber-800 dark:text-amber-200",
        active && tone === "engineering" && "bg-sky-500/10 text-sky-800 dark:text-sky-200",
      )}
      onClick={onClick}
    >
      <span className="[&_svg]:size-3.5">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {count > 0 && <span className="text-[10px] tabular-nums">{count}</span>}
    </button>
  );
}
