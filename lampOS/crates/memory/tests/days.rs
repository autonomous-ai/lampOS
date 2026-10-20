//! The optional store on a development host: provenance, forgetting, expiry,
//! snapshots and migration. No model, network connection or lamp is involved.
use lamp_memory::{
    Options, Person, Store,
    days::{DATABASE, Days, NOTE_BYTES},
    days_protocol,
    store::{Ask, Entry, Kind},
    tidy::{Category, Input, Output, Proposal},
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
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
            "lamp-days-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }

    fn options(&self, keep_days: u32) -> Options {
        Options {
            dir: self.0.clone(),
            keep_days,
            local: lamp_memory::store::LocalTime::Offset(0),
        }
    }

    fn open(&self, keep_days: u32, now: u64) -> Days {
        Days::open(self.options(keep_days), now).unwrap()
    }

    /// Called only while the Days owner has been dropped.
    fn sql(&self, statement: &str) {
        let connection = Connection::open(self.0.join(DATABASE)).unwrap();
        connection.execute_batch(statement).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(id: &str) -> Person {
    Person::new(id).unwrap()
}

fn ask(query: &str) -> Ask<'_> {
    Ask {
        query: Some(query),
        day: None,
        limit: 50,
        max_bytes: 16_384,
    }
}

fn found(store: &mut Days, person: &Person, query: &str) -> BTreeSet<u64> {
    store
        .search(person, &ask(query), false)
        .unwrap()
        .into_iter()
        .map(|record| record.id)
        .collect()
}

#[test]
fn rejected_tidy_requests_cannot_accumulate_empty_people() {
    let scratch = Scratch::new("rejected-people");
    let mut store = scratch.open(30, NOW);
    for n in 0..64 {
        let ghost = person(&format!("ghost_{n}"));
        assert!(store.conversation_end(&ghost, "missing", &[1]).is_err());
    }
    assert!(store.status(None).unwrap().is_empty());
    drop(store);
    assert!(scratch.open(30, NOW).status(None).unwrap().is_empty());
}

fn proposal(text: &str, sources: &[u64], supersedes: Option<u64>) -> Proposal {
    Proposal {
        text: text.into(),
        category: Category::Preference,
        source_ids: sources.to_vec(),
        supersedes,
    }
}

fn output(text: &str, sources: &[u64], supersedes: Option<u64>) -> Output {
    Output {
        observations: vec![proposal(text, sources, supersedes)],
    }
}

fn observe(
    store: &mut Days,
    person: &Person,
    conversation: &str,
    sources: &[u64],
    text: &str,
    supersedes: Option<u64>,
) -> u64 {
    let job = store
        .conversation_end(person, conversation, sources)
        .unwrap();
    store
        .tidy_apply(person, &job.job, &output(text, sources, supersedes))
        .unwrap()[0]
}

fn request(store: &mut Days, request: Value, now: u64) -> (Value, String) {
    let answer = days_protocol::answer(store, &serde_json::to_vec(&request).unwrap(), now);
    (serde_json::from_str(&answer.line).unwrap(), answer.log)
}

fn private_write(path: &std::path::Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

#[test]
// #2003 (the first builder's 00:27 and 00:56): an observation's evidence is
// one step back, its cited exchanges only. This test used to expect every
// ancestor gone, but an exchange's sources are what its session was shown:
// climbing them removed other facts that were only shown there.
fn forgetting_an_observation_removes_its_cited_exchanges_and_linked_descendants_only() {
    let scratch = Scratch::new("transitive");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let first = store
        .append(&room, NOW, "Coffee is black", "Noted", &[])
        .unwrap();
    let linked = store
        .append(&room, NOW, "Recall the coffee", "Black", &[first])
        .unwrap();
    let fact = observe(
        &mut store,
        &room,
        "first",
        &[linked],
        "Black coffee is preferred",
        None,
    );
    let later = store
        .append(&room, NOW, "Coffee again", "Still black", &[fact])
        .unwrap();
    let descendant = observe(
        &mut store,
        &room,
        "later",
        &[later],
        "Coffee remains black",
        None,
    );
    let unrelated = store.remember(&room, NOW, "Likes orchids").unwrap();
    let other = person("mai");
    let theirs = store.remember(&other, NOW, "Coffee with milk").unwrap();

    assert_eq!(
        store.record(&room, linked).unwrap().unwrap().source_ids,
        [first]
    );
    assert_eq!(
        store.record(&room, later).unwrap().unwrap().source_ids,
        [fact]
    );
    // The observation and the exchange it was taken from go; the fact that
    // exchange was only shown, and that fact's own evidence, stay.
    assert_eq!(store.forget(&room, descendant).unwrap(), 2);
    for id in [later, descendant] {
        assert!(store.record(&room, id).unwrap().is_none());
    }
    for id in [first, linked, fact] {
        assert!(store.record(&room, id).unwrap().is_some(), "{id}");
    }
    // Forgetting that fact in its turn takes its own cited exchange; the
    // exchange that was only shown before it stays.
    assert_eq!(store.forget(&room, fact).unwrap(), 2);
    assert!(store.record(&room, linked).unwrap().is_none());
    assert!(store.record(&room, first).unwrap().is_some());
    assert!(store.record(&room, unrelated).unwrap().is_some());
    assert!(store.record(&other, theirs).unwrap().is_some());
    assert_eq!(store.forget(&room, descendant).unwrap(), 0);
    drop(store);
    let mut reopened = scratch.open(30, NOW);
    assert_eq!(found(&mut reopened, &room, "coffee"), [first].into());
    assert!(found(&mut reopened, &other, "coffee").contains(&theirs));
}

#[test]
fn forgetting_either_supersession_version_removes_both_without_reviving_the_old_fact() {
    for forget_new in [false, true] {
        let scratch = Scratch::new("versions");
        let mut store = scratch.open(30, NOW);
        let room = Person::everyone();
        let original = store
            .append(&room, NOW, "Coffee with milk", "Noted", &[])
            .unwrap();
        let old = observe(
            &mut store,
            &room,
            "old",
            &[original],
            "Coffee with milk",
            None,
        );
        store.maintain(NOW + 1).unwrap();
        let correction = store
            .append(&room, NOW + 1, "Coffee is black now", "Changed", &[])
            .unwrap();
        let new = observe(
            &mut store,
            &room,
            "new",
            &[correction],
            "Coffee black",
            Some(old),
        );
        assert!(!store.record(&room, old).unwrap().unwrap().active);
        let new_record = store.record(&room, new).unwrap().unwrap();
        assert!(new_record.active);
        assert_eq!(new_record.supersedes, Some(old));
        let current = found(&mut store, &room, "coffee");
        assert!(current.contains(&new));
        assert!(!current.contains(&old));
        assert!(
            store
                .search(&room, &ask("coffee"), true)
                .unwrap()
                .iter()
                .any(|record| record.id == old)
        );
        assert_eq!(
            store
                .forget(&room, if forget_new { new } else { old })
                .unwrap(),
            4
        );
        assert!(found(&mut store, &room, "coffee").is_empty());
        drop(store);
        let mut reopened = scratch.open(30, NOW + 1);
        assert!(found(&mut reopened, &room, "coffee").is_empty());
    }
}

#[test]
fn ordinary_source_expiry_does_not_follow_a_replacement_to_its_independent_new_evidence() {
    let scratch = Scratch::new("expiry-versions");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let original = store
        .append(&room, NOW, "Coffee with milk", "Noted", &[])
        .unwrap();
    let old = observe(
        &mut store,
        &room,
        "old",
        &[original],
        "Coffee with milk",
        None,
    );
    store.maintain(NOW + 29 * DAY).unwrap();
    let correction = store
        .append(&room, NOW + 29 * DAY, "Coffee black now", "Changed", &[])
        .unwrap();
    let new = observe(
        &mut store,
        &room,
        "new",
        &[correction],
        "Coffee black",
        Some(old),
    );
    assert_eq!(store.maintain(NOW + 31 * DAY).unwrap(), 2);
    assert!(store.record(&room, original).unwrap().is_none());
    assert!(store.record(&room, old).unwrap().is_none());
    assert!(store.record(&room, correction).unwrap().is_some());
    assert!(store.record(&room, new).unwrap().unwrap().active);
    assert!(found(&mut store, &room, "coffee").contains(&new));
}

#[test]
fn missing_duplicate_foreign_and_nonjournal_tidy_sources_are_refused() {
    let scratch = Scratch::new("sources");
    let mut store = scratch.open(30, NOW);
    let mai = person("mai");
    let an = person("an");
    let local = store.append(&mai, NOW, "One", "Answer", &[]).unwrap();
    let only_mai = store.append(&mai, NOW, "Two", "Answer", &[]).unwrap();
    store
        .append(&an, NOW, "Other person", "Answer", &[])
        .unwrap();
    for sources in [vec![999], vec![local, local], vec![0], vec![local; 65]] {
        assert!(
            store
                .append(&mai, NOW, "Rejected exchange", "Answer", &sources)
                .is_err()
        );
        assert!(store.conversation_end(&mai, "invalid", &sources).is_err());
    }
    assert!(
        store
            .append(&an, NOW, "Foreign source", "Answer", &[only_mai])
            .is_err()
    );
    assert!(store.conversation_end(&an, "foreign", &[only_mai]).is_err());
    assert!(store.conversation_end(&mai, "empty", &[]).is_err());
    let kept = store.remember(&mai, NOW, "Kept fact").unwrap();
    assert!(store.conversation_end(&mai, "kept", &[kept]).is_err());
    let future = store
        .append(&mai, NOW + DAY, "Future", "Answer", &[])
        .unwrap();
    assert!(store.conversation_end(&mai, "future", &[future]).is_err());
    assert!(found(&mut store, &mai, "rejected").is_empty());
    assert!(store.record(&an, only_mai).unwrap().is_none());
}

#[test]
fn deleting_memory_invalidates_a_prepared_job_even_when_its_source_survives() {
    let scratch = Scratch::new("stale");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let other = store.remember(&room, NOW, "Another fact").unwrap();
    let job = store
        .conversation_end(&room, "before-delete", &[source])
        .unwrap();
    store.forget(&room, other).unwrap();
    assert!(store.record(&room, source).unwrap().is_some());
    let error = store
        .tidy_apply(&room, &job.job, &output("Coffee black", &[source], None))
        .unwrap_err();
    assert_eq!(error.code, "stale");
    assert_eq!(found(&mut store, &room, "coffee"), BTreeSet::from([source]));
    let replacement = store
        .conversation_end(&room, "after-delete", &[source])
        .unwrap();
    assert!(replacement.generation > job.generation);
    assert_ne!(replacement.job, job.job);
}

#[test]
fn delayed_model_work_retains_new_appends_and_retries_commit_only_once() {
    let scratch = Scratch::new("delayed");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let job = store.conversation_end(&room, "ended", &[source]).unwrap();
    assert_eq!(
        store.conversation_end(&room, "ended", &[source]).unwrap(),
        job
    );
    let arrived = store
        .append(&room, NOW, "Orchid on the sill", "I see", &[])
        .unwrap();
    assert!(
        store
            .conversation_end(&room, "ended", &[source, arrived])
            .is_err()
    );
    let result = output("Coffee black", &[source], None);
    let ids = store.tidy_apply(&room, &job.job, &result).unwrap();
    assert_eq!(store.tidy_apply(&room, &job.job, &result).unwrap(), ids);
    assert_eq!(
        store.record(&room, ids[0]).unwrap().unwrap().source_ids,
        [source]
    );
    assert!(store.record(&room, arrived).unwrap().is_some());
    assert_eq!(
        store
            .conversation_end(&room, "ended", &[source])
            .unwrap_err()
            .code,
        "done"
    );
    assert_eq!(
        store
            .tidy_apply(&person("someone_else"), &job.job, &result)
            .unwrap_err()
            .code,
        "stale"
    );
}

#[test]
fn a_bad_second_proposal_leaves_the_whole_batch_unwritten_and_the_job_retryable() {
    let scratch = Scratch::new("batch-validation");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let job = store.conversation_end(&room, "ended", &[source]).unwrap();
    let invalid = Output {
        observations: vec![
            proposal("First proposed observation", &[source], None),
            proposal("Second proposed observation", &[999], None),
        ],
    };
    assert!(store.tidy_apply(&room, &job.job, &invalid).is_err());
    assert!(found(&mut store, &room, "proposed").is_empty());
    let ids = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Valid observation", &[source], None),
        )
        .unwrap();
    assert_eq!(ids, [source + 1]);
}

#[test]
fn supersession_changed_after_preparation_rolls_back_an_earlier_proposal_in_the_same_batch() {
    let scratch = Scratch::new("stale-supersession");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee milk", "Noted", &[])
        .unwrap();
    let old = observe(&mut store, &room, "old", &[source], "Coffee milk", None);
    let correction = store
        .append(&room, NOW, "Coffee black now", "Noted", &[])
        .unwrap();
    let winner = store
        .conversation_end(&room, "winner", &[correction])
        .unwrap();
    let delayed = store
        .conversation_end(&room, "delayed", &[correction])
        .unwrap();
    let committed = store
        .tidy_apply(
            &room,
            &winner.job,
            &output("Coffee black", &[correction], Some(old)),
        )
        .unwrap();
    let invalid = Output {
        observations: vec![
            proposal("Partial mutation must disappear", &[correction], None),
            proposal("Obsolete replacement", &[correction], Some(old)),
        ],
    };
    assert!(store.tidy_apply(&room, &delayed.job, &invalid).is_err());
    assert!(found(&mut store, &room, "partial").is_empty());
    assert!(store.record(&room, committed[0] + 1).unwrap().is_none());
    let retry = store
        .tidy_apply(
            &room,
            &delayed.job,
            &output("Another supported fact", &[correction], None),
        )
        .unwrap();
    assert_eq!(retry, [committed[0] + 1]);
}

#[test]
fn observations_are_dated_by_their_own_cited_evidence_not_when_the_job_was_prepared() {
    let scratch = Scratch::new("evidence-dates");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let older = store
        .append(&room, NOW - 2 * DAY, "Coffee with milk", "Noted", &[])
        .unwrap();
    let newer = store
        .append(&room, NOW - DAY, "Coffee black now", "Changed", &[])
        .unwrap();
    let job = store
        .conversation_end(&room, "late-preparation", &[older, newer])
        .unwrap();
    let proposals = Output {
        observations: vec![
            proposal("An older coffee preference", &[older], None),
            proposal("Coffee preference changed", &[older, newer], None),
        ],
    };
    let ids = store.tidy_apply(&room, &job.job, &proposals).unwrap();
    assert_eq!(
        store.record(&room, ids[0]).unwrap().unwrap().at_unix_ms,
        NOW - 2 * DAY
    );
    assert_eq!(
        store.record(&room, ids[1]).unwrap().unwrap().at_unix_ms,
        NOW - DAY
    );
}

#[test]
fn a_delayed_old_conversation_cannot_supersede_a_fact_from_newer_evidence() {
    let scratch = Scratch::new("late-old-conversation");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let older = store
        .append(&room, NOW - 2 * DAY, "Coffee with milk", "Noted", &[])
        .unwrap();
    let newer = store
        .append(&room, NOW - DAY, "Coffee black now", "Changed", &[])
        .unwrap();
    let current = observe(&mut store, &room, "current", &[newer], "Coffee black", None);
    let delayed = store
        .conversation_end(&room, "old-arrived-late", &[older])
        .unwrap();
    assert!(delayed.previous.iter().any(|entry| entry.id == current));
    let replacement = output("Coffee with milk", &[older], Some(current));
    assert!(store.tidy_apply(&room, &delayed.job, &replacement).is_err());
    assert!(store.record(&room, current).unwrap().unwrap().active);
    assert!(store.record(&room, current + 1).unwrap().is_none());
    assert!(found(&mut store, &room, "black").contains(&current));
}

#[test]
fn a_relevant_old_fact_remains_available_to_correct_after_thirty_two_unrelated_observations() {
    let scratch = Scratch::new("older-relevant-prior");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let coffee = store
        .append(&room, NOW, "Coffee with milk", "Noted", &[])
        .unwrap();
    let old = observe(
        &mut store,
        &room,
        "old-coffee",
        &[coffee],
        "Coffee with milk",
        None,
    );
    for index in 0..32 {
        let source = store
            .append(&room, NOW, &format!("Orchid species {index}"), "Noted", &[])
            .unwrap();
        observe(
            &mut store,
            &room,
            &format!("orchid-{index}"),
            &[source],
            &format!("Interested in orchid species {index}"),
            None,
        );
    }
    store.maintain(NOW + 1).unwrap();
    let correction = store
        .append(&room, NOW + 1, "Coffee black now, no milk", "Changed", &[])
        .unwrap();
    let job = store
        .conversation_end(&room, "coffee-correction", &[correction])
        .unwrap();
    assert!(job.previous.len() <= 32);
    assert!(job.previous.iter().any(|prior| prior.id == old));
    let updated = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Coffee black", &[correction], Some(old)),
        )
        .unwrap();
    assert!(!store.record(&room, old).unwrap().unwrap().active);
    assert_eq!(
        store.record(&room, updated[0]).unwrap().unwrap().supersedes,
        Some(old)
    );
}

