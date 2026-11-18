//! The food diary (#7687, slice 1; the voice's `LAMP_FOOD_DIARY`, off by
//! default): what a known person told the lamp they ate or drank, in their
//! own words, so "what did I eat today?" has an answer, and nothing more.
//!
//! - **A known person's alone** (`p-` ids). Never the room's, a rules
//!   scope's or a guest's: a guest's meal is never kept (the first builder's
//!   ruling on #7687, question 9).
//! - **The least kept** (the design's "Stored, the minimum"): when, which
//!   meal (`breakfast`, `lunch`, `dinner`, `snack` or `drink`), the person's
//!   own words (one line, at most [`WORDS_BYTES`]) and how it came in
//!   (`told`, or `seen_asked` for the seen ask of a later slice). No number,
//!   no weight and never a picture.
//! - **A table of its own**, `diary`, so the taste rules, the nightly pass,
//!   the tidy and the day's expiry, which read `entries`, never reach it,
//!   and their "never health or diet unless asked" stays as it is for
//!   everything else.
//! - **28 days** ([`KEEP_MS`], the first builder's ruling, question 2): an
//!   older row goes at every request's upkeep (`Days::maintain`), so none
//!   is ever answered past it, and a diary is never kept beyond a month.
//! - **Forgets.** Forget everything and forgetting the person take every row
//!   and the care pause in the same transaction ([`empty`]); `diary_forget`
//!   takes the whole diary, or with `last` the newest row only. A row told
//!   before the person's last forget everything is refused, as a service
//!   record's is (`service_records_forgot`).
//! - **The care pause** (`care_pause`, the design's section 4): one flag a
//!   person, set and cleared by the voice, kept here so every forget clears
//!   it with the rows. A diary-wide forget clears it; forgetting the last
//!   row does not.
//!
//! Nothing here calls a model, writes a word to the journal (the request
//! line logs its op and person only) or tells anything on the bus.
use crate::{
    days::{DAY_MS, Days, Error, Result, ensure_person},
    store::{self, Day, Person},
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Value, json};

/// How long a row is kept: 28 days.
pub const KEEP_DAYS: u32 = 28;
pub const KEEP_MS: u64 = KEEP_DAYS as u64 * DAY_MS;
/// The most bytes of the person's words, cut at a whole character.
pub const WORDS_BYTES: usize = 200;
/// The most rows one person keeps: 28 days of a busy eater's meals and
/// drinks. Past it the oldest goes.
pub const MOST_PER_PERSON: usize = 600;
/// The meals a row may be.
pub const MEALS: [&str; 5] = ["breakfast", "lunch", "dinner", "snack", "drink"];
/// How a row came in.
pub const HOWS: [&str; 2] = ["told", "seen_asked"];

fn owner_only(person: &Person) -> Result<()> {
    if lamp_msgs::known::is_person_id(person.as_str()) {
        Ok(())
    } else {
        Err(Error::new(
            "bad_person",
            "only a known person has a food diary",
        ))
    }
}

