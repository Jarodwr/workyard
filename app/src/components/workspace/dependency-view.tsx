import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight, Bot, Check, GitFork, Link2,
  LockKeyhole, Minus, Plus, X, CircleHelp,
} from "lucide-react";
import { client, type Board, type Card, type Dependency, type Project, type Question } from "@/api";
import { AgentTerminalButton } from "@/components/workspace/agent-terminal-button";
import { stateTone } from "@/components/workspace/agents-dock";
import { agentState } from "@/lib/agents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card as UiCard } from "@/components/ui/card";
import { StakeholderPicker } from "@/components/workspace/question-card";
import {
  buildPhaseDependencyLayout, GRAPH_NODE_HEIGHT, GRAPH_NODE_WIDTH, questionOnlyBlockerCounts, routeDependencyEdges,
  traceDependencies, type GraphEdgeInput, type GraphNodeInput,
} from "@/lib/dependency-layout";
import { cn } from "@/lib/utils";

export function DependencyView({
  board, questions, stakeholders, onOpenCard, onOpenPhase, onAnswer, onAssignStakeholder,
  phaseFilter, showCompleted, onOpenInTerminal, onReleaseHold,
}: {
  board: Board;
  questions: Question[];
  stakeholders: Project["stakeholders"];
  onOpenCard: (id: string) => void;
  onOpenPhase: (id: string) => void;
  onAnswer: (question: Question, answer: string, note: string) => Promise<boolean>;
  onAssignStakeholder: (question: Question, stakeholderId: string) => Promise<boolean>;
  /** Chosen in the board toolbar. */
  phaseFilter: string;
  showCompleted: boolean;
  onOpenInTerminal: (card: Card) => Promise<boolean>;
  onReleaseHold: (card: Card) => Promise<boolean>;
}) {
  const [dependencies, setDependencies] = useState<Dependency[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [answeringId, setAnsweringId] = useState<string | null>(null);
  const [answer, setAnswer] = useState("");
  const [answerNote, setAnswerNote] = useState("");
  const graphScrollRef = useRef<HTMLDivElement>(null);
  const cards = useMemo(() => board.lanes.flatMap((lane) => lane.cards), [board]);
  const visiblePhases = board.phases.filter((phase) => showCompleted || phase.status !== "closed");
  // The board is re-read on every wake so session activity shows live, but edges only change
  // when a card does (adding or removing a dependency bumps its version), so only then reload them.
  const cardSignature = useMemo(() => cards.map((card) => `${card.id}:${card.version}`).join(","), [cards]);
  const cardsRef = useRef(cards);
  cardsRef.current = cards;

  useEffect(() => {
    let active = true;
    setLoadError(false);
    void Promise.all(cardsRef.current.map((card) => client.viewCard(card.key)))
      .then((details) => {
        if (!active) return;
        const unique = new Map<string, Dependency>();
        for (const detail of details) {
          for (const edge of detail.dependencies) {
            if (edge.from_card_id === detail.id) unique.set(edge.id, edge);
          }
        }
        setDependencies([...unique.values()]);
      })
      .catch(() => { if (active) setLoadError(true); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [cardSignature]);

  useEffect(() => setSelectedId(null), [board.id]);
  // A new phase selection starts from the top with nothing traced.
  useEffect(() => {
    setSelectedId(null);
    graphScrollRef.current?.scrollTo({ top: 0, left: 0 });
  }, [phaseFilter, showCompleted]);

  const { layout, questionOnlyCounts } = useMemo(() => {
    const laneById = new Map(board.lanes.map((lane) => [lane.id, lane]));
    const cardById = new Map(cards.map((card) => [card.id, card]));
    const questionById = new Map(
      questions.filter((question) => question.board_id === board.id && question.status === "open")
        .map((question) => [question.id, question]),
    );
    const allNodes: GraphNodeInput[] = [
      ...cards.map((card) => ({
        id: card.id,
        key: card.key,
        title: card.title,
        kind: "card" as const,
        detail: `${laneById.get(card.lane_id)?.name ?? "Card"}${card.agent_type ? ` · ${card.agent_type}` : ""}`,
        resolved: laneById.get(card.lane_id)?.kind === "done",
        phase_id: card.phase_id,
        cancelled: !!card.cancelled_at,
      })),
      ...[...questionById.values()].map((question) => ({
        id: question.id,
        key: question.key,
        title: question.title,
        kind: "question" as const,
        detail: `${question.kind} question`,
        resolved: question.status !== "open",
        phase_id: question.phase_id,
      })),
    ];
    const allEdges: GraphEdgeInput[] = dependencies.map((edge) => {
      const source = edge.to_card_id || edge.to_question_id || "";
      const sourceCard = cardById.get(source);
      const sourceQuestion = questionById.get(source);
      return {
        id: edge.id,
        source,
        target: edge.from_card_id,
        resolved: sourceCard
          ? laneById.get(sourceCard.lane_id)?.kind === "done"
          : sourceQuestion?.status !== "open",
        scope: edge.scope ?? "start",
      };
    });
    const questionOnlyCounts = questionOnlyBlockerCounts(allNodes, allEdges);
    const visiblePhaseIds = new Set(visiblePhases.map((phase) => phase.id));
    const visible = new Set(allNodes.filter((node) => visiblePhaseIds.has(node.phase_id ?? "") && (phaseFilter === "all" || node.phase_id === phaseFilter)).map((node) => node.id));
    const nodes = allNodes.filter((node) => visible.has(node.id));
    const edges = allEdges.filter((edge) => visible.has(edge.source) && visible.has(edge.target));
    const phases = visiblePhases.filter((phase) => phaseFilter === "all" || phase.id === phaseFilter);
    return { layout: buildPhaseDependencyLayout(nodes, edges, phases), questionOnlyCounts };
  }, [board, cards, questions, dependencies, phaseFilter, showCompleted]);

  const selected = selectedId ? layout.nodes.find((node) => node.id === selectedId) : null;
  const trace = selected ? traceDependencies(layout, selected.id) : null;
  const nodeById = new Map(layout.nodes.map((node) => [node.id, node]));
  const routes = useMemo(() => routeDependencyEdges(layout.nodes, layout.edges), [layout]);
  const cardById = new Map(cards.map((card) => [card.id, card]));
  const questionById = new Map(questions.map((question) => [question.id, question]));
  const answering = answeringId ? questionById.get(answeringId) : undefined;

  return (
    <div className="relative flex h-full min-h-0 flex-col">
      <div ref={graphScrollRef} className="min-h-0 flex-1 overflow-auto rounded-xl border bg-muted/20">
        {loading ? (
          <div className="flex h-full min-h-52 items-center justify-center text-sm text-muted-foreground">Loading dependency graph…</div>
        ) : loadError ? (
          <div className="flex h-full min-h-52 items-center justify-center text-sm text-destructive">Could not load dependencies. Switch views and try again.</div>
        ) : layout.phaseBands?.length === 0 ? (
          <UiCard className="m-4 flex min-h-52 flex-col items-center justify-center gap-2 border-dashed p-8 text-center shadow-none">
            <Link2 className="size-7 text-muted-foreground" />
            <strong className="text-sm">No connected work yet</strong>
            <p className="text-xs text-muted-foreground">{board.phases.some((phase)=>phase.status==="closed") && !showCompleted ? "Completed phases are hidden. Use Show completed to view them." : "Card and question dependencies will form a graph here."}</p>
          </UiCard>
        ) : (
          <div style={{ width: layout.width * zoom, height: layout.height * zoom }}>
            <div className="sticky top-0 z-20 -mb-14 flex h-14 border-b bg-background/95 backdrop-blur-sm" style={{ width: layout.width * zoom }}>
              {layout.phaseBands?.map((phase) => <div key={phase.id} className={cn("flex min-w-0 shrink-0 flex-col justify-center border-r border-border/70 px-4", phase.status === "current" ? "bg-primary/10" : "bg-background/95")} style={{ width: phase.width * zoom }}><button className="w-fit max-w-full truncate text-left text-xs font-semibold hover:text-primary hover:underline" onClick={()=>onOpenPhase(phase.id)} title={`Open ${phase.name} Kanban`}>{phase.name}</button><span className="line-clamp-2 text-[10px] leading-3 text-muted-foreground" title={phase.description}>{phase.description || `${phase.status} phase`}</span></div>)}
            </div>
            <div className="relative" style={{ width: layout.width, height: layout.height, transform: `scale(${zoom})`, transformOrigin: "top left" }}>
              {layout.phaseBands?.map((phase) => <div key={phase.id} className={cn("absolute top-0 h-full border-r border-dashed border-border/70", phase.status === "current" ? "bg-primary/5" : phase.status === "closed" ? "bg-muted/30" : "bg-background/50")} style={{ left: phase.x, width: phase.width }} />)}
              {layout.nodes.length === 0 && <p className="absolute left-6 top-24 text-xs text-muted-foreground">No cards or open questions in this phase yet.</p>}
              <svg className="pointer-events-none absolute inset-0" width={layout.width} height={layout.height} aria-hidden="true">
                <defs>
                  <marker id="arrow-sequence" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0 0 L7 3.5 L0 7 Z" fill="var(--primary)" /></marker>
                  <marker id="arrow-open" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0 0 L7 3.5 L0 7 Z" fill="#d89539" /></marker>
                  <marker id="arrow-resolved" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0 0 L7 3.5 L0 7 Z" fill="var(--muted-foreground)" /></marker>
                  <marker id="arrow-finish" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0 0 L7 3.5 L0 7 Z" fill="#6366f1" /></marker>
                </defs>
                {layout.edges.map((edge) => {
                  const from = nodeById.get(edge.source);
                  const to = nodeById.get(edge.target);
                  if (!from || !to) return null;
                  const path = routes.get(edge.id)?.d;
                  if (!path) return null;
                  const focused = !trace || (trace.related.has(edge.source) && trace.related.has(edge.target));
                  const arrow = edge.scope === "finish" && !edge.resolved ? "finish" : edge.longestChain ? "sequence" : edge.resolved ? "resolved" : "open";
                  return (
                    <path
                      key={edge.id}
                      d={path}
                      fill="none"
                      stroke={edge.scope === "finish" && !edge.resolved ? "#6366f1" : edge.longestChain ? "var(--primary)" : edge.resolved ? "var(--muted-foreground)" : "#d89539"}
                      strokeWidth={edge.longestChain || (focused && trace) ? 2.5 : 1.5}
                      strokeDasharray={edge.scope === "finish" && !edge.resolved ? "2 4" : !edge.resolved ? "5 4" : undefined}
                      markerEnd={`url(#arrow-${arrow})`}
                      opacity={trace ? focused ? 0.9 : 0.08 : edge.longestChain ? 0.95 : edge.resolved ? 0.3 : 0.48}
                    />
                  );
                })}
              </svg>
              {layout.nodes.map((node) => {
                const card = cardById.get(node.id);
                const question = questionById.get(node.id);
                const questionOnlyCount = questionOnlyCounts.get(node.id) ?? 0;
                const focused = !trace || trace.related.has(node.id);
                const agent = card ? agentState(card) : null;
                const running = agent?.kind === "running";
                return (
                  <div
                    key={node.id}
                    data-agent-state={agent?.kind}
                    className={cn(
                      "absolute border shadow-sm transition-[opacity,box-shadow]",
                      running && "node-running",
                      question ? "rounded-xl border-l-4" : "rounded-lg bg-card",
                      question?.kind === "engineering" ? "border-sky-500/70 bg-sky-500/10" : question?.kind === "business" ? "border-amber-500/70 bg-amber-500/10" : undefined,
                      node.chokepoint ? "ring-1 ring-violet-500/40" : node.kind === "card" && (node.longestChain ? "border-primary/60" : "border-border"),
                      selectedId === node.id && "ring-2 ring-primary",
                      questionOnlyCount > 0 && "outline outline-2 outline-red-500 outline-offset-1",
                      !focused && "opacity-25",
                      node.context && "opacity-60",
                      node.cancelled && "opacity-50 grayscale",
                    )}
                    style={{ left: node.x, top: node.y, width: GRAPH_NODE_WIDTH, height: GRAPH_NODE_HEIGHT }}
                  >
                    <button className="flex h-full w-full flex-col px-2.5 py-2 text-left" onClick={() => setSelectedId(selectedId === node.id ? null : node.id)} aria-pressed={selectedId === node.id} aria-label={`Trace dependencies for ${node.key} ${node.title}`}>
                      <span className="flex w-full items-center gap-1.5 pr-8 text-[10px] text-muted-foreground">
                        {question ? <CircleHelp className={cn("size-3.5", question.kind === "engineering" ? "text-sky-700 dark:text-sky-300" : "text-amber-700 dark:text-amber-300")} /> : node.cancelled ? <X className="size-3.5" /> : node.resolved ? <Check className="size-3.5 text-primary" /> : card?.blocked ? <LockKeyhole className="size-3.5 text-amber-600" /> : <Bot className="size-3.5" />}
                        <span className={cn("font-semibold", question?.kind === "engineering" && "text-sky-800 dark:text-sky-200", question?.kind === "business" && "text-amber-800 dark:text-amber-200")}>{question ? question.kind === "engineering" ? "Engineering" : "Business" : "Task"}</span>
                        {agent && (
                          <span className={cn("ml-auto inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[9px] font-semibold leading-3", stateTone[agent.kind])} title={agent.label}>
                            {running && <span className="agent-running-dot size-1.5 rounded-full bg-primary" aria-hidden />}
                            {agent.chip}
                          </span>
                        )}
                        {node.chokepoint && <Badge variant="outline" className={cn(agent ? "" : "ml-auto", "border-violet-500/30 bg-violet-500/10 px-1 py-0 text-[9px] text-violet-700 dark:text-violet-300")}><GitFork className="size-3" />Holds {node.impact}</Badge>}
                      </span>
                      <strong className="mt-1 line-clamp-2 w-full text-xs leading-4">{node.title}</strong>
                      <span className={cn("mt-auto w-full truncate pr-12 text-[10px] text-muted-foreground", running && card?.session?.activity && "text-primary")} title={running ? card?.session?.activity : undefined}>{node.cancelled ? `Cancelled: ${card?.cancellation_reason ?? ""}` : card ? (running && card.session?.activity ? card.session.activity : node.detail) : questionOnlyCount > 0 ? `Only questions block ${questionOnlyCount} task${questionOnlyCount === 1 ? "" : "s"}` : question?.recommended_option ? `Suggests: ${question.recommended_option}` : node.resolved ? "Resolved" : "Open"}</span>
                    </button>
                    {question && <div className="absolute right-1.5 top-1.5"><StakeholderPicker question={question} stakeholders={stakeholders} compact onAssign={(stakeholderId)=>onAssignStakeholder(question,stakeholderId)} /></div>}
                    {card && <AgentTerminalButton card={card} onOpen={onOpenInTerminal} onRelease={onReleaseHold} className="absolute bottom-1.5 right-7" />}
                    <button className="absolute bottom-1.5 right-1.5 rounded p-1 text-muted-foreground hover:bg-accent hover:text-primary" aria-label={card ? `Open ${node.key} details` : `Answer ${node.key} question`} onClick={() => card ? onOpenCard(card.id) : (setAnsweringId(node.id), setAnswer(""), setAnswerNote(""))}><ArrowUpRight className="size-3.5" /></button>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
      <div className="pointer-events-none absolute inset-x-4 bottom-4 z-30 flex items-end justify-between gap-3">
        {selected ? (
          <button className="pointer-events-auto flex items-center gap-1.5 rounded-md border bg-card/95 px-2.5 py-1.5 text-[11px] font-medium shadow-sm hover:text-primary" onClick={() => setSelectedId(null)}>
            {selected.key}: {trace?.upstream.size} upstream · {trace?.downstream.size} downstream <X className="size-3" />
          </button>
        ) : <span />}
        <div className="pointer-events-auto flex items-center gap-0.5 rounded-md border bg-card/95 p-0.5 shadow-sm">
          <Button variant="ghost" size="icon" className="size-6" aria-label="Zoom out" disabled={zoom <= 0.7} onClick={() => setZoom((value) => Math.max(0.7, +(value - 0.15).toFixed(2)))}><Minus className="size-3" /></Button>
          <span className="w-9 text-center font-mono text-[10px]">{Math.round(zoom * 100)}%</span>
          <Button variant="ghost" size="icon" className="size-6" aria-label="Zoom in" disabled={zoom >= 1.3} onClick={() => setZoom((value) => Math.min(1.3, +(value + 0.15).toFixed(2)))}><Plus className="size-3" /></Button>
        </div>
      </div>
      {answering && <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onMouseDown={(event)=>{if(event.target===event.currentTarget)setAnsweringId(null);}}>
        <div role="dialog" aria-modal="true" aria-label={`Answer ${answering.title}`} className="w-full max-w-lg rounded-xl border bg-background p-5 shadow-xl">
          <div className="flex items-start justify-between gap-3"><div className="min-w-0 flex-1"><p className="text-xs capitalize text-muted-foreground">{answering.kind} question</p><h2 className="mt-1 text-base font-semibold">{answering.title}</h2></div><StakeholderPicker question={answering} stakeholders={stakeholders} onAssign={(stakeholderId)=>onAssignStakeholder(answering,stakeholderId)} /><Button variant="ghost" size="icon" aria-label="Close answer dialog" onClick={()=>setAnsweringId(null)}><X className="size-4" /></Button></div>
          {answering.body && <p className="mt-2 text-sm text-muted-foreground">{answering.body}</p>}
          {answering.recommended_option && <p className="mt-3 rounded-md border bg-muted/30 p-2 text-xs"><strong>Agent recommends: {answering.recommended_option}</strong>{answering.recommendation_reason && <span className="block text-muted-foreground">{answering.recommendation_reason}</span>}</p>}
          <fieldset className="mt-4 space-y-2"><legend className="mb-2 text-xs font-medium">Choose an option</legend>{answering.options.map((option)=><label key={option} className="flex cursor-pointer items-center gap-2 rounded-md border p-2 text-sm"><input type="radio" name="graph-answer" checked={answer===option} onChange={()=>setAnswer(option)} />{option}</label>)}</fieldset>
          <textarea aria-label="Answer explanation" className="mt-3 w-full rounded-md border bg-background p-2 text-sm" rows={2} placeholder={answer==="Other" ? "Describe your answer…" : "Optional explanation…"} value={answerNote} onChange={(event)=>setAnswerNote(event.target.value)} />
          <div className="mt-3 flex justify-end"><Button size="sm" disabled={!answering.options.includes(answer) || (answer==="Other" && !answerNote.trim())} onClick={async()=>{if(await onAnswer(answering,answer,answerNote.trim()))setAnsweringId(null);}}>Answer question</Button></div>
        </div>
      </div>}
    </div>
  );
}
