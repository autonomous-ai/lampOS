//! What a forget removed, kept on the memory's disk until the voice has it
//! on its own queue for the account's history (#7771, the first builder's
//! switch-on condition on #8086: "a forget's ids reach the server across a
//! restart"). Only for a forget the voice asked with
//! [`lamp_msgs::MEMORY_HISTORY_UNSENT_FIELD`] (its `LAMP_HISTORY_FORGET=on`);
//! without it nothing is kept, as before, but for the app's removal of a
//! person (`forget_person`, which lamp-cloud never asks): their whole
//! history is always kept, one row, read by the voice on `memory/forgot`.
//!
//! The rows are written in the forget's own transaction (no commit of
//! their own, so no sync more before the reply), each the turn id the
//! answer tells ([`lamp_msgs::history_turn_id`]) or, for a known person's
//! forget everything, one row for their whole history. The voice's worker
//! says them done ([`lamp_msgs::MEMORY_HISTORY_UNSENT_DONE_OP`]) once they
//! are on its queue file; what it never said done (a forget waiting in its
//! hand-off at a crash or a stop, a rest let go or lost to the memory's
//! restart) it reads again at its next start
//! ([`lamp_msgs::MEMORY_HISTORY_UNSENT_OP`]). Read twice is harmless: the
//! queue is a set, and the server takes a delete of a turn it no longer has
//! as done.
//!
//! Never let go (the first builder's privacy read on #8125): a row goes
//! only when the voice says it done, so there is no bound but what the
//! lamp forgot while the voice could not queue it. Ids only, never a word.
use crate::days::{Days, Result};
use rusqlite::{Transaction, params};
use serde_json::{Value, json};

/// The most rows one `history_unsent` page gives: under 64 KiB at the
/// longest turn id, as a `forgot_exchanges` page.
pub const MOST_UNSENT_PAGE: u64 = 900;
/// The room's scope; a whole-history row is only ever a known person's.
const ROOM: &str = "everyone";

/// In a forget's transaction `tx`, when `asked`: each of `gone` (scope and
/// id, under `epoch`) kept, but `whole`'s own, which is kept as one row for
/// their whole history. Only the room's and known people's scopes, as the
/// voice sends. The rows' range, `None` for none.
pub(crate) fn keep(
    tx: &Transaction<'_>,
    asked: bool,
    epoch: &str,
    gone: &[(String, u64)],
    whole: Option<&str>,
) -> Result<Option<(u64, u64)>> {
    if !asked {
        return Ok(None);
    }
    let mut range: Option<(u64, u64)> = None;
    let mut took = |seq: i64| {
        let seq = seq as u64;
        range = Some(range.map_or((seq, seq), |(from, _)| (from, seq)));
    };
    // The room's forget everything is told by its ids, as any other.
    let whole = whole.filter(|person| lamp_msgs::known::is_person_id(person));
    if let Some(person) = whole {
        // Their whole history takes every row of theirs: one row, however
        // often they are forgotten before the voice takes it.
        tx.execute("DELETE FROM history_unsent WHERE scope=?1", [person])?;
        tx.execute(
            "INSERT INTO history_unsent(scope,turn) VALUES (?1,NULL)",
            [person],
        )?;
        took(tx.last_insert_rowid());
    }
    for (scope, id) in gone {
        if Some(scope.as_str()) == whole
            || !(scope == ROOM || lamp_msgs::known::is_person_id(scope))
        {
            continue;
        }
        tx.execute(
            "INSERT INTO history_unsent(scope,turn) VALUES (?1,?2)",
            params![scope, lamp_msgs::history_turn_id(epoch, scope, *id)],
        )?;
        took(tx.last_insert_rowid());
    }
    Ok(range)
}

/// Two ranges as one: the first's start, the last's end.
pub(crate) fn joined(kept: Option<(u64, u64)>, more: Option<(u64, u64)>) -> Option<(u64, u64)> {
    match (kept, more) {
        (Some((from, _)), Some((_, to))) => Some((from, to)),
        (kept, more) => kept.or(more),
    }
}

