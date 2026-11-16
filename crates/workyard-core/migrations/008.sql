CREATE TABLE phases (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  created_at TEXT NOT NULL,
  UNIQUE(board_id, name),
  UNIQUE(board_id, position)
);
ALTER TABLE boards ADD COLUMN current_phase_id TEXT REFERENCES phases(id) ON DELETE RESTRICT;
ALTER TABLE cards ADD COLUMN phase_id TEXT REFERENCES phases(id) ON DELETE RESTRICT;
ALTER TABLE questions ADD COLUMN phase_id TEXT REFERENCES phases(id) ON DELETE RESTRICT;
ALTER TABLE cards ADD COLUMN cancelled_at TEXT;
ALTER TABLE cards ADD COLUMN cancellation_reason TEXT;
ALTER TABLE cards ADD COLUMN cancelled_by TEXT;
CREATE INDEX cards_phase ON cards(phase_id);
CREATE INDEX questions_phase ON questions(phase_id);
CREATE INDEX phases_board_position ON phases(board_id, position);

CREATE TRIGGER cards_phase_insert BEFORE INSERT ON cards
WHEN NEW.phase_id IS NULL OR NOT EXISTS (SELECT 1 FROM phases WHERE id=NEW.phase_id AND board_id=NEW.board_id)
BEGIN SELECT RAISE(ABORT, 'card phase must belong to its board'); END;
CREATE TRIGGER cards_phase_update BEFORE UPDATE OF phase_id,board_id ON cards
WHEN NEW.phase_id IS NULL OR NOT EXISTS (SELECT 1 FROM phases WHERE id=NEW.phase_id AND board_id=NEW.board_id)
BEGIN SELECT RAISE(ABORT, 'card phase must belong to its board'); END;
CREATE TRIGGER questions_phase_insert BEFORE INSERT ON questions
WHEN NEW.phase_id IS NULL OR NOT EXISTS (SELECT 1 FROM phases WHERE id=NEW.phase_id AND board_id=NEW.board_id)
BEGIN SELECT RAISE(ABORT, 'question phase must belong to its board'); END;
CREATE TRIGGER questions_phase_update BEFORE UPDATE OF phase_id,board_id ON questions
WHEN NEW.phase_id IS NULL OR NOT EXISTS (SELECT 1 FROM phases WHERE id=NEW.phase_id AND board_id=NEW.board_id)
BEGIN SELECT RAISE(ABORT, 'question phase must belong to its board'); END;
CREATE TRIGGER boards_current_phase_update BEFORE UPDATE OF current_phase_id ON boards
WHEN NEW.current_phase_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM phases WHERE id=NEW.current_phase_id AND board_id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'current phase must belong to its board'); END;
