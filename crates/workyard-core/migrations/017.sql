-- How many phases past the current one the dispatcher may offer work from. NULL is unlimited;
-- 1 offers the next phase only once the current phase has no ready work.
ALTER TABLE boards ADD COLUMN phase_window INTEGER CHECK (phase_window IS NULL OR phase_window >= 1);
