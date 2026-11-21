//! What a Store app or a paired computer told Lamp to keep (#7415, C166,
//! `lamp_msgs::app_fact`): a kept thing under one known person, marked with
//! its source in `app_facts`, never found by its words. The wording, bounds
//! and ops are `lamp_msgs::app_fact`'s; this is the store's side.
//!
//! - **Whose.** Only a person the eyes know (`p-` id) that memory already
//!   holds (something of theirs other than an app's fact): the writer picks
//!   the lamp's known person (`lamp_msgs::app_fact::whose`), and memory
//!   refuses any other id, the room, a rules scope and a person it holds
//!   nothing of (`not_known`; `forgotten` after their forget), so an app's
//!   words never open a new scope.
//! - **Bounds.** The app's words at most `MOST_CHARS` characters, refused
//!   past it, never cut; `MOST_A_DAY` a source a day by memory's own clock
//!   when it took them (`too_many`; a backdated `at` never gets around it);
//!   `MOST_KEPT` a source in all, the first kept going first (never by
//!   `at`); gone `KEPT_DAYS` after they were kept. Not counted in the
//!   person's own kept list's room, never matched with their own line by its
//!   words.
//! - **Apart from the person's own list.** Never in `kept` (`kept_with`), so
//!   a busy app never pushes the person's own kept things out of the hits
//!   note; found by `recall` and `search` as any kept thing, with its mark.
//!   Listed alone by `kept {app_facts}` (`Days::kept_app_facts`), for a
//!   forget about a thing only.
//!   Never read by the nightly pass, so no episode, pattern or chapter is
//!   made from one without its mark or outlives its days.
//! - **Forget.** `forget_app_facts {source}` removes every fact of that
//!   source, with what was made from them, by their mark. A person's own
//!   forgets (one thing, about a thing, everything, the person) take them as
//!   any kept thing. A fact asked before the person's last forget
//!   everything is refused `forgotten`.
use crate::{
    days::{
        Days, Error, Record, Result, descendants, ensure_person, erase, insert_record, next_id,
        said_again_with_doomed,
    },
    store::{Day, Person},
};
use lamp_msgs::app_fact;
use rusqlite::{OptionalExtension, params};

/// How far ahead of memory's clock a fact's time may be: a future time
/// can't pass a later forget (as `linked_record`'s deed).
const AHEAD_MS: u64 = 5_000;

