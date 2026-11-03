//! #2176, recall's second version: a question's common words match
//! nothing, a span of days is asked, and a day finds what was kept on it.
//! Only a request that asks for it (`recall_v2`, `day_from`/`day_to`)
//! changes; the same requests without those fields answer as before.
use lamp_memory::{
    Options, Person,
    days::Days,
    days_protocol,
    store::{Day, LocalTime},
};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: u64 = 1_791_892_800_000;
const DAY: u64 = 86_400_000;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-recall-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
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

fn send(store: &mut Days, request: Value) -> Value {
    let answer = days_protocol::answer(store, &serde_json::to_vec(&request).unwrap(), NOW);
    serde_json::from_str(&answer.line).unwrap()
}

fn ids(answer: &Value) -> Vec<u64> {
    answer["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("{answer}"))
        .iter()
        .map(|entry| entry["id"].as_u64().unwrap())
        .collect()
}

/// The local day `days` days before now, as the memory names it.
fn day(days: u64) -> String {
    Day::of(NOW - days * DAY, 0).unwrap().to_string()
}

fn everyone() -> Person {
    Person::everyone()
}

/// An unrelated exchange that shares only "my" with the question.
fn sister(store: &mut Days) -> u64 {
    store
        .append(
            &everyone(),
            NOW - DAY,
            "my sister lives in Hue",
            "How lovely.",
            &[],
        )
        .unwrap()
}

#[test]
fn what_is_my_drink_order_does_not_match_an_exchange_on_my_alone() {
    let scratch = Scratch::new("drink");
    let mut store = scratch.open();
    let sister = sister(&mut store);
    let question = "what is my drink order";
    // Before, and still without the option: "my" alone matched it.
    for op in ["search", "recall"] {
        let plain = send(&mut store, json!({"op":op,"query":question}));
        assert_eq!(ids(&plain), [sister], "{op}: {plain}");
        let second = send(
            &mut store,
            json!({"op":op,"query":question,"recall_v2":true}),
        );
        assert_eq!(second["ok"], true, "{op}: {second}");
        assert_eq!(ids(&second), Vec::<u64>::new(), "{op}: {second}");
    }
}

#[test]
fn a_question_of_common_words_only_searches_nothing() {
    let scratch = Scratch::new("common");
    let mut store = scratch.open();
    sister(&mut store);
    store
        .remember(&everyone(), NOW - DAY, "Tôi thích trà của mẹ")
        .unwrap();
    for question in [
        "what is my",
        "What did you tell me about it?",
        "của tôi là gì",
    ] {
        for op in ["search", "recall"] {
            let second = send(
                &mut store,
                json!({"op":op,"query":question,"recall_v2":true}),
            );
            assert_eq!(ids(&second), Vec::<u64>::new(), "{op} {question}: {second}");
        }
    }
    // Without the option the same question still matches, as before.
    assert!(
        !ids(&send(
            &mut store,
            json!({"op":"search","query":"what is my"})
        ))
        .is_empty()
    );
    // Folded as the index is: a different composition of "tôi" is the same word.
    let decomposed = "to\u{302}i";
    let second = send(
        &mut store,
        json!({"op":"search","query":decomposed,"recall_v2":true}),
    );
    assert_eq!(ids(&second), Vec::<u64>::new(), "{second}");
}

#[test]
fn a_keyword_with_common_words_still_finds_its_exchange() {
    let scratch = Scratch::new("keyword");
    let mut store = scratch.open();
    sister(&mut store);
    let coffee = store
        .append(
            &everyone(),
            NOW - 2 * DAY,
            "my coffee is black",
            "Noted.",
            &[],
        )
        .unwrap();
    let tea = store
        .remember(&everyone(), NOW - 2 * DAY, "Mẹ thích trà")
        .unwrap();
    let second = send(
        &mut store,
        json!({"op":"search","query":"what is my coffee","recall_v2":true}),
    );
    assert_eq!(ids(&second), [coffee], "{second}");
    // Vietnamese: "của tôi là" are common; "trà" is the word.
    let second = send(
        &mut store,
        json!({"op":"search","query":"trà của tôi là gì","recall_v2":true}),
    );
    assert_eq!(ids(&second), [tea], "{second}");
    // An accented word that folds to a common one is not common: "chó"
    // (a dog) is not "cho" (for).
    let dog = store
        .append(&everyone(), NOW - DAY, "con chó tên Mực", "Dễ thương.", &[])
        .unwrap();
    let second = send(
        &mut store,
        json!({"op":"search","query":"chó","recall_v2":true}),
    );
    assert_eq!(ids(&second), [dog], "{second}");
}

#[test]
fn an_accented_word_that_folds_onto_a_common_one_still_matches_it() {
    // A known limit: "chó" (a dog) is kept as a key word, but the index
    // folds accents, so it also matches "cho" (for). Ranking does not
    // prefer the exact accent either: both fold to the same token, so
    // BM25 orders them by length alone. Folding before the common-word
    // check would drop "chó" from the query instead, which is worse.
    let scratch = Scratch::new("folds");
    let mut store = scratch.open();
    let dog = store
        .append(&everyone(), NOW - DAY, "con chó của tôi", "Dễ thương.", &[])
        .unwrap();
    let coffee = store
        .append(
            &everyone(),
            NOW - DAY,
            "cho tôi một ly cà phê",
            "Có ngay.",
            &[],
        )
        .unwrap();
    let second = send(
        &mut store,
        json!({"op":"search","query":"chó","recall_v2":true}),
    );
    let mut found = ids(&second);
    found.sort_unstable();
    assert_eq!(found, [dog, coffee], "{second}");
}

#[test]
fn a_span_of_seven_days_to_one_finds_three_days_ago_and_not_nine() {
    let scratch = Scratch::new("span");
    let mut store = scratch.open();
    let three = store
        .append(
            &everyone(),
            NOW - 3 * DAY,
            "the plumber fixed the sink",
            "Good.",
            &[],
        )
        .unwrap();
    store
        .append(
            &everyone(),
            NOW - 9 * DAY,
            "the plumber was late",
            "Oh no.",
            &[],
        )
        .unwrap();
    store
        .append(&everyone(), NOW, "the plumber called today", "Okay.", &[])
        .unwrap();
    let span = json!({"day_from":day(7),"day_to":day(1)});
    let mut recall = json!({"op":"recall"});
    let mut search = json!({"op":"search","query":"plumber"});
    for (key, value) in span.as_object().unwrap() {
        recall[key] = value.clone();
        search[key] = value.clone();
    }
    assert_eq!(ids(&send(&mut store, recall)), [three]);
    assert_eq!(ids(&send(&mut store, search)), [three]);
}

#[test]
fn a_thing_kept_two_days_ago_is_found_by_a_span_with_the_option() {
    let scratch = Scratch::new("kept");
    let mut store = scratch.open();
    let kept = store
        .remember(&everyone(), NOW - 2 * DAY, "the dentist is on Friday")
        .unwrap();
    let talk = store
        .append(
            &everyone(),
            NOW - 2 * DAY,
            "the dentist called",
            "Okay.",
            &[],
        )
        .unwrap();
    store
        .remember(&everyone(), NOW - 9 * DAY, "the dentist moved")
        .unwrap();
    let (from, to) = (day(7), day(1));
    // Without the option a day finds exchanges only, as before.
    let plain = send(
        &mut store,
        json!({"op":"recall","day_from":from,"day_to":to}),
    );
    assert_eq!(ids(&plain), [talk], "{plain}");
    let second = send(
        &mut store,
        json!({"op":"recall","day_from":from,"day_to":to,"recall_v2":true}),
    );
    let mut found = ids(&second);
    found.sort_unstable();
    assert_eq!(found, [kept, talk], "{second}");
    let kept_entry = second["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == kept)
        .unwrap();
    assert_eq!(kept_entry["kind"], "kept");
    let second = send(
        &mut store,
        json!({"op":"search","query":"dentist","day_from":from,"day_to":to,"recall_v2":true}),
    );
    let mut found = ids(&second);
    found.sort_unstable();
    assert_eq!(found, [kept, talk], "{second}");
    // One day, too.
    let second = send(
        &mut store,
        json!({"op":"recall","day":day(2),"recall_v2":true}),
    );
    let mut found = ids(&second);
    found.sort_unstable();
    assert_eq!(found, [kept, talk], "{second}");
}

#[test]
fn a_span_is_both_ends_oldest_first_at_most_fourteen_days_and_not_with_a_day() {
    let scratch = Scratch::new("bad-span");
    let mut store = scratch.open();
    for request in [
        json!({"op":"recall","day_from":day(14),"day_to":day(0)}),
        json!({"op":"recall","day_from":day(1),"day_to":day(7)}),
        json!({"op":"recall","day_from":day(7)}),
        json!({"op":"recall","day_to":day(1)}),
        json!({"op":"recall","day":day(2),"day_from":day(7),"day_to":day(1)}),
        json!({"op":"search","query":"x","day_from":day(20),"day_to":day(1)}),
        json!({"op":"recall","day_from":"2026-13-01","day_to":day(1)}),
    ] {
        let answer = send(&mut store, request.clone());
        assert_eq!(answer["error"], "bad_day", "{request}: {answer}");
    }
    // Fourteen days, inclusive, is a span.
    let answer = send(
        &mut store,
        json!({"op":"recall","day_from":day(14),"day_to":day(1)}),
    );
    assert_eq!(answer["ok"], true, "{answer}");
}

#[test]
fn without_the_option_answers_are_what_they_were() {
    let scratch = Scratch::new("off");
    let mut store = scratch.open();
    sister(&mut store);
    store
        .append(
            &everyone(),
            NOW - 2 * DAY,
            "my coffee is black",
            "Noted.",
            &[],
        )
        .unwrap();
    store
        .remember(&everyone(), NOW - 2 * DAY, "the dentist is on Friday")
        .unwrap();
    for request in [
        json!({"op":"search","query":"what is my drink order"}),
        json!({"op":"search","query":"what is my coffee","day":day(2)}),
        json!({"op":"recall","query":"what is my"}),
        json!({"op":"recall","day":day(2)}),
        json!({"op":"recall"}),
    ] {
        let before = send(&mut store, request.clone());
        let mut off = request.clone();
        off["recall_v2"] = json!(false);
        assert_eq!(send(&mut store, off), before, "{request}");
    }
}

#[test]
fn a_question_about_an_finds_an_likes_tea() {
    // "An" is a Vietnamese given name, so it is not a common word.
    let scratch = Scratch::new("an");
    let mut store = scratch.open();
    sister(&mut store);
    let tea = store
        .remember(&everyone(), NOW - DAY, "An likes tea")
        .unwrap();
    for op in ["search", "recall"] {
        let second = send(
            &mut store,
            json!({"op":op,"query":"what does An like","recall_v2":true}),
        );
        assert_eq!(ids(&second), [tea], "{op}: {second}");
    }
}

/// The voice's `LAMP_MEMORY_NEWER_FIRST` (`newer_first`, the memory
/// owner's Tiers-ideas plan, 3 Nov, adapted from memory-tiers): of what a
/// search or recall found, a later line of the person's own that corrects
/// an earlier one goes before it, and the earlier one says
/// `"earlier":true`. Only the order and that flag change.
mod newer_first {
    use super::*;

    const OLD: &str = "it is jasmine tea";
    const NEW: &str = "Actually, my tea is now peppermint, no longer jasmine";

    const MAI: &str = "p-0123456789abcdef";
    const BAO: &str = "p-00000000000b0b0b";

    fn mai() -> Person {
        Person::new(MAI).unwrap()
    }

    fn line(store: &mut Days, answer: Value) -> String {
        days_protocol::answer(store, &serde_json::to_vec(&answer).unwrap(), NOW).line
    }

    fn earlier(answer: &Value) -> Vec<bool> {
        answer["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("{answer}"))
            .iter()
            .map(|entry| entry.get("earlier") == Some(&json!(true)))
            .collect()
    }

    #[test]
    fn newer_correction_ranks_first() {
        let scratch = Scratch::new("newer-first");
        let mut store = scratch.open();
        let old = store
            .append(&mai(), NOW - 3 * DAY, OLD, "Lovely.", &[])
            .unwrap();
        let new = store.append(&mai(), NOW - DAY, NEW, "Noted.", &[]).unwrap();
        let ask = |newer_first: bool| json!({"op":"search","person":MAI,"query":"jasmine tea","newer_first":newer_first});
        // Today: the shorter, older line ranks first on the words alone.
        let today = send(&mut store, ask(false));
        assert_eq!(ids(&today), [old, new], "{today}");
        assert_eq!(earlier(&today), [false, false], "{today}");
        let first = send(&mut store, ask(true));
        assert_eq!(ids(&first), [new, old], "{first}");
        assert_eq!(earlier(&first), [false, true], "{first}");
        // A recall keeps its time order; only the flag says it.
        let recalled = send(
            &mut store,
            json!({"op":"recall","person":MAI,"query":"jasmine tea","newer_first":true}),
        );
        assert_eq!(ids(&recalled), [old, new], "{recalled}");
        assert_eq!(earlier(&recalled), [true, false], "{recalled}");
    }

    #[test]
    fn no_cue_keeps_fts_rank() {
        let scratch = Scratch::new("no-cue");
        let mut store = scratch.open();
        store
            .append(&mai(), NOW - 3 * DAY, OLD, "Lovely.", &[])
            .unwrap();
        store
            .append(
                &mai(),
                NOW - DAY,
                "my mother brought jasmine tea and cake",
                "Nice.",
                &[],
            )
            .unwrap();
        // A cue with only one shared word ("tea") is no correction either.
        store
            .append(&mai(), NOW - DAY / 2, "actually I want tea", "Sure.", &[])
            .unwrap();
        let ask = |field: Option<bool>| {
            let mut ask = json!({"op":"search","person":MAI,"query":"jasmine tea"});
            if let Some(on) = field {
                ask["newer_first"] = json!(on);
            }
            ask
        };
        let today = send(&mut store, ask(None));
        let on = send(&mut store, ask(Some(true)));
        assert_eq!(ids(&on), ids(&today), "{on}");
        assert_eq!(earlier(&on), vec![false; ids(&on).len()], "{on}");
    }

    #[test]
    fn other_person_never_reordered_in() {
        let scratch = Scratch::new("other-person");
        let mut store = scratch.open();
        let bao = Person::new(BAO).unwrap();
        let old = store
            .append(&mai(), NOW - 3 * DAY, OLD, "Lovely.", &[])
            .unwrap();
        store.append(&bao, NOW - DAY, NEW, "Noted.", &[]).unwrap();
        store
            .append(&everyone(), NOW - DAY, NEW, "Noted.", &[])
            .unwrap();
        for op in ["search", "recall"] {
            let on = send(
                &mut store,
                json!({"op":op,"person":MAI,"query":"jasmine tea","newer_first":true}),
            );
            assert_eq!(ids(&on), [old], "{op}: {on}");
            assert_eq!(earlier(&on), [false], "{op}: {on}");
        }
    }

    /// The room's set is many speakers' lines: one's correction never
    /// marks another's `earlier`, so `newer_first` is ignored there (the
    /// Reviewer-lead's HOLD on #5049: A's "my jasmine tea", then B's
    /// "actually my tea is now peppermint, no longer jasmine").
    #[test]
    fn room_scope_is_never_reordered() {
        let scratch = Scratch::new("room-scope");
        let mut store = scratch.open();
        store
            .append(&everyone(), NOW - 3 * DAY, "my jasmine tea", "Lovely.", &[])
            .unwrap();
        store
            .append(
                &everyone(),
                NOW - DAY,
                "actually my tea is now peppermint, no longer jasmine",
                "Noted.",
                &[],
            )
            .unwrap();
        for op in ["search", "recall"] {
            let ask = |newer_first: Option<bool>| {
                let mut ask = json!({"op":op,"query":"jasmine tea"});
                if let Some(on) = newer_first {
                    ask["newer_first"] = json!(on);
                }
                ask
            };
            let off = send(&mut store, ask(Some(false)));
            let on = send(&mut store, ask(Some(true)));
            assert_eq!(ids(&off).len(), 2, "{op}: {off}");
            assert_eq!(ids(&on), ids(&off), "{op}: {on}");
            assert_eq!(earlier(&on), [false, false], "{op}: {on}");
            assert_eq!(on, off, "{op}");
        }
    }

    #[test]
    fn off_is_byte_identical() {
        let scratch = Scratch::new("off-identical");
        let mut store = scratch.open();
        store
            .append(&mai(), NOW - 3 * DAY, OLD, "Lovely.", &[])
            .unwrap();
        store.append(&mai(), NOW - DAY, NEW, "Noted.", &[]).unwrap();
        store
            .remember(&mai(), NOW - 2 * DAY, "my tea is jasmine")
            .unwrap();
        for base in [
            json!({"op":"search","person":MAI,"query":"jasmine tea"}),
            json!({"op":"search","person":MAI,"query":"jasmine tea","history":true,"limit":2}),
            json!({"op":"search","person":MAI,"query":"jasmine tea","recall_v2":true,"day_from":day(4),"day_to":day(0)}),
            json!({"op":"recall","person":MAI,"query":"jasmine tea","recall_v2":true}),
        ] {
            let absent = line(&mut store, base.clone());
            let mut off = base.clone();
            off["newer_first"] = json!(false);
            assert_eq!(line(&mut store, off), absent, "{base}");
            assert!(!absent.contains("earlier"), "{absent}");
        }
        // The legacy recall, with no new field, is answered as it was: the
        // same line as before this switch, by the legacy path.
        let legacy = json!({"op":"recall","person":MAI,"query":"jasmine tea"});
        let answer = line(&mut store, legacy.clone());
        assert!(!answer.contains("earlier"), "{answer}");
        assert_eq!(line(&mut store, legacy), answer);
    }
}
