import { useEffect, useState } from "react";
import { FolderOpen, TriangleAlert } from "lucide-react";
import { client, type Project, type RepositoryInfo } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { folderName, pathProblem, projectNameFor } from "@/lib/new-project";

function message(error: unknown): string {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
  try { return JSON.parse(text).message ?? text; } catch { return text; }
}

export function NewProjectDialog({ onCreated, onClose }: { onCreated: (project: Project) => void; onClose: () => void }) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [branch, setBranch] = useState("");
  const [info, setInfo] = useState<RepositoryInfo | null>(null);
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);

  // Check the folder once the typed path settles; the branch follows the repository.
  useEffect(() => {
    setInfo(null);
    const local = pathProblem(path);
    if (local) { setProblem(path.trim() ? local : ""); return; }
    let active = true;
    const timer = window.setTimeout(() => {
      client.inspectRepository(path.trim()).then((value) => {
        if (!active) return;
        setInfo(value);
        setProblem("");
        setBranch(value.branch);
      }).catch((e) => { if (active) setProblem(message(e)); });
    }, 300);
    return () => { active = false; window.clearTimeout(timer); };
  }, [path]);

  async function browse() {
    try {
      const picked = await client.pickFolder();
      if (picked) setPath(picked);
    } catch (e) { setProblem(message(e)); }
  }

  async function create() {
    if (!info) return;
    setBusy(true);
    try {
      onCreated(await client.createProjectFromRepository(projectNameFor(name, path), description.trim(), info.path, branch.trim() || info.branch));
    } catch (e) {
      setProblem(message(e));
      setBusy(false);
    }
  }

  const defaultName = folderName(info?.path ?? path);
  return <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" role="dialog" aria-modal="true" aria-label="New project">
    <form className="w-full max-w-lg space-y-4 rounded-xl border bg-card p-6 shadow-lg" onSubmit={(event) => { event.preventDefault(); void create(); }}>
      <h2 className="text-lg font-semibold">New project</h2>
      <label className="block space-y-1.5 text-xs font-medium">Repository folder
        <div className="flex gap-2">
          <Input autoFocus value={path} onChange={(event) => setPath(event.target.value)} placeholder="/Users/you/code/app" spellCheck={false} />
          <Button type="button" variant="outline" onClick={() => void browse()}><FolderOpen /> Browse</Button>
        </div>
      </label>
      {problem && <p role="alert" className="text-xs text-destructive">{problem}</p>}
      {info?.dirty && <p className="flex gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5 text-xs"><TriangleAlert className="mt-0.5 size-3.5 shrink-0" />This repository has uncommitted changes. Workers merge into it, so commit or stash them first.</p>}
      <label className="block space-y-1.5 text-xs font-medium">Name
        <Input value={name} onChange={(event) => setName(event.target.value)} placeholder={defaultName || "Project name"} />
      </label>
      <label className="block space-y-1.5 text-xs font-medium">Target branch
        <Input value={branch} onChange={(event) => setBranch(event.target.value)} placeholder="Current branch of the repository" disabled={!info} />
      </label>
      <label className="block space-y-1.5 text-xs font-medium">Description (optional)
        <Input value={description} onChange={(event) => setDescription(event.target.value)} />
      </label>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>Cancel</Button>
        <Button type="submit" disabled={!info || busy || !projectNameFor(name, info.path)}>{busy ? "Creating…" : "Create project"}</Button>
      </div>
    </form>
  </div>;
}