impl Days {
    /// Keeps one meal of `person`'s, told at `at_unix_ms`: its id.
    pub fn diary_add(
        &mut self,
        person: &Person,
        at_unix_ms: u64,
        meal: &str,
        words: &str,
        how: &str,
    ) -> Result<u64> {
        owner_only(person)?;
        if !MEALS.contains(&meal) {
            return Err(Error::new(
                "bad_meal",
                "a meal is breakfast, lunch, dinner, snack or drink",
            ));
        }
        if !HOWS.contains(&how) {
            return Err(Error::new("bad_request", "how is told or seen_asked"));
        }
        let words = store::tidy(words, WORDS_BYTES);
        let words = words.trim();
        if words.is_empty() {
            return Err(Error::new("empty", "a meal needs the person's words"));
        }
        if at_unix_ms.saturating_add(KEEP_MS) < self.now {
            return Err(Error::new(
                "too_old",
                "a meal older than the diary keeps is not kept",
            ));
        }
        let tx = self.db.transaction()?;
        // Told before a forget everything of this person (or of everyone):
        // never kept, so one retried after the forget never comes back.
        let mark: Option<i64> = tx.query_row(
            "SELECT max(at_unix_ms) FROM service_records_forgot WHERE person IN (?1,'')",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if mark.is_some_and(|mark| u64::try_from(mark).unwrap_or(0) > at_unix_ms) {
            return Err(Error::new(
                "forgotten",
                "the meal was told before a forget that takes it",
            ));
        }
        ensure_person(&tx, person)?;
        tx.execute(
            "INSERT INTO diary(person,at_unix_ms,meal,words,how) VALUES(?1,?2,?3,?4,?5)",
            params![person.as_str(), at_unix_ms as i64, meal, words, how],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "DELETE FROM diary WHERE person=?1 AND rid IN (SELECT rid FROM diary WHERE person=?1 ORDER BY at_unix_ms DESC, rid DESC LIMIT -1 OFFSET ?2)",
            params![person.as_str(), MOST_PER_PERSON as i64],
        )?;
        tx.commit()?;
        Ok(u64::try_from(id).unwrap_or(0))
    }

    /// `person`'s meals of the last `days` local days, today the first
    /// (1 is today, 7 this week; at most [`KEEP_DAYS`]), oldest first,
    /// and whether the care pause is on.
    pub fn diary_list(&self, person: &Person, days: u32) -> Result<(Vec<Value>, bool)> {
        owner_only(person)?;
        if !(1..=KEEP_DAYS).contains(&days) {
            return Err(Error::new("bad_request", "days is 1 to 28"));
        }
        let from = Day::at(self.now, &self.local).and_then(|today| today.before(days - 1));
        let mut statement = self.db.prepare(
            "SELECT rid,at_unix_ms,meal,words,how FROM diary WHERE person=?1 AND at_unix_ms>=?2 ORDER BY at_unix_ms, rid",
        )?;
        let since = self.now.saturating_sub(KEEP_MS) as i64;
        let rows = statement
            .query_map(params![person.as_str(), since], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let meals = rows
            .into_iter()
            .filter_map(|(id, at, meal, words, how)| {
                let at = u64::try_from(at).ok()?;
                let day = Day::at(at, &self.local)?;
                (from.is_some_and(|from| day >= from)).then(|| {
                    json!({
                        "id": id,
                        "at_unix_ms": at,
                        "day": day.to_string(),
                        "meal": meal,
                        "words": words,
                        "how": how,
                    })
                })
            })
            .collect();
        Ok((meals, self.diary_care_paused(person)?))
    }

    /// `person`'s whole diary removed, the care pause with it, or with
    /// `last` the newest row only: how many rows went.
    pub fn diary_forget(&mut self, person: &Person, last: bool) -> Result<u64> {
        owner_only(person)?;
        let tx = self.db.transaction()?;
        let removed = if last {
            let newest: Option<i64> = tx
                .query_row(
                    "SELECT rid FROM diary WHERE person=?1 ORDER BY at_unix_ms DESC, rid DESC LIMIT 1",
                    [person.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            match newest {
                Some(rid) => tx.execute("DELETE FROM diary WHERE rid=?1", [rid])? as u64,
                None => 0,
            }
        } else {
            empty(&tx, Some(person))?
        };
        tx.commit()?;
        Ok(removed)
    }

    /// Sets `person`'s care pause, or clears it: whether it is on now.
    pub fn diary_care(&mut self, person: &Person, pause: bool) -> Result<bool> {
        owner_only(person)?;
        let tx = self.db.transaction()?;
        if pause {
            ensure_person(&tx, person)?;
            tx.execute(
                "INSERT INTO diary_care(person,at_unix_ms) VALUES(?1,?2) ON CONFLICT(person) DO NOTHING",
                params![person.as_str(), self.now as i64],
            )?;
        } else {
            tx.execute("DELETE FROM diary_care WHERE person=?1", [person.as_str()])?;
        }
        tx.commit()?;
        Ok(pause)
    }

    fn diary_care_paused(&self, person: &Person) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM diary_care WHERE person=?1",
                [person.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}

/// The rows older than [`KEEP_MS`] at `now` removed (`Days::maintain`).
pub(crate) fn expire(tx: &Transaction<'_>, now: u64) -> Result<u64> {
    Ok(tx.execute(
        "DELETE FROM diary WHERE at_unix_ms<?1",
        [now.saturating_sub(KEEP_MS) as i64],
    )? as u64)
}

/// Forget everything (`forget_all`, `forget_person`) and a diary-wide
/// `diary_forget`: `person`'s rows and care pause, or everyone's with
/// `None`. How many rows went.
pub(crate) fn empty(tx: &Transaction<'_>, person: Option<&Person>) -> Result<u64> {
    let who = person.map(Person::as_str);
    tx.execute(
        "DELETE FROM diary_care WHERE ?1 IS NULL OR person=?1",
        [who],
    )?;
    Ok(tx.execute("DELETE FROM diary WHERE ?1 IS NULL OR person=?1", [who])? as u64)
}

#[cfg(test)]
mod tests {
    use super::{KEEP_MS, MOST_PER_PERSON};
    use crate::{
        days::{DAY_MS, Days},
        days_protocol::answer,
        store::{LocalTime, Options},
    };
    use serde_json::{Value, json};
    use std::{fs, path::PathBuf};

    /// 2026-10-13 12:00 UTC.
    const NOW: u64 = 1_791_892_800_000;
    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> (Self, Days) {
            let dir =
                std::env::temp_dir().join(format!("lamp-diary-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let scratch = Self(dir);
            let store = scratch.open();
            (scratch, store)
        }

        fn open(&self) -> Days {
            Days::open(
                Options {
                    dir: self.0.clone(),
                    keep_days: 30,
                    local: LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn send_at(store: &mut Days, request: Value, now: u64) -> Value {
        serde_json::from_str(&answer(store, request.to_string().as_bytes(), now).line).unwrap()
    }

    fn send(store: &mut Days, request: Value) -> Value {
        send_at(store, request, NOW)
    }

    fn add_at(store: &mut Days, person: &str, at: u64, meal: &str, words: &str) -> Value {
        send_at(
            store,
            json!({"op":"diary_add","person":person,"at_unix_ms":at,"meal":meal,"words":words}),
            NOW.max(at),
        )
    }

    fn add(store: &mut Days, person: &str, meal: &str, words: &str) -> Value {
        add_at(store, person, NOW, meal, words)
    }

    fn list_at(store: &mut Days, person: &str, days: u32, now: u64) -> Value {
        let listed = send_at(
            store,
            json!({"op":"diary_list","person":person,"days":days}),
            now,
        );
        assert_eq!(listed["ok"], true, "{listed}");
        listed
    }

    fn words(listed: &Value) -> Vec<String> {
        listed["meals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|meal| meal["words"].as_str().unwrap().to_owned())
            .collect()
    }

    /// A told meal is kept with its words, meal and how, read back today
    /// and this week, kept across a restart, and only its person's.
    #[test]
    fn a_meal_is_kept_listed_and_kept_across_a_restart() {
        let (scratch, mut store) = Scratch::new("round-trip");
        let first = add_at(&mut store, OWNER, NOW - 2 * DAY_MS, "breakfast", "porridge");
        assert_eq!(first["ok"], true, "{first}");
        let lunch = add(&mut store, OWNER, "lunch", "  a small burger\nand fries ");
        assert_eq!(lunch["ok"], true, "{lunch}");
        assert!(lunch["id"].as_u64().unwrap() > first["id"].as_u64().unwrap());
        add(&mut store, OTHER, "dinner", "pho");
        drop(store);
        let mut store = scratch.open();
        let today = list_at(&mut store, OWNER, 1, NOW);
        assert_eq!(words(&today), ["a small burger and fries"]);
        let meal = &today["meals"][0];
        assert_eq!(meal["meal"], "lunch");
        assert_eq!(meal["how"], "told");
        assert_eq!(meal["day"], "2026-10-13");
        assert_eq!(meal["at_unix_ms"], NOW);
        assert_eq!(today["care_pause"], false);
        let week = list_at(&mut store, OWNER, 7, NOW);
        assert_eq!(words(&week), ["porridge", "a small burger and fries"]);
        assert_eq!(words(&list_at(&mut store, OTHER, 7, NOW)), ["pho"]);
    }

    /// Only a known person's, a meal it knows, the person's words and a
    /// span the diary keeps; nothing kept for a refusal.
    #[test]
    fn a_meal_is_a_known_persons_in_their_words() {
        let (_scratch, mut store) = Scratch::new("shapes");
        let room = send(
            &mut store,
            json!({"op":"diary_add","at_unix_ms":NOW,"meal":"lunch","words":"rice"}),
        );
        assert_eq!(room["error"], "bad_person", "{room}");
        assert_eq!(
            add(&mut store, "r-0123456789abcdef", "lunch", "rice")["error"],
            "bad_person"
        );
        assert_eq!(add(&mut store, OWNER, "feast", "rice")["error"], "bad_meal");
        assert_eq!(add(&mut store, OWNER, "lunch", " \n ")["error"], "empty");
        let how = send(
            &mut store,
            json!({"op":"diary_add","person":OWNER,"at_unix_ms":NOW,"meal":"lunch","words":"rice","how":"guessed"}),
        );
        assert_eq!(how["error"], "bad_request", "{how}");
        let calories = send(
            &mut store,
            json!({"op":"diary_add","person":OWNER,"at_unix_ms":NOW,"meal":"lunch","words":"rice","calories":700}),
        );
        assert_eq!(calories["ok"], false, "no number is kept: {calories}");
        let old = send(
            &mut store,
            json!({"op":"diary_add","person":OWNER,"at_unix_ms":NOW - KEEP_MS - 1,"meal":"lunch","words":"rice"}),
        );
        assert_eq!(old["error"], "too_old", "{old}");
        let long = "rice ".repeat(100);
        assert_eq!(add(&mut store, OWNER, "lunch", &long)["ok"], true);
        let kept = list_at(&mut store, OWNER, 1, NOW);
        assert!(kept["meals"][0]["words"].as_str().unwrap().len() <= 200);
        for days in [0, 29] {
            let refused = send(
                &mut store,
                json!({"op":"diary_list","person":OWNER,"days":days}),
            );
            assert_eq!(refused["error"], "bad_request", "{days}: {refused}");
        }
    }

    /// Rows older than 28 days go at the next request's upkeep, after a
    /// restart too, and a week's list never reaches past it.
    #[test]
    fn a_meal_older_than_28_days_expires() {
        let (scratch, mut store) = Scratch::new("expiry");
        add(&mut store, OWNER, "lunch", "noodles");
        add_at(&mut store, OWNER, NOW + 20 * DAY_MS, "dinner", "soup");
        let day27 = NOW + 27 * DAY_MS;
        assert_eq!(
            words(&list_at(&mut store, OWNER, 28, day27)),
            ["noodles", "soup"]
        );
        let day29 = NOW + KEEP_MS + 1;
        assert_eq!(words(&list_at(&mut store, OWNER, 28, day29)), ["soup"]);
        let count: i64 = store
            .db
            .query_row("SELECT count(*) FROM diary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "the old row is gone from the disk");
        drop(store);
        let mut store = scratch.open();
        store.maintain(NOW + 49 * DAY_MS).unwrap();
        let count: i64 = store
            .db
            .query_row("SELECT count(*) FROM diary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    /// `diary_forget` takes the newest row with `last`, and the whole diary
    /// and the care pause without it; forget everything of the person, of
    /// everyone, and forgetting the person take the diary and the pause in
    /// the same transaction; nobody else's goes.
    #[test]
    fn every_forget_erases_the_diary() {
        let (_scratch, mut store) = Scratch::new("forget");
        let fill = |store: &mut Days| {
            add_at(store, OWNER, NOW - 1000, "breakfast", "toast");
            add(store, OWNER, "lunch", "salad");
            add(store, OTHER, "lunch", "rice");
            let pause = send(
                store,
                json!({"op":"diary_care","person":OWNER,"pause":true}),
            );
            assert_eq!(pause, json!({"ok":true,"care_pause":true}));
        };
        fill(&mut store);
        let last = send(
            &mut store,
            json!({"op":"diary_forget","person":OWNER,"last":true}),
        );
        assert_eq!(last, json!({"ok":true,"removed":1}));
        let left = list_at(&mut store, OWNER, 1, NOW);
        assert_eq!(words(&left), ["toast"]);
        assert_eq!(
            left["care_pause"], true,
            "a last-row forget keeps the pause"
        );
        let all = send(&mut store, json!({"op":"diary_forget","person":OWNER}));
        assert_eq!(all, json!({"ok":true,"removed":1}));
        let none = list_at(&mut store, OWNER, 28, NOW);
        assert!(words(&none).is_empty());
        assert_eq!(none["care_pause"], false);
        assert_eq!(words(&list_at(&mut store, OTHER, 1, NOW)), ["rice"]);

        // Forget everything of the person.
        fill(&mut store);
        let forgot = send(&mut store, json!({"op":"forget_all","person":OWNER}));
        assert!(forgot["removed"].as_u64().unwrap() >= 2, "{forgot}");
        let none = list_at(&mut store, OWNER, 28, NOW);
        assert!(words(&none).is_empty());
        assert_eq!(none["care_pause"], false);
        assert_eq!(words(&list_at(&mut store, OTHER, 1, NOW)), ["rice", "rice"]);

        // Forgetting the person.
        let later = NOW + 10;
        add_at(&mut store, OWNER, later, "dinner", "pasta");
        send_at(
            &mut store,
            json!({"op":"diary_care","person":OWNER,"pause":true}),
            later,
        );
        let gone = send_at(
            &mut store,
            json!({"op":"forget_person","person":OWNER}),
            later,
        );
        assert_eq!(gone["ok"], true, "{gone}");
        let none = list_at(&mut store, OWNER, 28, later);
        assert!(words(&none).is_empty());
        assert_eq!(none["care_pause"], false);

        // Forget everything of everyone.
        let forgot = send_at(&mut store, json!({"op":"forget_all","person":null}), later);
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(words(&list_at(&mut store, OTHER, 28, later)).is_empty());
        let count: i64 = store
            .db
            .query_row("SELECT count(*) FROM diary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    /// A meal told before a forget everything is refused, so one retried
    /// after the forget never comes back; one told after it is kept.
    #[test]
    fn a_meal_told_before_a_forget_everything_is_refused() {
        let (_scratch, mut store) = Scratch::new("forget-mark");
        let at = NOW + 1000;
        let forgot = send_at(&mut store, json!({"op":"forget_all","person":OWNER}), at);
        assert_eq!(forgot["ok"], true, "{forgot}");
        let before = send_at(
            &mut store,
            json!({"op":"diary_add","person":OWNER,"at_unix_ms":NOW,"meal":"lunch","words":"rice"}),
            at,
        );
        assert_eq!(before["error"], "forgotten", "{before}");
        assert_eq!(
            add_at(&mut store, OWNER, at + 1, "lunch", "rice")["ok"],
            true
        );
    }

    /// Past the most a person keeps, the oldest goes.
    #[test]
    fn a_diary_is_bounded() {
        let (_scratch, mut store) = Scratch::new("bounded");
        for n in 0..=MOST_PER_PERSON as u64 {
            let added = add_at(&mut store, OWNER, NOW + n, "snack", &format!("bite {n}"));
            assert_eq!(added["ok"], true, "{added}");
        }
        let kept = words(&list_at(&mut store, OWNER, 1, NOW + 1000));
        assert_eq!(kept.len(), MOST_PER_PERSON);
        assert_eq!(kept[0], "bite 1");
    }
}
