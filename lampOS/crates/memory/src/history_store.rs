//! What the voice's save to the account's history needs of the Days store
//! (#7771, the first builder's 17:09 ruling on #7957):
//!
//! - **The store's own id** ([`STORE_KEY`]), 8 lowercase hex digits, made
//!   when the database is made (or first opened by a build that has it) and
//!   kept in its `settings`, in the memory's own folder. A reset that
//!   removes the database makes a new one, so a reset lamp's turns get new
//!   ids (`lamp_msgs::history::turn_id`).
//! - **`store`**: that id, and the last id given in each scope
//!   (`next_id - 1`), never a word.
//! - **`exchanges_after`**: a scope's exchanges after an id, oldest first,
//!   at most [`MOST_EXCHANGES`] and about [`MOST_TEXT_BYTES`] of text, so a
//!   voice that restarted sends what the server never took. Memory stays the
//!   one copy of the words: the voice keeps only a mark per scope.
use crate::days::{Days, Error, Result};
use crate::store::Person;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::io::Read;

/// The `settings` key of the store's id.
pub const STORE_KEY: &str = "history_store_id";
/// The most exchanges one `exchanges_after` gives.
pub const MOST_EXCHANGES: u64 = 100;
/// About the most text one `exchanges_after` gives; one exchange always.
pub const MOST_TEXT_BYTES: usize = 48 * 1024;

/// Makes the store's id if it has none: 32 random bits, never 0.
pub(crate) fn ensure_store_id(db: &Connection) -> Result<()> {
    let have: Option<i64> = db
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [STORE_KEY],
            |r| r.get(0),
        )
        .optional()?;
    if have.is_some() {
        return Ok(());
    }
    let id = i64::from(random_u32()?.max(1));
    db.execute(
        "INSERT OR IGNORE INTO settings(key,value) VALUES(?1,?2)",
        params![STORE_KEY, id],
    )?;
    Ok(())
}

fn random_u32() -> Result<u32> {
    let mut bytes = [0u8; 4];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .map_err(|_| Error::new("storage", "no random source for the store's id"))?;
    Ok(u32::from_le_bytes(bytes))
}

impl Days {
    /// The store's own id, 8 lowercase hex digits.
    pub fn store_id(&self) -> Result<String> {
        let id: i64 = self.db.query_row(
            "SELECT value FROM settings WHERE key=?1",
            [STORE_KEY],
            |r| r.get(0),
        )?;
        Ok(format!("{:08x}", id as u32))
    }

    /// The last id given in each scope, `next_id - 1`.
    pub fn issued(&self) -> Result<Vec<(String, u64)>> {
        let mut statement = self
            .db
            .prepare("SELECT person,next_id FROM people ORDER BY person")?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(person, next)| (person, next.saturating_sub(1)))
            .collect())
    }

    /// `person`'s exchanges after `after`, oldest first: at most `limit`
    /// (and [`MOST_EXCHANGES`]), stopping once about [`MOST_TEXT_BYTES`] of
    /// text is given; and whether more follow.
    pub fn exchanges_after(
        &self,
        person: &Person,
        after: u64,
        limit: u64,
    ) -> Result<(Vec<Value>, bool)> {
        let limit = limit.clamp(1, MOST_EXCHANGES);
        let mut statement = self.db.prepare(
            "SELECT id,at_unix_ms,heard,said FROM entries WHERE person=?1 AND kind='exchange' AND id>?2 ORDER BY id LIMIT ?3",
        )?;
        let rows = statement
            .query_map(params![person.as_str(), after, limit + 1], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut given = Vec::new();
        let mut bytes = 0usize;
        let mut more = rows.len() as u64 > limit;
        for (id, at_unix_ms, heard, said) in rows.into_iter().take(limit as usize) {
            let size = heard.len() + said.len();
            if !given.is_empty() && bytes + size > MOST_TEXT_BYTES {
                more = true;
                break;
            }
            bytes += size;
            given.push(json!({"id":id,"at_unix_ms":at_unix_ms,"heard":heard,"said":said}));
        }
        Ok((given, more))
    }
}

#[cfg(test)]
mod tests {
    use crate::days::Days;
    use crate::days_protocol::answer;
    use crate::store::LocalTime;
    use crate::store::Options as DaysOptions;
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};

    const NOON: u64 = 1_760_356_800_000;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-memory-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn open(dir: &Path) -> Days {
        Days::open(
            DaysOptions {
                dir: dir.to_path_buf(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            NOON,
        )
        .unwrap()
    }

    fn ask(store: &mut Days, line: Value) -> Value {
        let answer = answer(store, line.to_string().as_bytes(), NOON);
        serde_json::from_str(answer.line.trim()).unwrap()
    }

    /// The store's id is made with the store, kept across a reopen, and
    /// new after a reset (the database gone).
    #[test]
    fn the_store_id_lives_and_dies_with_the_store() {
        let dir = dir("id");
        let first = {
            let mut store = open(&dir);
            ask(&mut store, json!({"op":"store"}))["store"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert!(lamp_msgs::history::is_store_id(&first), "{first}");
        let again = {
            let mut store = open(&dir);
            ask(&mut store, json!({"op":"store"}))["store"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(first, again);
        std::fs::remove_dir_all(&dir).unwrap();
        let mut store = open(&dir);
        let reset = ask(&mut store, json!({"op":"store"}))["store"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(first, reset);
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `store` tells each scope's last id; `exchanges_after` a scope's
    /// exchanges after an id, oldest first, paged, never a kept thing.
    #[test]
    fn exchanges_after_an_id_are_given_oldest_first() {
        let dir = dir("after");
        let mut store = open(&dir);
        for n in 1..=5u64 {
            let line = json!({"op":"append","person":"p-a","at_unix_ms":NOON + n,"heard":format!("h{n}"),"said":format!("s{n}")});
            assert_eq!(ask(&mut store, line)["id"], n);
        }
        ask(
            &mut store,
            json!({"op":"remember","person":"p-a","at_unix_ms":NOON,"text":"tea"}),
        );
        ask(
            &mut store,
            json!({"op":"append","at_unix_ms":NOON,"heard":"room","said":"hi"}),
        );
        let told = ask(&mut store, json!({"op":"store"}));
        assert_eq!(told["issued"]["p-a"], 6, "{told}");
        assert_eq!(told["issued"]["everyone"], 1, "{told}");
        let page = ask(
            &mut store,
            json!({"op":"exchanges_after","person":"p-a","after":2,"limit":2}),
        );
        assert_eq!(page["more"], true, "{page}");
        let ids: Vec<u64> = page["exchanges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, [3, 4]);
        assert_eq!(page["exchanges"][0]["heard"], "h3");
        assert_eq!(page["exchanges"][0]["said"], "s3");
        assert_eq!(page["exchanges"][0]["at_unix_ms"], NOON + 3);
        let rest = ask(
            &mut store,
            json!({"op":"exchanges_after","person":"p-a","after":4,"limit":20}),
        );
        assert_eq!(rest["more"], false, "{rest}");
        assert_eq!(rest["exchanges"].as_array().unwrap().len(), 1, "{rest}");
        let room = ask(&mut store, json!({"op":"exchanges_after","after":0}));
        assert_eq!(room["exchanges"][0]["heard"], "room", "{room}");
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
