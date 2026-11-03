//! `--taste-rules` (plan C, step A, 3 Nov): a known person's stated tastes
//! kept by code rules inside the exchange's write, as source-linked
//! observations in their own words. No model, network or lamp.
use lamp_memory::{
    Person,
    days::{DATABASE, Days},
    store::{Ask, LocalTime, Options},
};
use rusqlite::{Connection, types::ValueRef};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const NOW: u64 = 1_791_892_800_000;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-tastes-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }

    fn open(&self, rules: bool) -> Days {
        let mut store = Days::open(
            Options {
                dir: self.0.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            NOW,
        )
        .unwrap();
        if rules {
            store.enable_taste_rules().unwrap();
        }
        store
    }

    /// Every row of the store's tables but the index's, in order: what a
    /// store holds. Called only while the Days owner has been dropped.
    fn dump(&self) -> Vec<String> {
        let db = Connection::open(self.0.join(DATABASE)).unwrap();
        let mut rows = Vec::new();
        let tables: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'search%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for table in tables {
            let mut statement = db.prepare(&format!("SELECT * FROM {table}")).unwrap();
            let columns = statement.column_count();
            let lines: Vec<String> = statement
                .query_map([], |r| {
                    let cells: Vec<String> = (0..columns)
                        .map(|i| match r.get_ref(i).unwrap() {
                            ValueRef::Null => "NULL".to_owned(),
                            ValueRef::Integer(n) => n.to_string(),
                            ValueRef::Real(x) => x.to_string(),
                            ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                            ValueRef::Blob(b) => format!("{b:?}"),
                        })
                        .collect();
                    Ok(format!("{table}: {}", cells.join(",")))
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            rows.extend(lines);
        }
        let search: u64 = db
            .query_row("SELECT count(*) FROM search", [], |r| r.get(0))
            .unwrap();
        rows.push(format!("search rows: {search}"));
        rows
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mai() -> Person {
    Person::new("p-0123456789abcdef").unwrap()
}

fn ask(query: &str) -> Ask<'_> {
    Ask {
        query: Some(query),
        day: None,
        limit: 8,
        max_bytes: 8192,
    }
}

/// The current observations of a scope: (id, text, sources, supersedes).
fn observations(store: &mut Days, person: &Person, query: &str) -> Vec<(u64, String)> {
    store
        .search(person, &ask(query), false)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == "observation")
        .map(|r| (r.id, r.text))
        .collect()
}

#[test]
fn a_stated_taste_is_kept_in_the_persons_words_linked_to_its_line() {
    let scratch = Scratch::new("kept");
    let mut store = scratch.open(true);
    let person = mai();
    let line = store
        .append(
            &person,
            NOW,
            "Oh, I love coffee. What time is it?",
            "It's ten.",
            &[],
        )
        .unwrap();
    let kept = observations(&mut store, &person, "coffee");
    assert_eq!(kept.len(), 1, "{kept:?}");
    let (id, text) = &kept[0];
    assert_eq!(text, "Oh, I love coffee.");
    let record = store.record(&person, *id).unwrap().unwrap();
    assert_eq!(record.source_ids, vec![line]);
    assert_eq!(
        record.category,
        Some(lamp_memory::tidy::Category::Preference)
    );
    assert_eq!(record.at_unix_ms, NOW);
    assert_eq!(
        store.taste_kind(&person, *id).unwrap().as_deref(),
        Some("preference")
    );
    // Offered in the next session's note without "remember".
    let note = store.session_open(&person, "s1", 1200).unwrap();
    assert!(note.note.contains("I love coffee"), "{}", note.note);
    assert!(note.source_ids.contains(id));
    // The same words again keep nothing new.
    store
        .append(&person, NOW + 1000, "I love coffee.", "Me too.", &[])
        .unwrap();
    assert_eq!(observations(&mut store, &person, "coffee").len(), 1);

    // Vietnamese, and a dislike.
    store
        .append(
            &person,
            NOW + 2000,
            "Tôi ghét sầu riêng.",
            "Mình nhớ rồi.",
            &[],
        )
        .unwrap();
    let durian = observations(&mut store, &person, "sầu riêng");
    assert_eq!(durian.len(), 1, "{durian:?}");
    assert_eq!(durian[0].1, "Tôi ghét sầu riêng.");
    assert_eq!(
        store.taste_kind(&person, durian[0].0).unwrap().as_deref(),
        Some("dislike")
    );
}

#[test]
fn only_the_persons_own_stated_words_are_kept() {
    let scratch = Scratch::new("only");
    let mut store = scratch.open(true);
    let person = mai();
    for (heard, said) in [
        // The lamp's own line is never read.
        (
            "What do you like?",
            "I love coffee. I'm allergic to nothing.",
        ),
        ("Do you like coffee?", "I do."),
        ("I like coffee?", "Do you?"),
        ("My wife likes coffee.", "Nice."),
        ("Maybe I like jazz.", "Maybe."),
        ("Có lẽ tôi thích cà phê", "Ừ."),
        ("Em có thích trà không", "Có."),
        ("My sister's name is Lan.", "Lovely name."),
        // Health, diet, money and other people are never kept by rule.
        ("I'm allergic to peanuts.", "Noted."),
        ("I'm vegetarian.", "Noted."),
        ("Tôi bị dị ứng với tôm.", "Ừ."),
        ("Tôi ăn chay.", "Ừ."),
        ("I hate my medicine.", "Oh."),
        ("I love saving money.", "Nice."),
        ("I love my wife's cooking.", "Nice."),
        ("Tôi thích nấu ăn cho vợ.", "Hay quá."),
    ] {
        store.append(&person, NOW, heard, said, &[]).unwrap();
    }
    for query in [
        "coffee",
        "jazz",
        "cà phê",
        "trà",
        "Lan",
        "allergic",
        "peanuts",
        "vegetarian",
        "tôm",
        "chay",
        "medicine",
        "money",
        "cooking",
        "nấu",
    ] {
        assert_eq!(observations(&mut store, &person, query), vec![], "{query}");
    }
    // The room's (guests and unknown speakers) and a rules scope: nothing.
    let room = Person::everyone();
    store
        .append(&room, NOW, "I love coffee.", "Noted.", &[])
        .unwrap();
    store
        .append_ruled_by(
            &room,
            NOW,
            "I hate rain.",
            "Okay.",
            &[],
            &[],
            Some("p-0123456789abcdef"),
        )
        .unwrap();
    assert_eq!(observations(&mut store, &room, "coffee"), vec![]);
    assert_eq!(observations(&mut store, &room, "rain"), vec![]);
    let rules = person.rules().unwrap();
    store
        .append(&rules, NOW, "I love tea.", "Okay.", &[])
        .unwrap();
    assert_eq!(observations(&mut store, &rules, "tea"), vec![]);
}

#[test]
fn a_newer_taste_about_the_same_thing_supersedes_the_older() {
    let scratch = Scratch::new("newer");
    let mut store = scratch.open(true);
    let person = mai();
    store
        .append(&person, NOW, "I love coffee.", "Great.", &[])
        .unwrap();
    let old = observations(&mut store, &person, "coffee")[0].0;
    store
        .append(
            &person,
            NOW + 60_000,
            "I don't like coffee anymore.",
            "Oh.",
            &[],
        )
        .unwrap();
    let now = observations(&mut store, &person, "coffee");
    assert_eq!(now.len(), 1, "{now:?}");
    let (new, text) = &now[0];
    assert_eq!(text, "I don't like coffee anymore.");
    let record = store.record(&person, *new).unwrap().unwrap();
    assert_eq!(record.supersedes, Some(old));
    assert_eq!(
        store.taste_kind(&person, *new).unwrap().as_deref(),
        Some("dislike")
    );
    // The older is kept as history, inactive.
    let history = store.search(&person, &ask("coffee"), true).unwrap();
    assert!(
        history.iter().any(|r| r.id == old && !r.active),
        "{history:?}"
    );
    // Another thing is not about coffee; a favourite replaces a favourite.
    store
        .append(
            &person,
            NOW + 61_000,
            "My favourite colour is teal.",
            "Nice.",
            &[],
        )
        .unwrap();
    store
        .append(&person, NOW + 62_000, "I love tea.", "Nice.", &[])
        .unwrap();
    store
        .append(
            &person,
            NOW + 63_000,
            "My favourite colour is violet.",
            "Nice.",
            &[],
        )
        .unwrap();
    let colour = observations(&mut store, &person, "colour");
    assert_eq!(colour.len(), 1, "{colour:?}");
    assert_eq!(colour[0].1, "My favourite colour is violet.");
    assert_eq!(observations(&mut store, &person, "coffee").len(), 1);
    assert_eq!(observations(&mut store, &person, "tea").len(), 1);
    // Vietnamese: "không thích … nữa" after "thích".
    store
        .append(&person, NOW + 64_000, "Tôi thích trà sữa", "Ừ.", &[])
        .unwrap();
    store
        .append(
            &person,
            NOW + 65_000,
            "Tôi không thích trà sữa nữa",
            "Ồ.",
            &[],
        )
        .unwrap();
    let milk_tea = observations(&mut store, &person, "sữa");
    assert_eq!(milk_tea.len(), 1, "{milk_tea:?}");
    assert_eq!(milk_tea[0].1, "Tôi không thích trà sữa nữa");
}

#[test]
fn a_forget_of_the_line_or_the_observation_takes_both() {
    let scratch = Scratch::new("forget");
    let mut store = scratch.open(true);
    let person = mai();
    // Forgetting the line takes the observation, and its history.
    let first = store
        .append(&person, NOW, "I love coffee.", "Great.", &[])
        .unwrap();
    let second = store
        .append(&person, NOW + 1000, "I hate coffee.", "Oh.", &[])
        .unwrap();
    let hated = observations(&mut store, &person, "coffee")[0].0;
    store.forget(&person, second).unwrap();
    assert!(store.record(&person, hated).unwrap().is_none());
    assert_eq!(store.taste_kind(&person, hated).unwrap(), None);
    store.forget(&person, first).unwrap();
    assert!(
        store
            .search(&person, &ask("coffee"), true)
            .unwrap()
            .is_empty()
    );
    // Forgetting the observation takes its line.
    let line = store
        .append(&person, NOW + 2000, "I can't stand olives.", "Noted.", &[])
        .unwrap();
    let olives = observations(&mut store, &person, "olives")[0].0;
    store.forget(&person, olives).unwrap();
    assert!(store.record(&person, line).unwrap().is_none());
    assert!(
        store
            .search(&person, &ask("olives"), true)
            .unwrap()
            .is_empty()
    );
    // "Forget that" (the last thing said) takes it too.
    store
        .append(&person, NOW + 3000, "Tôi thích phở.", "Ngon.", &[])
        .unwrap();
    assert_eq!(observations(&mut store, &person, "phở").len(), 1);
    store.forget_last(&person, None).unwrap();
    assert!(store.search(&person, &ask("phở"), true).unwrap().is_empty());
    // Everything kept of the person.
    store
        .append(&person, NOW + 4000, "I love jazz.", "Nice.", &[])
        .unwrap();
    store.forget_all(Some(&person)).unwrap();
    assert!(
        store
            .search(&person, &ask("jazz"), true)
            .unwrap()
            .is_empty()
    );
    drop(store);
    let db = Connection::open(scratch.0.join(DATABASE)).unwrap();
    let left: u64 = db
        .query_row("SELECT count(*) FROM tastes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

/// Off (the default), the store is as before: the same lines leave the
/// same rows, byte for byte, as a store whose lines state no taste would,
/// and no side table is made.
#[test]
fn off_the_store_is_unchanged() {
    let lines = [
        "I love coffee.",
        "Tôi ghét sầu riêng.",
        "I don't like coffee anymore.",
        "My favourite colour is teal.",
    ];
    let off = Scratch::new("off");
    let mut store = off.open(false);
    for (n, heard) in lines.iter().enumerate() {
        let id = store
            .append(&mai(), NOW + n as u64, heard, "Okay.", &[])
            .unwrap();
        assert_eq!(id, n as u64 + 1);
    }
    drop(store);
    // The same appends on a store with the rules on, but lines that state
    // nothing: what the off store holds is exactly that, the text aside.
    let plain = Scratch::new("plain");
    let mut store = plain.open(true);
    for n in 0..lines.len() {
        store
            .append(&mai(), NOW + n as u64, &format!("line {n}"), "Okay.", &[])
            .unwrap();
    }
    drop(store);
    let off_rows = off.dump();
    let plain_rows: Vec<String> = plain
        .dump()
        .into_iter()
        .filter(|row| !row.starts_with("tastes"))
        .collect();
    let without_text = |rows: &[String]| -> Vec<String> {
        rows.iter()
            .map(|row| {
                row.replace("I love coffee.", "line 0")
                    .replace("Tôi ghét sầu riêng.", "line 1")
                    .replace("I don't like coffee anymore.", "line 2")
                    .replace("My favourite colour is teal.", "line 3")
            })
            .collect()
    };
    let off_rows = without_text(&off_rows);
    let plain_rows = without_text(&plain_rows);
    assert_eq!(off_rows, plain_rows);
    assert!(
        off_rows
            .iter()
            .all(|row| !row.contains("'observation'") && !row.contains("observation"))
    );
    let db = Connection::open(off.0.join(DATABASE)).unwrap();
    let tables: u64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='tastes'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}

/// The rules add little to a write: measured beside the same writes with
/// them off, over lines that each state a taste, with a store already
/// holding 200 rule-kept observations. "On" also writes two observations a
/// line (their rows, index and side rows in the same durable commit), so
/// most of its difference is that disk work, not the rules' reading. The
/// bound is loose: each write's own durable commits dominate and vary on a
/// Mac.
#[test]
fn the_rules_add_little_to_the_write() {
    let time = |rules: bool| -> Duration {
        let scratch = Scratch::new(&format!("time-{rules}"));
        let mut store = scratch.open(rules);
        let person = mai();
        for n in 0..200 {
            store
                .append(&person, NOW + n, &format!("I like thing{n}."), "Okay.", &[])
                .unwrap();
        }
        let mut times: Vec<Duration> = (0..11)
            .map(|n| {
                let began = Instant::now();
                store
                    .append(
                        &person,
                        NOW + 1000 + n,
                        &format!("I love song{n}. Tôi không thích món{n} nữa."),
                        "Okay.",
                        &[],
                    )
                    .unwrap();
                began.elapsed()
            })
            .collect();
        times.sort();
        times[times.len() / 2]
    };
    let off = time(false);
    let on = time(true);
    println!("taste rules: a write's median off {off:?}, on {on:?}");
    assert!(
        on <= off * 2 + Duration::from_millis(20),
        "off {off:?}, on {on:?}"
    );
}
