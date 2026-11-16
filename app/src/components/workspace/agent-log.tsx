import { useEffect, useRef, useState } from "react";
import { Bot, MessageSquare, TriangleAlert, Wrench } from "lucide-react";
import { client, type AgentLogRow } from "@/api";
import { cn } from "@/lib/utils";

/**
 * The tail of one worker's log, refreshed while it is mounted. Follows the bottom unless the
 * reader scrolled up.
 */
export function AgentLog({ cardKey, limit = 60, className }: { cardKey: string; limit?: number; className?: string }) {
  const [rows, setRows] = useState<AgentLogRow[] | null>(null);
  const [failed, setFailed] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  useEffect(() => {
    let active = true;
    const load = () => {
      client
        .agentLogTail(cardKey, limit)
        .then((next) => {
          if (!active) return;
          setFailed(false);
          setRows((previous) => (JSON.stringify(previous) === JSON.stringify(next) ? previous : next));
        })
        .catch(() => { if (active) setFailed(true); });
    };
    load();
    const timer = window.setInterval(load, 2000);
    return () => { active = false; window.clearInterval(timer); };
  }, [cardKey, limit]);

  useEffect(() => {
    const element = box.current;
    if (element && pinned.current) element.scrollTop = element.scrollHeight;
  }, [rows]);

  return (
    <div
      ref={box}
      className={cn("max-h-56 overflow-y-auto rounded-md border bg-background/70 p-1.5 font-mono text-[10.5px] leading-4", className)}
      onScroll={(event) => {
        const el = event.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      role="log"
      aria-label={`Live output for ${cardKey}`}
    >
      {rows === null && !failed && <p className="p-1 text-muted-foreground">Reading the log…</p>}
      {failed && rows === null && <p className="p-1 text-muted-foreground">No log for this agent yet.</p>}
      {rows?.length === 0 && <p className="p-1 text-muted-foreground">Nothing logged yet.</p>}
      {rows?.map((row, index) => (
        <div key={index} className={cn("flex gap-1.5 rounded px-1 py-0.5", row.kind === "error" && "bg-destructive/10 text-destructive", row.kind === "result" && "text-muted-foreground")}>
          <span className="mt-0.5 shrink-0 text-muted-foreground" aria-hidden>
            {row.kind === "tool" ? <Wrench className="size-3" /> : row.kind === "error" ? <TriangleAlert className="size-3" /> : row.kind === "user" ? <MessageSquare className="size-3" /> : <Bot className="size-3" />}
          </span>
          <span className="min-w-0 whitespace-pre-wrap break-words">
            {row.kind === "tool" && <strong className="mr-1 font-semibold text-foreground">{row.tool}</strong>}
            {row.text}
          </span>
        </div>
      ))}
    </div>
  );
}
