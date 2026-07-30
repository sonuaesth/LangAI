CREATE TABLE provider_credentials (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('openai','elevenlabs')),
    ciphertext BYTEA NOT NULL,
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 12),
    key_hint TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, provider)
);
