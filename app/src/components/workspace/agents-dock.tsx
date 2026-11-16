import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, CircleHelp, Clock3, Square, Terminal, Unlock, X } from "lucide-react";
import type { Board, Card, Question } from "@/api";
import { AgentLog } from "@/components/workspace/agent-log";
import { Button } from "@/components/ui/button";
import { agentState, boardAgents, elapsed, pendingQuestions, progressDisplay, type AgentStateKind } from "@/lib/agents";
import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

export const stateTone: Record<AgentStateKind, string> = {
  running: "bg-primary/15 text-primary border-primary/30",
  question: "bg-sky-500/10 text-sky-800 border-sky-500/30 dark:text-sky-200",
  review: "bg-violet-500/10 text-violet-800 border-violet-500/30 dark:text-violet-200",
  integration: "bg-indigo-500/10 text-indigo-800 border-indigo-500/30 dark:text-indigo-200",
  held: "bg-amber-500/10 text-amber-800 border-amber-500/30 dark:text-amber-200",
  paused: "bg-muted text-muted-foreground border-border",
};

type Props = {
  board: Board;
  questions: Question[];
  onClose: () => void;
  onOpenCard: (id: string) => void;
  onAnswer: (question: Question, answer: string, note: string) => Promise<boolean>;
  onAddNote: (card: Card, body: string) => Promise<boolean>;
  onOpenInTerminal: (card: Card) => Promise<boolean>;
  onReleaseHold: (card: Card) => Promise<boolean>;
  onHold: (card: Card) => Promise<boolean>;
};

/** A right-hand dock listing every agent on the board, with a live view of each. */
export function AgentsDock({ board, questions, onClose, onOpenCard, onAnswer, onAddNote, onOpenInTerminal, onReleaseHold, onHold }: Props) {
  const agents = boardAgents(board);
  const [now, setNow] = useState(() => Date.now());
  const [expanded, setExpanded] = useState<string | null>(null);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => window.clearInterval(timer);
  }, []);
  return (
    <aside className="flex w-[340px] shrink-0 flex-col overflow-hidden rounded-xl border bg-card" aria-label="Agents">
      <div className="flex items-center justify-between border-b px-3 py-2">
        <h2 className="text-xs font-semibold">Agents <span className="ml-1 font-normal text-muted-foreground">{agents.length}</span></h2>
        <Button size="icon" variant="ghost" className="size-6" aria-label="Close the agents dock" onClick={onClose}><X className="size-3.5" /></Button>
      </div>
      <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto p-2">
        {agents.length === 0 && (
          <p className="rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">No agents are attached to cards on this board.</p>
        )}
        {agents.map((card) => (
          <AgentRow
            key={card.id}
            card={card}
            now={now}
            questions={pendingQuestions(card, questions)}
            open={expanded === card.id}
            onToggle={() => setExpanded(expanded === card.id ? null : card.id)}
            onOpenCard={onOpenCard}
            onAnswer={onAnswer}
            onAddNote={onAddNote}
            onOpenInTerminal={onOpenInTerminal}
            onReleaseHold={onReleaseHold}
            onHold={onHold}
          />
        ))}
      </div>
    </aside>
  );
}

