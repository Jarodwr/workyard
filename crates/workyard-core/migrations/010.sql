CREATE TABLE task_sessions (
  id TEXT PRIMARY KEY,
  card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE RESTRICT,
  owner_actor TEXT NOT NULL,
  runner TEXT NOT NULL CHECK (length(trim(runner)) > 0),
  external_session_id TEXT NOT NULL CHECK (length(trim(external_session_id)) > 0),
  attached_at TEXT NOT NULL,
  paused_at TEXT,
  waiting_question_id TEXT REFERENCES questions(id) ON DELETE RESTRICT,
  last_delivered_event_id INTEGER NOT NULL DEFAULT 0 CHECK (last_delivered_event_id >= 0),
  detached_at TEXT,
  CHECK (waiting_question_id IS NULL OR paused_at IS NOT NULL),
  UNIQUE (runner, external_session_id)
);
CREATE UNIQUE INDEX task_sessions_active_card ON task_sessions(card_id) WHERE detached_at IS NULL;
CREATE INDEX task_sessions_paused ON task_sessions(card_id, paused_at) WHERE detached_at IS NULL;
