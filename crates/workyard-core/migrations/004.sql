-- Existing questions predate choices. Keep them answerable with a generic fallback;
-- every newly raised question must provide its own substantive options.
ALTER TABLE questions ADD COLUMN options_json TEXT NOT NULL DEFAULT '["Yes","No","Other"]'
  CHECK (json_valid(options_json) AND json_type(options_json) = 'array' AND json_array_length(options_json) >= 2);
ALTER TABLE questions ADD COLUMN answer_note TEXT NOT NULL DEFAULT '';
