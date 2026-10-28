//! Conversation-end receipts through the real optional store, without a model.
//! Temporary synthetic data only; these are not lamp latency measurements.
use lamp_memory::{
    Options, Person,
    days::{DATABASE, Days},
    days_protocol,
    tidy::{Category, Output, Proposal},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: u64 = 1_791_892_800_000;
const PRIVATE: &str = "private_submit_fixture_must_not_be_logged";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "lamp-memory-submit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn open(&self) -> Days {
        Days::open(
            Options {
                dir: self.0.clone(),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            NOW,
        )
        .unwrap()
    }

    /// The owning Days value must have been dropped before inspection.
    fn counts(&self) -> (u64, u64, u64, u64) {
        let connection = Connection::open(self.0.join(DATABASE)).unwrap();
        connection
            .query_row(
                "SELECT (SELECT count(*) FROM people), (SELECT count(*) FROM entries), (SELECT count(*) FROM jobs), (SELECT count(*) FROM jobs WHERE state='pending')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ask(store: &mut Days, request: Value) -> Value {
    let answer = days_protocol::answer(store, &serde_json::to_vec(&request).unwrap(), NOW);
    assert!(!answer.log.contains(PRIVATE), "{}", answer.log);
    assert!(!answer.storage_fault, "{}", answer.log);
    assert!(!answer.wrote, "conversation receipts are not record writes");
    serde_json::from_str(&answer.line).unwrap()
}

fn submit(conversation: &str, sources: &[u64]) -> Value {
    json!({"op":"conversation_end","conversation":conversation,"source_ids":sources,"submit":true})
}

fn skipped() -> Value {
    json!({"ok":true,"submitted":false,"reason":"model_unconfigured"})
}

#[test]
fn unconfigured_submissions_do_not_allocate_jobs_or_fill_prepare_capacity() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    let person = Person::everyone();
    let source = store
        .append(&person, NOW, PRIVATE, "Recorded.", &[])
        .unwrap();
    for number in 0..20 {
        assert_eq!(
            ask(&mut store, submit(&format!("talk_{number}"), &[source])),
            skipped()
        );
    }
    drop(store);
    assert_eq!(scratch.counts(), (1, 1, 0, 0));

    let mut store = scratch.open();
    let prepared = ask(
        &mut store,
        json!({"op":"conversation_end","conversation":"prepare","source_ids":[source]}),
    );
    assert_eq!(prepared["ok"], true);
    assert_eq!(prepared["input"]["source_ids"], json!([source]));
    assert_eq!(prepared["input"]["records"][0]["heard"], PRIVATE);
    assert!(prepared.get("submitted").is_none());

    // Explicit false keeps the old frozen-input contract too, even after an
    // unrelated record is added. A skipped handoff does not replace that job.
    let later = store
        .append(&person, NOW, "Different later evidence.", "Yes.", &[])
        .unwrap();
    assert_eq!(ask(&mut store, submit("prepare", &[later])), skipped());
    assert_eq!(
        ask(
            &mut store,
            json!({"op":"conversation_end","conversation":"prepare","source_ids":[source],"submit":false})
        ),
        prepared
    );
    drop(store);
    assert_eq!(scratch.counts(), (1, 2, 1, 1));
}

#[test]
fn submission_checks_names_person_shape_and_finished_same_person_sources() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    let person = Person::everyone();
    let source = store.append(&person, NOW, PRIVATE, "Yes.", &[]).unwrap();
    let future = store
        .append(&person, NOW + 1_000, "Not happened yet.", "Later.", &[])
        .unwrap();
    let kept = store.remember(&person, NOW, PRIVATE).unwrap();
    let mut requests = vec![
        submit("empty", &[]),
        submit("duplicate", &[source, source]),
        submit("too_many", &(1..=65).collect::<Vec<_>>()),
        submit("zero", &[0]),
        submit("integer_range", &[u64::MAX]),
        submit("missing", &[9_999]),
        submit("future", &[future]),
        submit("not_exchange", &[kept]),
    ];
    for name in ["", "Upper", "../private", &"x".repeat(65)] {
        requests.push(submit(name, &[source]));
    }
    let mut wrong_person = submit("wrong_person", &[source]);
    wrong_person["person"] = json!("nobody");
    requests.push(wrong_person);
    for request in requests {
        let answer = ask(&mut store, request.clone());
        assert_eq!(answer["ok"], false, "{request}: {answer}");
        assert_eq!(answer["error"], "bad_request", "{request}: {answer}");
    }
    // An id the person was given and no longer has was forgotten since:
    // with none left the end closes with nothing to observe and no job
    // (N12), never refused. One never given (`missing`) is refused above.
    let gone = store
        .append(&person, NOW, "Soon forgotten.", "Yes.", &[])
        .unwrap();
    store.forget(&person, gone).unwrap();
    assert_eq!(
        ask(&mut store, submit("forgotten", &[gone])),
        json!({"ok":true,"submitted":false,"reason":"forgotten"})
    );
    for person in [Value::Null, json!("Not_a_person")] {
        let mut request = submit("bad_person", &[source]);
        request["person"] = person;
        assert_eq!(ask(&mut store, request)["error"], "bad_person");
    }
    for value in [Value::Null, json!("on"), json!(1)] {
        let mut request = submit("bad_switch", &[source]);
        request["submit"] = value;
        assert_eq!(ask(&mut store, request)["error"], "bad_request");
    }
    let mut extra = submit("unknown_field", &[source]);
    extra["instructions"] = json!(PRIVATE);
    assert_eq!(ask(&mut store, extra)["error"], "bad_request");
    drop(store);
    // Rejected requests never create a person or reserve a job.
    assert_eq!(scratch.counts(), (1, 3, 0, 0));
}

