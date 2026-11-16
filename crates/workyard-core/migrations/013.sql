-- Where a card's worker runs. `worktree` (default): its own git worktree and branch, enforced
-- by the orchestrator. `none`: investigative work that changes no repository.
ALTER TABLE cards ADD COLUMN workspace TEXT NOT NULL DEFAULT 'worktree' CHECK (workspace IN ('worktree','none'));
-- A human has taken a session over (for example in a terminal). It stays paused and is not
-- offered for resume until the hold is released.
ALTER TABLE task_sessions ADD COLUMN held_by TEXT;
