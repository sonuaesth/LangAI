CREATE TABLE user_settings (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    model TEXT NOT NULL DEFAULT 'gpt-5-mini',
    target_language TEXT NOT NULL DEFAULT 'English',
    elevenlabs_voice_id TEXT,
    elevenlabs_voice_name TEXT,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sentences (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_text TEXT NOT NULL CHECK (length(trim(source_text)) > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    UNIQUE (id, user_id)
);
CREATE INDEX sentences_user_created ON sentences(user_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE TABLE topics (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    UNIQUE (id, user_id)
);
CREATE UNIQUE INDEX topics_user_name_unique ON topics(user_id, lower(name))
    WHERE deleted_at IS NULL;

CREATE TABLE sentence_topics (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sentence_id UUID NOT NULL,
    topic_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    PRIMARY KEY (user_id, sentence_id, topic_id),
    FOREIGN KEY (sentence_id, user_id) REFERENCES sentences(id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (topic_id, user_id) REFERENCES topics(id, user_id) ON DELETE CASCADE
);

CREATE TABLE sentence_languages (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sentence_id UUID NOT NULL,
    target_language TEXT NOT NULL CHECK (length(trim(target_language)) > 0),
    status TEXT NOT NULL DEFAULT 'unprepared'
        CHECK (status IN ('unprepared','queued','generating','ready','failed')),
    error TEXT,
    translation_comment TEXT,
    active_preparation_id UUID,
    audio_object_key TEXT,
    audio_name TEXT,
    audio_mime TEXT,
    audio_sha256 TEXT,
    audio_size BIGINT CHECK (audio_size IS NULL OR audio_size >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    PRIMARY KEY (user_id, sentence_id, target_language),
    FOREIGN KEY (sentence_id, user_id) REFERENCES sentences(id, user_id) ON DELETE CASCADE
);
CREATE INDEX sentence_languages_filter
    ON sentence_languages(user_id, target_language, status)
    WHERE deleted_at IS NULL;

CREATE TABLE preparations (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sentence_id UUID NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    target_language TEXT NOT NULL,
    model TEXT NOT NULL,
    translation TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    UNIQUE (id, user_id),
    UNIQUE (user_id, sentence_id, target_language, version),
    FOREIGN KEY (sentence_id, user_id) REFERENCES sentences(id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (user_id, sentence_id, target_language)
        REFERENCES sentence_languages(user_id, sentence_id, target_language) ON DELETE CASCADE
);
CREATE INDEX preparations_sentence
    ON preparations(user_id, sentence_id, target_language, version DESC)
    WHERE deleted_at IS NULL;

ALTER TABLE sentence_languages
    ADD CONSTRAINT sentence_languages_active_preparation
    FOREIGN KEY (active_preparation_id, user_id)
    REFERENCES preparations(id, user_id) ON DELETE SET NULL (active_preparation_id);

CREATE TABLE blocks (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    preparation_id UUID NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    correct TEXT NOT NULL,
    hint TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    UNIQUE (id, user_id),
    UNIQUE (user_id, preparation_id, position),
    FOREIGN KEY (preparation_id, user_id) REFERENCES preparations(id, user_id) ON DELETE CASCADE
);

CREATE TABLE options (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    block_id UUID NOT NULL,
    text TEXT NOT NULL,
    is_correct BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    UNIQUE (id, user_id),
    FOREIGN KEY (block_id, user_id) REFERENCES blocks(id, user_id) ON DELETE CASCADE
);
CREATE INDEX options_block ON options(user_id, block_id) WHERE deleted_at IS NULL;

CREATE TABLE sync_operations (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    operation_id UUID NOT NULL,
    device_id UUID REFERENCES devices(id) ON DELETE SET NULL,
    response JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, operation_id)
);

CREATE TABLE sync_changes (
    sequence BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL,
    entity_id UUID NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('upsert','delete')),
    revision BIGINT NOT NULL CHECK (revision > 0),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX sync_changes_user_cursor ON sync_changes(user_id, sequence);