impl Days {
    /// Keeps `text` from `source` (named `name`) as `person`'s, at `at`. The
    /// new id.
    pub fn remember_app_fact(
        &mut self,
        person: &Person,
        source: &str,
        name: &str,
        at: u64,
        text: &str,
    ) -> Result<u64> {
        if !lamp_msgs::known::is_person_id(person.as_str()) {
            return Err(Error::new(
                "not_known",
                "an app's fact is kept only for the lamp's known person",
            ));
        }
        let Some(kept_day) = Day::at(self.now, &self.local) else {
            return Err(Error::new(
                "bad_time",
                "memory's clock is outside 1970 through 9999",
            ));
        };
        let Some(source) = app_fact::Source::parse(source) else {
            return Err(Error::new(
                "bad_source",
                "a source is app:<id> or computer:<id>",
            ));
        };
        let line = app_fact::tidy(text, app_fact::MOST_CHARS).map_err(|refused| {
            Error::new(
                refused.code(),
                "an app's fact is 1 to 280 characters, never cut",
            )
        })?;
        if at > self.now.saturating_add(AHEAD_MS) {
            return Err(Error::new(
                "bad_time",
                "the fact's time is ahead of memory's clock",
            ));
        }
        let Some(day) = Day::at(at, &self.local) else {
            return Err(Error::new(
                "bad_time",
                "the timestamp is outside 1970 through 9999",
            ));
        };
        let words = app_fact::text(&app_fact::name(name), &day.to_string(), &line);
        let expired = kept_day
            .before(app_fact::KEPT_DAYS)
            .map_or_else(|| "1970-01-01".into(), |d| d.to_string());
        let tx = self.db.transaction()?;
        let forgot: Option<u64> = tx
            .query_row(
                "SELECT max(at_unix_ms) FROM service_records_forgot WHERE person IN (?1,'')",
                [person.as_str()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        // Held: something of theirs other than an app's fact. After a
        // forget of the person (or everything) nothing is, until they are
        // heard again.
        let held: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE person=?1 AND rid NOT IN (SELECT rid FROM app_facts))",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if !held {
            return Err(match forgot {
                Some(_) => Error::new(
                    "forgotten",
                    "the person was forgotten, and memory holds nothing of theirs since",
                ),
                None => Error::new(
                    "not_known",
                    "an app's fact is kept only for a person memory already holds",
                ),
            });
        }
        if forgot.is_some_and(|forgot| at < forgot) {
            return Err(Error::new(
                "forgotten",
                "the fact was told before a forget everything",
            ));
        }
        // Counted by the day memory took them, never by `at`.
        let today: u32 = tx.query_row(
            "SELECT count(*) FROM app_facts WHERE source=?1 AND kept_day=?2",
            params![source.as_str(), kept_day.to_string()],
            |r| r.get(0),
        )?;
        if today >= app_fact::MOST_A_DAY {
            return Err(Error::new(
                "too_many",
                "this source has kept 20 facts today",
            ));
        }
        tx.execute("DELETE FROM doomed", [])?;
        // Past its days, and past the source's 200 (the first kept first,
        // by the order memory took them, room made for this one).
        tx.execute(
            "INSERT OR IGNORE INTO doomed SELECT rid FROM app_facts WHERE source=?1 AND kept_day<=?2",
            params![source.as_str(), expired],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO doomed SELECT rid FROM (SELECT rid FROM app_facts WHERE source=?1 AND rid NOT IN (SELECT rid FROM doomed) ORDER BY rid DESC LIMIT -1 OFFSET ?2)",
            params![source.as_str(), app_fact::MOST_KEPT - 1],
        )?;
        let going: u64 = tx.query_row("SELECT count(*) FROM doomed", [], |r| r.get(0))?;
        if going > 0 {
            // What was made from them goes by its link ([`descendants`]):
            // an app's fact has no source exchange to share with a talk
            // note or a nightly item, and the nightly pass never reads one.
            descendants(&tx, None)?;
            said_again_with_doomed(&tx)?;
            erase(&tx)?;
        }
        ensure_person(&tx, person)?;
        let id = next_id(&tx, person)?;
        insert_record(
            &tx,
            &Record {
                id,
                person: person.as_str().into(),
                kind: "kept".into(),
                at_unix_ms: at,
                heard: String::new(),
                said: String::new(),
                text: words,
                category: None,
                source_ids: vec![],
                supersedes: None,
                active: true,
            },
            &self.local,
            self.now,
        )?;
        tx.execute(
            "INSERT INTO app_facts(rid,source,kept_day) SELECT rid,?3,?4 FROM entries WHERE person=?1 AND id=?2",
            params![person.as_str(), id, source.as_str(), kept_day.to_string()],
        )?;
        tx.commit()?;
        if going > 0 {
            self.forget_set_aside()?;
        }
        Ok(id)
    }

