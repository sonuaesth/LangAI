ALTER TABLE sentences ADD COLUMN sync_order BIGINT NOT NULL DEFAULT 0;
CREATE INDEX sentences_user_chronological
    ON sentences(user_id, created_at DESC, sync_order DESC)
    WHERE deleted_at IS NULL;
