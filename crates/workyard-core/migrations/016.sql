-- A tool a worker asked to use in `safe` mode, waiting for a human. The request and its
-- decision are also events (permission_requested, permission_decided); this table is the
-- current state for listing. `run_rule` is the rule an "allow for this run" decision grants.
CREATE TABLE permission_requests (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  card_id TEXT NOT NULL REFERENCES cards(id),
  -- The runtime's own request id, echoed back when answering.
  request_id TEXT NOT NULL,
  tool_use_id TEXT NOT NULL DEFAULT '',
  tool_name TEXT NOT NULL,
  input_json TEXT NOT NULL DEFAULT '{}',
  description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','allowed_once','allowed_run','denied','expired')),
  run_rule TEXT NOT NULL DEFAULT '',
  decided_by TEXT,
  reason TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  decided_at TEXT
);
CREATE INDEX permission_requests_card ON permission_requests(card_id, status);
