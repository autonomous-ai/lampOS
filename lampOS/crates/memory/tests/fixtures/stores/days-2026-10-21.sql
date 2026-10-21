PRAGMA page_size = 4096;
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
INSERT INTO settings(key,value) VALUES('import_complete',1);
CREATE TABLE people (
    person TEXT PRIMARY KEY,
    next_id INTEGER NOT NULL DEFAULT 1,
    generation INTEGER NOT NULL DEFAULT 0,
    skipped INTEGER NOT NULL DEFAULT 0
);
INSERT INTO people(person,next_id,generation,skipped) VALUES('everyone',4,0,0);
INSERT INTO people(person,next_id,generation,skipped) VALUES('an',2,0,0);
CREATE TABLE entries (
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
INSERT INTO entries(rid,person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(1,'everyone',1,'exchange',1791892800000,'2026-10-13','When is the bread done?','In ten minutes.','',NULL,1791892800000,1);
INSERT INTO entries(rid,person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(2,'everyone',2,'exchange',1791892860000,'2026-10-13','Thank you.','You''re welcome.','',NULL,1791892800000,1);
INSERT INTO entries(rid,person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(3,'everyone',3,'kept',1791892920000,'2026-10-13','','','The bread takes ten minutes',NULL,1791892800000,1);
INSERT INTO entries(rid,person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(4,'an',1,'kept',1791892980000,'2026-10-13','','','Tea without sugar',NULL,1791892800000,1);
CREATE INDEX entries_person_day ON entries(person,day);
CREATE INDEX entries_person_kind ON entries(person,kind,id);
CREATE TABLE sources (
    child INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    parent INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    PRIMARY KEY(child,parent),
    CHECK(child != parent)
);
INSERT INTO sources(child,parent) VALUES(3,1);
CREATE INDEX sources_parent ON sources(parent);
CREATE TABLE replacements (
    newer INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    older INTEGER NOT NULL UNIQUE REFERENCES entries(rid) ON DELETE CASCADE
);
CREATE TABLE sessions (
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
CREATE TABLE jobs (
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
CREATE VIRTUAL TABLE search USING fts5(
    body, tokenize='unicode61 remove_diacritics 2'
);
INSERT INTO search(rowid,body) VALUES(1,'when is the bread done? in ten minutes. ');
INSERT INTO search(rowid,body) VALUES(2,'thank you. you''re welcome. ');
INSERT INTO search(rowid,body) VALUES(3,'  the bread takes ten minutes');
INSERT INTO search(rowid,body) VALUES(4,'  tea without sugar');
CREATE TABLE follow_ups (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    person TEXT NOT NULL,
    asked_unix_ms INTEGER NOT NULL
);
INSERT INTO sqlite_sequence(name,seq) VALUES('entries',4);
PRAGMA user_version = 1;
