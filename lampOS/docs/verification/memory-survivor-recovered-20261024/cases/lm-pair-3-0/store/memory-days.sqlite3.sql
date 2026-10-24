BEGIN TRANSACTION;
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
INSERT INTO "entries" VALUES(5,'p-2222222222222222',1,'kept',1792829869151,'2026-10-24','','','pair other scope',NULL,1792829869406,1);
INSERT INTO "entries" VALUES(6,'everyone',1,'kept',1792829869151,'2026-10-24','','','pair room scope',NULL,1792829869409,1);
INSERT INTO "entries" VALUES(9,'p-1111111111111111',6,'kept',1792829869151,'2026-10-24','','','pair final fresh fact',NULL,1792829870022,1);
CREATE TABLE event_days (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    day TEXT NOT NULL
);
CREATE TABLE follow_up_offers (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    person TEXT NOT NULL,
    offers INTEGER NOT NULL,
    offered_unix_ms INTEGER NOT NULL
);
CREATE TABLE follow_ups (
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    person TEXT NOT NULL,
    asked_unix_ms INTEGER NOT NULL
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
CREATE TABLE people (
    person TEXT PRIMARY KEY,
    next_id INTEGER NOT NULL DEFAULT 1,
    generation INTEGER NOT NULL DEFAULT 0,
    skipped INTEGER NOT NULL DEFAULT 0
);
INSERT INTO "people" VALUES('p-1111111111111111',7,2,0);
INSERT INTO "people" VALUES('p-2222222222222222',2,0,0);
INSERT INTO "people" VALUES('everyone',2,0,0);
INSERT INTO "people" VALUES('r-1111111111111111',2,1,0);
CREATE TABLE replacements (
    newer INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,
    older INTEGER NOT NULL UNIQUE REFERENCES entries(rid) ON DELETE CASCADE
);
PRAGMA writable_schema=ON;
INSERT INTO sqlite_master(type,name,tbl_name,rootpage,sql)VALUES('table','search','search',0,'CREATE VIRTUAL TABLE search USING fts5(
    body, tokenize=''unicode61 remove_diacritics 2''
)');
INSERT INTO "search" VALUES('  pair other scope');
INSERT INTO "search" VALUES('  pair room scope');
INSERT INTO "search" VALUES('  pair final fresh fact');
CREATE TABLE 'search_config'(k PRIMARY KEY, v) WITHOUT ROWID;
INSERT INTO "search_config" VALUES('secure-delete',1);
INSERT INTO "search_config" VALUES('version',5);
CREATE TABLE 'search_content'(id INTEGER PRIMARY KEY, c0);
INSERT INTO "search_content" VALUES(5,'  pair other scope');
INSERT INTO "search_content" VALUES(6,'  pair room scope');
INSERT INTO "search_content" VALUES(9,'  pair final fresh fact');
CREATE TABLE 'search_data'(id INTEGER PRIMARY KEY, block BLOB);
INSERT INTO "search_data" VALUES(1,X'030A');
INSERT INTO "search_data" VALUES(10,X'000000040109090009010101020101030101040101050101060101070101080101090101');
INSERT INTO "search_data" VALUES(137438953473,X'00000004');
INSERT INTO "search_data" VALUES(274877906945,X'00000004');
INSERT INTO "search_data" VALUES(412316860417,X'00000004');
INSERT INTO "search_data" VALUES(549755813889,X'00000004');
INSERT INTO "search_data" VALUES(687194767361,X'0000002106306F74686572050203010470616972050202010573636F7065050204040A09');
INSERT INTO "search_data" VALUES(824633720833,X'000000200530706169720602020104726F6F6D060203010573636F7065060204040909');
INSERT INTO "search_data" VALUES(962072674305,X'00000004');
INSERT INTO "search_data" VALUES(1099511627777,X'00000004');
INSERT INTO "search_data" VALUES(1236950581249,X'000000280530666163740902050204696E616C09020302047265736809020401047061697209020204090909');
CREATE TABLE 'search_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO "search_docsize" VALUES(5,X'03');
INSERT INTO "search_docsize" VALUES(6,X'03');
INSERT INTO "search_docsize" VALUES(9,X'04');
CREATE TABLE 'search_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO "search_idx" VALUES(1,X'',2);
INSERT INTO "search_idx" VALUES(2,X'',2);
INSERT INTO "search_idx" VALUES(3,X'',2);
INSERT INTO "search_idx" VALUES(4,X'',2);
INSERT INTO "search_idx" VALUES(5,X'',2);
INSERT INTO "search_idx" VALUES(6,X'',2);
INSERT INTO "search_idx" VALUES(7,X'',2);
INSERT INTO "search_idx" VALUES(8,X'',2);
INSERT INTO "search_idx" VALUES(9,X'',2);
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
INSERT INTO "sessions" VALUES('p-1111111111111111','pair-session',1792829869452,0,1200,'','[]',1);
INSERT INTO "sessions" VALUES('p-2222222222222222','pair-session',1792829869462,0,1200,'{"facts":[{"id":1,"kind":"preference","text":"pair other scope"}],"scope":"caller-supplied person; identity not verified","untrusted":true}','[1]',0);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
INSERT INTO "settings" VALUES('import_complete',1);
CREATE TABLE sources (
    child INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    parent INTEGER NOT NULL REFERENCES entries(rid) ON DELETE CASCADE,
    PRIMARY KEY(child,parent),
    CHECK(child != parent)
);
CREATE INDEX entries_person_day ON entries(person,day);
CREATE INDEX entries_person_kind ON entries(person,kind,id);
CREATE INDEX sources_parent ON sources(parent);
PRAGMA writable_schema=OFF;
DELETE FROM "sqlite_sequence";
INSERT INTO "sqlite_sequence" VALUES('entries',9);
COMMIT;
