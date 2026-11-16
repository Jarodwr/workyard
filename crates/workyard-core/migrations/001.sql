PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS id_counters (
  kind TEXT PRIMARY KEY CHECK (kind IN ('board', 'card', 'question')),
  next_value INTEGER NOT NULL CHECK (next_value > 0)
);

CREATE TABLE IF NOT EXISTS boards (
  id TEXT PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  description TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0)
);

CREATE TABLE IF NOT EXISTS lanes (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  kind TEXT NOT NULL CHECK (kind IN ('backlog', 'active', 'review', 'done', 'custom')),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
  UNIQUE (id, board_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS lanes_active_name ON lanes(board_id, name) WHERE archived = 0;
CREATE INDEX IF NOT EXISTS lanes_board_order ON lanes(board_id, position);

CREATE TABLE IF NOT EXISTS cards (
  id TEXT PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  lane_id TEXT NOT NULL,
  position INTEGER NOT NULL CHECK (position >= 0),
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  description TEXT NOT NULL DEFAULT '',
  priority TEXT NOT NULL DEFAULT 'P2' CHECK (priority IN ('P0', 'P1', 'P2', 'P3')),
  progress INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
  status_note TEXT NOT NULL DEFAULT '',
  assignee TEXT NOT NULL DEFAULT '',
  owner_actor TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
  UNIQUE (id, board_id),
  FOREIGN KEY (lane_id, board_id) REFERENCES lanes(id, board_id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS cards_lane_order ON cards(lane_id, position);
CREATE INDEX IF NOT EXISTS cards_board ON cards(board_id);

CREATE TABLE IF NOT EXISTS questions (
  id TEXT PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  kind TEXT NOT NULL CHECK (kind IN ('engineering', 'business')),
  title TEXT NOT NULL CHECK (length(trim(title)) > 0),
  body TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'answered', 'dropped')),
  answer TEXT NOT NULL DEFAULT '',
  owner TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  answered_at TEXT,
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
  UNIQUE (id, board_id)
);
CREATE INDEX IF NOT EXISTS questions_board_status ON questions(board_id, status);

CREATE TABLE IF NOT EXISTS dependencies (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  from_card_id TEXT NOT NULL,
  to_card_id TEXT,
  to_question_id TEXT,
  created_at TEXT NOT NULL,
  CHECK ((to_card_id IS NOT NULL) != (to_question_id IS NOT NULL)),
  CHECK (to_card_id IS NULL OR from_card_id != to_card_id),
  FOREIGN KEY (from_card_id, board_id) REFERENCES cards(id, board_id) ON DELETE RESTRICT,
  FOREIGN KEY (to_card_id, board_id) REFERENCES cards(id, board_id) ON DELETE RESTRICT,
  FOREIGN KEY (to_question_id, board_id) REFERENCES questions(id, board_id) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX IF NOT EXISTS deps_card_unique ON dependencies(from_card_id, to_card_id) WHERE to_card_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS deps_question_unique ON dependencies(from_card_id, to_question_id) WHERE to_question_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS deps_from ON dependencies(from_card_id);
CREATE INDEX IF NOT EXISTS deps_to_card ON dependencies(to_card_id);
CREATE INDEX IF NOT EXISTS deps_to_question ON dependencies(to_question_id);

CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  actor TEXT NOT NULL,
  type TEXT NOT NULL,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE RESTRICT,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  before_json TEXT,
  after_json TEXT,
  effects_json TEXT NOT NULL DEFAULT '[]',
  note TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS events_board_id ON events(board_id, id);