#[test]
fn explicit_previous_observations_must_exist_in_scope_and_a_prepared_selection_cannot_change() {
    let scratch = Scratch::new("explicit-priors");
    let mut store = scratch.open(30, NOW);
    let mai = person("mai");
    let an = person("an");
    let source = store
        .append(&mai, NOW, "Coffee milk", "Noted", &[])
        .unwrap();
    let old = observe(&mut store, &mai, "old", &[source], "Coffee milk", None);
    let correction = store
        .append(&mai, NOW, "Coffee black now", "Noted", &[])
        .unwrap();
    let theirs = store.append(&an, NOW, "Tea jasmine", "Noted", &[]).unwrap();
    for invalid in [
        vec![0],
        vec![999],
        vec![source],
        vec![old, old],
        vec![old; 33],
    ] {
        assert!(
            store
                .conversation_end_with(&mai, "invalid-prior", &[correction], &invalid)
                .is_err()
        );
    }
    assert!(
        store
            .conversation_end_with(&an, "foreign-prior", &[theirs], &[old])
            .is_err()
    );
    let job = store
        .conversation_end_with(&mai, "selected", &[correction], &[old])
        .unwrap();
    assert!(job.previous.iter().any(|prior| prior.id == old));
    assert_eq!(
        store
            .conversation_end_with(&mai, "selected", &[correction], &[old])
            .unwrap(),
        job
    );
    // Omitting the selector is a read of the same frozen context, not a new
    // selection. An observation created after preparation cannot be added.
    assert_eq!(
        store
            .conversation_end_with(&mai, "selected", &[correction], &[])
            .unwrap(),
        job
    );
    let later_source = store
        .append(&mai, NOW, "Likes orchids", "Noted", &[])
        .unwrap();
    let later_prior = observe(
        &mut store,
        &mai,
        "later-prior",
        &[later_source],
        "Likes orchids",
        None,
    );
    assert!(
        store
            .conversation_end_with(&mai, "selected", &[correction], &[later_prior])
            .is_err()
    );
    assert!(
        store
            .conversation_end_with(&mai, "selected", &[source], &[old])
            .is_err()
    );
    store
        .tidy_apply(
            &mai,
            &job.job,
            &output("Coffee black", &[correction], Some(old)),
        )
        .unwrap();
}

