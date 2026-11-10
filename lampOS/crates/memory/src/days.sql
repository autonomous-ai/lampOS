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
-- #4753: a kept thing said again (`remember` with `once` finding it) records
-- when, so a recall tells when it was last said, not only when it was first
-- kept: one row a time it was said again, written with the line that said it
-- (`origin`, that exchange's id in its own scope; never 0). No row: it was
-- last said at its own `at_unix_ms`, as every kept thing of an older build.
-- A forget of a time's line takes that time's rows, so a forgotten line
-- leaves no time behind; a line that only expires leaves them.
-- Its rows go with the entry. Older builds pass this table over;
-- `user_version` stays 1.
CREATE TABLE IF NOT EXISTS said_again (
    rid INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    at_unix_ms INTEGER NOT NULL,
    origin INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(rid,at_unix_ms,origin)
);
-- Nightly pass B (the memory owner's extraction plan C, step B, 3 Nov;
-- `nightly.rs`): an item's own fields beside its `entries` row (an
-- observation of category episode, fact, profile, pattern, ahead or
-- feeling): its kind, the night that made it, the local day after which it
-- closes (`expires`, none for no age expiry) and its fields as JSON. Its
-- row goes with the entry. Older builds pass these tables over;
-- `user_version` stays 1.
CREATE TABLE IF NOT EXISTS nightly_items (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    person TEXT NOT NULL,
    kind TEXT NOT NULL,
    night TEXT NOT NULL,
    expires TEXT,
    detail TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS nightly_items_by ON nightly_items(person,kind);
-- A nightly job: the ids it gave, never their text, so a forget never
-- needs to erase one and a forget during a pass drops only what rests on
-- what it forgot. Kept a day.
CREATE TABLE IF NOT EXISTS nightly_jobs (
    job INTEGER PRIMARY KEY AUTOINCREMENT,
    person TEXT NOT NULL,
    day TEXT NOT NULL,
    after_id INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    at_unix_ms INTEGER NOT NULL,
    ids TEXT NOT NULL,
    existing TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','done')),
    result TEXT NOT NULL DEFAULT '',
    UNIQUE(person,day,after_id)
);
-- A forget by a known person (El's option 2 on #5333): the latest time of
-- one local day they asked any forget. The night passes over their
-- exchanges of that day said before it, so a line a topic forget had
-- nothing to match isn't kept that night. Goes with `forget_all`,
-- `forget_person` and with the day's exchanges. Older builds pass this
-- table over; `user_version` stays 1.
CREATE TABLE IF NOT EXISTS nightly_forgets (
    person TEXT NOT NULL,
    day TEXT NOT NULL,
    at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY(person,day)
);
-- One night's profile allowance: the lines it began with, and how many
-- it changed and added.
CREATE TABLE IF NOT EXISTS nightly_nights (
    person TEXT NOT NULL,
    day TEXT NOT NULL,
    base INTEGER NOT NULL,
    changed INTEGER NOT NULL DEFAULT 0,
    added INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(person,day)
);
-- A role a known person asked the lamp to play (the voice's LAMP_MEMORY_ROLE,
-- the memory owner's design of 6 Nov): the name it answers to with them,
-- until they end it. One row a known person (`p-` ids only), set by
-- `role_set`, removed by `role_end`, and read only by their `session_open`
-- with `role`. Goes with `forget_all` and `forget_person`. Older builds pass
-- this table over; `user_version` stays 1.
CREATE TABLE IF NOT EXISTS roles (
    person TEXT PRIMARY KEY REFERENCES people(person),
    name TEXT NOT NULL,
    at_unix_ms INTEGER NOT NULL
);
-- Service records (the voice's LAMP_SERVICE_RECORDS, the connector records
-- plan, PR 1a; `service_records.rs`): one row for each thing a known person
-- asked of a linked service, keyed by the voice's own `key`, updated in
-- place until its status is no longer `prepared`. Kept until forgotten: no
-- expiry, tidy or nightly pass reads this table; forget everything, forget
-- the person and a forget about a thing remove rows. `links`: the keys of
-- the records it came from (a redo); `replaced_by`: the key of the record
-- that replaced it (set even on a final one); `result_id`: the service's
-- own id of what was made (an event id), for a later edit. Older builds pass this
-- table over; `user_version` stays 1.
CREATE TABLE IF NOT EXISTS service_records (
    person TEXT NOT NULL REFERENCES people(person),
    key TEXT NOT NULL,
    service TEXT NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('prepared','done','cancelled','refused','failed','timed_out','no_result')),
    reason TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT '{}',
    result_id TEXT NOT NULL DEFAULT '',
    asked_unix_ms INTEGER NOT NULL,
    changed_unix_ms INTEGER NOT NULL,
    links TEXT NOT NULL DEFAULT '[]',
    replaced_by TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(person,key)
);
CREATE INDEX IF NOT EXISTS service_records_by ON service_records(person,changed_unix_ms);
-- When each known person's everything was last forgotten (forget everything,
-- forget the person; '' for everyone's): a service record asked before it is
-- refused, so a change the voice sent before the forget and the memory took
-- after it is never kept (Jarvis's P2 on #6073). The person's id and a time
-- only, never what was forgotten.
CREATE TABLE IF NOT EXISTS service_records_forgot (
    person TEXT PRIMARY KEY,
    at_unix_ms INTEGER NOT NULL
);
