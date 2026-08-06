CREATE TABLE sync_audio_state (
    sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
    target_language TEXT NOT NULL,
    local_sha256 TEXT,
    remote_sha256 TEXT,
    remote_mime TEXT,
    remote_name TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(sentence_id, target_language)
);