#[test]
fn protocol_accepts_explicit_prior_ids_and_freezes_them_in_the_prepared_job() {
    let scratch = Scratch::new("prior-protocol");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee milk", "Noted", &[])
        .unwrap();
    let old = observe(&mut store, &room, "old", &[source], "Coffee milk", None);
    let correction = store
        .append(&room, NOW, "Coffee black now", "Changed", &[])
        .unwrap();
    let line = json!({"op":"conversation_end","conversation":"selected","source_ids":[correction],"previous_ids":[old]});
    let (first, _) = request(&mut store, line.clone(), NOW);
    assert_eq!(first["ok"], true);
    assert_eq!(request(&mut store, line, NOW).0, first);
    let (changed, _) = request(
        &mut store,
        json!({"op":"conversation_end","conversation":"selected","source_ids":[correction],"previous_ids":[]}),
        NOW,
    );
    assert_eq!(changed, first);
    let later_source = store
        .append(&room, NOW, "Likes orchids", "Noted", &[])
        .unwrap();
    let later_prior = observe(
        &mut store,
        &room,
        "later-prior",
        &[later_source],
        "Likes orchids",
        None,
    );
    let (changed, _) = request(
        &mut store,
        json!({"op":"conversation_end","conversation":"selected","source_ids":[correction],"previous_ids":[later_prior]}),
        NOW,
    );
    assert_eq!(changed["ok"], false);
    let (invalid, _) = request(
        &mut store,
        json!({"op":"conversation_end","conversation":"invalid","source_ids":[correction],"previous_ids":[source]}),
        NOW,
    );
    assert_eq!(invalid["ok"], false);
}

#[test]
fn successful_ids_are_never_reused_after_forgetting_everything_and_restarting() {
    let scratch = Scratch::new("identities");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let first = store.remember(&room, NOW, "First").unwrap();
    let second = store.remember(&room, NOW, "Second").unwrap();
    assert_eq!(
        store
            .forget_last(&room, Some(Kind::Kept))
            .unwrap()
            .unwrap()
            .id,
        second
    );
    drop(store);
    let mut store = scratch.open(30, NOW);
    let third = store.remember(&room, NOW, "Third").unwrap();
    assert!(third > second);
    assert_eq!(store.forget_all(Some(&room)).unwrap(), 2);
    assert!(store.record(&room, first).unwrap().is_none());
    drop(store);
    let mut store = scratch.open(30, NOW);
    let fourth = store
        .append(&room, NOW, "New conversation", "Answer", &[])
        .unwrap();
    assert!(fourth > third);
    assert_eq!(store.forget_all(None).unwrap(), 1);
}

