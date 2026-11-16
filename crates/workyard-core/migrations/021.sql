-- Environment variables every worker of a board is started with (a JSON object), and the
-- variable names the running orchestrator passes (never the values), for `orch status`.
ALTER TABLE boards ADD COLUMN worker_env TEXT NOT NULL DEFAULT '{}';
ALTER TABLE orchestrator_sessions ADD COLUMN worker_env_keys TEXT NOT NULL DEFAULT '[]';
