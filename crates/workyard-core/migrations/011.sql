-- One orchestrator session per board: desired state, event cursor, pool size, and a lease.
-- `epoch` is a fencing token: every takeover increments it, and orchestrator writes made
-- with a stale epoch are rejected by the core.
CREATE TABLE orchestrator_sessions (
  board_id TEXT PRIMARY KEY REFERENCES boards(id) ON DELETE RESTRICT,
  desired_state TEXT NOT NULL DEFAULT 'running' CHECK (desired_state IN ('running','paused','draining')),
  cursor INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
  pool_size INTEGER NOT NULL DEFAULT 1 CHECK (pool_size BETWEEN 1 AND 64),
  holder TEXT,
  host TEXT,
  pid INTEGER,
  epoch INTEGER NOT NULL DEFAULT 0 CHECK (epoch >= 0),
  heartbeat_at TEXT,
  lease_expires_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK ((holder IS NULL) = (lease_expires_at IS NULL))
);
