-- MEEV schema v1
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ---------------------------------------------------------------- users
CREATE TABLE IF NOT EXISTS users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username      VARCHAR(24) NOT NULL UNIQUE,
    email         VARCHAR(254) NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name  VARCHAR(40) NOT NULL,
    avatar_url    TEXT,
    banner_url    TEXT,
    bio           VARCHAR(300),
    interests     TEXT[] NOT NULL DEFAULT '{}',
    location_lat  DOUBLE PRECISION,
    location_lng  DOUBLE PRECISION,
    location_name VARCHAR(120),
    country       VARCHAR(80),
    is_private    BOOLEAN NOT NULL DEFAULT FALSE,
    is_active     BOOLEAN NOT NULL DEFAULT TRUE,
    is_online     BOOLEAN NOT NULL DEFAULT FALSE,
    last_seen     TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT username_format CHECK (username ~ '^[a-z0-9_.]{3,24}$'),
    CONSTRAINT display_name_not_empty CHECK (length(btrim(display_name)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_users_username_lower ON users (LOWER(username));
CREATE INDEX IF NOT EXISTS idx_users_email_lower ON users (LOWER(email));
CREATE INDEX IF NOT EXISTS idx_users_location ON users (location_lat, location_lng);
CREATE INDEX IF NOT EXISTS idx_users_last_seen ON users (last_seen DESC NULLS LAST);

-- ---------------------------------------------------------------- refresh tokens (revocable sessions)
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash CHAR(64) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens (user_id);

-- ---------------------------------------------------------------- follows
CREATE TABLE IF NOT EXISTS follows (
    follower_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    followee_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status      VARCHAR(10) NOT NULL DEFAULT 'accepted',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (follower_id, followee_id),
    CONSTRAINT follows_status_check CHECK (status IN ('accepted', 'pending', 'rejected')),
    CONSTRAINT no_self_follow CHECK (follower_id <> followee_id)
);
CREATE INDEX IF NOT EXISTS idx_follows_followee ON follows (followee_id, status);
CREATE INDEX IF NOT EXISTS idx_follows_follower ON follows (follower_id, status);

-- ---------------------------------------------------------------- blocks
CREATE TABLE IF NOT EXISTS blocks (
    blocker_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (blocker_id, blocked_id),
    CONSTRAINT no_self_block CHECK (blocker_id <> blocked_id)
);

-- ---------------------------------------------------------------- interests
CREATE TABLE IF NOT EXISTS interests (
    name       VARCHAR(32) PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_interests (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name    VARCHAR(32) NOT NULL REFERENCES interests(name) ON DELETE CASCADE,
    PRIMARY KEY (user_id, name)
);
CREATE INDEX IF NOT EXISTS idx_user_interests_name ON user_interests (name);

-- ---------------------------------------------------------------- attachments
CREATE TABLE IF NOT EXISTS attachments (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    message_id    UUID,           -- set once attached to a message
    original_name VARCHAR(180) NOT NULL,
    mime_type     VARCHAR(120) NOT NULL,
    size          BIGINT NOT NULL CHECK (size >= 0),
    kind          VARCHAR(12) NOT NULL DEFAULT 'file',
    stored_rel    TEXT NOT NULL,
    thumb_rel     TEXT,
    width         INT,
    height        INT,
    duration_ms   BIGINT,
    hash          CHAR(64) NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT attachments_kind_check CHECK (kind IN ('image', 'video', 'audio', 'file'))
);
CREATE INDEX IF NOT EXISTS idx_attachments_owner ON attachments (owner_id);
CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments (message_id);

-- ---------------------------------------------------------------- conversations
CREATE TABLE IF NOT EXISTS conversations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    is_group        BOOLEAN NOT NULL DEFAULT FALSE,
    name            VARCHAR(60),
    avatar_url      TEXT,
    created_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    last_message_at TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_conversations_last ON conversations (last_message_at DESC NULLS LAST);

CREATE TABLE IF NOT EXISTS conversation_members (
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            VARCHAR(12) NOT NULL DEFAULT 'member',
    muted           BOOLEAN NOT NULL DEFAULT FALSE,
    last_read_at    TIMESTAMPTZ,
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (conversation_id, user_id),
    CONSTRAINT member_role_check CHECK (role IN ('owner', 'admin', 'member'))
);
CREATE INDEX IF NOT EXISTS idx_conv_members_user ON conversation_members (user_id);

-- ---------------------------------------------------------------- messages
CREATE TABLE IF NOT EXISTS messages (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    sender_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content         TEXT NOT NULL DEFAULT '',
    attachment_id   UUID REFERENCES attachments(id) ON DELETE SET NULL,
    edited_at       TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ,
    sent_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_messages_conv_sent ON messages (conversation_id, sent_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages (sender_id);

-- Keep schema version for ops.
CREATE TABLE IF NOT EXISTS schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO schema_meta (key, value) VALUES ('version', '1') ON CONFLICT (key) DO NOTHING;
