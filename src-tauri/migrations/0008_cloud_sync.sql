CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    server_url TEXT,
    user_id TEXT,
    device_id TEXT,
    pull_cursor INTEGER NOT NULL DEFAULT 0 CHECK (pull_cursor >= 0),
    status TEXT NOT NULL DEFAULT 'disconnected'
        CHECK (status IN ('disconnected','synced','syncing','offline','error','pending')),
    last_error TEXT,
    last_sync_at TEXT,
    initial_upload_completed INTEGER NOT NULL DEFAULT 0
        CHECK (initial_upload_completed IN (0,1))
);
INSERT INTO sync_state(id) VALUES(1);

CREATE TABLE sync_entity_ids (
    entity_type TEXT NOT NULL,
    local_key TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    server_revision INTEGER,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(entity_type, local_key),
    UNIQUE(entity_type, remote_id)
);

CREATE TABLE sync_outbox (
    operation_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    base_revision INTEGER,
    payload TEXT,
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending','sending','failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_attempt_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX sync_outbox_pending
    ON sync_outbox(state, next_attempt_at, created_at);

CREATE TABLE sync_import_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    backup_path TEXT,
    started_at TEXT,
    completed_at TEXT,
    last_error TEXT
);
INSERT INTO sync_import_state(id) VALUES(1);
