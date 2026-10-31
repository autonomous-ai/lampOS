pub(crate) const SQL: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA secure_delete = ON;
PRAGMA journal_mode = DELETE;
PRAGMA synchronous = FULL;
CREATE TABLE IF NOT EXISTS meta (
    one INTEGER PRIMARY KEY CHECK(one=1), owner TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0, revision INTEGER NOT NULL DEFAULT 0,
    paused INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS episodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT, topic TEXT NOT NULL,
    started_ms INTEGER NOT NULL, ended_ms INTEGER, importance REAL NOT NULL,
    gist TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '',
    last_recalled_ms INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS records (
    id INTEGER PRIMARY KEY AUTOINCREMENT, episode_id INTEGER NOT NULL,
    at_ms INTEGER NOT NULL, speaker TEXT NOT NULL, body TEXT NOT NULL,
    processed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS episode_sources (
    episode_id INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    PRIMARY KEY(episode_id,record_id)
);
CREATE INDEX IF NOT EXISTS records_pending ON records(processed,speaker,id);
CREATE INDEX IF NOT EXISTS records_episode ON records(episode_id,id);
CREATE TABLE IF NOT EXISTS facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL,
    active INTEGER NOT NULL, expires_ms INTEGER, used_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS fact_sources (
    fact_id INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    PRIMARY KEY(fact_id,record_id)
);
CREATE INDEX IF NOT EXISTS fact_sources_record ON fact_sources(record_id);
CREATE TABLE IF NOT EXISTS lines (
    id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, body TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS line_sources (
    line_id INTEGER NOT NULL REFERENCES lines(id) ON DELETE CASCADE,
    fact_id INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    PRIMARY KEY(line_id,fact_id)
);
CREATE INDEX IF NOT EXISTS line_sources_fact ON line_sources(fact_id);
CREATE TABLE IF NOT EXISTS fingerprints (digest TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS settings (name TEXT PRIMARY KEY,value TEXT NOT NULL);
INSERT OR IGNORE INTO settings VALUES('time_zone','UTC');
CREATE TABLE IF NOT EXISTS weekly_progress (
    cycle INTEGER PRIMARY KEY, cursor INTEGER NOT NULL DEFAULT 0,
    last_id INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS model_rewrites (
    cycle INTEGER PRIMARY KEY, original_ids TEXT NOT NULL, changed_ids TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
    body, kind UNINDEXED, item_id UNINDEXED, tokenize='unicode61 remove_diacritics 2'
);
INSERT INTO search(search,rank) VALUES('secure-delete',1);
CREATE TRIGGER IF NOT EXISTS record_forget BEFORE DELETE ON records BEGIN
    DELETE FROM facts WHERE id IN (SELECT fact_id FROM fact_sources WHERE record_id=old.id);
    UPDATE episodes SET topic='',gist='',detail='',importance=0
      WHERE id=old.episode_id;
    DELETE FROM search WHERE (kind='record' AND item_id=old.id)
       OR (kind='episode' AND item_id=old.episode_id);
END;
CREATE TRIGGER IF NOT EXISTS fact_forget BEFORE DELETE ON facts BEGIN
    DELETE FROM lines WHERE id IN (SELECT line_id FROM line_sources WHERE fact_id=old.id);
    DELETE FROM search WHERE kind='fact' AND item_id=old.id;
END;
CREATE TRIGGER IF NOT EXISTS line_forget BEFORE DELETE ON lines BEGIN
    DELETE FROM search WHERE kind='line' AND item_id=old.id;
END;
PRAGMA user_version = 2;
"#;
