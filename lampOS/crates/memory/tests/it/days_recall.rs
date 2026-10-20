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
