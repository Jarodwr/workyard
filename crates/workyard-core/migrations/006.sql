-- Preserve substantive choices and keep exactly one canonical Other last.
-- This changes schema data without creating a user-action event or version bump.
UPDATE questions
SET options_json = json_insert(
  (SELECT json_group_array(value) FROM (
    SELECT value FROM json_each(questions.options_json)
    WHERE lower(trim(value)) != 'other'
    ORDER BY CAST(key AS INTEGER)
  )),
  '$[#]',
  'Other'
);
UPDATE questions SET recommended_option = 'Other'
WHERE lower(trim(recommended_option)) = 'other';
