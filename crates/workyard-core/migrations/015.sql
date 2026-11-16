-- A session paused to wait for integration is resumed once its card's changes are on the
-- target branch, not as soon as a question is answered.
ALTER TABLE task_sessions ADD COLUMN waiting_integration INTEGER NOT NULL DEFAULT 0 CHECK (waiting_integration IN (0,1));
-- What the worker is doing right now, derived by the orchestrator from the runtime's output.
-- It is not an event: it changes constantly and exists to be shown, not replayed.
ALTER TABLE task_sessions ADD COLUMN activity TEXT NOT NULL DEFAULT '';
ALTER TABLE task_sessions ADD COLUMN activity_at TEXT;
-- Human review of commits is off unless a board turns it on.
ALTER TABLE boards ADD COLUMN review_required INTEGER NOT NULL DEFAULT 0 CHECK (review_required IN (0,1));
-- How much workers may do without asking: `full` runs them with permission checks off.
ALTER TABLE orchestrator_sessions ADD COLUMN permissions TEXT NOT NULL DEFAULT 'full' CHECK (permissions IN ('full','safe'));
