CREATE TABLE IF NOT EXISTS chat_conversations (
    id          TEXT    PRIMARY KEY,
    title       TEXT    NOT NULL,
    messages    TEXT    NOT NULL,  -- JSON array of { role, content }
    created_at  INTEGER NOT NULL,  -- unix epoch seconds
    updated_at  INTEGER NOT NULL   -- unix epoch seconds
);

CREATE INDEX IF NOT EXISTS idx_chat_conversations_updated_at
    ON chat_conversations(updated_at DESC);