#[test]
fn selected_previous_ids_must_be_distinct_current_observations() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    let person = Person::everyone();
    let source = store.append(&person, NOW, PRIVATE, "Yes.", &[]).unwrap();
    let prepare = store.conversation_end(&person, "seed", &[source]).unwrap();
    let observed = store
        .tidy_apply(
            &person,
            &prepare.job,
            &Output {
                observations: vec![Proposal {
                    text: "A synthetic fixture preference.".into(),
                    category: Category::Preference,
                    source_ids: vec![source],
                    supersedes: None,
                    on: None,
                }],
            },
        )
        .unwrap()[0];
    let mut request = submit("with_previous", &[source]);
    request["previous_ids"] = json!([observed]);
    assert_eq!(ask(&mut store, request), skipped());
    for previous in [
        vec![source],
        vec![0],
        vec![u64::MAX],
        vec![9_999],
        vec![observed, observed],
        (1..=33).collect(),
    ] {
        let mut request = submit("bad_previous", &[source]);
        request["previous_ids"] = json!(previous);
        assert_eq!(ask(&mut store, request)["error"], "bad_request");
    }
    let correction = store
        .append(&person, NOW, "A correction.", "Changed.", &[])
        .unwrap();
    let prepare = store
        .conversation_end_with(&person, "correct", &[correction], &[observed])
        .unwrap();
    let replacement = store
        .tidy_apply(
            &person,
            &prepare.job,
            &Output {
                observations: vec![Proposal {
                    text: "The synthetic preference, a correction.".into(),
                    category: Category::Preference,
                    source_ids: vec![correction],
                    supersedes: Some(observed),
                    on: None,
                }],
            },
        )
        .unwrap()[0];
    for (previous, expected) in [(observed, false), (replacement, true)] {
        let mut request = submit("after_correction", &[correction]);
        request["previous_ids"] = json!([previous]);
        let answer = ask(&mut store, request);
        assert_eq!(answer["ok"], expected, "{answer}");
        if expected {
            assert_eq!(answer, skipped());
        }
    }
    drop(store);
    assert_eq!(scratch.counts(), (1, 4, 2, 0));
}

#[test]
fn unconfigured_submission_never_builds_the_bounded_model_payload() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    let person = Person::everyone();
    let mut sources = Vec::new();
    for _ in 0..64 {
        sources.push(
            store
                .append(&person, NOW, &"h".repeat(600), &"s".repeat(800), &[])
                .unwrap(),
        );
    }
    // These valid references would exceed the 32-KiB model-input cap. A
    // missing adapter does not need a payload or select previous text at all.
    assert_eq!(ask(&mut store, submit("large", &sources)), skipped());
    assert_eq!(
        ask(
            &mut store,
            json!({"op":"conversation_end","conversation":"prepare_large","source_ids":sources})
        )["error"],
        "bad_request"
    );
    drop(store);
    assert_eq!(scratch.counts(), (1, 64, 0, 0));
}