#[test]
fn ranked_search_handles_english_and_vietnamese_without_crossing_person_scope() {
    let scratch = Scratch::new("languages");
    let mut store = scratch.open(30, NOW);
    let mai = person("mai");
    let an = person("an");
    let source = store
        .append(&mai, NOW, "Mai thích cà phê đen ở Đà Nẵng", "Đã nhớ", &[])
        .unwrap();
    let fact = observe(
        &mut store,
        &mai,
        "coffee",
        &[source],
        "Thích cà phê đen",
        None,
    );
    let english = store.remember(&mai, NOW, "Prefers black coffee").unwrap();
    let other = store
        .append(&an, NOW, "An likes jasmine tea", "Noted", &[])
        .unwrap();
    for query in [
        "CÀ PHÊ ĐEN",
        "ca phe den",
        "ca\u{300} phe\u{302} đen",
        "Đà Nẵng",
    ] {
        assert!(found(&mut store, &mai, query).contains(&source), "{query}");
    }
    assert!(found(&mut store, &mai, "den").contains(&fact));
    assert!(found(&mut store, &mai, "BLACK COFFEE").contains(&english));
    assert!(found(&mut store, &mai, "jasmine").is_empty());
    assert_eq!(found(&mut store, &an, "jasmine"), BTreeSet::from([other]));
    assert!(found(&mut store, &an, "coffee ca phe").is_empty());
    for query in ["", "  \t  ", "astronaut", "a ? *", "\" OR * --"] {
        assert!(found(&mut store, &mai, query).is_empty(), "{query}");
    }
    let legacy = store.recall(&mai, &ask("ca phe coffee")).unwrap();
    assert!(
        legacy
            .iter()
            .all(|entry| matches!(entry, Entry::Exchange(_) | Entry::Kept(_)))
    );
    assert!(!legacy.iter().any(|entry| entry.id() == fact));
    assert!(!store.recall(&mai, &ask("")).unwrap().is_empty());
    assert!(
        store
            .search(&mai, &ask("coffee"), false)
            .unwrap()
            .iter()
            .all(|entry| entry.person == "mai")
    );
}

#[test]
fn session_notes_are_frozen_under_the_total_utf8_cap_and_deletion_invalidates_them() {
    let scratch = Scratch::new("notes");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    for index in 0..8 {
        store
            .remember(
                &room,
                NOW,
                &format!("Preference {index}: {}", "é".repeat(115)),
            )
            .unwrap();
    }
    let frozen = store.session_open(&room, "frozen", 420).unwrap();
    assert!(frozen.note.len() <= 420);
    assert!(!frozen.source_ids.is_empty());
    let data: Value = serde_json::from_str(&frozen.note).unwrap();
    assert_eq!(data["untrusted"], true);
    assert_eq!(data["scope"], "said in this room; speaker unknown");
    let later = store
        .remember(&room, NOW + 1, "A new small preference")
        .unwrap();
    assert_eq!(store.session_open(&room, "frozen", 420).unwrap(), frozen);
    assert!(store.session_open(&room, "frozen", 419).is_err());
    let fresh = store
        .session_open(&room, "fresh", NOTE_BYTES + 100)
        .unwrap();
    assert!(fresh.note.len() <= NOTE_BYTES);
    assert!(fresh.source_ids.contains(&later));
    for cap in [0, 1, 50, 100] {
        let note = store
            .session_open(&room, &format!("small-{cap}"), cap)
            .unwrap();
        assert!(note.note.len() <= cap);
        if !note.note.is_empty() {
            serde_json::from_str::<Value>(&note.note).unwrap();
        }
    }
    drop(store);
    let mut store = scratch.open(30, NOW);
    assert_eq!(store.session_open(&room, "frozen", 420).unwrap(), frozen);
    let other = person("mai");
    store.remember(&other, NOW, "Keeps a fern").unwrap();
    let their_note = store.session_open(&other, "frozen", 420).unwrap();
    store.forget(&room, later).unwrap();
    let invalidated = store.session_open(&room, "frozen", 420).unwrap();
    assert!(invalidated.invalidated);
    assert!(invalidated.note.is_empty());
    assert!(invalidated.source_ids.is_empty());
    assert_eq!(
        store.session_open(&other, "frozen", 420).unwrap(),
        their_note
    );
    store.session_close(&room, "frozen").unwrap();
    let reopened = store.session_open(&room, "frozen", 420).unwrap();
    assert!(!reopened.invalidated);
    assert!(!reopened.source_ids.contains(&later));
}

