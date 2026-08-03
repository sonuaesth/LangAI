ALTER TABLE sync_state ADD COLUMN chronology_upload_enqueued INTEGER NOT NULL DEFAULT 0
    CHECK (chronology_upload_enqueued IN (0,1));
