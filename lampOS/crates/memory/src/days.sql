CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS people (
    person TEXT PRIMARY KEY,
    next_id INTEGER NOT NULL DEFAULT 1,
    generation INTEGER NOT NULL DEFAULT 0,
    skipped INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS entries (
    rid INTEGER PRIMARY KEY AUTOINCREMENT,
    person TEXT NOT NULL REFERENCES people(person),
    id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('exchange','kept','observation')),
    at_unix_ms INTEGER NOT NULL,
    day TEXT NOT NULL,
    heard TEXT NOT NULL DEFAULT '',
    said TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL DEFAULT '',
    category TEXT,
    used_unix_ms INTEGER NOT NULL,
    active INTEGER NOT NULL DEFAULT 1,
    UNIQUE(person,id)
);
CREATE INDEX IF NOT EXISTS entries_person_day ON entries(person,day);
CREATE INDEX IF NOT EXISTS entries_person_kind ON entries(person,kind,id);
CREATE TABLE IF NOT EXISTS sources (
    child INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    parent INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    PRIMARY KEY(child,parent),
    CHECK(child != parent)
);
CREATE INDEX IF NOT EXISTS sources_parent ON sources(parent);
CREATE TABLE IF NOT EXISTS replacements (
    newer INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    older INTEGER NOT NULL UNIQUE REFERENCES entries(rid) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS sessions (
    person TEXT NOT NULL REFERENCES people(person),
    session TEXT NOT NULL,
    opened_unix_ms INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    max_bytes INTEGER NOT NULL,
    note TEXT NOT NULL,
    ids TEXT NOT NULL,
    invalidated INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(person,session)
);
CREATE TABLE IF NOT EXISTS jobs (
    job INTEGER PRIMARY KEY AUTOINCREMENT,
    person TEXT NOT NULL REFERENCES people(person),
    conversation TEXT NOT NULL,
    generation INTEGER NOT NULL,
    at_unix_ms INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','done')),
    input TEXT NOT NULL DEFAULT '',
    result TEXT NOT NULL DEFAULT '[]',
    UNIQUE(person,conversation)
);
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
    body, tokenize='unicode61 remove_diacritics 2'
);
INSERT INTO search(search,rank) VALUES('secure-delete',1);
CREATE TEMP TABLE IF NOT EXISTS doomed(rid INTEGER PRIMARY KEY);
-- #1693 (3): an observation asked about once, unprompted. Its row goes with
-- the observation when that is forgotten. Older builds pass this table over.
CREATE TABLE IF NOT EXISTS follow_ups (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    person TEXT NOT NULL,
    asked_unix_ms INTEGER NOT NULL
);
