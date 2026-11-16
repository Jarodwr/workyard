// Where the user was, kept in localStorage per database path so a reload or restart returns there.
// Every access is wrapped: storage can be missing, full or hold something stale or hand-edited.

export type Slice = {
  projectId?: string | null;
  boardKey?: string | null;
  section?: "dependencies" | "kanban";
  phaseId?: string | null;
  zoom?: number;
  phaseFilter?: string;
  showCompleted?: boolean;
  agentsOpen?: boolean;
};

export type Store = Pick<Storage, "getItem" | "setItem">;

const PREFIX = "workyard.view:";

function defaultStore(): Store | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

const str = (value: unknown): string | null => (typeof value === "string" ? value : null);

/** Drops anything of the wrong type, so a corrupt entry reads as "nothing saved". */
export function sanitize(raw: unknown): Slice {
  if (!raw || typeof raw !== "object") return {};
  const r = raw as Record<string, unknown>;
  const out: Slice = {};
  if ("projectId" in r) out.projectId = str(r.projectId);
  if ("boardKey" in r) out.boardKey = str(r.boardKey);
  if (r.section === "dependencies" || r.section === "kanban") out.section = r.section;
  if ("phaseId" in r) out.phaseId = str(r.phaseId);
  if (typeof r.zoom === "number" && Number.isFinite(r.zoom)) out.zoom = r.zoom;
  if (typeof r.phaseFilter === "string") out.phaseFilter = r.phaseFilter;
  if (typeof r.showCompleted === "boolean") out.showCompleted = r.showCompleted;
  if (typeof r.agentsOpen === "boolean") out.agentsOpen = r.agentsOpen;
  return out;
}

export function loadViewState(scope: string, store: Store | null = defaultStore()): Slice {
  if (!store) return {};
  try {
    const text = store.getItem(PREFIX + scope);
    return text ? sanitize(JSON.parse(text)) : {};
  } catch {
    return {};
  }
}

/** Merges `patch` into what is saved for `scope`; the app and the board views each save their own keys. */
export function saveViewState(scope: string, patch: Slice, store: Store | null = defaultStore()): void {
  if (!store) return;
  try {
    store.setItem(PREFIX + scope, JSON.stringify({ ...loadViewState(scope, store), ...patch }));
  } catch {
    /* storage unavailable or full: the view still works, it just is not remembered */
  }
}

/** The saved zoom if it is one of the offered levels, else `fallback`. */
export function pickZoom(saved: number | undefined, levels: readonly number[], fallback: number): number {
  return saved !== undefined && levels.includes(saved) ? saved : fallback;
}

/** The saved phase filter if it is "all" or a phase of this board, else "all". */
export function pickPhaseFilter(saved: string | undefined, phaseIds: readonly string[]): string {
  return saved !== undefined && phaseIds.includes(saved) ? saved : "all";
}

/**
 * Where to start. A saved project or board that no longer exists falls back one level
 * (board gone → its project; project gone → the project list).
 */
export function restoreTarget(
  saved: Slice,
  projectIds: readonly string[],
  boards: readonly { key: string; project_id?: string | null }[],
  unassignedId: string,
): { projectId: string | null; boardKey: string | null } {
  const board = saved.boardKey ? boards.find((item) => item.key === saved.boardKey) : undefined;
  const projectId = saved.projectId ?? null;
  const projectOk =
    projectId !== null &&
    (projectId === unassignedId ? boards.some((item) => !item.project_id) : projectIds.includes(projectId));
  if (!projectOk) return { projectId: null, boardKey: null };
  const boardInProject =
    board && (projectId === unassignedId ? !board.project_id : board.project_id === projectId);
  return { projectId, boardKey: boardInProject ? board.key : null };
}
