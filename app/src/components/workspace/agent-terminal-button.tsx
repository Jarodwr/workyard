import { Terminal, Unlock } from "lucide-react";
import type { Card } from "@/api";
import { cn } from "@/lib/utils";

/**
 * On a card with a live Claude session: open that agent in your terminal (stopping the
 * orchestrator's worker first), or, while you hold it, give it back.
 */
export function AgentTerminalButton({
  card,
  onOpen,
  onRelease,
  className,
}: {
  card: Card;
  onOpen: (card: Card) => Promise<boolean>;
  onRelease: (card: Card) => Promise<boolean>;
  className?: string;
}) {
  const session = card.session;
  if (!session || session.runner !== "claude") return null;
  const held = !!session.held_by;
  const label = held ? `Release ${card.key} (held by ${session.held_by})` : `Open ${card.key}'s agent in your terminal`;
  return (
    <button
      type="button"
      className={cn(
        "rounded p-1 text-muted-foreground hover:bg-accent hover:text-primary",
        held && "text-amber-600 dark:text-amber-300",
        className,
      )}
      title={held ? `Held by ${session.held_by}. Click to release it back to the orchestrator.` : "Open this agent in your terminal. The orchestrator's worker is stopped while you hold it."}
      aria-label={label}
      onClick={(event) => {
        event.stopPropagation();
        void (held ? onRelease(card) : onOpen(card));
      }}
    >
      {held ? <Unlock className="size-3.5" /> : <Terminal className="size-3.5" />}
    </button>
  );
}
