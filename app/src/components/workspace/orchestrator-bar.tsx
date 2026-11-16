import { Pause, Play, Waves } from "lucide-react";
import type { Orchestrator } from "@/api";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type Props = {
  board: string;
  orchestrator: Orchestrator | null;
  onState: (state: Orchestrator["desired_state"]) => void;
};

const stateLabel: Record<Orchestrator["desired_state"], string> = {
  running: "Running",
  paused: "Paused — no new work is started",
  draining: "Draining — finishing current work",
};

/** Shows whether an orchestrator holds the board and lets a human pause or resume it. */
export function OrchestratorBar({ board, orchestrator, onState }: Props) {
  if (!orchestrator) return null;
  const { connected, desired_state: state } = orchestrator;
  return (
    <div className="mb-3 flex shrink-0 items-center gap-3 rounded-lg border bg-card px-3 py-1.5 text-xs" aria-label="Orchestrator">
      <span className={cn("size-2 shrink-0 rounded-full", connected ? "bg-emerald-500" : "bg-muted-foreground/40")} aria-hidden />
      <span className="min-w-0 flex-1 truncate">
        {connected ? (
          <>
            <span className="font-medium">Orchestrator</span>
            <span className="text-muted-foreground"> · {stateLabel[state]} · up to {orchestrator.pool_size} at once</span>
          </>
        ) : (
          <span className="text-muted-foreground">
            No orchestrator running. Start one with <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">workyard orch run {board}</code>
            {state !== "running" && ` (the board is ${state})`}
          </span>
        )}
      </span>
      {state === "running" ? (
        <>
          <Button size="sm" variant="outline" className="h-7 gap-1 px-2 text-xs" onClick={() => onState("paused")}>
            <Pause className="size-3" /> Pause
          </Button>
          <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs" onClick={() => onState("draining")} title="Start nothing new and let running workers finish">
            <Waves className="size-3" /> Drain
          </Button>
        </>
      ) : (
        <Button size="sm" variant="outline" className="h-7 gap-1 px-2 text-xs" onClick={() => onState("running")}>
          <Play className="size-3" /> Resume
        </Button>
      )}
    </div>
  );
}
