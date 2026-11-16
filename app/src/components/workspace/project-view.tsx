import { FolderKanban, GitBranch, Sparkles, Users } from "lucide-react";
import type { Project } from "@/api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

export function ProjectOverview({ project, onOpenBoard, onPlan }: { project: Project; onOpenBoard: (key: string) => void; onPlan: () => void }) {
  const repository = project.repositories[0];

  return <div className="h-full overflow-auto px-2 py-2">
    <header className="mb-5 flex items-start justify-between gap-4">
      <div>
        <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground"><FolderKanban className="size-4" /> Project</div>
        <h1 className="mt-1 text-2xl font-bold tracking-tight">{project.name}</h1>
        {project.description && <p className="mt-1 text-sm text-muted-foreground">{project.description}</p>}
      </div>
      {repository && <Button onClick={onPlan}><Sparkles /> Plan</Button>}
    </header>
    <div className="space-y-5">
      <div className="grid gap-3 sm:grid-cols-2">
        <Metric icon={<FolderKanban />} label="Boards" value={project.boards.length} />
        <Metric icon={<Users />} label="Stakeholders" value={project.stakeholders.length} />
      </div>
      <section>
        <h2 className="mb-2 text-sm font-semibold">Boards</h2>
        <Card className="divide-y overflow-hidden py-0 shadow-none">
          {project.boards.length ? project.boards.map((board)=><button key={board.id} className="block w-full px-4 py-3 text-left hover:bg-accent" onClick={()=>onOpenBoard(board.key)}><span className="text-sm font-semibold">{board.name}</span>{board.description && <p className="mt-0.5 text-xs text-muted-foreground">{board.description}</p>}</button>) : <div className="flex items-center justify-between gap-3 px-4 py-6"><p className="text-xs text-muted-foreground">This project has no boards yet. Open a planner to describe what you want built; it creates the board.</p>{repository && <Button size="sm" onClick={onPlan}><Sparkles /> Plan</Button>}</div>}
        </Card>
      </section>
      <section className="max-w-2xl space-y-2">
        <h2 className="text-sm font-semibold">Repository</h2>
        <Card className="flex gap-3 p-4 shadow-none">
          <GitBranch className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0">{repository ? <><p className="break-all text-sm font-medium">{repository.path}</p><p className="mt-1 text-xs text-muted-foreground">Target branch: {repository.target_branch} · {repository.remote_url || "Local only"}</p></> : <p className="text-xs text-muted-foreground">No repository configured for this project.</p>}</div>
        </Card>
      </section>
    </div>
  </div>;
}

function Metric({icon,label,value}:{icon:React.ReactNode;label:string;value:number}) {
  return <Card className="flex flex-row items-center gap-3 p-4 shadow-none"><span className="text-muted-foreground [&_svg]:size-4">{icon}</span><div><p className="text-[11px] text-muted-foreground">{label}</p><p className="text-lg font-semibold tabular-nums">{value}</p></div></Card>;
}
