CREATE TABLE administrator (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    username TEXT NOT NULL COLLATE NOCASE UNIQUE,
    password_hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE bootstrap (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    token_hash TEXT NOT NULL,
    consumed_at INTEGER
);

CREATE TABLE backend (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    controller_url TEXT NOT NULL,
    secret TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    absolute_expires_at INTEGER NOT NULL,
    ip TEXT NOT NULL,
    user_agent TEXT NOT NULL
);
CREATE INDEX sessions_expiry ON sessions(absolute_expires_at);

CREATE TABLE web_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    settings_json TEXT NOT NULL
);