function AgentRow({ card, now, questions, open, onToggle, onOpenCard, onAnswer, onAddNote, onOpenInTerminal, onReleaseHold, onHold }: {
  card: Card;
  now: number;
  questions: Question[];
  open: boolean;
  onToggle: () => void;
  onOpenCard: (id: string) => void;
  onAnswer: Props["onAnswer"];
  onAddNote: Props["onAddNote"];
  onOpenInTerminal: Props["onOpenInTerminal"];
  onReleaseHold: Props["onReleaseHold"];
  onHold: Props["onHold"];
}) {
  const state = agentState(card)!;
  const session = card.session!;
  const since = elapsed(state.kind === "running" ? session.attached_at ?? session.activity_at : session.paused_at ?? session.attached_at, now);
  return (
    <div className={cn("rounded-lg border bg-background/60", state.kind === "running" && "border-primary/40")}>
      <button className="flex w-full items-start gap-1.5 px-2 py-1.5 text-left" onClick={onToggle} aria-expanded={open}>
        {open ? <ChevronDown className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" /> : <ChevronRight className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />}
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5">
            <span className="font-mono text-[10px] text-muted-foreground">{card.key}</span>
            <span className="min-w-0 flex-1 truncate text-xs font-medium">{card.title}</span>
          </span>
          <span className="mt-1 flex flex-wrap items-center gap-1.5 text-[10px]">
            <span className={cn("inline-flex items-center gap-1 rounded-full border px-1.5 py-px font-medium", stateTone[state.kind])}>
              {state.kind === "running" && <span className="agent-running-dot size-1.5 rounded-full bg-primary" aria-hidden />}
              {state.label}
            </span>
            {card.stale && <span className="inline-flex items-center gap-0.5 rounded-full border border-amber-500/40 bg-amber-500/10 px-1.5 py-px text-amber-700 dark:text-amber-300" title={`No activity since ${card.last_activity_at ?? "it was claimed"}`}><Clock3 className="size-2.5" />Stale</span>}
            {since && <span className="text-muted-foreground">{state.kind === "running" ? `for ${since}` : `${since} ago`}</span>}
          </span>
          {state.kind === "running" && (() => {
            const shown = progressDisplay(card);
            return (
              <span className="mt-1 flex items-center gap-1.5" title={shown.estimated ? "Estimated" : undefined}>
                <Progress value={shown.value} estimated={shown.estimated} className="h-1 flex-1" />
                <span className="w-7 text-right text-[9px] tabular-nums text-muted-foreground">{shown.estimated ? "~" : ""}{shown.value}%</span>
              </span>
            );
          })()}
          {session.activity && state.kind === "running" && <span className="mt-1 block truncate text-[10.5px] text-muted-foreground" title={session.activity}>{session.activity}</span>}
        </span>
      </button>
      {questions.map((question) => (
        <InlineQuestion key={question.id} question={question} onAnswer={onAnswer} onOpenCard={() => onOpenCard(card.id)} />
      ))}
      {open && (
        <div className="space-y-2 border-t p-2">
          <AgentLog cardKey={card.key} />
          <NoteBox card={card} onAddNote={onAddNote} />
          <div className="flex flex-wrap gap-1.5">
            {session.held_by ? (
              <Button size="sm" variant="outline" className="h-7 gap-1 px-2 text-xs" onClick={() => void onReleaseHold(card)}><Unlock className="size-3" />Release hold</Button>
            ) : (
              <>
                {session.runner === "claude" && (
                  <Button size="sm" variant="outline" className="h-7 gap-1 px-2 text-xs" title="Stops the running worker and opens this session in your terminal" onClick={() => void onOpenInTerminal(card)}><Terminal className="size-3" />Open in terminal</Button>
                )}
                <Button size="sm" variant="outline" className="h-7 gap-1 px-2 text-xs" title="Hold this session: its worker stops and nothing resumes it until you release the hold" onClick={() => void onHold(card)}><Square className="size-3" />Stop</Button>
              </>
            )}
            <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => onOpenCard(card.id)}>Open card</Button>
          </div>
        </div>
      )}
    </div>
  );
}

function InlineQuestion({ question, onAnswer, onOpenCard }: { question: Question; onAnswer: Props["onAnswer"]; onOpenCard: () => void }) {
  const [choice, setChoice] = useState("");
  const options = question.options.filter((option) => option !== "Other");
  return (
    <div className="mx-2 mb-2 rounded-md border border-sky-500/30 bg-sky-500/5 p-2">
      <p className="flex items-start gap-1.5 text-[11px] font-medium"><CircleHelp className="mt-0.5 size-3 shrink-0 text-sky-700 dark:text-sky-300" /><span><span className="font-mono text-muted-foreground">{question.key}</span> {question.title}</span></p>
      <div className="mt-1.5 flex flex-wrap gap-1" role="radiogroup" aria-label={`Options for ${question.key}`}>
        {options.map((option) => (
          <button
            key={option}
            role="radio"
            aria-checked={choice === option}
            className={cn("max-w-full rounded-md border px-1.5 py-0.5 text-left text-[10.5px] leading-4 hover:bg-accent", choice === option && "border-primary bg-primary/10 font-medium")}
            onClick={() => setChoice(option)}
            title={option === question.recommended_option ? "Recommended by the agent" : undefined}
          >
            {option === question.recommended_option && "★ "}{option}
          </button>
        ))}
      </div>
      <div className="mt-1.5 flex items-center gap-1.5">
        <Button size="sm" className="h-6 px-2 text-[11px]" disabled={!choice} onClick={() => void onAnswer(question, choice, "")}>Answer</Button>
        {question.options.includes("Other") && <button className="text-[11px] text-muted-foreground underline-offset-2 hover:text-primary hover:underline" onClick={onOpenCard}>Something else…</button>}
      </div>
    </div>
  );
}

function NoteBox({ card, onAddNote }: { card: Card; onAddNote: Props["onAddNote"] }) {
  const [note, setNote] = useState("");
  return (
    <form
      className="flex items-end gap-1.5"
      onSubmit={async (event) => {
        event.preventDefault();
        if (note.trim() && (await onAddNote(card, note.trim()))) setNote("");
      }}
    >
      <label htmlFor={`dock-note-${card.id}`} className="sr-only">Send {card.key} a note</label>
      <textarea
        id={`dock-note-${card.id}`}
        rows={2}
        placeholder="Send the agent a note…"
        value={note}
        onChange={(event) => setNote(event.target.value)}
        className="min-w-0 flex-1 resize-none rounded-md border bg-background px-2 py-1 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      <Button type="submit" size="sm" className="h-7 px-2 text-xs" disabled={!note.trim()} title="Delivered to the running worker at its next step">Send</Button>
    </form>
  );
}
