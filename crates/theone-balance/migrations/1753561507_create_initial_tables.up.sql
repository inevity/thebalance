CREATE TABLE keys (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    provider TEXT NOT NULL,
    model_coolings TEXT,
    total_cooling_seconds INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'active',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE UNIQUE INDEX provider_key_unq_idx ON keys(provider, key);
CREATE INDEX provider_status_created_at_idx ON keys(provider, status, created_at);
CREATE INDEX total_cooling_seconds_idx ON keys(total_cooling_seconds);
