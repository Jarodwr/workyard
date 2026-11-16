/** The folder name of a path, which becomes the default project name. */
export function folderName(path: string): string {
  const parts = path.trim().replace(/[\\/]+$/, "").split(/[\\/]/);
  return (parts[parts.length - 1] ?? "").trim();
}

/** Why a typed repository path cannot work yet, or null when it may be checked with git. */
export function pathProblem(path: string): string | null {
  const text = path.trim();
  if (!text) return "Choose the repository folder.";
  if (!/^([/~]|[A-Za-z]:[\\/])/.test(text)) return "Use an absolute path, such as /Users/you/code/app.";
  return null;
}

/** The project name to use: what was typed, else the folder name. */
export function projectNameFor(typed: string, path: string): string {
  return typed.trim() || folderName(path);
}
