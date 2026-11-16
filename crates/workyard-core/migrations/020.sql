-- How workers bring a card branch onto the target: merge the target in (default) or rebase onto it.
ALTER TABLE boards ADD COLUMN integration TEXT NOT NULL DEFAULT 'merge' CHECK (integration IN ('merge','rebase'));