#[test]
fn session_and_pending_job_limits_are_bounded_and_released_without_duplicate_work() {
    let scratch = Scratch::new("bounded-work");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    for index in 0..lamp_memory::days::MAX_SESSIONS {
        store
            .session_open(&room, &format!("session-{index}"), NOTE_BYTES)
            .unwrap();
    }
    // At the limit, the oldest session is closed and the new one opens
    // (#1722, D): sessions a stopped voice left behind no longer fill it.
    let sessions = |name: &str| -> (u64, bool) {
        let db = Connection::open(scratch.0.join(DATABASE)).unwrap();
        let count = db
            .query_row(
                "SELECT count(*) FROM sessions WHERE person='everyone'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let there = db
            .query_row(
                "SELECT 1 FROM sessions WHERE person='everyone' AND session=?",
                [name],
                |r| r.get::<_, u64>(0),
            )
            .optional()
            .unwrap()
            .is_some();
        (count, there)
    };
    assert!(store.session_open(&room, "overflow", NOTE_BYTES).is_ok());
    assert_eq!(
        sessions("overflow"),
        (lamp_memory::days::MAX_SESSIONS, true)
    );
    assert!(!sessions("session-0").1);
    assert!(sessions("session-1").1);
    // A session still open is answered as it was, and closes as before.
    assert!(store.session_open(&room, "session-1", NOTE_BYTES).is_ok());
    assert_eq!(
        sessions("session-1"),
        (lamp_memory::days::MAX_SESSIONS, true)
    );
    store.session_close(&room, "session-1").unwrap();
    assert_eq!(
        sessions("session-1"),
        (lamp_memory::days::MAX_SESSIONS - 1, false)
    );
    let mut jobs = vec![];
    for index in 0..lamp_memory::days::MAX_PENDING {
        jobs.push(
            store
                .conversation_end(&room, &format!("conversation-{index}"), &[source])
                .unwrap(),
        );
    }
    assert_eq!(
        store
            .conversation_end(&room, "overflow", &[source])
            .unwrap_err()
            .code,
        "busy"
    );
    assert_eq!(
        store
            .conversation_end(&room, "conversation-0", &[source])
            .unwrap(),
        jobs[0]
    );
    store
        .tidy_apply(
            &room,
            &jobs[0].job,
            &output("Coffee black", &[source], None),
        )
        .unwrap();
    assert!(store.conversation_end(&room, "overflow", &[source]).is_ok());
}

#[test]
fn forgetting_everything_preserves_invalidated_snapshots_until_the_caller_closes_them() {
    for all_people in [false, true] {
        let scratch = Scratch::new("forget-all-notes");
        let mut store = scratch.open(30, NOW);
        let room = Person::everyone();
        let other = person("mai");
        store.remember(&room, NOW, "Coffee black").unwrap();
        store.remember(&other, NOW, "Tea jasmine").unwrap();
        store.session_open(&room, "active", NOTE_BYTES).unwrap();
        let other_note = store.session_open(&other, "active", NOTE_BYTES).unwrap();
        let scope = if all_people { None } else { Some(&room) };
        assert_eq!(
            store.forget_all(scope).unwrap(),
            if all_people { 2 } else { 1 }
        );
        let invalidated = store.session_open(&room, "active", NOTE_BYTES).unwrap();
        assert!(
            invalidated.invalidated
                && invalidated.note.is_empty()
                && invalidated.source_ids.is_empty()
        );
        let their_after = store.session_open(&other, "active", NOTE_BYTES).unwrap();
        if all_people {
            assert!(their_after.invalidated && their_after.note.is_empty());
        } else {
            assert_eq!(their_after, other_note);
        }
        let next = store
            .remember(&room, NOW, "A fresh fact after forgetting")
            .unwrap();
        assert_eq!(
            store.session_open(&room, "active", NOTE_BYTES).unwrap(),
            invalidated
        );
        drop(store);
        let mut store = scratch.open(30, NOW);
        assert_eq!(
            store.session_open(&room, "active", NOTE_BYTES).unwrap(),
            invalidated
        );
        store.session_close(&room, "active").unwrap();
        let opened = store.session_open(&room, "active", NOTE_BYTES).unwrap();
        assert!(!opened.invalidated);
        assert!(opened.source_ids.contains(&next));
    }
}

#[test]
fn forgetting_an_empty_person_still_invalidates_their_existing_empty_snapshot() {
    let scratch = Scratch::new("forget-empty-note");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    store.session_open(&room, "empty", NOTE_BYTES).unwrap();
    assert_eq!(store.forget_all(Some(&room)).unwrap(), 0);
    let invalidated = store.session_open(&room, "empty", NOTE_BYTES).unwrap();
    assert!(invalidated.invalidated && invalidated.note.is_empty());
    store
        .remember(&room, NOW, "A fact after forgetting an empty store")
        .unwrap();
    assert_eq!(
        store.session_open(&room, "empty", NOTE_BYTES).unwrap(),
        invalidated
    );
}

#[test]
fn a_full_kept_list_refuses_the_next_fact_without_evicting_an_explicit_memory() {
    let scratch = Scratch::new("kept-cap");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let mut ids = vec![];
    for index in 0..lamp_memory::store::KEPT_ENTRIES {
        ids.push(
            store
                .remember(&room, NOW, &format!("Kept preference {index}"))
                .unwrap(),
        );
    }
    assert_eq!(
        store
            .remember(&room, NOW, "A fact beyond the cap")
            .unwrap_err()
            .code,
        "full"
    );
    assert!(store.record(&room, ids[0]).unwrap().is_some());
    assert!(store.record(&room, *ids.last().unwrap()).unwrap().is_some());
    store.maintain(NOW + 365 * DAY).unwrap();
    assert_eq!(store.kept(&room, 16_384).unwrap().len(), ids.len());
}

#[test]
fn unused_observations_lapse_but_kept_facts_do_not_and_note_reads_do_not_extend_use() {
    let scratch = Scratch::new("unused");
    let mut store = scratch.open(90, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let fact = observe(&mut store, &room, "coffee", &[source], "Coffee black", None);
    let kept = store
        .remember(&room, NOW, "Coffee black forever until forgotten")
        .unwrap();
    assert_eq!(store.maintain(NOW + 30 * DAY).unwrap(), 0);
    let note = store
        .session_open(&room, "before-expiry", NOTE_BYTES)
        .unwrap();
    assert!(note.source_ids.contains(&fact));
    assert_eq!(store.maintain(NOW + 31 * DAY).unwrap(), 1);
    assert!(store.record(&room, fact).unwrap().is_none());
    assert!(store.record(&room, source).unwrap().is_some());
    assert!(store.record(&room, kept).unwrap().is_some());
    let expired = store
        .session_open(&room, "before-expiry", NOTE_BYTES)
        .unwrap();
    assert!(expired.invalidated && expired.note.is_empty());
    store.maintain(NOW + 365 * DAY).unwrap();
    assert!(store.record(&room, source).unwrap().is_none());
    assert!(store.record(&room, kept).unwrap().is_some());
}

#[test]
fn successful_retrieval_extends_observation_use_but_clock_rollback_does_not() {
    let scratch = Scratch::new("used");
    let mut store = scratch.open(90, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let fact = observe(&mut store, &room, "coffee", &[source], "Coffee black", None);
    store.maintain(NOW + 29 * DAY).unwrap();
    assert!(found(&mut store, &room, "coffee").contains(&fact));
    store.maintain(NOW).unwrap();
    assert!(found(&mut store, &room, "coffee").contains(&fact));
    assert_eq!(store.maintain(NOW + 31 * DAY).unwrap(), 0);
    assert!(store.record(&room, fact).unwrap().is_some());
    assert_eq!(store.maintain(NOW + 60 * DAY).unwrap(), 1);
    assert!(store.record(&room, fact).unwrap().is_none());
    assert!(store.record(&room, source).unwrap().is_some());
}

#[test]
fn expiring_journal_evidence_removes_even_a_recently_used_observation_and_stales_jobs() {
    let scratch = Scratch::new("source-expiry");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let fact = observe(&mut store, &room, "coffee", &[source], "Coffee black", None);
    let kept = store.remember(&room, NOW, "Keep this").unwrap();
    store.maintain(NOW + 29 * DAY).unwrap();
    assert!(found(&mut store, &room, "coffee").contains(&fact));
    let job = store.conversation_end(&room, "late", &[source]).unwrap();
    assert_eq!(store.maintain(NOW + 31 * DAY).unwrap(), 2);
    assert!(found(&mut store, &room, "coffee").is_empty());
    assert!(store.record(&room, kept).unwrap().is_some());
    assert_eq!(
        store
            .tidy_apply(&room, &job.job, &output("Coffee black", &[source], None))
            .unwrap_err()
            .code,
        "stale"
    );
}

#[test]
fn a_sqlite_delete_failure_rolls_back_entries_search_notes_and_pending_jobs() {
    let scratch = Scratch::new("delete-rollback");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let fact = observe(&mut store, &room, "coffee", &[source], "Coffee black", None);
    let linked = store
        .append(&room, NOW, "Coffee again", "Black", &[fact])
        .unwrap();
    let note = store.session_open(&room, "frozen", NOTE_BYTES).unwrap();
    let pending = store.conversation_end(&room, "pending", &[linked]).unwrap();
    let before = found(&mut store, &room, "coffee");
    drop(store);
    scratch.sql("CREATE TRIGGER refuse_delete BEFORE DELETE ON entries BEGIN SELECT RAISE(ABORT,'private_abort_payload'); END;");
    let mut store = scratch.open(30, NOW);
    let error = store.forget(&room, source).unwrap_err();
    assert_eq!(error.code, "storage");
    assert!(!error.to_string().contains("private_abort_payload"));
    assert_eq!(found(&mut store, &room, "coffee"), before);
    for id in [source, fact, linked] {
        assert!(store.record(&room, id).unwrap().is_some());
    }
    assert_eq!(
        store.session_open(&room, "frozen", NOTE_BYTES).unwrap(),
        note
    );
    assert_eq!(
        store.conversation_end(&room, "pending", &[linked]).unwrap(),
        pending
    );
    drop(store);
    scratch.sql("DROP TRIGGER refuse_delete;");
    let mut store = scratch.open(30, NOW);
    store
        .tidy_apply(
            &room,
            &pending.job,
            &output("Coffee remains black", &[linked], None),
        )
        .unwrap();
    assert_eq!(store.forget(&room, source).unwrap(), 4);
    assert!(found(&mut store, &room, "coffee").is_empty());
}

#[test]
fn a_sqlite_failure_on_the_second_proposal_rolls_back_the_first_and_its_index() {
    let scratch = Scratch::new("apply-rollback");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let source = store
        .append(&room, NOW, "Coffee black", "Noted", &[])
        .unwrap();
    let job = store.conversation_end(&room, "pending", &[source]).unwrap();
    drop(store);
    scratch.sql("CREATE TRIGGER refuse_second BEFORE INSERT ON entries WHEN NEW.kind='observation' AND NEW.text='abort-second' BEGIN SELECT RAISE(ABORT,'private_abort_payload'); END;");
    let mut store = scratch.open(30, NOW);
    let proposals = Output {
        observations: vec![
            proposal("First phantom observation", &[source], None),
            proposal("abort-second", &[source], None),
        ],
    };
    let error = store.tidy_apply(&room, &job.job, &proposals).unwrap_err();
    assert_eq!(error.code, "storage");
    assert!(!error.to_string().contains("private_abort_payload"));
    assert!(store.record(&room, source + 1).unwrap().is_none());
    assert!(found(&mut store, &room, "phantom").is_empty());
    assert_eq!(
        store.conversation_end(&room, "pending", &[source]).unwrap(),
        job
    );
    drop(store);
    scratch.sql("DROP TRIGGER refuse_second;");
    let mut store = scratch.open(30, NOW);
    let accepted = store
        .tidy_apply(&room, &job.job, &output("Coffee black", &[source], None))
        .unwrap();
    assert_eq!(accepted, [source + 1]);
}

#[test]
fn protocol_keeps_legacy_shapes_and_limits_without_logging_private_text() {
    let scratch = Scratch::new("protocol");
    let mut store = scratch.open(30, NOW);
    let secret = "unlogged_private_sentence";
    let (added, log) = request(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":secret,"said":"Answer"}),
        NOW,
    );
    assert_eq!(added, json!({"ok":true,"id":1}));
    assert!(!log.contains(secret));
    let (kept, _) = request(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":secret}),
        NOW,
    );
    assert_eq!(kept, json!({"ok":true,"id":2}));
    let (recalled, log) = request(&mut store, json!({"op":"recall","query":secret}), NOW);
    assert_eq!(
        recalled,
        json!({"ok":true,"entries":[
            {"kind":"exchange","id":1,"at_unix_ms":NOW,"heard":secret,"said":"Answer"},
            {"kind":"kept","id":2,"at_unix_ms":NOW,"text":secret}
        ]})
    );
    assert!(!log.contains(secret));
    let (kept, _) = request(&mut store, json!({"op":"kept"}), NOW);
    assert_eq!(
        kept,
        json!({"ok":true,"entries":[{"id":2,"at_unix_ms":NOW,"text":secret}]})
    );
    for bad in [
        json!({"op":secret}),
        json!({"op":"search","query":secret,"person":"../unlogged_private_sentence"}),
        json!({"op":"search","query":secret,"unexpected":secret}),
        json!({"op":"tidy_apply","job":secret,"observations":[]}),
        json!({"op":"recall","day":secret}),
    ] {
        let (answer, log) = request(&mut store, bad, NOW);
        assert_eq!(answer["ok"], false);
        assert!(!log.contains(secret));
        assert!(!answer["message"].as_str().unwrap().contains(secret));
    }
    let malformed = days_protocol::answer(&mut store, secret.as_bytes(), NOW);
    assert!(!malformed.log.contains(secret));
    let too_long = days_protocol::answer(&mut store, &vec![b'x'; 65_537], NOW);
    assert_eq!(
        serde_json::from_str::<Value>(&too_long.line).unwrap()["error"],
        "too_long"
    );
    for operation in [
        "append",
        "remember",
        "recall",
        "kept",
        "forget_last",
        "search",
        "forget",
        "session_open",
        "conversation_end",
    ] {
        let mut value = match operation {
            "append" => json!({"op":operation,"at_unix_ms":NOW,"heard":"Hello"}),
            "remember" => json!({"op":operation,"at_unix_ms":NOW,"text":"Fact"}),
            "search" => json!({"op":operation,"query":"coffee"}),
            "forget" => json!({"op":operation,"id":1}),
            "session_open" => json!({"op":operation,"session":"null"}),
            "conversation_end" => json!({"op":operation,"conversation":"null","source_ids":[1]}),
            _ => json!({"op":operation}),
        };
        value["person"] = Value::Null;
        assert_eq!(
            request(&mut store, value, NOW).0["error"],
            "bad_person",
            "{operation}"
        );
    }
    for index in 0..55 {
        request(
            &mut store,
            json!({"op":"append","at_unix_ms":NOW,"heard":"readable ".repeat(60),"said":format!("reply {index} {}", "é".repeat(500))}),
            NOW,
        );
    }
    let (limited, _) = request(
        &mut store,
        json!({"op":"recall","limit":u64::MAX,"max_bytes":u64::MAX}),
        NOW,
    );
    let entries = limited["entries"].as_array().unwrap();
    assert!(!entries.is_empty() && entries.len() <= 50);
    let mut bytes = 0;
    for entry in entries {
        let heard = entry["heard"].as_str().unwrap();
        let said = entry["said"].as_str().unwrap();
        assert!(heard.len() <= 600 && said.len() <= 800);
        bytes += heard.len() + said.len();
    }
    assert!(bytes <= 16_384);
}

#[test]
fn protocol_round_trip_records_links_prepares_and_applies_a_job_then_verifies_forgetting() {
    let scratch = Scratch::new("new-requests");
    let mut store = scratch.open(30, NOW);
    let (raw, _) = request(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"Coffee black","said":"Noted"}),
        NOW,
    );
    let first = raw["id"].as_u64().unwrap();
    let (linked, _) = request(
        &mut store,
        json!({"op":"append_linked","at_unix_ms":NOW,"heard":"Coffee?","said":"Black","source_ids":[first]}),
        NOW,
    );
    let linked = linked["id"].as_u64().unwrap();
    let (prepared, log) = request(
        &mut store,
        json!({"op":"conversation_end","conversation":"ended","source_ids":[linked]}),
        NOW,
    );
    assert!(!log.contains("Coffee"));
    let job: Input = serde_json::from_value(prepared["input"].clone()).unwrap();
    let (applied, _) = request(
        &mut store,
        json!({"op":"tidy_apply","job":job.job,"observations":[proposal("Coffee black", &[linked], None)]}),
        NOW,
    );
    let fact = applied["ids"][0].as_u64().unwrap();
    let (search, _) = request(&mut store, json!({"op":"search","query":"coffee"}), NOW);
    assert!(
        search["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == fact && entry["kind"] == "observation")
    );
    let (opened, _) = request(
        &mut store,
        json!({"op":"session_open","session":"active"}),
        NOW,
    );
    assert_eq!(opened["snapshot"]["invalidated"], false);
    // The fact and the exchange it was taken from go; the earlier exchange
    // that one was only shown stays (one step back, #2003).
    let (forgotten, _) = request(&mut store, json!({"op":"forget","id":fact}), NOW);
    assert_eq!(forgotten, json!({"ok":true,"removed":2}));
    let (search, _) = request(&mut store, json!({"op":"search","query":"coffee"}), NOW);
    let found: Vec<u64> = search["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_u64().unwrap())
        .collect();
    assert_eq!(found, [first]);
    let (invalidated, _) = request(
        &mut store,
        json!({"op":"session_open","session":"active"}),
        NOW,
    );
    assert_eq!(invalidated["snapshot"]["invalidated"], true);
    assert_eq!(invalidated["snapshot"]["note"], "");
    assert_eq!(
        request(
            &mut store,
            json!({"op":"session_close","session":"active"}),
            NOW
        )
        .0,
        json!({"ok":true})
    );
}

#[test]
fn protocol_expires_old_sources_before_returning_a_read() {
    let scratch = Scratch::new("protocol-expiry");
    let mut store = scratch.open(30, NOW);
    request(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"Old coffee","said":"Noted"}),
        NOW,
    );
    request(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"Kept orchid"}),
        NOW,
    );
    let (answer, _) = request(
        &mut store,
        json!({"op":"recall","query":"coffee"}),
        NOW + 31 * DAY,
    );
    assert_eq!(answer, json!({"ok":true,"entries":[]}));
    let (kept, _) = request(&mut store, json!({"op":"kept"}), NOW + 31 * DAY);
    assert_eq!(kept["entries"].as_array().unwrap().len(), 1);
}

