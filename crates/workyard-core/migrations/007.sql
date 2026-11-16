CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE CHECK (length(trim(name)) > 0),
  description TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0)
);
ALTER TABLE boards ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE RESTRICT;
CREATE INDEX boards_project ON boards(project_id);

CREATE TABLE project_repositories (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  path TEXT NOT NULL,
  target_branch TEXT NOT NULL DEFAULT 'main',
  remote_url TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(project_id,path)
);
CREATE TABLE project_documents (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  repository_id TEXT NOT NULL REFERENCES project_repositories(id) ON DELETE RESTRICT,
  path TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('adr','diagram','documentation')),
  title TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(repository_id,path)
);
CREATE TABLE project_stakeholders (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  name TEXT NOT NULL CHECK (length(trim(name)) > 0),
  role TEXT NOT NULL DEFAULT '',
  contact TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);
ALTER TABLE questions ADD COLUMN stakeholder_id TEXT REFERENCES project_stakeholders(id) ON DELETE RESTRICT;

ALTER TABLE dependencies ADD COLUMN scope TEXT NOT NULL DEFAULT 'start' CHECK (scope IN ('start','finish'));

CREATE TABLE card_changes (
  id TEXT PRIMARY KEY,
  card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE RESTRICT,
  repository_id TEXT NOT NULL REFERENCES project_repositories(id) ON DELETE RESTRICT,
  working_branch TEXT NOT NULL CHECK (length(trim(working_branch)) > 0),
  target_branch TEXT NOT NULL CHECK (length(trim(target_branch)) > 0),
  commit_oid TEXT NOT NULL DEFAULT '',
  review_required INTEGER NOT NULL DEFAULT 0 CHECK (review_required IN (0,1)),
  approved_commit_oid TEXT,
  reviewer TEXT,
  version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
  created_at TEXT NOT NULL,
  UNIQUE(card_id,repository_id,working_branch)
);

ALTER TABLE events RENAME TO events_v6;
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  actor TEXT NOT NULL,
  type TEXT NOT NULL,
  board_id TEXT REFERENCES boards(id) ON DELETE RESTRICT,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  before_json TEXT,
  after_json TEXT,
  effects_json TEXT NOT NULL DEFAULT '[]',
  note TEXT NOT NULL DEFAULT ''
);
INSERT INTO events SELECT * FROM events_v6;
DROP TABLE events_v6;
CREATE INDEX events_board_id ON events(board_id,id);
