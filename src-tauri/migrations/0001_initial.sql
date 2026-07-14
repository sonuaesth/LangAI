PRAGMA foreign_keys = ON;
CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id=1), model TEXT NOT NULL, target_language TEXT NOT NULL);
INSERT INTO settings VALUES(1,'gpt-5-mini','English');
CREATE TABLE sentences (id INTEGER PRIMARY KEY AUTOINCREMENT, source_text TEXT NOT NULL CHECK(length(trim(source_text))>0), status TEXT NOT NULL DEFAULT 'unprepared' CHECK(status IN('unprepared','queued','generating','ready','failed')), error TEXT, active_preparation_id INTEGER, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE preparations (id INTEGER PRIMARY KEY AUTOINCREMENT, sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE, version INTEGER NOT NULL, target_language TEXT NOT NULL, model TEXT NOT NULL, translation TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(sentence_id,version));
CREATE TABLE blocks (id INTEGER PRIMARY KEY AUTOINCREMENT, preparation_id INTEGER NOT NULL REFERENCES preparations(id) ON DELETE CASCADE, position INTEGER NOT NULL, correct TEXT NOT NULL, hint TEXT, UNIQUE(preparation_id,position));
CREATE TABLE options (id INTEGER PRIMARY KEY AUTOINCREMENT, block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE, text TEXT NOT NULL, is_correct INTEGER NOT NULL CHECK(is_correct IN(0,1)));
CREATE INDEX idx_preparations_sentence ON preparations(sentence_id);
CREATE INDEX idx_blocks_preparation ON blocks(preparation_id);
