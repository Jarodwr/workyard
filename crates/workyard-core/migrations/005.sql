-- Older questions had no agent recommendation. They remain readable and can
-- gain one through question-options; new questions require one on creation.
ALTER TABLE questions ADD COLUMN recommended_option TEXT NOT NULL DEFAULT '';
ALTER TABLE questions ADD COLUMN recommendation_reason TEXT NOT NULL DEFAULT '';
