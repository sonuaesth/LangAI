CREATE TABLE sentence_languages (
  sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
  target_language TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'unprepared' CHECK(status IN('unprepared','queued','generating','ready','failed')),
  error TEXT,
  active_preparation_id INTEGER REFERENCES preparations(id) ON DELETE SET NULL,
  PRIMARY KEY(sentence_id, target_language)
);
INSERT INTO sentence_languages(sentence_id,target_language,status,error,active_preparation_id)
SELECT id,target_language,status,error,active_preparation_id FROM sentences;
CREATE INDEX idx_sentence_languages_filter ON sentence_languages(target_language,status);
