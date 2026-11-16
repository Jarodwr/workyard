-- A session paused to wait for review is resumed once its card can finish (every review gate
-- approved, every finish dependency met), not as soon as it is paused.
ALTER TABLE task_sessions ADD COLUMN waiting_review INTEGER NOT NULL DEFAULT 0 CHECK (waiting_review IN (0,1));
