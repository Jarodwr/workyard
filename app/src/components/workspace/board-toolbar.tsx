import { useState, type ReactNode } from "react";
import { Bot, ClipboardList, Hourglass, Info, Pause, Play, Settings2, ShieldAlert, ShieldCheck, Square, Waves } from "lucide-react";
import { getSupervisorPreference, setSupervisorPreference, type Orchestrator } from "@/api";
import { Button } from "@/components/ui/button";
import { Popover } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

type Props = {
  boardKey: string;
  /** View-specific controls: the phase picker on the graph, the way back on a phase board. */
  left: ReactNode;
  orchestrator: Orchestrator | null;
  onOrchestratorState: (state: Orchestrator["desired_state"]) => void;
  onStartOrchestrator: () => void;
  onStopOrchestrator: () => void;
  onPlan: () => void;
  /** The legend explains the graph's lines, so only the graph shows it. */
  showLegend: boolean;
  agentsOpen: boolean;
  runningAgents: number;
  onToggleAgents: () => void;
  reviewRequired: boolean;
  onReviewRequired: (required: boolean) => void;
};

/** One slim bar: view controls on the left, legend, orchestrator and planner on the right. */
export function BoardToolbar({ boardKey, left, orchestrator, onOrchestratorState, onStartOrchestrator, onStopOrchestrator, onPlan, showLegend, agentsOpen, runningAgents, onToggleAgents, reviewRequired, onReviewRequired }: Props) {
  return (
    <div className="mb-2 flex shrink-0 items-center gap-2 rounded-lg border bg-card px-2.5 py-1.5 text-xs">
      <div className="flex min-w-0 flex-1 items-center gap-3">{left}</div>
      {showLegend && <LegendButton />}
      <Button size="sm" variant="ghost" className={cn("h-7 gap-1.5 px-2 text-xs", agentsOpen && "bg-accent")} onClick={onToggleAgents} aria-pressed={agentsOpen} title="Show or hide the agents dock">
        <Bot className="size-3.5" /> Agents
        {runningAgents > 0 && <span className="rounded-full bg-primary px-1.5 text-[10px] font-semibold tabular-nums leading-4 text-primary-foreground" aria-label={`${runningAgents} running`}>{runningAgents}</span>}
      </Button>
      <SettingsButton boardKey={boardKey} orchestrator={orchestrator} reviewRequired={reviewRequired} onReviewRequired={onReviewRequired} />
      <OrchestratorControls boardKey={boardKey} orchestrator={orchestrator} onState={onOrchestratorState} onStart={onStartOrchestrator} onStop={onStopOrchestrator} />
      <Button size="sm" variant="outline" className="h-7 gap-1.5 px-2.5 text-xs" onClick={onPlan} title="Open a planner agent in your terminal, pointed at this board">
        <ClipboardList className="size-3.5" /> Plan
      </Button>
    </div>
  );
}