    /// Every fact of `source`, found by its mark, each going as a `forget
    /// {id}` of it would, with what was made from it: the app removed, the
    /// computer revoked or its grant turned off. How many entries went.
    pub fn forget_app_facts(&mut self, source: &str) -> Result<u64> {
        let Some(source) = app_fact::Source::parse(source) else {
            return Err(Error::new(
                "bad_source",
                "a source is app:<id> or computer:<id>",
            ));
        };
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM doomed", [])?;
        tx.execute(
            "INSERT INTO doomed SELECT rid FROM app_facts WHERE source=?1",
            [source.as_str()],
        )?;
        let found: u64 = tx.query_row("SELECT count(*) FROM doomed", [], |r| r.get(0))?;
        if found == 0 {
            return Ok(0);
        }
        descendants(&tx, None)?;
        said_again_with_doomed(&tx)?;
        let removed = erase(&tx)?;
        tx.commit()?;
        self.forget_set_aside()?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use crate::{days::Days, store::Options};
    use rusqlite::params;
    use serde_json::{Value, json};
    use std::{fs, path::PathBuf};

    const NOW: u64 = 1_791_892_800_000;
    const DAY_MS: u64 = 86_400_000;
    const ANN: &str = "p-0123456789abcdef";
    const BOB: &str = "p-fedcba9876543210";
    const COACH: &str = "app:gym-coach";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> (Self, Days) {
            let dir =
                std::env::temp_dir().join(format!("lamp-app-facts-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let store = Days::open(
                Options {
                    dir: dir.clone(),
                    keep_days: 30,
                    local: crate::store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap();
            (Self(dir), store)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn send(store: &mut Days, request: Value, now: u64) -> Value {
        serde_json::from_str(
            &crate::days_protocol::answer(store, request.to_string().as_bytes(), now).line,
        )
        .unwrap()
    }

    fn talk(store: &mut Days, person: &str, now: u64) {
        let said = send(
            store,
            json!({"op":"append","person":person,"at_unix_ms":now,"heard":"hello","said":"Hi"}),
            now,
        );
        assert_eq!(said["ok"], true, "{said}");
    }

    /// An app's fact told at `at`, taken by memory at `now`.
    fn note_at(
        store: &mut Days,
        person: &str,
        source: &str,
        at: u64,
        now: u64,
        text: &str,
    ) -> Value {
        send(
            store,
            json!({"op": lamp_msgs::app_fact::REMEMBER_OP, "person": person, "source": source,
                "name": "Gym Coach", "at_unix_ms": at, "text": text}),
            now,
        )
    }

    fn note(store: &mut Days, person: &str, source: &str, at: u64, text: &str) -> Value {
        note_at(store, person, source, at, at, text)
    }

    /// The person's own kept list, as the voice reads it.
    fn kept(store: &mut Days, person: &str, now: u64) -> Vec<String> {
        send(
            store,
            json!({"op":"kept","person":person,"max_bytes":16384}),
            now,
        )["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["text"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The person's app facts at `now` (after its upkeep), the first kept
    /// first.
    fn facts(store: &mut Days, person: &str, now: u64) -> Vec<String> {
        let status = send(store, json!({"op":"status","person":person}), now);
        assert_eq!(status["ok"], true, "{status}");
        let mut statement = store
            .db
            .prepare("SELECT e.text FROM app_facts a JOIN entries e ON e.rid=a.rid WHERE e.person=?1 ORDER BY a.rid")
            .unwrap();
        statement
            .query_map([person], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// A line made from `parent` (an id of `person`'s): a talk note, or a
    /// nightly episode, linked to it as its source, as a build that let
    /// them be made from an app's fact would have left it.
    fn derive(store: &mut Days, person: &str, parent: u64, id: u64, nightly: bool) {
        let db = &store.db;
        let category = if nightly { "episode" } else { "talk" };
        db.execute(
            "INSERT INTO entries(person,id,kind,at_unix_ms,day,text,category,used_unix_ms) VALUES(?1,?2,'observation',?3,'2026-10-13','made from it',?4,?3)",
            params![person, id, NOW, category],
        )
        .unwrap();
        let child = db.last_insert_rowid();
        db.execute(
            "INSERT INTO sources(child,parent) SELECT ?1,rid FROM entries WHERE person=?2 AND id=?3",
            params![child, person, parent],
        )
        .unwrap();
        if nightly {
            db.execute(
                "INSERT INTO nightly_items(rid,person,kind,night) VALUES(?1,?2,'episode','2026-10-13')",
                params![child, person],
            )
            .unwrap();
        }
    }

    fn derived(store: &Days, person: &str) -> u64 {
        store
            .db
            .query_row(
                "SELECT count(*) FROM entries WHERE person=?1 AND text='made from it'",
                [person],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// An app's fact is kept under the known person, quoted with its
    /// source, and only theirs: an id memory never held, the room or a bad
    /// source keeps nothing. Recall finds it with its mark.
    #[test]
    fn an_apps_fact_is_kept_for_the_known_person_marked_with_its_source() {
        let (_scratch, mut store) = Scratch::new("kept");
        talk(&mut store, ANN, NOW);
        let kept_one = note(
            &mut store,
            ANN,
            COACH,
            NOW + 1,
            "squats 3x10, chest up twice",
        );
        assert_eq!(kept_one["ok"], true, "{kept_one}");
        let marked = "Noted by the app Gym Coach, 2026-10-13: \"squats 3x10, chest up twice\"";
        assert_eq!(facts(&mut store, ANN, NOW + 2), [marked]);
        // Found as the voice's recall asks (the second version): about the
        // workout, and by the day, where kept things come first.
        for recall in [
            json!({"op":"recall","person":ANN,"query":"squats","limit":6,"max_bytes":2400}),
            json!({"op":"recall","person":ANN,"query":null,"day":"2026-10-13","limit":6,"max_bytes":2400,"recall_v2":true}),
            json!({"op":"search","person":ANN,"query":"squats","limit":5}),
        ] {
            let found = send(&mut store, recall.clone(), NOW + 2);
            assert!(
                found.to_string().contains(&marked.replace('"', "\\\"")),
                "{recall}: {found}"
            );
        }
        for (person, error) in [
            (BOB, "not_known"),
            ("everyone", "not_known"),
            ("r-x", "not_known"),
        ] {
            let refused = note(&mut store, person, COACH, NOW + 3, "squats");
            assert_eq!(refused["error"], error, "{person}: {refused}");
        }
        assert!(facts(&mut store, BOB, NOW + 3).is_empty());
        let bad = note(&mut store, ANN, "gym-coach", NOW + 3, "squats");
        assert_eq!(bad["error"], "bad_source", "{bad}");
        let long = note(&mut store, ANN, COACH, NOW + 3, &"a".repeat(281));
        assert_eq!(long["error"], "too_long", "{long}");
        let empty = note(&mut store, ANN, COACH, NOW + 3, " https://x.io ");
        assert_eq!(empty["error"], "empty", "{empty}");
        assert_eq!(facts(&mut store, ANN, NOW + 3).len(), 1);
    }

    /// The fact's time may be at most 5 s ahead of memory's clock.
    #[test]
    fn a_fact_more_than_five_seconds_ahead_is_refused() {
        let (_scratch, mut store) = Scratch::new("ahead");
        talk(&mut store, ANN, NOW);
        let ahead = note_at(&mut store, ANN, COACH, NOW + 6_000, NOW, "squats");
        assert_eq!(ahead["error"], "bad_time", "{ahead}");
        let near = note_at(&mut store, ANN, COACH, NOW + 4_000, NOW, "squats");
        assert_eq!(near["ok"], true, "{near}");
    }

    /// The app's removal takes its facts by their mark, with what was made
    /// from them, and nothing else: a person's own line in the same words
    /// and another source's stay.
    #[test]
    fn forget_app_facts_takes_one_sources_facts_by_their_mark() {
        let (_scratch, mut store) = Scratch::new("forget-source");
        talk(&mut store, ANN, NOW);
        let coach = note(&mut store, ANN, COACH, NOW + 1, "squats 3x10");
        assert_eq!(coach["ok"], true, "{coach}");
        let laptop = note(
            &mut store,
            ANN,
            "computer:dev-1",
            NOW + 2,
            "built the coach",
        );
        assert_eq!(laptop["ok"], true, "{laptop}");
        derive(&mut store, ANN, coach["id"].as_u64().unwrap(), 9001, false);
        derive(&mut store, ANN, coach["id"].as_u64().unwrap(), 9002, true);
        derive(&mut store, ANN, laptop["id"].as_u64().unwrap(), 9003, true);
        let same_words = "Noted by the app Gym Coach, 2026-10-13: \"squats 3x10\"";
        let own = send(
            &mut store,
            json!({"op":"remember","person":ANN,"at_unix_ms":NOW+3,"text":same_words,"once":true}),
            NOW + 3,
        );
        assert_eq!(own["ok"], true, "{own}");
        assert_eq!(
            kept(&mut store, ANN, NOW + 4),
            [same_words],
            "once never matched the app's"
        );
        let gone = send(
            &mut store,
            json!({"op": lamp_msgs::app_fact::FORGET_OP, "source": COACH}),
            NOW + 4,
        );
        assert_eq!(
            gone,
            json!({"ok":true,"removed":3}),
            "the fact and its two lines"
        );
        assert_eq!(derived(&store, ANN), 1, "the laptop's line stays");
        assert_eq!(kept(&mut store, ANN, NOW + 4), [same_words]);
        let left = facts(&mut store, ANN, NOW + 4);
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].contains("built the coach"), "{left:?}");
        assert_eq!(
            send(
                &mut store,
                json!({"op": lamp_msgs::app_fact::FORGET_OP, "source": COACH}),
                NOW + 5
            ),
            json!({"ok":true,"removed":0})
        );
    }

    /// A person's forget everything takes their app facts, and a fact told
    /// before it is refused after it; one forget of the fact's id takes it.
    /// After a forget of the person, an app's write for them is refused.
    #[test]
    fn a_persons_forgets_take_their_app_facts() {
        let (_scratch, mut store) = Scratch::new("forget-all");
        talk(&mut store, ANN, NOW);
        let one = note(&mut store, ANN, COACH, NOW + 1, "squats 3x10");
        assert_eq!(one["ok"], true, "{one}");
        let forgot = send(
            &mut store,
            json!({"op":"forget","person":ANN,"id":one["id"]}),
            NOW + 2,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(facts(&mut store, ANN, NOW + 2).is_empty());
        assert_eq!(
            note(&mut store, ANN, COACH, NOW + 3, "lunges 2x8")["ok"],
            true
        );
        let all = send(&mut store, json!({"op":"forget_all","person":ANN}), NOW + 4);
        assert_eq!(all["ok"], true, "{all}");
        assert!(facts(&mut store, ANN, NOW + 5).is_empty());
        let nothing_since = note(&mut store, ANN, COACH, NOW + 5, "after it");
        assert_eq!(nothing_since["error"], "forgotten", "{nothing_since}");
        talk(&mut store, ANN, NOW + 5);
        let late = note(&mut store, ANN, COACH, NOW + 3, "told before the forget");
        assert_eq!(late["error"], "forgotten", "{late}");
        assert_eq!(
            note(&mut store, ANN, COACH, NOW + 6, "after it")["ok"],
            true
        );
        // The app's removal of the person: no write for them after it.
        let person = send(
            &mut store,
            json!({"op":"forget_person","person":ANN}),
            NOW + 7,
        );
        assert_eq!(person["ok"], true, "{person}");
        let after = note(&mut store, ANN, COACH, NOW + 8, "squats");
        assert_eq!(after["error"], "forgotten", "{after}");
        assert!(facts(&mut store, ANN, NOW + 8).is_empty());
    }

    /// `kept {app_facts}`, a forget about a thing's listing alone: the
    /// person's own app facts by the ids a `forget {id}` takes, a page below
    /// `before_id`; never another person's, never their own kept things, and
    /// the plain `kept` (the hits note's) still without them.
    #[test]
    fn kept_app_facts_lists_only_the_persons_own_app_facts() {
        let (_scratch, mut store) = Scratch::new("kept-app-facts");
        talk(&mut store, ANN, NOW);
        talk(&mut store, BOB, NOW);
        let own = send(
            &mut store,
            json!({"op":"remember","person":ANN,"at_unix_ms":NOW+1,"text":"Sister's wedding is on Saturday."}),
            NOW + 1,
        );
        assert_eq!(own["ok"], true, "{own}");
        let squats = note(&mut store, ANN, COACH, NOW + 2, "squats 3x10");
        let lunges = note(&mut store, ANN, COACH, NOW + 3, "lunges 2x8");
        let bobs = note(&mut store, BOB, COACH, NOW + 4, "squats 5x5");
        assert_eq!(bobs["ok"], true, "{bobs}");
        let listed = |store: &mut Days, request: Value| -> Vec<(u64, String)> {
            let answer = send(store, request.clone(), NOW + 5);
            assert_eq!(answer["ok"], true, "{request}: {answer}");
            answer["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["id"].as_u64().unwrap(),
                        e["text"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        };
        let all = listed(
            &mut store,
            json!({"op":"kept","person":ANN,"max_bytes":16384,"app_facts":true}),
        );
        let ids: Vec<u64> = all.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            [
                squats["id"].as_u64().unwrap(),
                lunges["id"].as_u64().unwrap()
            ]
        );
        assert!(all[0].1.contains("squats 3x10") && all[1].1.contains("lunges 2x8"));
        let page = listed(
            &mut store,
            json!({"op":"kept","person":ANN,"app_facts":true,"before_id":lunges["id"]}),
        );
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].0, squats["id"].as_u64().unwrap());
        assert_eq!(
            kept(&mut store, ANN, NOW + 5),
            ["Sister's wedding is on Saturday."]
        );
        let theirs = listed(
            &mut store,
            json!({"op":"kept","person":BOB,"app_facts":true}),
        );
        assert_eq!(theirs.len(), 1);
        assert!(theirs[0].1.contains("squats 5x5"));
        for bad in [
            json!({"op":"kept","person":ANN,"app_facts":true,"observations":true}),
            json!({"op":"kept","person":ANN,"before_id":3}),
        ] {
            assert_eq!(
                send(&mut store, bad.clone(), NOW + 5)["error"],
                "bad_request",
                "{bad}"
            );
        }
        // Its id is the one a forget takes; the other stays.
        let forgot = send(
            &mut store,
            json!({"op":"forget","person":ANN,"id":squats["id"],"selective":true}),
            NOW + 6,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        let left = listed(
            &mut store,
            json!({"op":"kept","person":ANN,"app_facts":true}),
        );
        assert_eq!(left.len(), 1);
        assert!(left[0].1.contains("lunges 2x8"));
    }

    /// 20 a source a day by memory's clock: a backdated `at` never gets
    /// around it; another source, and the next day, keep on.
    #[test]
    fn a_source_keeps_twenty_facts_a_day() {
        let (_scratch, mut store) = Scratch::new("a-day");
        talk(&mut store, ANN, NOW);
        for n in 0..u64::from(lamp_msgs::app_fact::MOST_A_DAY) {
            let kept_one = note(&mut store, ANN, COACH, NOW + n, &format!("set {n}"));
            assert_eq!(kept_one["ok"], true, "{n}: {kept_one}");
        }
        let over = note(&mut store, ANN, COACH, NOW + 100, "one too many");
        assert_eq!(over["error"], "too_many", "{over}");
        let backdated = note_at(&mut store, ANN, COACH, NOW - DAY_MS, NOW + 101, "yesterday");
        assert_eq!(backdated["error"], "too_many", "{backdated}");
        assert_eq!(
            note(&mut store, ANN, "app:other", NOW + 102, "another app")["ok"],
            true
        );
        assert_eq!(
            note(&mut store, ANN, COACH, NOW + DAY_MS, "next day")["ok"],
            true
        );
    }

    /// 200 a source in all, the first kept going first (never by `at`, so a
    /// backdated fact can't take a newer one's place), with what was made
    /// from it; never in the person's own kept list, which keeps all of
    /// theirs; gone 90 days after they were kept.
    #[test]
    fn a_source_keeps_two_hundred_the_first_kept_going_and_none_past_ninety_days() {
        let (_scratch, mut store) = Scratch::new("in-all");
        talk(&mut store, ANN, NOW);
        let mut own = Vec::new();
        for n in 0..3 {
            let text = format!("her own thing {n}");
            let kept_one = send(
                &mut store,
                json!({"op":"remember","person":ANN,"at_unix_ms":NOW+n,"text":text}),
                NOW + n,
            );
            assert_eq!(kept_one["ok"], true, "{kept_one}");
            own.push(text);
        }
        let most = u64::from(lamp_msgs::app_fact::MOST_KEPT);
        let a_day = u64::from(lamp_msgs::app_fact::MOST_A_DAY);
        let mut first = None;
        for n in 0..most {
            let at = NOW + (n / a_day) * DAY_MS + 10 + n;
            let kept_one = note(&mut store, ANN, COACH, at, &format!("set {n}"));
            assert_eq!(kept_one["ok"], true, "{n}: {kept_one}");
            first.get_or_insert(kept_one["id"].as_u64().unwrap());
        }
        derive(&mut store, ANN, first.unwrap(), 9001, true);
        let last = NOW + (most / a_day) * DAY_MS + 10 + most;
        // The person's own list is theirs alone, all of it.
        assert_eq!(kept(&mut store, ANN, last), own);
        // A backdated fact: the first kept goes, never a newer one.
        let backdated = note_at(&mut store, ANN, COACH, NOW, last, "backdated set");
        assert_eq!(backdated["ok"], true, "{backdated}");
        let list = facts(&mut store, ANN, last);
        assert_eq!(list.len() as u64, most, "the first kept went");
        assert!(
            !list.iter().any(|text| text.ends_with("\"set 0\"")),
            "{:?}",
            &list[..2]
        );
        assert!(list.iter().any(|text| text.ends_with("\"set 1\"")));
        assert!(list.iter().any(|text| text.ends_with("\"backdated set\"")));
        assert_eq!(derived(&store, ANN), 0, "its nightly line went with it");
        // 90 days after the last was kept, every app fact is gone, and her
        // own stay.
        let later = last + u64::from(lamp_msgs::app_fact::KEPT_DAYS) * DAY_MS;
        assert!(facts(&mut store, ANN, later).is_empty());
        assert_eq!(kept(&mut store, ANN, later), own);
    }

    /// The nightly pass is never given an app's fact, so nothing is made
    /// from one without its mark or past its days.
    #[test]
    fn the_nightly_pass_is_never_given_an_apps_fact() {
        let (_scratch, mut store) = Scratch::new("nightly");
        talk(&mut store, ANN, NOW);
        let fact = note(&mut store, ANN, COACH, NOW + 1, "squats 3x10");
        assert_eq!(fact["ok"], true, "{fact}");
        let own = send(
            &mut store,
            json!({"op":"remember","person":ANN,"at_unix_ms":NOW+2,"text":"takes her coffee black"}),
            NOW + 2,
        );
        assert_eq!(own["ok"], true, "{own}");
        let prepared = send(
            &mut store,
            json!({"op":"nightly_prepare","person":ANN,"day":"2026-10-13"}),
            NOW + 3,
        );
        assert_eq!(prepared["ok"], true, "{prepared}");
        let ids: Vec<u64> = prepared["input"]["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record["id"].as_u64().unwrap())
            .collect();
        assert!(!ids.contains(&fact["id"].as_u64().unwrap()), "{prepared}");
        assert!(ids.contains(&own["id"].as_u64().unwrap()), "{prepared}");
        assert!(!prepared.to_string().contains("squats"), "{prepared}");
    }

    /// Moving the room's kept things to a person counts only their own: a
    /// person with 199 of their own and app facts beside still takes one.
    #[test]
    fn adopting_counts_only_the_persons_own_kept_things() {
        let (_scratch, mut store) = Scratch::new("adopt");
        talk(&mut store, ANN, NOW);
        for n in 0..(crate::store::KEPT_ENTRIES as u64 - 1) {
            let kept_one = send(
                &mut store,
                json!({"op":"remember","person":ANN,"at_unix_ms":NOW+n,"text":format!("own thing {n}")}),
                NOW + n,
            );
            assert_eq!(kept_one["ok"], true, "{n}: {kept_one}");
        }
        for n in 0..2 {
            assert_eq!(
                note(&mut store, ANN, COACH, NOW + 300 + n, &format!("set {n}"))["ok"],
                true
            );
        }
        let room = send(
            &mut store,
            json!({"op":"remember","person":"everyone","at_unix_ms":NOW+400,"text":"the room's tea is mint"}),
            NOW + 400,
        );
        assert_eq!(room["ok"], true, "{room}");
        let moved = send(
            &mut store,
            json!({"op":"adopt","person":"everyone","to":ANN,"ids":[room["id"]]}),
            NOW + 401,
        );
        assert_eq!(moved["ok"], true, "{moved}");
        assert_eq!(
            kept(&mut store, ANN, NOW + 402).len(),
            crate::store::KEPT_ENTRIES
        );
    }

    /// The first builder's condition on #8068 (11:09), held for #8167 too
    /// (00:59 on #8167): a store made by stable's schema opens under this
    /// build unchanged. `app_facts` is on stable now; stable's schema is
    /// `days.sql` without #8167's `history_unsent` block, found by its
    /// markers, so a store made here with that table dropped is stable's.
    /// Opened again here: the same kept list, recall and search hits;
    /// `maintain` dooms nothing; `history_unsent` is made, empty. Read then
    /// with stable's own SQL (the kept count, the kept-already list, the
    /// nightly read): the same.
    #[test]
    fn a_store_made_by_stables_schema_opens_and_reads_the_same() {
        // #8167's block: from its comment's first line to its table's end.
        const START: &str = "-- #7771 (the first builder's switch-on condition on #8086)";
        const TABLE: &str = "CREATE TABLE IF NOT EXISTS history_unsent (";
        let schema = include_str!("days.sql");
        let from = schema
            .find(START)
            .expect("the history_unsent block's start");
        let table = from + schema[from..].find(TABLE).expect("its table");
        let to = table + schema[table..].find(");").expect("its table's end") + 2;
        let block = &schema[from..to];
        assert_eq!(
            block.matches("CREATE ").count(),
            1,
            "#8167's block is one table, no index"
        );
        let stable = format!("{}{}", &schema[..from], &schema[to..]);
        assert!(!stable.contains("history_unsent"), "{stable}");
        assert!(
            stable.contains("CREATE TABLE IF NOT EXISTS app_facts (")
                && stable.contains("CREATE INDEX IF NOT EXISTS app_facts_source "),
            "app_facts is stable's"
        );
        let (scratch, mut store) = Scratch::new("stable-schema");
        talk(&mut store, ANN, NOW);
        talk(&mut store, BOB, NOW + 1);
        for (n, text) in [
            "takes her coffee black",
            "squats on Mondays",
            "sister visits Friday",
        ]
        .iter()
        .enumerate()
        {
            let kept_one = send(
                &mut store,
                json!({"op":"remember","person":ANN,"at_unix_ms":NOW+10+n as u64,"text":text}),
                NOW + 10 + n as u64,
            );
            assert_eq!(kept_one["ok"], true, "{kept_one}");
        }
        store
            .db
            .execute(
                "INSERT INTO entries(person,id,kind,at_unix_ms,day,text,category,used_unix_ms) VALUES(?1,900,'observation',?2,'2026-10-13','likes mornings','fact',?2)",
                params![ANN, NOW + 20],
            )
            .unwrap();
        // Stable's store: this build's, less #8167's one block.
        store
            .db
            .execute_batch("DROP TABLE history_unsent;")
            .unwrap();
        drop(store);
        let path = scratch.0.join(crate::days::DATABASE);
        // What stable's SQL reads, before and after this build opens it.
        let stable_reads = || -> (u64, Vec<(u64, String)>, Vec<u64>, u64) {
            let db = rusqlite::Connection::open(&path).unwrap();
            let count = db
                .query_row(
                    "SELECT count(*) FROM entries WHERE person=? AND kind='kept' AND active=1",
                    [ANN],
                    |r| r.get(0),
                )
                .unwrap();
            let already = db
                .prepare("SELECT id,text FROM entries WHERE person=? AND kind='kept' AND active=1 ORDER BY id")
                .unwrap()
                .query_map([ANN], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let nightly = db
                .prepare("SELECT id FROM entries WHERE person=?1 AND day=?2 AND id>?3 AND active=1 AND ((kind='exchange' AND trim(heard)!='' AND at_unix_ms>coalesce((SELECT f.at_unix_ms+?4 FROM nightly_forgets f WHERE f.person=?1 AND f.day=?2),-1)) OR kind='kept') ORDER BY id")
                .unwrap()
                .query_map(params![ANN, "2026-10-13", 0, crate::nightly::FORGET_TURN_MS], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let entries = db
                .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
                .unwrap();
            (count, already, nightly, entries)
        };
        let before = stable_reads();
        assert_eq!(before.0, 3);
        let reads = |store: &mut Days| {
            (
                kept(store, ANN, NOW + 30),
                send(
                    store,
                    json!({"op":"recall","person":ANN,"query":"coffee","limit":6,"max_bytes":2400,"recall_v2":true}),
                    NOW + 30,
                ),
                send(
                    store,
                    json!({"op":"recall","person":ANN,"query":null,"day":"2026-10-13","limit":6,"max_bytes":2400,"recall_v2":true}),
                    NOW + 30,
                ),
                send(
                    store,
                    json!({"op":"search","person":ANN,"query":"squats","limit":5}),
                    NOW + 30,
                ),
                send(store, json!({"op":"status","person":ANN}), NOW + 30),
            )
        };
        let reopen = || {
            Days::open(
                Options {
                    dir: scratch.0.clone(),
                    keep_days: 30,
                    local: crate::store::LocalTime::Offset(0),
                },
                NOW + 30,
            )
            .unwrap()
        };
        let mut store = reopen();
        assert_eq!(
            store.maintain(NOW + 30).unwrap(),
            0,
            "maintain dooms nothing"
        );
        let empty: u64 = store
            .db
            .query_row("SELECT count(*) FROM history_unsent", [], |r| r.get(0))
            .unwrap();
        assert_eq!(empty, 0, "history_unsent is made, empty");
        let first = reads(&mut store);
        assert_eq!(
            first.0,
            [
                "takes her coffee black",
                "squats on Mondays",
                "sister visits Friday"
            ]
        );
        assert!(first.1.to_string().contains("coffee black"), "{}", first.1);
        assert!(
            first.2.to_string().contains("squats on Mondays"),
            "{}",
            first.2
        );
        assert!(
            first.3.to_string().contains("squats on Mondays"),
            "{}",
            first.3
        );
        drop(store);
        // Opened again by this build: the same answers.
        let mut store = reopen();
        assert_eq!(store.maintain(NOW + 30).unwrap(), 0);
        assert_eq!(reads(&mut store), first);
        drop(store);
        // And stable's SQL reads it as before.
        assert_eq!(stable_reads(), before);
    }
}