#[test]
fn legacy_default_stays_jsonl_and_upgrade_removes_duplicate_files_before_serving() {
    let scratch = Scratch::new("migration");
    let room = Person::everyone();
    let mut legacy = Store::open(scratch.options(30), NOW).unwrap();
    let exchange = legacy
        .append(&room, NOW, "Cà phê đen", "Đã nhớ", NOW)
        .unwrap();
    let kept = legacy.remember(&room, NOW, "Coffee black").unwrap();
    assert!(legacy.recall(&room, &ask("ca phe")).unwrap().is_empty());
    assert!(!scratch.0.join(DATABASE).exists());
    assert!(scratch.0.join("everyone/journal-2026-10-13.jsonl").exists());
    drop(legacy);
    let directory = scratch.0.join("everyone");
    private_write(
        &directory.join("kept.jsonl.tmp"),
        b"a duplicate private fact",
    );
    private_write(&directory.join("notes.txt"), b"unrelated operator notes");
    let mut journal = OpenOptions::new()
        .append(true)
        .open(directory.join("journal-2026-10-13.jsonl"))
        .unwrap();
    journal.write_all(b"not a complete JSON record\n").unwrap();
    drop(journal);
    let mut upgraded = scratch.open(30, NOW);
    assert!(found(&mut upgraded, &room, "ca phe").contains(&exchange));
    assert!(upgraded.record(&room, kept).unwrap().is_some());
    assert_eq!(upgraded.status(Some(&room)).unwrap()[0].skipped_lines, 1);
    assert!(!directory.join("kept.jsonl").exists());
    assert!(!directory.join("kept.jsonl.tmp").exists());
    assert!(!directory.join("journal-2026-10-13.jsonl").exists());
    assert_eq!(
        fs::read(directory.join("notes.txt")).unwrap(),
        b"unrelated operator notes"
    );
    assert_eq!(
        fs::metadata(scratch.0.join(DATABASE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(upgraded);
    assert!(Store::open(scratch.options(30), NOW).is_err());
    // Imitate interruption after the import committed but before its source
    // files were removed. The committed store must win over the duplicate.
    private_write(&directory.join("kept.jsonl"), format!("{{\"id\":{kept},\"at_unix_ms\":{NOW},\"text\":\"Stale duplicate must not replace the imported fact\"}}\n").as_bytes());
    let mut reopened = scratch.open(30, NOW);
    assert_eq!(
        reopened.record(&room, kept).unwrap().unwrap().text,
        "Coffee black"
    );
    assert!(!directory.join("kept.jsonl").exists());
    assert!(reopened.remember(&room, NOW, "A new fact").unwrap() > kept);
    assert_eq!(reopened.status(Some(&room)).unwrap()[0].exchanges, 1);
}

#[test]
fn a_failed_import_rolls_back_the_database_and_preserves_all_legacy_bytes_for_retry() {
    let scratch = Scratch::new("failed-migration");
    let room = Person::everyone();
    let mut legacy = Store::open(scratch.options(30), NOW).unwrap();
    legacy
        .append(&room, NOW, "Original exchange", "Original answer", NOW)
        .unwrap();
    legacy.remember(&room, NOW, "Original kept fact").unwrap();
    drop(legacy);
    let journal = scratch.0.join("everyone/journal-2026-10-13.jsonl");
    let kept = scratch.0.join("everyone/kept.jsonl");
    let original_journal = fs::read(&journal).unwrap();
    let correct_kept = fs::read(&kept).unwrap();
    let duplicate =
        format!("{{\"id\":1,\"at_unix_ms\":{NOW},\"text\":\"Duplicate ID must abort import\"}}\n");
    private_write(&kept, duplicate.as_bytes());
    assert!(Days::open(scratch.options(30), NOW).is_err());
    assert_eq!(fs::read(&journal).unwrap(), original_journal);
    assert_eq!(fs::read(&kept).unwrap(), duplicate.as_bytes());
    let database = Connection::open(scratch.0.join(DATABASE)).unwrap();
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM entries", [], |row| row
                .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert!(
        database
            .query_row(
                "SELECT value FROM settings WHERE key='import_complete'",
                [],
                |row| row.get::<_, u64>(0)
            )
            .optional()
            .unwrap()
            .is_none()
    );
    drop(database);
    private_write(&kept, &correct_kept);
    let restored = scratch.open(30, NOW);
    let status = restored.status(Some(&room)).unwrap();
    assert_eq!((status[0].exchanges, status[0].kept), (1, 1));
    assert!(!journal.exists() && !kept.exists());
}

/// One open thread of a known person's, made at `now`, from an exchange.
fn thread(store: &mut Days, person: &Person, now: u64, text: &str) -> u64 {
    store.maintain(now).unwrap();
    let said = store.append(person, now, text, "Oh, nice", &[]).unwrap();
    let job = store
        .conversation_end(person, &format!("c{now}"), &[said])
        .unwrap();
    let mut out = output(text, &[said], None);
    out.observations[0].category = Category::Thread;
    store.tidy_apply(person, &job.job, &out).unwrap()[0]
}

/// #1693 (3): a known person's open thread is given once to ask about,
/// not on the day it was said, not when too old, at most one a day, never
/// for the room, and forgetting it removes its record of being asked.
#[test]
fn a_follow_up_is_one_open_thread_given_once_a_day_and_never_for_the_room() {
    let scratch = Scratch::new("follow-up");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let wedding = thread(&mut store, &minh, NOW, "Sister's wedding is on Saturday");
    // The same conversation, an hour on: too fresh.
    store.maintain(NOW + 3_600_000).unwrap();
    assert_eq!(store.follow_up(&minh).unwrap(), None);
    // The next morning: given, once.
    store.maintain(NOW + DAY).unwrap();
    let given = store.follow_up(&minh).unwrap().expect("one to ask about");
    assert_eq!(given["id"], wedding);
    assert_eq!(given["category"], "thread");
    assert_eq!(given["text"], "Sister's wedding is on Saturday");
    // Another open thread, old enough: not within the same day.
    let commute = thread(&mut store, &minh, NOW + DAY, "Tired of the commute");
    store.maintain(NOW + DAY + 7 * 3_600_000).unwrap();
    assert_eq!(store.follow_up(&minh).unwrap(), None);
    // A day on, the other one; the wedding is never given again.
    store.maintain(NOW + 2 * DAY + 3_600_000).unwrap();
    assert_eq!(store.follow_up(&minh).unwrap().unwrap()["id"], commute);
    store.maintain(NOW + 4 * DAY).unwrap();
    assert_eq!(store.follow_up(&minh).unwrap(), None);
    // Too old: a thread from three weeks ago isn't given.
    let mai = person("p-mai");
    thread(&mut store, &mai, NOW + 4 * DAY, "Starting a new job");
    store.maintain(NOW + 25 * DAY).unwrap();
    assert_eq!(store.follow_up(&mai).unwrap(), None);
    // Never the room's, nor a rules scope.
    assert_eq!(
        store.follow_up(&Person::everyone()).unwrap_err().code,
        "bad_person"
    );
    assert_eq!(
        store.follow_up(&person("r-minh")).unwrap_err().code,
        "bad_person"
    );
    // Forgetting the thread removes its follow-up row with it.
    assert!(store.forget(&minh, wedding).unwrap() > 0);
    drop(store);
    let connection = Connection::open(scratch.0.join(DATABASE)).unwrap();
    let rows: u64 = connection
        .query_row(
            "SELECT count(*) FROM follow_ups WHERE person='p-minh'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "only the commute's row is left");
}

/// The protocol's `follow_up`: answered for a person, refused for the room.
#[test]
fn the_follow_up_request_answers_for_a_person_and_refuses_the_room() {
    let scratch = Scratch::new("follow-up-protocol");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = thread(&mut store, &minh, NOW, "Sister's wedding is on Saturday");
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh"}),
        NOW + DAY,
    );
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["follow_up"]["id"], id);
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh"}),
        NOW + 3 * DAY,
    );
    assert_eq!(answer, json!({"ok":true,"follow_up":null}));
    let (answer, _) = request(&mut store, json!({"op":"follow_up"}), NOW + 3 * DAY);
    assert_eq!(answer["ok"], false, "{answer}");
}

/// #1693 (2): a pass that found nothing worth keeping, or failed, closes its
/// job with nothing kept, so jobs never wait out their day and fill the
/// bounded queue (`busy` at `MAX_PENDING`).
#[test]
fn an_empty_result_closes_the_job_with_nothing_kept_and_frees_its_place() {
    let scratch = Scratch::new("empty-result");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let mut sources = vec![];
    for n in 0..lamp_memory::days::MAX_PENDING {
        let source = store
            .append(&room, NOW, &format!("Hello {n}"), "Hi", &[])
            .unwrap();
        sources.push(source);
        store
            .conversation_end(&room, &format!("c{n}"), &[source])
            .unwrap();
    }
    let extra = store.append(&room, NOW, "One more", "Hi", &[]).unwrap();
    assert_eq!(
        store
            .conversation_end(&room, "full", &[extra])
            .unwrap_err()
            .code,
        "busy"
    );
    let first = store.conversation_end(&room, "c0", &[sources[0]]).unwrap();
    let empty = Output {
        observations: vec![],
    };
    assert_eq!(
        store.tidy_apply(&room, &first.job, &empty).unwrap(),
        Vec::<u64>::new()
    );
    assert!(
        store
            .tidy_apply(&room, &first.job, &empty)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .conversation_end(&room, "c0", &[sources[0]])
            .unwrap_err()
            .code,
        "done"
    );
    // Its place is free again, and nothing was written.
    store.conversation_end(&room, "full", &[extra]).unwrap();
    for id in found(&mut store, &room, "Hello") {
        assert_ne!(
            store.record(&room, id).unwrap().unwrap().kind,
            "observation"
        );
    }
}

/// #1693: a kept thing remembered with the conversation it came up in goes
/// with that conversation's exchanges, one step back and the person's own
/// only: an unrelated conversation, a thing only shown in it, and another
/// person's stay. Without `from` it is as before.
#[test]
fn a_kept_thing_goes_with_the_conversation_it_came_up_in_and_nothing_else() {
    let scratch = Scratch::new("kept-origins");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let kettle = store.remember(&room, NOW, "the kettle is orange").unwrap();
    let earlier = store
        .append(&room, NOW, "what's the weather", "Sunny", &[])
        .unwrap();
    let said = store
        .append(
            &room,
            NOW,
            "my violet umbrella is in the car",
            "Good to know",
            &[kettle],
        )
        .unwrap();
    let umbrella = store
        .remember_from(&room, NOW, "the violet umbrella is in the car", &[said])
        .unwrap();
    let echo = store
        .append(&room, NOW, "where's my umbrella", "In the car", &[umbrella])
        .unwrap();
    let mai = person("mai");
    let theirs = store
        .append(&mai, NOW, "my bike is blue", "Noted", &[])
        .unwrap();
    // Only the person's own exchanges may be where it came from.
    assert!(store.remember_from(&room, NOW, "x", &[theirs]).is_err());
    assert!(store.remember_from(&room, NOW, "x", &[kettle]).is_err());

    store.forget(&room, umbrella).unwrap();
    for gone in [umbrella, said, echo] {
        assert!(store.record(&room, gone).unwrap().is_none(), "{gone}");
    }
    for kept in [kettle, earlier] {
        assert!(store.record(&room, kept).unwrap().is_some(), "{kept}");
    }
    assert!(store.record(&mai, theirs).unwrap().is_some());

    // Kept without its conversation, it is as before: that stays.
    let plain_said = store
        .append(&room, NOW, "the bins go out Friday", "OK", &[])
        .unwrap();
    let bins = store
        .remember(&room, NOW, "the bins go out on Friday")
        .unwrap();
    store.forget(&room, bins).unwrap();
    assert!(store.record(&room, plain_said).unwrap().is_some());
}

/// #2003: forgetting an observation removes the exchange it was taken from,
/// never a kept thing that exchange's session was only shown.
#[test]
fn forgetting_an_observation_never_reaches_a_kept_thing_its_evidence_was_only_shown() {
    let scratch = Scratch::new("observation-shown");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let kettle = store.remember(&room, NOW, "the kettle is orange").unwrap();
    // An exchange in a session that was shown the kettle: linked to it.
    let said = store
        .append(
            &room,
            NOW,
            "I take my tea with no sugar",
            "Noted",
            &[kettle],
        )
        .unwrap();
    let job = store.conversation_end(&room, "c", &[said]).unwrap();
    let ids = store
        .tidy_apply(&room, &job.job, &output("Tea with no sugar", &[said], None))
        .unwrap();
    store.forget(&room, ids[0]).unwrap();
    assert!(
        store.record(&room, said).unwrap().is_none(),
        "its evidence goes"
    );
    assert!(
        store.record(&room, kettle).unwrap().is_some(),
        "the kettle was only shown, never evidence"
    );
}