function SettingsButton({ boardKey, orchestrator, reviewRequired, onReviewRequired }: { boardKey: string; orchestrator: Orchestrator | null; reviewRequired: boolean; onReviewRequired: (required: boolean) => void }) {
  const full = orchestrator?.permissions !== "safe";
  const [supervisor, setSupervisor] = useState(() => getSupervisorPreference(boardKey));
  return (
    <Popover
      trigger={({ open, toggle }) => (
        <Button size="icon" variant="ghost" className={cn("size-7", open && "bg-accent")} onClick={toggle} aria-expanded={open} aria-label="Board settings" title="Worker permissions and review">
          <Settings2 className="size-3.5" />
        </Button>
      )}
      className="w-80"
    >
      <p className="mb-2 font-medium text-foreground">Workers</p>
      <p className="flex items-start gap-2 text-muted-foreground">
        {full ? <ShieldAlert className="mt-0.5 size-3.5 shrink-0 text-amber-600 dark:text-amber-300" /> : <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-emerald-600" />}
        <span>
          <strong className="text-foreground">{full ? "Full access" : "Safe"}</strong>
          {full
            ? ": workers run without permission prompts. A worktree isolates git state, not your machine."
            : ": workers may only use the tools on the allowlist; anything else is refused."}
          {!orchestrator?.connected && " Set when the orchestrator starts."}
        </span>
      </p>
      <label className="mt-3 flex cursor-pointer items-start gap-2 text-muted-foreground">
        <input type="checkbox" className="mt-0.5" checked={supervisor} onChange={(event) => { setSupervisor(event.target.checked); setSupervisorPreference(boardKey, event.target.checked); }} />
        <span><strong className="text-foreground">Supervisor agent</strong>: also run an agent that handles what needs judgement. Applies the next time you start the orchestrator.</span>
      </label>
      <p className="mb-2 mt-3 border-t pt-3 font-medium text-foreground">Review</p>
      <label className="flex cursor-pointer items-start gap-2 text-muted-foreground">
        <input type="checkbox" className="mt-0.5" checked={reviewRequired} onChange={(event) => onReviewRequired(event.target.checked)} />
        <span><strong className="text-foreground">Require review before merge</strong>: new code changes wait for your approval in the card. Off by default for local projects.</span>
      </label>
    </Popover>
  );
}

function LegendButton() {
  return (
    <Popover
      trigger={({ open, toggle }) => (
        <Button size="sm" variant="ghost" className={cn("h-7 gap-1.5 px-2 text-xs", open && "bg-accent")} onClick={toggle} aria-expanded={open}>
          <Info className="size-3.5" /> Legend
        </Button>
      )}
    >
      <p className="mb-2 font-medium text-foreground">Prerequisite → dependent</p>
      <ul className="space-y-1.5 text-muted-foreground">
        <li className="flex items-center gap-2"><span className="h-0.5 w-6 shrink-0 bg-primary" /> Longest sequence</li>
        <li className="flex items-center gap-2"><span className="w-6 shrink-0 border-t-2 border-dashed border-amber-500" /> Open dependency</li>
        <li className="flex items-center gap-2"><span className="h-0.5 w-6 shrink-0 bg-slate-400" /> Resolved</li>
        <li className="flex items-center gap-2"><span className="w-6 shrink-0 border-t-2 border-dotted border-indigo-500" /> Must finish first</li>
        <li className="flex items-center gap-2"><span className="size-3 shrink-0 rounded-sm border-2 border-red-500" /> Only questions remain</li>
      </ul>
      <p className="mt-2 border-t pt-2 text-muted-foreground">Select a node to trace its chain upstream and downstream.</p>
    </Popover>
  );
}

function OrchestratorControls({ boardKey, orchestrator, onState, onStart, onStop }: { boardKey: string; orchestrator: Orchestrator | null; onState: (state: Orchestrator["desired_state"]) => void; onStart: () => void; onStop: () => void }) {
  if (!orchestrator) return null;
  const { connected, desired_state: state } = orchestrator;
  const label = !connected ? "No orchestrator" : state === "running" ? `Running · ${orchestrator.pool_size} at once` : state === "paused" ? "Paused" : "Draining";
  const until = orchestrator.backoff_until ? new Date(orchestrator.backoff_until) : null;
  return (
    <div className="flex items-center gap-1.5 border-l pl-2.5" aria-label="Orchestrator">
      <span className={cn("size-2 shrink-0 rounded-full", connected ? "bg-emerald-500" : "bg-muted-foreground/40")} aria-hidden />
      <span
        className="text-muted-foreground"
        title={connected ? `Orchestrator ${orchestrator.holder ?? ""}` : `Start one with: workyard orch run ${boardKey} --cwd <repository>`}
      >
        {label}{connected && orchestrator.permissions && ` · ${orchestrator.permissions === "safe" ? "Safe" : "Full access"}`}
      </span>
      {orchestrator.backing_off && (
        <span className="inline-flex items-center gap-1 text-amber-700 dark:text-amber-300" title={orchestrator.backoff_reason}>
          <Hourglass className="size-3" />
          until {until?.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </span>
      )}
      {!connected ? (
        <Button size="sm" variant="outline" className="h-7 gap-1.5 px-2 text-xs" title={`Start an orchestrator for ${boardKey}: workers run in their own git worktrees`} onClick={onStart}>
          <Play className="size-3.5" /> Start
        </Button>
      ) : (
        <Button size="icon" variant="ghost" className="size-7" aria-label="Stop the orchestrator" title="Stop: running workers are paused and the board is released" onClick={onStop}>
          <Square className="size-3.5" />
        </Button>
      )}
      {state === "running" ? (
        <>
          <Button size="icon" variant="ghost" className="size-7" aria-label="Pause the orchestrator" title="Pause: start nothing new; running workers continue" onClick={() => onState("paused")}>
            <Pause className="size-3.5" />
          </Button>
          <Button size="icon" variant="ghost" className="size-7" aria-label="Drain the orchestrator" title="Drain: start nothing new and let running workers finish" onClick={() => onState("draining")}>
            <Waves className="size-3.5" />
          </Button>
        </>
      ) : (
        <Button size="icon" variant="ghost" className="size-7" aria-label="Resume the orchestrator" title="Resume dispatching" onClick={() => onState("running")}>
          <Play className="size-3.5" />
        </Button>
      )}
    </div>
  );
}

export type { ReactNode };
