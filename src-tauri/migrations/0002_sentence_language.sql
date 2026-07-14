ALTER TABLE sentences ADD COLUMN target_language TEXT NOT NULL DEFAULT 'English';
UPDATE sentences
SET target_language = COALESCE((SELECT target_language FROM settings WHERE id = 1), 'English');
CREATE INDEX idx_sentences_language_status ON sentences(target_language, status);
