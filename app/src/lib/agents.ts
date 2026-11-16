import type { Board, Card, Question } from "@/api";

export type AgentStateKind = "running" | "question" | "review" | "integration" | "held" | "paused";

export type AgentState = {
  kind: AgentStateKind;
  /** Short label for chips on graph nodes. */
  chip: string;
  /** Longer label for the agents dock. */
  label: string;
};

/**
 * What a card's agent session is doing, or null when the card has no session. "Running" is a
 * session that is attached and not paused; everything else is a way of waiting.
 */
export function agentState(card: Card): AgentState | null {
  const session = card.session;
  if (!session) return null;
  if (session.held_by) return { kind: "held", chip: "Held", label: `Held by ${session.held_by}` };
  if (!session.paused) return { kind: "running", chip: "Running", label: "Running" };
  if (session.waiting_review) return { kind: "review", chip: "Review", label: "Waiting for review" };
  if (session.waiting_integration) return { kind: "integration", chip: "Merge", label: "Waiting to be merged" };
  if (session.waiting_question_key || card.open_question_count > 0) {
    return { kind: "question", chip: "Answer", label: session.waiting_question_key ? `Waiting for ${session.waiting_question_key}` : "Waiting for an answer" };
  }
  return { kind: "paused", chip: "Paused", label: "Paused" };
}

export function isRunning(card: Card): boolean {
  return agentState(card)?.kind === "running";
}

/** Every card on the board with an attached agent session, running ones first. */
export function boardAgents(board: Board): Card[] {
  const order: Record<AgentStateKind, number> = { running: 0, question: 1, review: 2, integration: 3, paused: 4, held: 5 };
  return board.lanes
    .flatMap((lane) => lane.cards)
    .filter((card) => card.session && !card.cancelled_at)
    .sort((a, b) => order[agentState(a)!.kind] - order[agentState(b)!.kind] || a.key.localeCompare(b.key, undefined, { numeric: true }));
}

/** Open questions that hold this card. */
export function pendingQuestions(card: Card, questions: Question[]): Question[] {
  return questions.filter((question) => question.status === "open" && question.blocks.includes(card.key));
}

/** "4m", "1h 05m": how long ago an ISO time was. Empty when unknown. */
export function elapsed(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

/** Highest value an estimate can show, so it never reads as finished. */
export const ESTIMATE_CAP = 90;

/**
 * A guess at progress from effort so far: it rises with tool uses and run time, slows as it
 * climbs, and never passes ESTIMATE_CAP.
 */
export function estimateProgress(toolUses: number, elapsedSecs: number): number {
  const effort = Math.max(0, toolUses) / 40 + Math.max(0, elapsedSecs) / 900;
  return Math.min(ESTIMATE_CAP, Math.floor(ESTIMATE_CAP * (1 - Math.exp(-effort))));
}

/**
 * The bar a card should show: reported progress when there is any, else an estimate while the
 * agent is running, else nothing estimated.
 */
export function progressDisplay(card: Card, now = Date.now()): { value: number; estimated: boolean } {
  if (card.progress > 0 || !isRunning(card)) return { value: card.progress, estimated: false };
  const session = card.session!;
  const attached = session.attached_at ? Date.parse(session.attached_at) : NaN;
  const secs = Number.isNaN(attached) ? session.elapsed_secs ?? 0 : Math.max(0, (now - attached) / 1000);
  return { value: estimateProgress(session.tool_uses ?? 0, secs), estimated: true };
}
