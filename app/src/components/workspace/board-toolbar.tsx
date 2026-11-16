import type { ReactNode } from "react";
import { ClipboardList, Hourglass, Info, Pause, Play, Waves } from "lucide-react";
import type { Orchestrator } from "@/api";
import { Button } from "@/components/ui/button";
import { Popover } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

type Props = {
  boardKey: string;
  /** View-specific controls: the phase picker on the graph, the way back on a phase board. */
  left: ReactNode;
  orchestrator: Orchestrator | null;
  onOrchestratorState: (state: Orchestrator["desired_state"]) => void;
  onPlan: () => void;
  /** The legend explains the graph's lines, so only the graph shows it. */
  showLegend: boolean;
};

/** One slim bar: view controls on the left, legend, orchestrator and planner on the right. */
export function BoardToolbar({ boardKey, left, orchestrator, onOrchestratorState, onPlan, showLegend }: Props) {
  return (
    <div className="mb-2 flex shrink-0 items-center gap-2 rounded-lg border bg-card px-2.5 py-1.5 text-xs">
      <div className="flex min-w-0 flex-1 items-center gap-3">{left}</div>
      {showLegend && <LegendButton />}
      <OrchestratorControls boardKey={boardKey} orchestrator={orchestrator} onState={onOrchestratorState} />
      <Button size="sm" variant="outline" className="h-7 gap-1.5 px-2.5 text-xs" onClick={onPlan} title="Open a planner agent in your terminal, pointed at this board">
        <ClipboardList className="size-3.5" /> Plan
      </Button>
    </div>
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

function OrchestratorControls({ boardKey, orchestrator, onState }: { boardKey: string; orchestrator: Orchestrator | null; onState: (state: Orchestrator["desired_state"]) => void }) {
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
        {label}
      </span>
      {orchestrator.backing_off && (
        <span className="inline-flex items-center gap-1 text-amber-700 dark:text-amber-300" title={orchestrator.backoff_reason}>
          <Hourglass className="size-3" />
          until {until?.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </span>
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
