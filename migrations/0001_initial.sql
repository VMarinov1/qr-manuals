-- Потребители, които качват упътвания
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    email         TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Сесии след вход (id е случаен токен в бисквитка)
CREATE TABLE sessions (
    id         TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX sessions_user_id ON sessions (user_id);

-- Езиците, на които може да има упътване
CREATE TABLE languages (
    code       TEXT    PRIMARY KEY,
    name       TEXT    NOT NULL,
    sort_order INTEGER NOT NULL
);
INSERT INTO languages (code, name, sort_order) VALUES
    ('bg', 'Български', 1),
    ('en', 'English',   2),
    ('de', 'Deutsch',   3),
    ('ro', 'Română',    4);

CREATE TABLE products (
    id               INTEGER PRIMARY KEY,
    name             TEXT    NOT NULL,
    sku              TEXT    UNIQUE,
    description      TEXT    NOT NULL DEFAULT '',
    -- Постоянен публичен идентификатор, отпечатан в QR кода. Не се сменя.
    public_slug      TEXT    NOT NULL UNIQUE,
    default_language TEXT    NOT NULL DEFAULT 'bg' REFERENCES languages (code),
    created_at       TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at       TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Едно упътване на език за всеки продукт
CREATE TABLE manuals (
    id         INTEGER PRIMARY KEY,
    product_id INTEGER NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    language   TEXT    NOT NULL REFERENCES languages (code),
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (product_id, language)
);

-- Снимки и PDF файлове на упътване
CREATE TABLE files (
    id            INTEGER PRIMARY KEY,
    manual_id     INTEGER NOT NULL REFERENCES manuals (id) ON DELETE CASCADE,
    original_name TEXT    NOT NULL,
    -- Име на файла в UPLOAD_DIR (случайно, без връзка с оригиналното)
    stored_name   TEXT    NOT NULL UNIQUE,
    mime          TEXT    NOT NULL CHECK (mime IN ('image/jpeg', 'image/png', 'image/webp', 'application/pdf')),
    size_bytes    INTEGER NOT NULL,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX files_manual_id ON files (manual_id, sort_order);
