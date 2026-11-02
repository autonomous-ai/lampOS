//! What changed in the Days entries, for the tiered store's mirror (#4746,
//! `LAMP_MEMORY_TIERS`). Temporary triggers on the owning connection write
//! each added, removed or moved exchange and kept thing to a temporary
//! table, in the same transaction as the change: a change rolled back is
//! never told, and every removal (a forget of any kind, the forgotten-source
//! sweep, forget_everything, a person's removal, the days kept running out)
//! is told, since every one of them deletes the entry's row. The table is in
//! memory (`temp_store=MEMORY`) and taken after each request. Off unless
//! enabled; nothing is created on a connection that never enables it.
use rusqlite::Connection;

/// One change to an exchange or a kept thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A new entry, with what it holds as Days holds it.
    Added {
        person: String,
        id: u64,
        entry: Held,
    },
    /// The entry is gone from Days.
    Removed { person: String, id: u64 },
    /// The entry is now another scope's (`adopt`), under a new id.
    Moved {
        from: String,
        from_id: u64,
        to: String,
        to_id: u64,
        entry: Held,
    },
}

/// What an entry holds, as Days keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub kind: String,
    pub at_unix_ms: u64,
    pub heard: String,
    pub said: String,
    pub text: String,
    pub active: bool,
}

pub(crate) fn enable(db: &mut Connection) -> rusqlite::Result<()> {
    let enabled: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE type='table' AND name='memory_tiers_log')",
        [],
        |row| row.get(0),
    )?;
    if enabled {
        return Ok(());
    }
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TEMP TABLE memory_tiers_log (
            seq INTEGER PRIMARY KEY,
            op TEXT NOT NULL CHECK(op IN ('add','remove','move')),
            person TEXT NOT NULL, id INTEGER NOT NULL,
            to_person TEXT, to_id INTEGER,
            kind TEXT NOT NULL, at_unix_ms INTEGER NOT NULL,
            heard TEXT NOT NULL, said TEXT NOT NULL, text TEXT NOT NULL,
            active INTEGER NOT NULL
        );
        CREATE TEMP TRIGGER memory_tiers_insert AFTER INSERT ON main.entries
            WHEN NEW.kind IN ('exchange','kept') BEGIN
            INSERT INTO memory_tiers_log(op,person,id,kind,at_unix_ms,heard,said,text,active)
                VALUES('add',NEW.person,NEW.id,NEW.kind,NEW.at_unix_ms,NEW.heard,NEW.said,NEW.text,NEW.active);
        END;
        CREATE TEMP TRIGGER memory_tiers_delete AFTER DELETE ON main.entries
            WHEN OLD.kind IN ('exchange','kept') BEGIN
            INSERT INTO memory_tiers_log(op,person,id,kind,at_unix_ms,heard,said,text,active)
                VALUES('remove',OLD.person,OLD.id,OLD.kind,0,'','','',0);
        END;
        CREATE TEMP TRIGGER memory_tiers_move AFTER UPDATE OF person,id ON main.entries
            WHEN NEW.kind IN ('exchange','kept') AND (OLD.person!=NEW.person OR OLD.id!=NEW.id) BEGIN
            INSERT INTO memory_tiers_log(op,person,id,to_person,to_id,kind,at_unix_ms,heard,said,text,active)
                VALUES('move',OLD.person,OLD.id,NEW.person,NEW.id,NEW.kind,NEW.at_unix_ms,NEW.heard,NEW.said,NEW.text,NEW.active);
        END;",
    )?;
    tx.commit()
}

/// The triggers and the table dropped, with any change not taken. Nothing
/// to drop is not an error.
pub(crate) fn disable(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "DROP TRIGGER IF EXISTS temp.memory_tiers_insert;
        DROP TRIGGER IF EXISTS temp.memory_tiers_delete;
        DROP TRIGGER IF EXISTS temp.memory_tiers_move;
        DROP TABLE IF EXISTS temp.memory_tiers_log;",
    )
}

/// The changes since the last take, oldest first; the table is emptied.
pub(crate) fn take(db: &Connection) -> rusqlite::Result<Vec<Change>> {
    let mut statement = db.prepare_cached(
        "SELECT op,person,id,to_person,to_id,kind,at_unix_ms,heard,said,text,active FROM temp.memory_tiers_log ORDER BY seq",
    )?;
    let rows = statement.query_map([], |r| {
        let op: String = r.get(0)?;
        let entry = || -> rusqlite::Result<Held> {
            Ok(Held {
                kind: r.get(5)?,
                at_unix_ms: r.get(6)?,
                heard: r.get(7)?,
                said: r.get(8)?,
                text: r.get(9)?,
                active: r.get(10)?,
            })
        };
        Ok(match op.as_str() {
            "add" => Change::Added {
                person: r.get(1)?,
                id: r.get(2)?,
                entry: entry()?,
            },
            "move" => Change::Moved {
                from: r.get(1)?,
                from_id: r.get(2)?,
                to: r.get(3)?,
                to_id: r.get(4)?,
                entry: entry()?,
            },
            _ => Change::Removed {
                person: r.get(1)?,
                id: r.get(2)?,
            },
        })
    })?;
    let changes = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if !changes.is_empty() {
        db.execute("DELETE FROM temp.memory_tiers_log", [])?;
    }
    Ok(changes)
}

/// Every exchange and kept thing Days holds, by scope and id: what the
/// tiers may hold at start, and nothing else.
pub(crate) fn known(db: &Connection) -> rusqlite::Result<Vec<(String, u64)>> {
    let mut statement =
        db.prepare("SELECT person,id FROM main.entries WHERE kind IN ('exchange','kept')")?;
    let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}
