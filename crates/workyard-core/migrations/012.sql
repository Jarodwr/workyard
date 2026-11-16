-- While `backoff_until` is in the future the board dispatches nothing, for example after a
-- usage limit. It lives on the board so every orchestrator restart and the desktop see it.
ALTER TABLE orchestrator_sessions ADD COLUMN backoff_until TEXT;
ALTER TABLE orchestrator_sessions ADD COLUMN backoff_reason TEXT NOT NULL DEFAULT '';
