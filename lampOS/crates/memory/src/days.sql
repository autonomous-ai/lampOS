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
-- #1693: the local day, YYYY-MM-DD, a thread or promise is about (`on` in a
-- tidy proposal). Its row goes with the observation when that is forgotten
-- or expires. Older builds pass this table over.
CREATE TABLE IF NOT EXISTS event_days (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    day TEXT NOT NULL
);
-- #1693, the voice's LAMP_MEMORY_EVENTS (the first builder on #2602,
-- 22 Oct 09:4x; an undated one too, 13:1x, the voice's LAMP_MEMORY_THREADS):
-- a follow-up given with `until_asked`, not yet asked about:
-- how many sessions it was offered in and when last. It is recorded in
-- `follow_ups` when the voice says the lamp asked (`follow_up_asked`) or on
-- its last offer. Its row goes with the entry when that is forgotten or
-- expires. Older builds pass this table over.
CREATE TABLE IF NOT EXISTS follow_up_offers (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    person TEXT NOT NULL,
    offers INTEGER NOT NULL,
    offered_unix_ms INTEGER NOT NULL
);
-- #2849 (the first builder's yes, 23 Oct 09:37; the voice's
-- LAMP_MEMORY_ROOM_SPEAKER): who said a room line, the id of the one known
-- person whose face was seen saying it; no row when nobody was. Only for the
-- room's (`everyone`) exchanges and kept things, and read only by
-- `forget_said`, never told. Its row goes with the entry when that is
-- forgotten or expires. Older builds pass this table over; `user_version`
-- stays 1.
CREATE TABLE IF NOT EXISTS speakers (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    speaker TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS speakers_by ON speakers(speaker);
-- The reviewer-three's P1 on #4479: a kept thing's or an observation's
-- source exchanges by id in its scope, kept past those exchanges' expiry,
-- so a talk note (which never expires) still goes with a forget of a fact
-- from the same exchanges once the exchanges and their `sources` links are
-- gone. Filled from `sources` before any exchange expires (`maintain`) and
-- read only by a forget. Its rows go with the entry. Older builds pass this
-- table over; `user_version` stays 1.
CREATE TABLE IF NOT EXISTS origins (
    rid INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    origin INTEGER NOT NULL,
    PRIMARY KEY(rid,origin)
);
CREATE INDEX IF NOT EXISTS origins_by ON origins(origin);
