-- The binary that holds a board's lease: package version and schema version. A binary that
-- would migrate the database refuses while a live lease belongs to an older schema.
ALTER TABLE orchestrator_sessions ADD COLUMN binary_version TEXT;
ALTER TABLE orchestrator_sessions ADD COLUMN schema_version INTEGER;
