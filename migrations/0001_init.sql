-- TAIS Core Engine — SQLite Schema v1
-- Run with: sqlx migrate or via include_str! at runtime

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Sessions: conversation sessions
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,         -- session_id (UUID v4)
    user_id         TEXT,                     -- optional user binding
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    metadata        TEXT DEFAULT '{}'         -- JSON blob
);

-- Conversation turns within a session
CREATE TABLE IF NOT EXISTS turns (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,         -- turn number within session
    role            TEXT NOT NULL CHECK(role IN ('student', 'tutor', 'system')),
    content         TEXT NOT NULL,
    concept         TEXT,
    metadata        TEXT DEFAULT '{}',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(session_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);

-- User profiles
CREATE TABLE IF NOT EXISTS users (
    id              TEXT PRIMARY KEY,         -- user_id
    display_name    TEXT,
    grade_level     TEXT,
    preferred_style TEXT,
    password_hash   TEXT,
    wechat_openid   TEXT UNIQUE,
    total_sessions  INTEGER DEFAULT 0,
    total_turns     INTEGER DEFAULT 0,
    metadata        TEXT DEFAULT '{}',        -- JSON: preferred_subjects, llm_config
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    last_active     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_users_openid ON users(wechat_openid);

-- Student mastery per concept
CREATE TABLE IF NOT EXISTS user_mastery (
    user_id         TEXT NOT NULL,
    concept         TEXT NOT NULL,
    level           REAL DEFAULT 0.0,         -- 0.0-1.0
    exposures       INTEGER DEFAULT 0,
    correct_responses INTEGER DEFAULT 0,
    last_practiced  TEXT NOT NULL DEFAULT (datetime('now')),
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, concept),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- User misconceptions
CREATE TABLE IF NOT EXISTS misconceptions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    concept         TEXT NOT NULL,
    description     TEXT NOT NULL,
    occurrences     INTEGER DEFAULT 1,
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen       TEXT NOT NULL DEFAULT (datetime('now')),
    resolved        INTEGER DEFAULT 0
);

-- Shared knowledge graph
CREATE TABLE IF NOT EXISTS knowledge_nodes (
    concept         TEXT PRIMARY KEY,
    domain          TEXT NOT NULL,
    description     TEXT DEFAULT '',
    prerequisites   TEXT DEFAULT '[]',        -- JSON array
    typical_misconceptions TEXT DEFAULT '[]', -- JSON array
    best_explanation TEXT DEFAULT '',
    probing_questions     TEXT DEFAULT '[]',  -- JSON array
    difficulty      REAL DEFAULT 0.5,
    created_by      TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Shared FAQs
CREATE TABLE IF NOT EXISTS faqs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    question        TEXT NOT NULL,
    answer_template TEXT NOT NULL,
    domain          TEXT NOT NULL,
    usage_count     INTEGER DEFAULT 0,
    effectiveness   REAL DEFAULT 0.5,
    related_concepts TEXT DEFAULT '[]',       -- JSON array
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Shared teaching strategies
CREATE TABLE IF NOT EXISTS strategies (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL,
    applicable_concepts TEXT DEFAULT '[]',    -- JSON array
    success_rate    REAL DEFAULT 0.5,
    times_used      INTEGER DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Habit rule definitions (7 built-in H01-H07)
CREATE TABLE IF NOT EXISTS habit_rules (
    id              TEXT PRIMARY KEY,         -- "H01", "H02", ...
    name            TEXT NOT NULL,
    description     TEXT NOT NULL,
    trigger_type    TEXT NOT NULL CHECK(trigger_type IN ('periodic', 'event_driven', 'conditional')),
    trigger_config  TEXT NOT NULL DEFAULT '{}', -- JSON: interval/event/predicate
    learning_rate   REAL NOT NULL DEFAULT 0.1,  -- eta
    decay_rate      REAL NOT NULL DEFAULT 0.05, -- lambda
    enabled         INTEGER DEFAULT 1
);

-- Habit execution logs
CREATE TABLE IF NOT EXISTS habit_logs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id         TEXT NOT NULL REFERENCES habit_rules(id),
    triggered_at    TEXT NOT NULL DEFAULT (datetime('now')),
    context         TEXT DEFAULT '{}',        -- JSON: what triggered
    action_result   TEXT DEFAULT '',
    success         INTEGER DEFAULT 0,
    duration_ms     INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_habit_logs_rule ON habit_logs(rule_id);

-- Evolution records for tracking prompt evolution
CREATE TABLE IF NOT EXISTS evolution_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    agent           TEXT NOT NULL,
    action          TEXT NOT NULL,
    old_prompt      TEXT DEFAULT '',
    new_prompt      TEXT DEFAULT '',
    composite_before REAL DEFAULT 0.0,
    composite_after  REAL DEFAULT 0.0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Agent loop decisions
CREATE TABLE IF NOT EXISTS agent_decisions (
    id              TEXT PRIMARY KEY,
    student_id      TEXT NOT NULL,
    concept         TEXT NOT NULL,
    skill           TEXT,
    strategy        TEXT,
    overall_score   REAL DEFAULT 0.0,
    action          TEXT,                     -- deploy/retain/flag/retire
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
