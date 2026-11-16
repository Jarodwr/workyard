CREATE TABLE card_notes (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  card_id TEXT NOT NULL,
  actor TEXT NOT NULL CHECK (length(trim(actor)) > 0),
  body TEXT NOT NULL CHECK (length(trim(body)) > 0),
  created_at TEXT NOT NULL,
  FOREIGN KEY (card_id, board_id) REFERENCES cards(id, board_id) ON DELETE RESTRICT
);
CREATE INDEX card_notes_card_time ON card_notes(card_id, created_at, id);
