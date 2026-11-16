import {
  AnimatePresence,
  LayoutGroup,
  motion,
  useReducedMotion,
} from "motion/react";
import {
  ArrowLeft,
  LockKeyhole,
  Bot,
  GitFork,
  CircleHelp,
} from "lucide-react";
import type { Board, Project, Question } from "@/api";
import { DependencyView } from "@/components/workspace/dependency-view";
import { Badge } from "@/components/ui/badge";
import { Card as UiCard } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

type Props = {
  board: Board;
  questions: Question[];
  stakeholders: Project["stakeholders"];
  view: BoardSection;
  phaseId: string | null;
  cues: Record<string, string>;
  onOpen: (id: string) => void;
  onAnswer: (question: Question, answer: string, note: string) => Promise<boolean>;
  onAssignStakeholder: (question: Question, stakeholderId: string) => Promise<boolean>;
  onOpenPhase: (phaseId: string) => void;
  onBackToGraph: () => void;
};

export type BoardSection = "kanban" | "dependencies";

const laneTone: Record<string, string> = {
  backlog: "bg-slate-400",
  active: "bg-blue-500",
  done: "bg-emerald-500",
  custom: "bg-violet-500",
};

export function BoardView({
  board,
  questions,
  stakeholders,
  view,
  phaseId,
  cues,
  onOpen,
  onAnswer,
  onAssignStakeholder,
  onOpenPhase,
  onBackToGraph,
}: Props) {
  const reduced = useReducedMotion();
  const phase = board.phases.find((item)=>item.id===phaseId) ?? board.phases.find((item)=>item.status==="current") ?? board.phases[0];
  return (
    <div className="flex h-full min-h-0 flex-col">
      <h1 className="sr-only">{board.name} — {view}</h1>
      {view === "dependencies" ? (
        <div className="min-h-0 flex-1">
          <DependencyView
            board={board}
            questions={questions}
            stakeholders={stakeholders}
            onOpenCard={onOpen}
            onAnswer={onAnswer}
            onAssignStakeholder={onAssignStakeholder}
            onOpenPhase={onOpenPhase}
          />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <div className="shrink-0 rounded-lg border bg-card px-3 py-2">
            <button className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground hover:text-foreground" onClick={onBackToGraph}><ArrowLeft className="size-3.5" />Back to phase graph</button>
            <div className="mt-1 flex items-center gap-2"><h2 className="text-base font-semibold">{phase?.name ?? "Phase"}</h2>{phase && <Badge variant="secondary">{phase.status === "closed" ? "Completed" : phase.status === "future" ? "Upcoming" : "Current"}</Badge>}</div>
            {phase?.description && <p className="mt-0.5 text-xs text-muted-foreground">{phase.description}</p>}
          </div>
          <LayoutGroup id={`board-${board.id}`}>
            <div className="grid min-h-0 flex-1 grid-flow-col auto-cols-[minmax(265px,1fr)] gap-3 overflow-x-auto pb-1">
              {board.lanes.map((lane) => {
                const visible = lane.cards.filter((card)=>!card.cancelled_at && card.phase_id===phase?.id);
                return (
                  <section
                    className="flex min-h-0 flex-col overflow-hidden rounded-xl border bg-muted/40 p-2"
                    key={lane.id}
                  >
                    <div className="mb-2 flex items-center gap-2 px-1 py-1">
                      <span
                        className={cn(
                          "size-2 rounded-full",
                          laneTone[lane.kind] ?? laneTone.custom,
                        )}
                      />
                      <h2 className="text-sm font-semibold">{lane.name}</h2>
                      <Badge
                        variant="secondary"
                        className="ml-auto tabular-nums"
                      >
                        {visible.length}
                      </Badge>
                    </div>
                    <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto pr-1">
                      {visible.map((card) => (
                        <motion.div
                          key={card.id}
                          layout
                          layoutId={`card-${card.id}`}
                          transition={
                            reduced
                              ? { duration: 0 }
                              : { layout: { duration: 0.4, ease: "easeInOut" } }
                          }
                          className={cn(
                            "relative",
                            cues[card.id] && `card-cue-${cues[card.id]}`,
                          )}
                        >
                          <UiCard
                            className="cursor-pointer rounded-lg border-border/90 px-2.5 py-1.5 shadow-sm transition-colors hover:border-primary/40"
                            onClick={() => onOpen(card.id)}
                          >
                            <div className="flex items-start gap-1.5">
                              <h3 className="min-w-0 flex-1 line-clamp-2 text-[13px] font-semibold leading-4">
                                {card.title}
                              </h3>
                              {card.blocking_impact > 0 && (
                                <Badge variant="outline" className="shrink-0 gap-0.5 border-violet-500/30 bg-violet-500/10 px-1 py-0 text-[9px] leading-4 text-violet-700 dark:text-violet-300" title={`Holds up ${card.blocking_impact} downstream cards`}>
                                  <GitFork className="size-2.5" /> {card.blocking_impact}
                                </Badge>
                              )}
                            </div>
                            {(board.phases.length>1 || card.agent_type || card.blocked || card.open_question_count > 0 || !card.can_finish) && (
                              <div className="mt-1 flex min-w-0 items-center gap-2 text-[10px] text-muted-foreground">
                                {board.phases.length>1 && <span className="shrink-0 truncate">{board.phases.find((phase)=>phase.id===card.phase_id)?.name}</span>}
                                {card.agent_type && (
                                  <span className="flex min-w-0 items-center gap-1 truncate">
                                    <Bot className="size-3 shrink-0" />
                                    {card.agent_type}
                                  </span>
                                )}
                                {card.blocked && (
                                  <span className="flex min-w-0 items-center gap-1 truncate text-amber-700 dark:text-amber-300">
                                    <LockKeyhole className="size-3 shrink-0" />
                                    {card.blockers[0]?.cancelled ? "Prerequisite cancelled" : "Blocked"}{card.blockers[0] ? `: ${card.blockers[0].title}` : ""}
                                  </span>
                                )}
                                {!card.can_finish && !card.blocked && (
                                  <span className="flex min-w-0 items-center gap-1 truncate text-indigo-700 dark:text-indigo-300">
                                    <GitFork className="size-3 shrink-0" />
                                    {card.finish_blockers?.[0] ? `Finishes after ${card.finish_blockers[0].key}` : "Awaiting review"}
                                  </span>
                                )}
                                {card.open_question_count > 0 && (
                                  <span className="ml-auto flex shrink-0 items-center gap-0.5">
                                    <CircleHelp className="size-3" />
                                    {card.open_question_count}
                                  </span>
                                )}
                              </div>
                            )}
                            <div className="mt-1.5 flex items-center gap-2">
                              <Progress value={card.progress} className="h-1" />
                              <span className="w-7 text-right font-mono text-[10px] tabular-nums text-muted-foreground">
                                {card.progress}%
                              </span>
                            </div>
                            <AnimatePresence mode="wait">
                              {card.status_note && (
                                <motion.p
                                  key={card.status_note}
                                  initial={{ opacity: 0 }}
                                  animate={{ opacity: 1 }}
                                  exit={{ opacity: 0 }}
                                  transition={{ duration: reduced ? 0 : 0.15 }}
                                  className="mt-0.5 truncate text-[10px] text-muted-foreground"
                                >
                                  {card.status_note}
                                </motion.p>
                              )}
                            </AnimatePresence>
                          </UiCard>
                        </motion.div>
                      ))}
                      {visible.length === 0 && (
                        <div className="flex min-h-28 items-center justify-center rounded-lg border border-dashed border-border/80 px-4 text-center text-xs text-muted-foreground">
                          No cards here yet
                        </div>
                      )}
                    </div>
                  </section>
                );
              })}
            </div>
          </LayoutGroup>
        </div>
      )}
    </div>
  );
}
