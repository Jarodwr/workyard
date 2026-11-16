-- Conversation between humans and agents about an open question. The question is resolved only
-- by a human answering or dropping it; messages never resolve it.
CREATE TABLE question_messages(
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  question_id TEXT NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
  board_id TEXT NOT NULL REFERENCES boards(id),
  author TEXT NOT NULL,
  author_is_human INTEGER NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL,
  -- The event that announced the message; compared with a session's delivery cursor to decide
  -- whether a paused worker has been told about it yet.
  event_id INTEGER NOT NULL,
  -- Agent messages a human has not looked at yet drive the unread badge.
  read_by_human INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX question_messages_by_question ON question_messages(question_id, seq);
