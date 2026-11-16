-- One markdown report per card (the write-up of an investigation), replaced as a whole; version counts the changes.
CREATE TABLE card_reports (
  card_id TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) <= 262144),
  actor TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL
);
