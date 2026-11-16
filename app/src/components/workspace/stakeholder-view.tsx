import { useState } from "react";
import { UserRound } from "lucide-react";
import type { Project, Question } from "@/api";
import { Card } from "@/components/ui/card";
import { QuestionCard } from "@/components/workspace/question-card";
import { cn } from "@/lib/utils";

type Stakeholder = Project["stakeholders"][number];

export function StakeholderView({
  project,
  stakeholder,
  questions,
  onAnswer,
  onAssignStakeholder,
}: {
  project: Project;
  stakeholder: Stakeholder;
  questions: Question[];
  onAnswer: (question: Question, answer: string, note: string) => Promise<boolean>;
  onAssignStakeholder: (question: Question, stakeholderId: string) => Promise<boolean>;
}) {
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [notes, setNotes] = useState<Record<string, string>>({});
  const boardById = new Map(project.boards.map((board) => [board.id, board]));
  const assigned = questions.filter((question) => question.stakeholder_id === stakeholder.id && boardById.has(question.board_id));
  const open = assigned.filter((question) => question.status === "open");
  const resolved = assigned.filter((question) => question.status !== "open");

  return <div className="h-full overflow-auto px-2 py-2">
    <header className="mb-5">
      <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground"><UserRound className="size-4" /> {project.name} / Stakeholders</div>
      <h1 className="mt-1 text-2xl font-bold tracking-tight">{stakeholder.name}</h1>
      {stakeholder.role && <p className="mt-1 text-sm text-muted-foreground">{stakeholder.role}</p>}
      {stakeholder.contact && <p className="mt-2 text-xs text-muted-foreground">{stakeholder.contact}</p>}
    </header>
    <div className="max-w-3xl space-y-5">
      <section>
        <h2 className="mb-2 text-sm font-semibold">To ask <span className="text-muted-foreground">({open.length})</span></h2>
        <div className="space-y-3">
          {open.length ? open.map((question)=><QuestionCard key={question.id} question={question} stakeholders={project.stakeholders} phaseName={boardById.get(question.board_id)?.name} answer={answers[question.id] ?? ""} note={notes[question.id] ?? ""} onAnswerChange={(value)=>setAnswers((current)=>({...current,[question.id]:value}))} onNoteChange={(value)=>setNotes((current)=>({...current,[question.id]:value}))} onAssignStakeholder={(stakeholderId)=>onAssignStakeholder(question,stakeholderId)} onAnswer={async()=>{if(await onAnswer(question,answers[question.id] ?? "",notes[question.id]?.trim() ?? "")){setAnswers((current)=>({...current,[question.id]:""}));setNotes((current)=>({...current,[question.id]:""}));}}} />) : <Card className="border-dashed px-4 py-6 text-xs text-muted-foreground shadow-none">No open questions assigned to this stakeholder.</Card>}
        </div>
      </section>
      {resolved.length > 0 && <section>
        <h2 className="mb-2 text-sm font-semibold">Answered or dropped <span className="text-muted-foreground">({resolved.length})</span></h2>
        <Card className="divide-y overflow-hidden py-0 shadow-none">
          {resolved.map((question)=><ResolvedQuestion key={question.id} question={question} boardName={boardById.get(question.board_id)?.name ?? "Board"} />)}
        </Card>
      </section>}
    </div>
  </div>;
}

function ResolvedQuestion({ question, boardName }: { question: Question; boardName: string }) {
  return <div className="flex w-full items-start gap-3 px-4 py-3 text-left">
    <span className={cn("mt-0.5 size-2 shrink-0 rounded-full", question.kind === "business" ? "bg-amber-500" : "bg-sky-500")} />
    <span className="min-w-0 flex-1"><strong className="block text-sm">{question.title}</strong><span className="mt-1 block text-xs text-muted-foreground">{boardName} · {question.kind === "business" ? "Business" : "Engineering"} · {question.status}</span>{question.answer && <span className="mt-1 block text-xs text-muted-foreground">{question.answer}{question.answer_note ? ` — ${question.answer_note}` : ""}</span>}</span>
  </div>;
}