impl Days {
    /// The kept rows after `after`, oldest first, at most
    /// [`MOST_UNSENT_PAGE`], as `history_unsent` answers them.
    pub fn history_unsent(&self, after: u64) -> Result<Value> {
        let mut statement = self.db.prepare(
            "SELECT seq,scope,turn FROM history_unsent WHERE seq>?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![after, MOST_UNSENT_PAGE + 1], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = rows.len() as u64 > MOST_UNSENT_PAGE;
        let mut exchanges: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        let mut all = Vec::new();
        let mut to = None;
        for (seq, scope, turn) in rows.into_iter().take(MOST_UNSENT_PAGE as usize) {
            to = Some(seq);
            match turn {
                Some(turn) => exchanges.entry(scope).or_default().push(turn),
                None => all.push(scope),
            }
        }
        Ok(json!({"ok":true,"exchanges":exchanges,"all":all,"to":to,"more":more}))
    }

    /// The kept rows `from` to `to`, both included, let go: the voice has
    /// them on its queue. How many went.
    pub fn history_unsent_done(&mut self, from: u64, to: u64) -> Result<u64> {
        Ok(self.db.execute(
            "DELETE FROM history_unsent WHERE seq BETWEEN ?1 AND ?2",
            params![from, to],
        )? as u64)
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
    const MINH: &str = "p-0123456789abcdef";
    const LAN: &str = "p-fedcba9876543210";

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-memory-unsent-{name}-{}", std::process::id()));
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

    fn said(store: &mut Days, person: Option<&str>, n: u64) -> u64 {
        let mut line = json!({"op":"append","at_unix_ms":NOON + n,"heard":format!("h{n}"),"said":format!("s{n}")});
        if let Some(person) = person {
            line["person"] = json!(person);
        }
        ask(store, line)["id"].as_u64().unwrap()
    }

    fn turn(store: &Days, scope: &str, id: u64) -> String {
        lamp_msgs::history_turn_id(store.history_epoch(), scope, id)
    }

    /// A forget asked with the field keeps what it removed on disk, in its
    /// own transaction, across a restart of the memory, until the voice
    /// says it done; then it is never given again. Without the field,
    /// nothing is kept.
    #[test]
    fn a_forget_asked_keeps_its_ids_across_a_restart_until_done() {
        let dir = dir("restart");
        let mut store = open(&dir);
        let (one, two, three) = (
            said(&mut store, Some(MINH), 1),
            said(&mut store, Some(MINH), 2),
            said(&mut store, Some(MINH), 3),
        );
        let plain = ask(&mut store, json!({"op":"forget","person":MINH,"id":one}));
        assert_eq!(plain["ok"], true, "{plain}");
        assert!(plain.get("unsent").is_none(), "{plain}");
        let asked = ask(
            &mut store,
            json!({"op":"forget","person":MINH,"id":two,"history_unsent":true}),
        );
        assert_eq!(asked["ok"], true, "{asked}");
        let range = asked["unsent"].clone();
        assert_eq!(range.as_array().map(Vec::len), Some(2), "{asked}");
        let epoch = store.history_epoch().to_owned();
        drop(store);
        // The memory restarted: the row is still there, the plain forget's
        // never was.
        let mut store = open(&dir);
        let page = ask(&mut store, json!({"op":"history_unsent","after":0}));
        assert_eq!(
            page["exchanges"],
            json!({MINH:[lamp_msgs::history_turn_id(&epoch, MINH, two)]}),
            "{page}"
        );
        assert_eq!(page["all"], json!([]));
        assert_eq!(page["more"], false);
        assert_eq!(page["to"], range[1]);
        let done = ask(
            &mut store,
            json!({"op":"history_unsent_done","from":range[0],"to":range[1]}),
        );
        assert_eq!(done["removed"], 1, "{done}");
        let empty = ask(&mut store, json!({"op":"history_unsent","after":0}));
        assert_eq!(empty["exchanges"], json!({}), "{empty}");
        assert_eq!(empty["to"], Value::Null);
        // The field is the voice's: an unknown one is still refused.
        let refused = ask(
            &mut store,
            json!({"op":"forget","person":MINH,"id":three,"unsent_please":true}),
        );
        assert_eq!(refused["ok"], false, "{refused}");
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A known person's forget everything is kept as one row for their
    /// whole history, the room's lines it took by id; done by its range,
    /// a later forget's rows stay.
    #[test]
    fn forget_everything_is_kept_whole_and_done_by_its_own_range() {
        let dir = dir("whole");
        let mut store = open(&dir);
        said(&mut store, Some(MINH), 1);
        said(&mut store, Some(MINH), 2);
        let lan = said(&mut store, Some(LAN), 3);
        let all = ask(
            &mut store,
            json!({"op":"forget_all","person":MINH,"history_unsent":true}),
        );
        assert_eq!(all["ok"], true, "{all}");
        let later = ask(
            &mut store,
            json!({"op":"forget","person":LAN,"id":lan,"history_unsent":true}),
        );
        let page = ask(&mut store, json!({"op":"history_unsent"}));
        assert_eq!(page["all"], json!([MINH]), "{page}");
        assert_eq!(page["exchanges"], json!({LAN:[turn(&store, LAN, lan)]}));
        // Oldest first: the forget everything's row comes before the later
        // forget's.
        assert!(all["unsent"][1].as_u64() < later["unsent"][0].as_u64());
        ask(
            &mut store,
            json!({"op":"history_unsent_done","from":all["unsent"][0],"to":all["unsent"][1]}),
        );
        let page = ask(&mut store, json!({"op":"history_unsent"}));
        assert_eq!(page["all"], json!([]), "{page}");
        assert_eq!(page["exchanges"], json!({LAN:[turn(&store, LAN, lan)]}));
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The Reviewer-lead's HOLD on #8125: the app's removal of a person
    /// (`forget_person`, from lamp-cloud on the forget socket, which never
    /// asks) keeps their whole history as one row, in the removal's own
    /// transaction, across a restart of the memory; their rules' scope has
    /// no saved turn, so nothing more. Removed again before the voice took
    /// it, still one row.
    #[test]
    fn the_apps_removal_of_a_person_keeps_their_whole_history_across_a_restart() {
        let dir = dir("removal");
        let mut store = open(&dir);
        said(&mut store, Some(MINH), 1);
        said(&mut store, Some(MINH), 2);
        said(&mut store, Some(LAN), 3);
        for _ in 0..2 {
            let removed = ask(&mut store, json!({"op":"forget_person","person":MINH}));
            assert_eq!(removed["ok"], true, "{removed}");
        }
        drop(store);
        let mut store = open(&dir);
        let page = ask(&mut store, json!({"op":"history_unsent","after":0}));
        assert_eq!(page["all"], json!([MINH]), "{page}");
        assert_eq!(page["exchanges"], json!({}), "{page}");
        let to = page["to"].clone();
        ask(
            &mut store,
            json!({"op":"history_unsent_done","from":0,"to":to}),
        );
        let empty = ask(&mut store, json!({"op":"history_unsent","after":0}));
        assert_eq!(empty["all"], json!([]), "{empty}");
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Pages are oldest first, [`super::MOST_UNSENT_PAGE`] at most, each
    /// ending at its `to`, the next read after it; none is ever let go.
    #[test]
    fn pages_are_oldest_first_and_none_let_go() {
        let dir = dir("pages");
        let mut store = open(&dir);
        let epoch = store.history_epoch().to_owned();
        let tx = store.db.transaction().unwrap();
        let gone: Vec<(String, u64)> = (1..=2 * super::MOST_UNSENT_PAGE + 5)
            .map(|id| (MINH.to_owned(), id))
            .collect();
        let range = super::keep(&tx, true, &epoch, &gone, None)
            .unwrap()
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(range.1 - range.0 + 1, 2 * super::MOST_UNSENT_PAGE + 5);
        let mut after = 0;
        let mut ids = Vec::new();
        for _ in 0..64 {
            let page = store.history_unsent(after).unwrap();
            let turns = page["exchanges"][MINH]
                .as_array()
                .cloned()
                .unwrap_or_default();
            assert!(turns.len() as u64 <= super::MOST_UNSENT_PAGE);
            ids.extend(turns.into_iter().map(|t| {
                let t = t.as_str().unwrap().to_owned();
                t.rsplit_once(':').unwrap().1.parse::<u64>().unwrap()
            }));
            if page["more"] != true {
                break;
            }
            after = page["to"].as_u64().unwrap();
        }
        // Every one, in order.
        assert_eq!(
            ids,
            (1..=2 * super::MOST_UNSENT_PAGE + 5).collect::<Vec<_>>()
        );
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
