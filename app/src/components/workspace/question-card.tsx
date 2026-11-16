import { useEffect, useRef, useState } from "react";
import { ArrowRight, Check, Lightbulb, UserRound } from "lucide-react";
import type { Project, Question } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { QuestionThread, UnreadReplyBadge } from "@/components/workspace/question-thread";
import { cn } from "@/lib/utils";

export function QuestionCard({
  question,
  stakeholders,
  onAssignStakeholder,
  phaseName,
  answer,
  note,
  onAnswerChange,
  onNoteChange,
  onAnswer,
}: {
  question: Question;
  stakeholders: Project["stakeholders"];
  onAssignStakeholder: (stakeholderId: string) => Promise<boolean>;
  phaseName?: string;
  answer: string;
  note: string;
  onAnswerChange: (value: string) => void;
  onNoteChange: (value: string) => void;
  onAnswer: () => Promise<void>;
}) {
  const tone = questionTone[question.kind];
  return (
    <Card id={`question-${question.id}`} className={cn("rounded-xl border-l-4 shadow-none", tone.card)}>
      <CardHeader className="flex flex-row items-start gap-3 pb-3">
        <div className="min-w-0 flex-1 space-y-1">
          {(phaseName || (question.unread_messages ?? 0) > 0) && <div className="flex items-center gap-2">{phaseName && <span className="text-[10px] font-medium text-muted-foreground">{phaseName}</span>}<UnreadReplyBadge question={question} /></div>}
          <CardTitle className="text-base leading-snug">{question.title}</CardTitle>
          {question.body && <CardDescription>{question.body}</CardDescription>}
        </div>
        <StakeholderPicker question={question} stakeholders={stakeholders} onAssign={onAssignStakeholder} />
      </CardHeader>
      <CardContent className="space-y-3">
        {question.blocks.length > 0 && <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <ArrowRight className="size-3.5" />
              <span>Blocks {question.blocks.length} {question.blocks.length === 1 ? "task" : "tasks"}</span>
        </div>}
        {question.recommended_option && (
          <div className={cn("rounded-lg border px-3 py-2.5 text-sm", tone.recommendation)}>
            <div className={cn("flex items-center gap-1.5 text-xs font-semibold", tone.text)}>
              <Lightbulb className="size-3.5" /> Agent recommends: {question.recommended_option}
            </div>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{question.recommendation_reason}</p>
          </div>
        )}
        <fieldset className="space-y-1.5">
          <legend className="mb-2 text-xs font-medium">Choose an option</legend>
          {question.options.map((option) => (
            <label key={option} className={cn("flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-sm", answer === option ? tone.selected : "border-border bg-background/70")}>
              <input type="radio" name={`answer-${question.id}`} value={option} checked={answer === option} onChange={() => onAnswerChange(option)} className="accent-primary" />
              <span>{option}</span>
            </label>
          ))}
        </fieldset>
        <div className="flex gap-2">
          <Input
            aria-label={`Explanation for ${question.key}`}
            placeholder={answer === "Other" ? "Describe your answer…" : "Optional explanation…"}
            value={note}
            onChange={(e) => onNoteChange(e.target.value)}
          />
          <Button
            size="sm"
            disabled={!question.options.includes(answer) || (answer === "Other" && !note.trim())}
            onClick={() => void onAnswer()}
          >
            Answer
          </Button>
        </div>
        <QuestionThread question={question} collapsible />
      </CardContent>
    </Card>
  );
}

export function StakeholderPicker({
  question,
  stakeholders,
  onAssign,
  compact = false,
}: {
  question: Question;
  stakeholders: Project["stakeholders"];
  onAssign: (stakeholderId: string) => Promise<boolean>;
  compact?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const assigned = stakeholders.find((person) => person.id === question.stakeholder_id);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  async function choose(stakeholderId: string) {
    if (stakeholderId === (question.stakeholder_id ?? "")) {
      setOpen(false);
      return;
    }
    setSaving(true);
    try {
      if (await onAssign(stakeholderId)) setOpen(false);
    } finally {
      setSaving(false);
    }
  }

  return <div ref={root} className="relative shrink-0">
    <button
      ref={trigger}
      type="button"
      aria-label={assigned ? `Stakeholder: ${assigned.name}. Change assignment` : "Assign stakeholder"}
      title={assigned ? `Assigned to ${assigned.name}` : "No stakeholder assigned"}
      aria-expanded={open}
      aria-controls={`stakeholder-menu-${question.id}`}
      disabled={saving}
      onClick={() => setOpen((value) => !value)}
      className={cn(
        "flex items-center justify-center rounded-full border-2 font-semibold transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        compact ? "size-7 text-[9px]" : "size-9 text-xs",
        assigned ? "border-border bg-background text-foreground" : "border-dotted border-muted-foreground/70 bg-background/50 text-muted-foreground",
      )}
    >
      {assigned ? initials(assigned.name) : <UserRound className={compact ? "size-3.5" : "size-4"} />}
    </button>
    {open && <div id={`stakeholder-menu-${question.id}`} className="absolute right-0 top-full z-30 mt-2 w-64 rounded-lg border bg-popover p-1 shadow-lg">
      <p className="px-2 py-1.5 text-[11px] font-semibold text-muted-foreground">Stakeholder</p>
      <button type="button" className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-xs hover:bg-accent" onClick={() => void choose("")}>
        <span className="flex size-6 items-center justify-center rounded-full border border-dotted border-muted-foreground/70"><UserRound className="size-3" /></span>
        <span className="min-w-0 flex-1">Unassigned</span>
        {!question.stakeholder_id && <Check className="size-3.5" />}
      </button>
      {stakeholders.length ? stakeholders.map((person) => <button key={person.id} type="button" className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left hover:bg-accent" onClick={() => void choose(person.id)}>
        <span className="flex size-6 shrink-0 items-center justify-center rounded-full border bg-background text-[10px] font-semibold">{initials(person.name)}</span>
        <span className="min-w-0 flex-1"><span className="block truncate text-xs font-medium">{person.name}</span>{person.role && <span className="block truncate text-[10px] text-muted-foreground">{person.role}</span>}</span>
        {question.stakeholder_id === person.id && <Check className="size-3.5" />}
      </button>) : <p className="px-2 py-2 text-xs text-muted-foreground">No stakeholders in this project.</p>}
    </div>}
  </div>;
}

function initials(name: string): string {
  return name.trim().split(/\s+/).slice(0, 2).map((part) => part[0]?.toUpperCase() ?? "").join("");
}

const questionTone: Record<Question["kind"], {card: string; recommendation: string; text: string; selected: string}> = {
  engineering: {
    card: "border-sky-500/70 bg-sky-500/5",
    recommendation: "border-sky-500/25 bg-sky-500/10",
    text: "text-sky-800 dark:text-sky-200",
    selected: "border-sky-500 bg-sky-500/10",
  },
  business: {
    card: "border-amber-500/70 bg-amber-500/5",
    recommendation: "border-amber-500/25 bg-amber-500/10",
    text: "text-amber-800 dark:text-amber-200",
    selected: "border-amber-500 bg-amber-500/10",
  },
};
