//! The optional store on a development host: provenance, forgetting, expiry,
//! snapshots and migration. No model, network connection or lamp is involved.
use lamp_memory::{
    Options, Person, Store,
    days::{DATABASE, Days, FOLLOW_UP_OFFERS, NOTE_BYTES},
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
        replaces: None,
        on: None,
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
        .append(&room, NOW, "Recall the black coffee", "Black", &[first])
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
        .append(
            &room,
            NOW,
            "Coffee again, still black",
            "Still black",
            &[fact],
        )
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
            proposal("First proposed coffee observation", &[source], None),
            proposal("Second proposed observation", &[999], None),
        ],
    };
    assert!(store.tidy_apply(&room, &job.job, &invalid).is_err());
    assert!(found(&mut store, &room, "proposed").is_empty());
    let ids = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Valid coffee observation", &[source], None),
        )
        .unwrap();
    assert_eq!(ids, [source + 1]);
}

/// Finding M on #3504 (lamp-4ace and lamp-06c1, 27 Oct): what only the
/// lamp said is never kept as theirs. "How do I take my coffee?" answered
/// "Green tea's more my sort of thing" keeps no "prefers green tea"; the
/// same proposal from their own "I like green tea" is kept; in one batch
/// only the one not from the lamp's words is; theirs in Vietnamese or in
/// other words is kept (the reviewer-lead's P2 on #3634); a recall's echo
/// with the lamp's distinctive word is not, nor a restatement of what is
/// kept.
#[test]
fn an_observation_never_rests_only_on_the_lamps_own_words() {
    let scratch = Scratch::new("lamp-words");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let lamp_only = store
        .append(
            &room,
            NOW,
            "How do I take my coffee?",
            "Green tea's more my sort of thing",
            &[],
        )
        .unwrap();
    let job = store.conversation_end(&room, "lamp", &[lamp_only]).unwrap();
    let kept = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Prefers green tea", &[lamp_only], None),
        )
        .unwrap();
    assert!(kept.is_empty(), "{kept:?}");
    assert!(
        found(&mut store, &room, "green")
            .iter()
            .all(|id| *id == lamp_only)
    );
    let theirs = store
        .append(&room, NOW, "I like green tea", "Nice", &[])
        .unwrap();
    let job = store.conversation_end(&room, "theirs", &[theirs]).unwrap();
    let kept = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Prefers green tea", &[theirs], None),
        )
        .unwrap();
    assert_eq!(kept.len(), 1);
    let both = store
        .append(&room, NOW, "My bike is red", "I prefer jazz myself", &[])
        .unwrap();
    let job = store.conversation_end(&room, "both", &[both]).unwrap();
    let kept = store
        .tidy_apply(
            &room,
            &job.job,
            &Output {
                observations: vec![
                    proposal("Their bike is red", &[both], None),
                    proposal("Likes jazz", &[both], None),
                ],
            },
        )
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert!(
        found(&mut store, &room, "jazz")
            .iter()
            .all(|id| *id == both)
    );
    // The reviewer-lead's P2 on #3634: their words in another language,
    // or said otherwise, are still theirs.
    for (conversation, heard, said, proposed) in [
        (
            "vietnamese",
            "Tôi thích cà phê sữa",
            "Ồ, hay quá",
            "Likes milk coffee",
        ),
        (
            "paraphrase",
            "Remind me to call mom",
            "Sure",
            "Wants to phone their mother",
        ),
    ] {
        let line = store.append(&room, NOW, heard, said, &[]).unwrap();
        let job = store
            .conversation_end(&room, conversation, &[line])
            .unwrap();
        let kept = store
            .tidy_apply(&room, &job.job, &output(proposed, &[line], None))
            .unwrap();
        assert_eq!(kept.len(), 1, "{heard}");
    }
    // The reviewer-lead's HOLD on #3649: the lamp's words said otherwise,
    // to an assent that says nothing itself, are not theirs; theirs in
    // Vietnamese is (a scope of its own, so nothing restates it).
    let vi = person("vi-speaker");
    for (who, conversation, heard, said, proposed, kept_count) in [
        (
            &room,
            "lamp-vi",
            "ừ",
            "Mình thì thích trà xanh hơn",
            "Likes green tea",
            0,
        ),
        (
            &room,
            "lamp-other-words",
            "sure",
            "Chamomile could help you sleep",
            "Drinks herbal tea at night",
            0,
        ),
        (
            &vi,
            "theirs-vi",
            "Tôi thích trà xanh",
            "Ồ, hay quá",
            "Likes green tea",
            1,
        ),
    ] {
        let line = store.append(who, NOW, heard, said, &[]).unwrap();
        let job = store.conversation_end(who, conversation, &[line]).unwrap();
        let kept = store
            .tidy_apply(who, &job.job, &output(proposed, &[line], None))
            .unwrap();
        assert_eq!(kept.len(), kept_count, "{heard} / {said}");
    }
    // lamp-06c1, 28 Oct: a recall's echo. Their question's generic words
    // were heard, "hibiscus" only in the lamp's answer: nothing kept.
    let echo = store
        .append(
            &room,
            NOW,
            "What is my favorite drink?",
            "Hibiscus tea",
            &[],
        )
        .unwrap();
    let job = store.conversation_end(&room, "echo", &[echo]).unwrap();
    let kept = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Favorite drink is hibiscus tea", &[echo], None),
        )
        .unwrap();
    assert!(kept.is_empty(), "{kept:?}");
    // Their own words saying again what is kept: nothing new kept.
    store
        .remember(&room, NOW, "Favourite drink: hibiscus tea")
        .unwrap();
    let again = store
        .append(&room, NOW, "I love hibiscus tea", "Me too", &[])
        .unwrap();
    let job = store.conversation_end(&room, "again", &[again]).unwrap();
    let kept = store
        .tidy_apply(
            &room,
            &job.job,
            &output("Favorite drink is hibiscus tea", &[again], None),
        )
        .unwrap();
    assert!(kept.is_empty(), "{kept:?}");
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
            proposal(
                "Partial coffee mutation must disappear",
                &[correction],
                None,
            ),
            proposal("Obsolete coffee replacement", &[correction], Some(old)),
        ],
    };
    assert!(store.tidy_apply(&room, &delayed.job, &invalid).is_err());
    assert!(found(&mut store, &room, "partial").is_empty());
    assert!(store.record(&room, committed[0] + 1).unwrap().is_none());
    let retry = store
        .tidy_apply(
            &room,
            &delayed.job,
            &output("Another supported coffee fact", &[correction], None),
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
            proposal("Coffee preference now black", &[older, newer], None),
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

/// #2510 (reviewer-lead P3): a replaced kept thing is history, not a slot.
/// One fact corrected five times leaves the room of one active thing, and a
/// list with 199 active things and its replaced versions still keeps one more.
#[test]
fn replaced_kept_versions_do_not_use_the_kept_lists_room() {
    let scratch = Scratch::new("kept-cap-replaced");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let mut current = store.remember(&room, NOW, "Favourite colour 0").unwrap();
    for version in 1..=5 {
        current = store
            .remember_replacing(
                &room,
                NOW,
                &format!("Favourite colour {version}"),
                &[],
                current,
                &[current],
            )
            .unwrap();
    }
    assert_eq!(store.kept(&room, 16_384).unwrap().len(), 1);
    for index in 1..lamp_memory::store::KEPT_ENTRIES - 1 {
        store
            .remember(&room, NOW, &format!("Kept preference {index}"))
            .unwrap();
    }
    // 199 active and 5 replaced: one more fits, then the list is full.
    store.remember(&room, NOW, "The last that fits").unwrap();
    assert_eq!(
        store.kept(&room, 16_384).unwrap().len(),
        lamp_memory::store::KEPT_ENTRIES
    );
    assert_eq!(
        store
            .remember(&room, NOW, "A fact beyond the cap")
            .unwrap_err()
            .code,
        "full"
    );
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
        .append(&room, NOW, "Coffee again, black", "Black", &[fact])
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
        .append(&room, NOW, "Coffee black, abort second", "Noted", &[])
        .unwrap();
    let job = store.conversation_end(&room, "pending", &[source]).unwrap();
    drop(store);
    scratch.sql("CREATE TRIGGER refuse_second BEFORE INSERT ON entries WHEN NEW.kind='observation' AND NEW.text='abort-second' BEGIN SELECT RAISE(ABORT,'private_abort_payload'); END;");
    let mut store = scratch.open(30, NOW);
    let proposals = Output {
        observations: vec![
            proposal("First phantom coffee observation", &[source], None),
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
        json!({"op":"append_linked","at_unix_ms":NOW,"heard":"Coffee black.","said":"Black","source_ids":[first]}),
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
    // that one was only shown stays (one step back, #2003). A forget with
    // no `selective` (an older voice's) is as before #3613.
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

/// Monday 2026-10-19, noon UTC (the tests' lamp keeps UTC).
const MONDAY: u64 = NOW + 6 * DAY;

/// An open thread of a known person's, said at `now`, about the day `on`.
fn dated(
    store: &mut Days,
    person: &Person,
    now: u64,
    text: &str,
    on: Option<&str>,
    category: Category,
) -> Result<u64, lamp_memory::days::Error> {
    store.maintain(now).unwrap();
    let said = store.append(person, now, text, "Oh, nice", &[]).unwrap();
    let job = store
        .conversation_end(
            person,
            &format!("e{now}-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
            &[said],
        )
        .unwrap();
    let mut out = output(text, &[said], None);
    out.observations[0].category = category;
    out.observations[0].on = on.map(str::to_owned);
    store.tidy_apply(person, &job.job, &out).map(|ids| ids[0])
}

fn wedding(store: &mut Days, person: &Person) -> u64 {
    dated(
        store,
        person,
        MONDAY,
        "Sister's wedding is on Saturday",
        Some("2026-10-24"),
        Category::Thread,
    )
    .unwrap()
}

fn event_rows(scratch: &Scratch) -> u64 {
    Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row("SELECT count(*) FROM event_days", [], |r| r.get(0))
        .unwrap()
}

/// #1693: with `events`, a wedding on Saturday, said on Monday, is not
/// asked about before it or on its day, is asked about on Sunday with its
/// day, and never again.
#[test]
fn an_event_is_asked_about_after_its_day_once() {
    let scratch = Scratch::new("event-after");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    // Tuesday, Wednesday, Friday, Saturday noon and Saturday's last minute.
    for at in [
        MONDAY + DAY,
        MONDAY + 2 * DAY,
        MONDAY + 4 * DAY,
        MONDAY + 5 * DAY,
        MONDAY + 5 * DAY + 12 * 3_600_000 - 60_000,
    ] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
    // Sunday the 25th, just after midnight.
    store.maintain(MONDAY + 5 * DAY + 12 * 3_600_000).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap(),
        Some(
            json!({"id":id,"category":"thread","text":"Sister's wedding is on Saturday","on":"2026-10-24"})
        )
    );
    for at in [MONDAY + 8 * DAY, MONDAY + 12 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
}

/// A due dated one comes before a newer undated one; the undated one is
/// given the next day, by its own rules.
#[test]
fn a_due_event_comes_before_a_newer_undated_thread() {
    let scratch = Scratch::new("event-first");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    let commute = thread(&mut store, &minh, MONDAY + 5 * DAY, "Tired of the commute");
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap().unwrap()["id"],
        id
    );
    store.maintain(MONDAY + 7 * DAY + 3_600_000).unwrap();
    let given = store.follow_up_with(&minh, true).unwrap().unwrap();
    assert_eq!(given["id"], commute);
    assert_eq!(given.get("on"), None, "an undated one has no day");
}

/// Fourteen days after its day is the last it is asked about; fifteen, never.
#[test]
fn an_event_is_not_asked_about_fifteen_days_after_it() {
    let scratch = Scratch::new("event-late");
    let mut store = scratch.open(60, NOW);
    let (minh, mai) = (person("p-minh"), person("p-mai"));
    wedding(&mut store, &minh);
    let hers = wedding(&mut store, &mai);
    // The 24th, plus 14 days: 2026-11-07, still asked about.
    store.maintain(MONDAY + 19 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&mai, true).unwrap().unwrap()["id"],
        hers
    );
    // Plus 15 days: 2026-11-08.
    store.maintain(MONDAY + 20 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&minh, true).unwrap(), None);
}

/// A dated follow-up is given only in the scope that kept it: never to
/// another person, nor to the room, even when due; once to its own.
#[test]
fn an_event_is_given_only_to_the_person_who_said_it() {
    let scratch = Scratch::new("event-scope");
    let mut store = scratch.open(30, NOW);
    let (minh, mai) = (person("p-minh"), person("p-mai"));
    let id = wedding(&mut store, &minh);
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&mai, true).unwrap(), None);
    assert_eq!(
        store
            .follow_up_with(&Person::everyone(), true)
            .unwrap_err()
            .code,
        "bad_person"
    );
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","events":true}),
        MONDAY + 6 * DAY,
    );
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap().unwrap()["id"],
        id
    );
    store.maintain(MONDAY + 8 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&minh, true).unwrap(), None);
    assert_eq!(store.follow_up_with(&mai, true).unwrap(), None);
}

fn offer_rows(scratch: &Scratch) -> u64 {
    Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row("SELECT count(*) FROM follow_up_offers", [], |r| r.get(0))
        .unwrap()
}

/// #1693 (the first builder on #2602, 22 Oct 09:4x): with `until_asked`, a
/// dated one is given without being marked asked, so a session where the
/// lamp never asked is followed by another offer on a later day, at most
/// one a day; once the voice says the lamp asked, never again. Only its own
/// person marks it, and only once.
#[test]
fn a_dated_one_given_until_asked_is_offered_again_until_marked_asked() {
    let scratch = Scratch::new("event-until-asked");
    let mut store = scratch.open(30, NOW);
    let (minh, mai) = (person("p-minh"), person("p-mai"));
    let id = wedding(&mut store, &minh);
    let offered = json!({"id":id,"category":"thread","text":"Sister's wedding is on Saturday","on":"2026-10-24","until_asked":true});
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(
        store.follow_up_until_asked(&minh, true, true).unwrap(),
        Some(offered.clone())
    );
    // The same day, a later session: one a day.
    store.maintain(MONDAY + 6 * DAY + 3_600_000).unwrap();
    assert_eq!(
        store.follow_up_until_asked(&minh, true, true).unwrap(),
        None
    );
    // The next day, not asked yet: offered again.
    store.maintain(MONDAY + 7 * DAY).unwrap();
    assert_eq!(
        store.follow_up_until_asked(&minh, true, true).unwrap(),
        Some(offered)
    );
    assert!(!store.follow_up_asked(&mai, id).unwrap(), "not hers");
    assert!(store.follow_up_asked(&minh, id).unwrap());
    assert!(!store.follow_up_asked(&minh, id).unwrap(), "once");
    for at in [MONDAY + 8 * DAY, MONDAY + 9 * DAY, MONDAY + 12 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, true, true).unwrap(),
            None,
            "{at}"
        );
    }
}

/// A dated one the lamp is never heard asking about is offered in at most
/// `FOLLOW_UP_OFFERS` sessions, each on its own day: the last is recorded
/// as given, as without `until_asked`, and carries no `until_asked`.
#[test]
fn a_dated_one_never_asked_is_offered_at_most_three_times() {
    let scratch = Scratch::new("event-offers");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    assert_eq!(FOLLOW_UP_OFFERS, 3);
    let held = json!(true);
    for (day, until_asked) in [(6, Some(&held)), (7, Some(&held)), (8, None)] {
        store.maintain(MONDAY + day * DAY).unwrap();
        let given = store
            .follow_up_until_asked(&minh, true, true)
            .unwrap()
            .expect("offered");
        assert_eq!(given["id"], id, "day {day}");
        assert_eq!(given.get("until_asked"), until_asked, "day {day}");
    }
    assert!(
        !store.follow_up_asked(&minh, id).unwrap(),
        "already recorded"
    );
    for at in [MONDAY + 9 * DAY, MONDAY + 10 * DAY, MONDAY + 15 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, true, true).unwrap(),
            None,
            "{at}"
        );
    }
}

/// #1693 (the first builder on #2602, 22 Oct 13:1x, after lamp-06c1 on 22
/// Oct: an undated thread was told at "good morning", recorded as given,
/// and never asked): with `until_asked`, an undated thread takes the dated
/// one's path, with `events` or without. Given, it is offered, not
/// recorded; the same day, nothing; the next day, offered again; once the
/// voice says the lamp asked, recorded and never again.
#[test]
fn an_undated_thread_given_until_asked_is_offered_again_until_asked() {
    for events in [false, true] {
        let scratch = Scratch::new(&format!("thread-until-asked-{events}"));
        let mut store = scratch.open(30, NOW);
        let minh = person("p-minh");
        let commute = thread(&mut store, &minh, NOW, "Tired of the commute");
        let offered = json!({"id":commute,"category":"thread","text":"Tired of the commute","until_asked":true});
        store.maintain(NOW + DAY).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, events, true).unwrap(),
            Some(offered.clone()),
            "events {events}"
        );
        assert_eq!(
            follow_up_rows(&scratch, "p-minh"),
            0,
            "told, not asked: not counted"
        );
        assert_eq!(offer_rows(&scratch), 1);
        store.maintain(NOW + DAY + 3_600_000).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, events, true).unwrap(),
            None,
            "one a day"
        );
        store.maintain(NOW + 2 * DAY).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, events, true).unwrap(),
            Some(offered),
            "the next session, offered again"
        );
        assert!(store.follow_up_asked(&minh, commute).unwrap());
        assert_eq!(follow_up_rows(&scratch, "p-minh"), 1, "asked: counted");
        for at in [NOW + 3 * DAY, NOW + 4 * DAY, NOW + 8 * DAY] {
            store.maintain(at).unwrap();
            assert_eq!(
                store.follow_up_until_asked(&minh, events, true).unwrap(),
                None,
                "{at}"
            );
        }
    }
}

/// An undated thread the lamp is never heard asking about is offered in at
/// most `FOLLOW_UP_OFFERS` sessions, each on its own day, as a dated one:
/// the last is recorded as given and carries no `until_asked`.
#[test]
fn an_undated_thread_never_asked_is_offered_at_most_three_times() {
    for events in [false, true] {
        let scratch = Scratch::new(&format!("thread-offers-{events}"));
        let mut store = scratch.open(30, NOW);
        let minh = person("p-minh");
        let commute = thread(&mut store, &minh, NOW, "Tired of the commute");
        let held = json!(true);
        for (day, until_asked) in [(1, Some(&held)), (2, Some(&held)), (3, None)] {
            store.maintain(NOW + day * DAY).unwrap();
            let given = store
                .follow_up_until_asked(&minh, events, true)
                .unwrap()
                .expect("offered");
            assert_eq!(given["id"], commute, "day {day}");
            assert_eq!(given.get("until_asked"), until_asked, "day {day}");
        }
        assert_eq!(
            follow_up_rows(&scratch, "p-minh"),
            1,
            "the last offer is recorded"
        );
        assert!(!store.follow_up_asked(&minh, commute).unwrap());
        for at in [NOW + 4 * DAY, NOW + 5 * DAY, NOW + 9 * DAY] {
            store.maintain(at).unwrap();
            assert_eq!(
                store.follow_up_until_asked(&minh, events, true).unwrap(),
                None,
                "{at}"
            );
        }
    }
}

/// Without `until_asked` (the voice's `LAMP_MEMORY_THREADS` off never asks
/// at all), an undated thread is recorded when given, as before, and no
/// offer is kept.
#[test]
fn without_until_asked_an_undated_thread_is_recorded_when_given() {
    for events in [false, true] {
        let scratch = Scratch::new(&format!("thread-not-held-{events}"));
        let mut store = scratch.open(30, NOW);
        let minh = person("p-minh");
        let commute = thread(&mut store, &minh, NOW, "Tired of the commute");
        store.maintain(NOW + DAY).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, events, false).unwrap(),
            Some(json!({"id":commute,"category":"thread","text":"Tired of the commute"}))
        );
        assert_eq!(
            (follow_up_rows(&scratch, "p-minh"), offer_rows(&scratch)),
            (1, 0)
        );
        assert!(!store.follow_up_asked(&minh, commute).unwrap());
        store.maintain(NOW + 2 * DAY).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, events, false).unwrap(),
            None
        );
    }
}

/// The protocol: `follow_up` with `until_asked` and `follow_up_asked`;
/// unknown fields refused, as every new operation's; a forget takes the
/// offer with its entry (`ON DELETE CASCADE`).
#[test]
fn the_follow_up_asked_request_marks_and_a_forget_takes_the_offer() {
    let scratch = Scratch::new("event-asked-protocol");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    let at = MONDAY + 6 * DAY;
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh","events":true,"until_asked":true}),
        at,
    );
    assert_eq!(answer["follow_up"]["until_asked"], true, "{answer}");
    assert_eq!(offer_rows(&scratch), 1);
    let (answer, log) = request(
        &mut store,
        json!({"op":"follow_up_asked","person":"p-minh","id":id,"why":"x"}),
        at,
    );
    assert_eq!(answer["ok"], false, "{answer}");
    assert!(log.contains("bad_request"), "{log}");
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up_asked","person":"p-mai","id":id}),
        at,
    );
    assert_eq!(answer, json!({"ok":true,"asked":false}));
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up_asked","person":"p-minh","id":id}),
        at,
    );
    assert_eq!(answer, json!({"ok":true,"asked":true}));
    assert!(store.forget(&minh, id).unwrap() >= 1);
    assert_eq!(offer_rows(&scratch), 0);
}

/// The first builder on #2602, 22 Oct 22:17: an offer whose session began
/// with a request is taken back (`follow_up_unoffered`): not counted, so
/// three offers are still made, each on its own day; the last one's record
/// as given goes with it. Only its own person's, only while offered; one
/// asked about is never taken back.
#[test]
fn an_offer_taken_back_is_not_counted() {
    let scratch = Scratch::new("event-unoffered");
    let mut store = scratch.open(30, NOW);
    let (minh, mai) = (person("p-minh"), person("p-mai"));
    let id = wedding(&mut store, &minh);
    assert!(
        !store.follow_up_unoffered(&minh, id).unwrap(),
        "never offered"
    );
    let held = json!(true);
    // Day 6 offered and taken back, day 7 offered and taken back: the
    // three offers are still to come, on days 8, 9 and 10.
    for day in [6, 7] {
        store.maintain(MONDAY + day * DAY).unwrap();
        let given = store.follow_up_until_asked(&minh, true, true).unwrap();
        assert_eq!(given.unwrap()["until_asked"], held, "day {day}");
        assert!(!store.follow_up_unoffered(&mai, id).unwrap(), "not hers");
        assert!(store.follow_up_unoffered(&minh, id).unwrap(), "day {day}");
        assert!(!store.follow_up_unoffered(&minh, id).unwrap(), "once");
        // Still one a day.
        store.maintain(MONDAY + day * DAY + 3_600_000).unwrap();
        assert_eq!(
            store.follow_up_until_asked(&minh, true, true).unwrap(),
            None
        );
    }
    for (day, until_asked) in [(8, Some(&held)), (9, Some(&held)), (10, None)] {
        store.maintain(MONDAY + day * DAY).unwrap();
        let given = store
            .follow_up_until_asked(&minh, true, true)
            .unwrap()
            .expect("offered");
        assert_eq!(given.get("until_asked"), until_asked, "day {day}");
    }
    // The last, recorded as given, taken back: offered again the next day,
    // and recorded again.
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up_unoffered","person":"p-minh","id":id}),
        MONDAY + 10 * DAY,
    );
    assert_eq!(answer, json!({"ok":true,"unoffered":true}));
    store.maintain(MONDAY + 11 * DAY).unwrap();
    let given = store.follow_up_until_asked(&minh, true, true).unwrap();
    assert_eq!(given.unwrap().get("until_asked"), None);
    store.maintain(MONDAY + 12 * DAY).unwrap();
    assert_eq!(
        store.follow_up_until_asked(&minh, true, true).unwrap(),
        None
    );
}

/// The first builder on #2602, 22 Oct 22:17: one the lamp asked about is
/// never taken back.
#[test]
fn an_offer_asked_about_is_not_taken_back() {
    let scratch = Scratch::new("event-unoffered-asked");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert!(
        store
            .follow_up_until_asked(&minh, true, true)
            .unwrap()
            .is_some()
    );
    store.maintain(MONDAY + 6 * DAY + 60_000).unwrap();
    assert!(store.follow_up_asked(&minh, id).unwrap());
    assert!(!store.follow_up_unoffered(&minh, id).unwrap());
    store.maintain(MONDAY + 7 * DAY).unwrap();
    assert_eq!(
        store.follow_up_until_asked(&minh, true, true).unwrap(),
        None
    );
}

/// #2894 (a read): a kept thing said again for a later day (`once`) loses
/// its offers with its day, so the new day is offered afresh, the full
/// `FOLLOW_UP_OFFERS` times; kept, a third offer would be its last.
#[test]
fn a_re_dated_event_is_offered_afresh() {
    let scratch = Scratch::new("event-redated-offers");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    store.maintain(MONDAY).unwrap();
    let (id, _) = store
        .remember_on(
            &minh,
            MONDAY,
            "Dentist on Friday",
            &[],
            true,
            Some("2026-10-23"),
        )
        .unwrap();
    for day in [5, 6] {
        store.maintain(MONDAY + day * DAY).unwrap();
        let given = store
            .follow_up_until_asked(&minh, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            (given["id"].as_u64(), &given["until_asked"]),
            (Some(id), &json!(true))
        );
    }
    store.maintain(MONDAY + 6 * DAY + 3_600_000).unwrap();
    let (again, existing) = store
        .remember_on(
            &minh,
            MONDAY + 6 * DAY + 3_600_000,
            "Dentist on Friday",
            &[],
            true,
            Some("2026-10-30"),
        )
        .unwrap();
    assert_eq!((again, existing), (id, true));
    assert_eq!(offer_rows(&scratch), 0);
    store.maintain(MONDAY + 12 * DAY).unwrap();
    let given = store
        .follow_up_until_asked(&minh, true, true)
        .unwrap()
        .unwrap();
    assert_eq!(given["on"], "2026-10-30");
    assert_eq!(given["until_asked"], true, "offered afresh");
}

/// #2894 (a read): an offer adopted with its entry is the new person's:
/// theirs to mark, never the room's.
#[test]
fn an_adopted_offer_is_the_new_persons_to_mark() {
    let scratch = Scratch::new("event-adopted-offer");
    let mut store = scratch.open(30, NOW);
    let (room, minh) = (Person::everyone(), person("p-minh"));
    store.maintain(MONDAY).unwrap();
    let (id, _) = store
        .remember_on(
            &room,
            MONDAY,
            "Dentist on Friday",
            &[],
            false,
            Some("2026-10-23"),
        )
        .unwrap();
    // The room is never offered one; an offer is written here to see it move.
    Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .execute(
            "INSERT INTO follow_up_offers(rid,person,offers,offered_unix_ms) SELECT rid,'everyone',1,?2 FROM entries WHERE person='everyone' AND id=?1",
            rusqlite::params![id, MONDAY],
        )
        .unwrap();
    let moved = store.adopt(&room, &minh, &[id]).unwrap();
    let new = moved.iter().find(|(old, _)| *old == id).unwrap().1;
    let whose: String = Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row("SELECT person FROM follow_up_offers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(whose, "p-minh");
    assert!(!store.follow_up_asked(&room, id).unwrap());
    assert!(store.follow_up_asked(&minh, new).unwrap());
}

/// The event day kept for observation `id` of `person`'s, if any; `None`
/// also when there is no such observation, so callers check it is kept.
fn event_day_of(scratch: &Scratch, person: &Person, id: u64) -> Option<String> {
    let connection = Connection::open(scratch.0.join(DATABASE)).unwrap();
    let kept: u64 = connection
        .query_row(
            "SELECT count(*) FROM entries WHERE person=?1 AND id=?2 AND kind='observation'",
            rusqlite::params![person.as_str(), id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "observation {id} is kept");
    connection
        .query_row(
            "SELECT d.day FROM entries e JOIN event_days d ON d.rid=e.rid WHERE e.person=?1 AND e.id=?2",
            rusqlite::params![person.as_str(), id],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

/// Whether `person` still has observation `id`.
fn has_observation(scratch: &Scratch, person: &Person, id: u64) -> bool {
    Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM entries WHERE person=?1 AND id=?2 AND kind='observation'",
            rusqlite::params![person.as_str(), id],
            |r| r.get::<_, u64>(0),
        )
        .unwrap()
        == 1
}

/// `on` that is not a day, is before its evidence's day, is more than 60
/// days after it, or is on a preference is dropped: the observation is
/// kept, undated, never the whole result refused.
#[test]
fn a_bad_event_day_is_dropped_and_the_observation_kept() {
    let scratch = Scratch::new("event-dropped");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    for (on, category) in [
        ("Saturday", Category::Thread),
        ("2026-02-30", Category::Thread),
        ("2026-10-24 ", Category::Promise),
        ("2026-10-18", Category::Thread),
        ("2026-12-19", Category::Promise),
        ("2026-10-24", Category::Preference),
    ] {
        let id = dated(&mut store, &minh, MONDAY, "Wedding", Some(on), category).unwrap();
        assert_eq!(event_day_of(&scratch, &minh, id), None, "{on}");
    }
    assert_eq!(event_rows(&scratch), 0);
    // The evidence's own day and 60 days after it are days.
    for on in ["2026-10-19", "2026-12-18"] {
        let id = dated(
            &mut store,
            &minh,
            MONDAY,
            "Wedding",
            Some(on),
            Category::Promise,
        )
        .unwrap();
        assert_eq!(event_day_of(&scratch, &minh, id).as_deref(), Some(on));
    }
    assert_eq!(event_rows(&scratch), 2);
    // Over the protocol, `on` beside the proposal's other fields.
    let said = store
        .append(&minh, MONDAY, "Exam on Friday", "Good luck", &[])
        .unwrap();
    let job = store.conversation_end(&minh, "exam", &[said]).unwrap();
    let (answer, _) = request(
        &mut store,
        json!({"op":"tidy_apply","person":"p-minh","job":job.job,"observations":[{"text":"Exam on Friday","category":"thread","source_ids":[said],"supersedes":null,"on":"2026-10-23"}]}),
        MONDAY,
    );
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(event_rows(&scratch), 3);
}

/// "My exam was yesterday, it went badly", dated yesterday by the model:
/// before its evidence's day, so the day is dropped, and the observation
/// and the rest of its conversation's batch are kept.
#[test]
fn a_past_event_day_keeps_the_observation_and_its_batch() {
    let scratch = Scratch::new("event-past");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    store.maintain(MONDAY).unwrap();
    let said = store
        .append(
            &minh,
            MONDAY,
            "My exam was yesterday, it went badly. My sister's wedding is on Saturday. I take my coffee black",
            "Oh no, sorry",
            &[],
        )
        .unwrap();
    let job = store.conversation_end(&minh, "exam-past", &[said]).unwrap();
    let mut out = output("Exam yesterday went badly", &[said], None);
    out.observations[0].category = Category::Thread;
    out.observations[0].on = Some("2026-10-18".into());
    let mut wedding = proposal("Sister's wedding is on Saturday", &[said], None);
    wedding.category = Category::Thread;
    wedding.on = Some("2026-10-24".into());
    out.observations.push(wedding);
    out.observations
        .push(proposal("Coffee black", &[said], None));
    let ids = store.tidy_apply(&minh, &job.job, &out).unwrap();
    assert_eq!(ids.len(), 3, "the whole batch is kept");
    assert_eq!(event_day_of(&scratch, &minh, ids[0]), None);
    assert_eq!(
        event_day_of(&scratch, &minh, ids[1]).as_deref(),
        Some("2026-10-24")
    );
    assert_eq!(event_day_of(&scratch, &minh, ids[2]), None);
    assert_eq!(event_rows(&scratch), 1, "no row for yesterday's exam");
}

/// An event 50 days out, never searched for, outlives the 30 unused days
/// and its exchange's expiry, is asked about the day after it, and expires
/// as usual once its follow-up window is over, its event day with it.
#[test]
fn an_event_far_off_is_kept_to_its_follow_up_window() {
    let scratch = Scratch::new("event-kept");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    // Monday 2026-10-19 plus 50 days: Tuesday 2026-12-08.
    let id = dated(
        &mut store,
        &minh,
        MONDAY,
        "Moving house on 8 December",
        Some("2026-12-08"),
        Category::Thread,
    )
    .unwrap();
    let undated = thread(&mut store, &minh, MONDAY, "Tired of the commute");
    store.maintain(MONDAY + 40 * DAY).unwrap();
    assert!(has_observation(&scratch, &minh, id), "kept for its day");
    assert!(
        !has_observation(&scratch, &minh, undated),
        "unused, expired"
    );
    assert_eq!(event_rows(&scratch), 1);
    // Its day, then the day after it.
    store.maintain(MONDAY + 50 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&minh, true).unwrap(), None);
    store.maintain(MONDAY + 51 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap().unwrap()["id"],
        id
    );
    // 14 days after its day, the last of its window: still kept.
    store.maintain(MONDAY + 64 * DAY).unwrap();
    assert!(has_observation(&scratch, &minh, id));
    // 15 days after: expired as unused, and its event day goes with it.
    store.maintain(MONDAY + 65 * DAY).unwrap();
    assert!(!has_observation(&scratch, &minh, id));
    assert_eq!(event_rows(&scratch), 0, "expiry clears event_days");
}

/// Without `events` (the switch turned off later), a dated thread is not
/// asked about before its day is over; an undated one keeps its rules; the
/// dated one is given after its day, with no `on`, and never again.
#[test]
fn without_events_an_event_is_not_asked_about_before_its_day() {
    let scratch = Scratch::new("event-off");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    let commute = thread(&mut store, &minh, MONDAY, "Tired of the commute");
    // Tuesday: the undated one, never the wedding to come.
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh"}),
        MONDAY + DAY,
    );
    assert_eq!(answer["follow_up"]["id"], commute, "{answer}");
    // Friday and Saturday, the day itself: nothing.
    for at in [MONDAY + 4 * DAY, MONDAY + 5 * DAY] {
        let (answer, _) = request(
            &mut store,
            json!({"op":"follow_up","person":"p-minh","events":false}),
            at,
        );
        assert_eq!(answer, json!({"ok":true,"follow_up":null}), "{at}");
    }
    // Sunday: given by the old rules, with no `on`.
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh","events":false}),
        MONDAY + 6 * DAY,
    );
    assert_eq!(
        answer,
        json!({"ok":true,"follow_up":{"id":id,"category":"thread","text":"Sister's wedding is on Saturday"}})
    );
    // With `events` over the protocol: it was given already.
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh","events":true}),
        MONDAY + 8 * DAY,
    );
    assert_eq!(answer, json!({"ok":true,"follow_up":null}));
}

/// Forgetting by id, everything, or the person leaves no event day.
#[test]
fn forgetting_leaves_no_event_day() {
    let scratch = Scratch::new("event-forget");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = wedding(&mut store, &minh);
    assert_eq!(event_rows(&scratch), 1);
    store.forget(&minh, id).unwrap();
    assert_eq!(event_rows(&scratch), 0, "forget");
    wedding(&mut store, &minh);
    store.forget_all(Some(&minh)).unwrap();
    assert_eq!(event_rows(&scratch), 0, "forget_all");
    wedding(&mut store, &minh);
    store.forget_person(&minh).unwrap();
    assert_eq!(event_rows(&scratch), 0, "forget_person");
}

/// A kept thing remembered over the protocol at `at`, with `on` when
/// given: its id.
fn kept_on(store: &mut Days, person: &str, at: u64, text: &str, on: Option<&str>) -> u64 {
    let mut line = json!({"op":"remember","person":person,"at_unix_ms":at,"text":text});
    if let Some(on) = on {
        line["on"] = json!(on);
    }
    let (answer, _) = request(store, line, at);
    assert_eq!(answer["ok"], true, "{answer}");
    answer["id"].as_u64().unwrap()
}

const KEPT_WEDDING: &str = "My sister's wedding is on Saturday.";

/// #1693, `remember {on}`: a kept wedding on Saturday, said on Monday, is
/// not given before Sunday, is given once on Sunday with its day, and
/// never again.
#[test]
fn a_kept_thing_with_a_day_is_given_once_the_day_after_it() {
    let scratch = Scratch::new("kept-on");
    let mut store = scratch.open(30, MONDAY);
    let minh = person("p-minh");
    let id = kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    assert_eq!(event_rows(&scratch), 1);
    for at in [MONDAY + DAY, MONDAY + 4 * DAY, MONDAY + 5 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap(),
        Some(json!({"id":id,"category":"kept","text":KEPT_WEDDING,"on":"2026-10-24"}))
    );
    for at in [MONDAY + 7 * DAY + 3_600_000, MONDAY + 12 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
    // Without `events` a kept thing is never given, dated or not.
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-minh"}),
        MONDAY + 13 * DAY,
    );
    assert_eq!(answer, json!({"ok":true,"follow_up":null}));
}

/// Kept without a day, or with a day the store can't take (past, over 60
/// days out, not a date), it is kept undated and never given.
#[test]
fn a_kept_thing_without_a_good_day_is_never_given() {
    let scratch = Scratch::new("kept-undated");
    let mut store = scratch.open(30, MONDAY);
    let minh = person("p-minh");
    for on in [
        None,
        Some("2026-10-18"),
        Some("2026-12-19"),
        Some("2026-02-30"),
        Some("Saturday"),
    ] {
        kept_on(&mut store, "p-minh", MONDAY, KEPT_WEDDING, on);
    }
    assert_eq!(event_rows(&scratch), 0);
    for at in [
        MONDAY + DAY,
        MONDAY + 2 * DAY,
        MONDAY + 6 * DAY,
        MONDAY + 10 * DAY,
    ] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
    // The furthest day it takes is 60 days out.
    kept_on(
        &mut store,
        "p-minh",
        MONDAY + 10 * DAY,
        "Exam",
        Some("2026-12-28"),
    );
    assert_eq!(event_rows(&scratch), 1);
}

/// Replaced by a correction (#2510), its day is gone with it and nothing is
/// given; a correction with its own day is given after that day.
#[test]
fn a_replaced_kept_thing_loses_its_day() {
    let scratch = Scratch::new("kept-replaced");
    let mut store = scratch.open(30, MONDAY);
    let minh = person("p-minh");
    let old = kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    let (answer, _) = request(
        &mut store,
        json!({"op":"remember","person":"p-minh","at_unix_ms":MONDAY + DAY,"text":"My sister's wedding was called off.","replaces":old,"shown":[old]}),
        MONDAY + DAY,
    );
    assert_eq!(answer["replaced"], old, "{answer}");
    assert_eq!(event_rows(&scratch), 0, "the replaced one's day is gone");
    for at in [MONDAY + 6 * DAY, MONDAY + 8 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
    // A correction to a new day keeps that day, the old one's gone.
    let first = kept_on(
        &mut store,
        "p-minh",
        MONDAY + 8 * DAY,
        "Dentist on Friday.",
        Some("2026-10-30"),
    );
    let (answer, _) = request(
        &mut store,
        json!({"op":"remember","person":"p-minh","at_unix_ms":MONDAY + 8 * DAY,"text":"Dentist on Thursday.","replaces":first,"shown":[first],"on":"2026-10-29"}),
        MONDAY + 8 * DAY,
    );
    let new = answer["id"].as_u64().unwrap();
    assert_eq!(event_rows(&scratch), 1);
    store.maintain(MONDAY + 11 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap().unwrap()["id"],
        new
    );
}

/// Forgetting by id, everything, or the person removes a kept thing's day.
#[test]
fn forgetting_a_kept_thing_removes_its_day() {
    let scratch = Scratch::new("kept-forget");
    let mut store = scratch.open(30, MONDAY);
    let minh = person("p-minh");
    let id = kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    assert_eq!(event_rows(&scratch), 1);
    let (answer, _) = request(
        &mut store,
        json!({"op":"forget","person":"p-minh","id":id}),
        MONDAY,
    );
    assert_eq!(answer["removed"], 1, "{answer}");
    assert_eq!(event_rows(&scratch), 0, "forget");
    kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    let (answer, _) = request(
        &mut store,
        json!({"op":"forget_last","person":"p-minh"}),
        MONDAY,
    );
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(event_rows(&scratch), 0, "forget_last");
    kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    request(
        &mut store,
        json!({"op":"forget_all","person":"p-minh"}),
        MONDAY,
    );
    assert_eq!(event_rows(&scratch), 0, "forget_all");
    kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    store.forget_person(&minh).unwrap();
    assert_eq!(event_rows(&scratch), 0, "forget_person");
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&minh, true).unwrap(), None);
}

/// A kept thing `once` finds again over the protocol at `at`, with `on`:
/// its id, and that nothing new was written.
fn kept_once_on(store: &mut Days, at: u64, text: &str, on: &str) -> u64 {
    let (answer, _) = request(
        store,
        json!({"op":"remember","person":"p-minh","at_unix_ms":at,"text":text,"once":true,"on":on}),
        at,
    );
    assert_eq!(answer["ok"], true, "{answer}");
    answer["id"].as_u64().unwrap()
}

/// The day kept for `person`'s entry `id`, of any kind.
fn day_of(scratch: &Scratch, person: &str, id: u64) -> Option<String> {
    Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row(
            "SELECT d.day FROM entries e JOIN event_days d ON d.rid=e.rid WHERE e.person=?1 AND e.id=?2",
            rusqlite::params![person, id],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

/// Reviewer-lead P3 on #2680: a weekly "dentist on Friday" said again with
/// `once` for a later Friday takes that day and is followed up once after
/// it, as after the first; an earlier or the same day changes nothing.
#[test]
fn a_weekly_kept_thing_said_again_for_a_later_day_is_followed_up_again() {
    let scratch = Scratch::new("kept-weekly");
    let mut store = scratch.open(30, MONDAY);
    let minh = person("p-minh");
    const DENTIST: &str = "Dentist on Friday.";
    // Monday 19 October: Friday 23.
    let id = kept_once_on(&mut store, MONDAY, DENTIST, "2026-10-23");
    store.maintain(MONDAY + 5 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap().unwrap()["on"],
        "2026-10-23"
    );
    // Sunday 25: next Friday, the 30th; the same kept thing.
    assert_eq!(
        kept_once_on(&mut store, MONDAY + 6 * DAY, DENTIST, "2026-10-30"),
        id
    );
    assert_eq!(
        day_of(&scratch, "p-minh", id).as_deref(),
        Some("2026-10-30")
    );
    // Monday 26: the same day, then an earlier one, change nothing.
    for on in ["2026-10-30", "2026-10-28"] {
        assert_eq!(kept_once_on(&mut store, MONDAY + 7 * DAY, DENTIST, on), id);
        assert_eq!(
            day_of(&scratch, "p-minh", id).as_deref(),
            Some("2026-10-30"),
            "{on}"
        );
    }
    for at in [MONDAY + 7 * DAY, MONDAY + 10 * DAY, MONDAY + 11 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&minh, true).unwrap(), None, "{at}");
    }
    store.maintain(MONDAY + 12 * DAY).unwrap();
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap(),
        Some(json!({"id":id,"category":"kept","text":DENTIST,"on":"2026-10-30"}))
    );
    store.maintain(MONDAY + 14 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&minh, true).unwrap(), None);
}

/// Another person and the room never get a kept thing's day; the room's
/// own dated kept thing is never given to anyone.
#[test]
fn a_kept_day_is_given_only_to_the_person_who_kept_it() {
    let scratch = Scratch::new("kept-scope");
    let mut store = scratch.open(30, MONDAY);
    let (minh, mai) = (person("p-minh"), person("p-mai"));
    let id = kept_on(
        &mut store,
        "p-minh",
        MONDAY,
        KEPT_WEDDING,
        Some("2026-10-24"),
    );
    // The room's, said with no person.
    let (answer, _) = request(
        &mut store,
        json!({"op":"remember","at_unix_ms":MONDAY,"text":"Party on Saturday.","on":"2026-10-24"}),
        MONDAY,
    );
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(event_rows(&scratch), 2);
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&mai, true).unwrap(), None);
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","events":true}),
        MONDAY + 6 * DAY,
    );
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(
        store.follow_up_with(&minh, true).unwrap().unwrap()["id"],
        id
    );
    store.maintain(MONDAY + 8 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&minh, true).unwrap(), None);
    assert_eq!(store.follow_up_with(&mai, true).unwrap(), None);
}

/// What the room's scope holds: entries, event days and follow-up rows.
fn room_rows(scratch: &Scratch) -> (u64, u64, u64) {
    let connection = Connection::open(scratch.0.join(DATABASE)).unwrap();
    let count = |sql: &str| -> u64 { connection.query_row(sql, [], |r| r.get(0)).unwrap() };
    (
        count("SELECT count(*) FROM entries WHERE person='everyone'"),
        count(
            "SELECT count(*) FROM event_days d JOIN entries e ON e.rid=d.rid WHERE e.person='everyone'",
        ),
        count("SELECT count(*) FROM follow_ups WHERE person='everyone'"),
    )
}

/// `person`'s follow-up rows: the things they were asked about.
fn follow_up_rows(scratch: &Scratch, person: &str) -> u64 {
    Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM follow_ups WHERE person=?1",
            [person],
            |r| r.get(0),
        )
        .unwrap()
}

/// #2807, the lamp run without a lamp: two remembered faces, A tells a
/// dated event. After its day B talks first: B's note and follow-up have
/// nothing of it, asking B marks nothing, and the room's scope holds none
/// of it. Then A is asked once, and never again, nor is B.
#[test]
fn after_its_day_another_face_talking_first_is_never_asked_and_the_teller_is_once() {
    let scratch = Scratch::new("event-teller");
    let mut store = scratch.open(30, NOW);
    let (a, b) = (person("p-minh"), person("p-mai"));
    store.remember(&b, NOW, "Tea, no sugar").unwrap();
    let id = wedding(&mut store, &a);
    assert_eq!(room_rows(&scratch), (0, 0, 0), "told by A, kept as A's");
    // Sunday, the day after: B talks first, more than once.
    let sunday = MONDAY + 6 * DAY;
    store.maintain(sunday).unwrap();
    let theirs = store.session_open(&b, "b-sunday", NOTE_BYTES).unwrap();
    assert!(theirs.note.contains("Tea, no sugar"), "{}", theirs.note);
    assert!(!theirs.note.contains("wedding"), "{}", theirs.note);
    for _ in 0..3 {
        assert_eq!(store.follow_up_with(&b, true).unwrap(), None);
    }
    let (answer, _) = request(
        &mut store,
        json!({"op":"follow_up","person":"p-mai","events":true}),
        sunday,
    );
    assert_eq!(answer, json!({"ok":true,"follow_up":null}));
    assert_eq!(
        follow_up_rows(&scratch, "p-mai"),
        0,
        "asking B marks nothing"
    );
    assert_eq!(follow_up_rows(&scratch, "p-minh"), 0, "still owed to A");
    // The room's scope has none of it, in its rows or its note.
    let room = store
        .session_open(&Person::everyone(), "room-sunday", NOTE_BYTES)
        .unwrap();
    assert!(!room.note.contains("wedding"), "{}", room.note);
    assert_eq!(room_rows(&scratch), (0, 0, 0));
    // Then A: asked once, with its day.
    assert_eq!(
        store.follow_up_with(&a, true).unwrap(),
        Some(
            json!({"id":id,"category":"thread","text":"Sister's wedding is on Saturday","on":"2026-10-24"})
        )
    );
    assert_eq!(follow_up_rows(&scratch, "p-minh"), 1);
    // The same day, the next, and the last of its window: never again.
    for at in [sunday + 3_600_000, sunday + DAY, MONDAY + 19 * DAY] {
        store.maintain(at).unwrap();
        assert_eq!(store.follow_up_with(&a, true).unwrap(), None, "{at}");
        assert_eq!(store.follow_up_with(&b, true).unwrap(), None, "{at}");
    }
    assert_eq!(room_rows(&scratch), (0, 0, 0));
}

/// #2807 with #2176's early words: a dated thing kept in the room's scope
/// before A's face was known, then adopted by A, takes its day with it.
/// After the day the room keeps nothing of it, B is never asked, A once.
#[test]
fn a_dated_thing_adopted_from_the_room_is_asked_only_of_its_teller() {
    let scratch = Scratch::new("event-adopted");
    let mut store = scratch.open(30, MONDAY);
    let (a, b) = (person("p-minh"), person("p-mai"));
    let (answer, _) = request(
        &mut store,
        json!({"op":"remember","at_unix_ms":MONDAY,"text":KEPT_WEDDING,"on":"2026-10-24"}),
        MONDAY,
    );
    let early = answer["id"].as_u64().unwrap();
    assert_eq!(room_rows(&scratch), (1, 1, 0));
    let moved = store.adopt(&Person::everyone(), &a, &[early]).unwrap();
    assert_eq!(moved.len(), 1);
    let id = moved[0].1;
    assert_eq!(
        day_of(&scratch, "p-minh", id).as_deref(),
        Some("2026-10-24")
    );
    assert_eq!(room_rows(&scratch), (0, 0, 0), "adopted with its day");
    store.maintain(MONDAY + 6 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&b, true).unwrap(), None);
    assert_eq!(follow_up_rows(&scratch, "p-mai"), 0);
    assert_eq!(
        store.follow_up_with(&a, true).unwrap(),
        Some(json!({"id":id,"category":"kept","text":KEPT_WEDDING,"on":"2026-10-24"}))
    );
    store.maintain(MONDAY + 8 * DAY).unwrap();
    assert_eq!(store.follow_up_with(&a, true).unwrap(), None);
    assert_eq!(store.follow_up_with(&b, true).unwrap(), None);
    assert_eq!(room_rows(&scratch), (0, 0, 0));
}

/// A store made before `event_days` gets it on open, and its threads are
/// asked about as undated ones.
#[test]
fn a_store_without_event_days_opens_and_answers() {
    let scratch = Scratch::new("event-old");
    let mut store = scratch.open(30, NOW);
    let minh = person("p-minh");
    let id = thread(&mut store, &minh, MONDAY, "Sister's wedding is on Saturday");
    drop(store);
    scratch.sql("DROP TABLE event_days;");
    let mut store = scratch.open(30, MONDAY + DAY);
    assert_eq!(event_rows(&scratch), 0);
    let given = store.follow_up_with(&minh, true).unwrap().unwrap();
    assert_eq!((given["id"].as_u64(), given.get("on")), (Some(id), None));
    wedding(&mut store, &minh);
    assert_eq!(event_rows(&scratch), 1);
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

    // Kept without its conversation, an exchange that is a copy of it goes
    // all the same, by its words (the first builder's condition on #3504:
    // a copy left behind is the failure that matters); one with other
    // words stays.
    let plain_said = store
        .append(&room, NOW, "the bins go out Friday", "OK", &[])
        .unwrap();
    let other = store
        .append(&room, NOW, "the bins are green", "OK", &[])
        .unwrap();
    let bins = store
        .remember(&room, NOW, "the bins go out on Friday")
        .unwrap();
    store.forget(&room, bins).unwrap();
    assert!(store.record(&room, plain_said).unwrap().is_none());
    assert!(store.record(&room, other).unwrap().is_some());
}

/// #2176: a kept thing remembered with its conversation outlives that
/// conversation's expiry, as explicit kept facts do not expire; only its
/// link to the gone exchange goes. Forgetting it still takes the exchanges
/// (the test above).
#[test]
fn a_kept_thing_outlives_the_expiry_of_the_conversation_it_came_up_in() {
    let scratch = Scratch::new("kept-outlives");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    let said = store
        .append(&room, NOW, "my violet umbrella is in the car", "Noted", &[])
        .unwrap();
    let umbrella = store
        .remember_from(&room, NOW, "the violet umbrella is in the car", &[said])
        .unwrap();
    assert_eq!(
        store.record(&room, umbrella).unwrap().unwrap().source_ids,
        [said]
    );

    assert_eq!(store.maintain(NOW + 32 * DAY).unwrap(), 1);
    assert!(store.record(&room, said).unwrap().is_none());
    let kept = store.record(&room, umbrella).unwrap();
    assert!(
        kept.is_some(),
        "the kept thing expired with its conversation"
    );
    assert!(
        kept.unwrap().source_ids.is_empty(),
        "no link to a gone exchange"
    );
    assert!(found(&mut store, &room, "umbrella").contains(&umbrella));
    let list = store.kept(&room, 16_384).unwrap();
    assert_eq!(list.iter().map(|k| k.id).collect::<Vec<_>>(), [umbrella]);
    drop(store);
    let dangling: u64 = Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM sources WHERE child NOT IN (SELECT rid FROM entries) OR parent NOT IN (SELECT rid FROM entries)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0, "no sources row is left to a gone entry");
    let mut store = scratch.open(30, NOW + 32 * DAY);
    assert!(store.record(&room, umbrella).unwrap().is_some());
    store.forget(&room, umbrella).unwrap();
    assert!(store.record(&room, umbrella).unwrap().is_none());
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

/// #1680: memory a newer build wrote (`user_version` above 1) doesn't stop
/// the part. The database is kept whole under a new name, never changed,
/// until a forget or `keep_days` removes it; this build starts a new one.
#[test]
fn a_newer_database_version_is_set_aside_whole_and_a_new_store_begins() {
    let scratch = Scratch::new("newer-database");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let kept = store.remember(&room, NOW, "Coffee black").unwrap();
    drop(store);
    scratch.sql("PRAGMA user_version=2; CREATE TABLE later(value TEXT)");
    let original = fs::read(scratch.0.join(DATABASE)).unwrap();
    let mut store =
        Days::open(scratch.options(30), NOW).expect("a newer database does not stop the part");
    let aside = store
        .set_aside()
        .expect("the kept database is said")
        .to_owned();
    assert!(
        aside.starts_with("memory-days.sqlite3.newer-db2-"),
        "{aside}"
    );
    assert_eq!(
        fs::read(scratch.0.join(&aside)).unwrap(),
        original,
        "kept whole"
    );
    assert_eq!(
        fs::metadata(scratch.0.join(&aside))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        store.status(None).unwrap().is_empty(),
        "the new store is empty"
    );
    assert!(store.record(&room, kept).unwrap().is_none());
    let after = store.remember(&room, NOW, "Tea green").unwrap();
    drop(store);
    let reopened = scratch.open(30, NOW);
    assert_eq!(
        reopened.set_aside(),
        Some(aside.as_str()),
        "said on every start"
    );
    assert_eq!(
        reopened.record(&room, after).unwrap().unwrap().text,
        "Tea green"
    );
    drop(reopened);
    assert_eq!(fs::read(scratch.0.join(&aside)).unwrap(), original);
    let newer = Connection::open(scratch.0.join(&aside)).unwrap();
    assert_eq!(
        newer
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        newer
            .query_row("SELECT text FROM entries WHERE id=?", [kept], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "Coffee black"
    );
}

/// #1680: an observation with a category a newer build wrote is read and
/// kept as it is: never read as a thread, offered to a tidy or superseded.
#[test]
fn an_observation_with_a_newer_category_is_kept_and_never_tidied_over() {
    let scratch = Scratch::new("newer-category");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let source = store
        .append(&room, NOW, "Coffee with milk", "Noted", &[])
        .unwrap();
    let later = observe(
        &mut store,
        &room,
        "later",
        &[source],
        "Coffee with milk",
        None,
    );
    drop(store);
    scratch.sql("UPDATE entries SET category='habit' WHERE kind='observation'");
    let mut store = scratch.open(30, NOW);
    let read = store.record(&room, later).unwrap().unwrap();
    assert_eq!(read.category, None, "not read as a thread");
    assert!(read.active);
    store.maintain(NOW + 1).unwrap();
    let correction = store
        .append(&room, NOW + 1, "Coffee black now, no milk", "Changed", &[])
        .unwrap();
    let job = store
        .conversation_end(&room, "correction", &[correction])
        .unwrap();
    assert!(
        job.previous.iter().all(|prior| prior.id != later),
        "{job:?}"
    );
    assert!(
        store
            .conversation_end_with(&room, "chosen", &[correction], &[later])
            .is_err(),
        "it can't be chosen as a previous observation"
    );
    let mut over = output("Coffee black", &[correction], Some(later));
    over.observations[0].category = Category::Thread;
    assert!(store.tidy_apply(&room, &job.job, &over).is_err());
    assert!(store.record(&room, later).unwrap().unwrap().active);
    drop(store);
    let stored: String = Connection::open(scratch.0.join(DATABASE))
        .unwrap()
        .query_row("SELECT category FROM entries WHERE id=?", [later], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored, "habit");
}

/// A store opened over a database a newer build wrote, with `-wal` and
/// `-shm` beside the set-aside file, and one fact remembered here.
fn set_aside_store(scratch: &Scratch, room: &Person) -> (Days, u64) {
    let mut store = scratch.open(30, NOW);
    store.remember(room, NOW, "Coffee black").unwrap();
    drop(store);
    scratch.sql("PRAGMA user_version=2");
    let mut store = scratch.open(30, NOW);
    let aside = store.set_aside().expect("set aside").to_owned();
    for sidecar in ["-wal", "-shm"] {
        fs::write(scratch.0.join(format!("{aside}{sidecar}")), b"").unwrap();
    }
    let fact = store.remember(room, NOW, "Favourite colour green").unwrap();
    (store, fact)
}

fn assert_no_set_aside(scratch: &Scratch, store: Days) {
    assert_eq!(store.set_aside(), None);
    let left: Vec<String> = fs::read_dir(&scratch.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.contains(".newer-db"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    drop(store);
    assert_eq!(
        scratch.open(30, NOW).set_aside(),
        None,
        "nor on a new start"
    );
}

/// #2067: a set-aside database is the person's memory too, so forgetting
/// everything removes it whole, with its sidecars, and it is no longer said.
#[test]
fn forgetting_everything_also_removes_a_set_aside_database() {
    let scratch = Scratch::new("forget-all-set-aside");
    let room = Person::everyone();
    let (mut store, _) = set_aside_store(&scratch, &room);
    store.forget_all(None).unwrap();
    assert_no_set_aside(&scratch, store);
}

/// #2067 (Third): this build can't reach one fact inside the set-aside file,
/// so forgetting one fact removes the whole file too.
#[test]
fn forgetting_one_fact_also_removes_a_set_aside_database() {
    let scratch = Scratch::new("forget-one-set-aside");
    let room = Person::everyone();
    let (mut store, fact) = set_aside_store(&scratch, &room);
    assert!(store.forget(&room, fact).unwrap() > 0);
    assert_no_set_aside(&scratch, store);
}

/// #2067: so does forgetting the last thing said.
#[test]
fn forgetting_the_last_thing_also_removes_a_set_aside_database() {
    let scratch = Scratch::new("forget-last-set-aside");
    let room = Person::everyone();
    let (mut store, fact) = set_aside_store(&scratch, &room);
    assert_eq!(store.forget_last(&room, None).unwrap().unwrap().id, fact);
    assert_no_set_aside(&scratch, store);
}

fn set_aside_files(scratch: &Scratch) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&scratch.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.contains(".newer-db"))
        .collect();
    names.sort();
    names
}

/// #2067: a set-aside database lasts no longer than `keep_days`, by its
/// `<seconds>` stamp. One second either side: the one a second past it goes
/// with its sidecars, the one a second short stays and is still said, until
/// it too is `keep_days` old.
#[test]
fn a_set_aside_database_goes_once_keep_days_old_one_second_either_side() {
    let scratch = Scratch::new("set-aside-expiry");
    let room = Person::everyone();
    let (store, _) = set_aside_store(&scratch, &room);
    let older = store.set_aside().unwrap().to_owned();
    drop(store);
    scratch.sql("PRAGMA user_version=2");
    let mut store = scratch.open(30, NOW + 2_000);
    let younger = store.set_aside().unwrap().to_owned();
    assert_eq!(
        older,
        format!("memory-days.sqlite3.newer-db2-{}", NOW / 1000)
    );
    assert_eq!(
        younger,
        format!("memory-days.sqlite3.newer-db2-{}", NOW / 1000 + 2)
    );
    let keep = 30 * DAY;
    store.maintain(NOW + keep + 1_000).unwrap();
    assert_eq!(
        set_aside_files(&scratch),
        vec![younger.clone()],
        "a second past keep_days goes, sidecars too; a second short stays"
    );
    assert_eq!(store.set_aside(), Some(younger.as_str()));
    store.maintain(NOW + keep + 2_000).unwrap();
    assert!(set_aside_files(&scratch).is_empty());
    assert_eq!(store.set_aside(), None, "health's warning clears");
    drop(store);
    assert_eq!(scratch.open(30, NOW + keep + 2_000).set_aside(), None);
}

/// #2067: an observation in a newer category takes none of the 32 previous
/// slots, so a tidy pass still sees 32 real previous observations.
#[test]
fn observations_in_a_newer_category_take_no_previous_slot() {
    let scratch = Scratch::new("newer-category-slots");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let mut real = BTreeSet::new();
    for n in 0..37u64 {
        let source = store
            .append(&room, NOW, &format!("Item{n} liked"), "Noted", &[])
            .unwrap();
        let id = observe(
            &mut store,
            &room,
            &format!("c{n}"),
            &[source],
            &format!("Likes item{n}"),
            None,
        );
        if n < 32 {
            real.insert(id);
        }
    }
    drop(store);
    // The five newest were written by a newer build.
    scratch.sql(&format!(
        "UPDATE entries SET category='habit' WHERE kind='observation' AND id NOT IN ({})",
        real.iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    ));
    let mut store = scratch.open(30, NOW + 100);
    let said = store
        .append(&room, NOW + 100, "Weather today", "Sunny", &[])
        .unwrap();
    let job = store.conversation_end(&room, "end", &[said]).unwrap();
    let previous: BTreeSet<u64> = job.previous.iter().map(|prior| prior.id).collect();
    assert_eq!(previous, real);
}

/// #2067: in a session note an observation in a newer category shows as a
/// plain fact, as a kept fact does, never as its newer category.
#[test]
fn a_newer_category_shows_in_a_session_note_as_a_plain_fact() {
    let scratch = Scratch::new("newer-category-note");
    let room = Person::everyone();
    let mut store = scratch.open(30, NOW);
    let kept = store.remember(&room, NOW, "Tea green").unwrap();
    let source = store
        .append(&room, NOW, "Coffee with milk", "Noted", &[])
        .unwrap();
    let later = observe(&mut store, &room, "c", &[source], "Coffee with milk", None);
    drop(store);
    scratch.sql("UPDATE entries SET category='habit' WHERE kind='observation'");
    let mut store = scratch.open(30, NOW);
    let note = store.session_open(&room, "s", NOTE_BYTES).unwrap();
    let facts: Value = serde_json::from_str(&note.note).unwrap();
    let kind = |id: u64| {
        facts["facts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|fact| fact["id"] == id)
            .unwrap_or_else(|| panic!("{facts}"))["kind"]
            .clone()
    };
    assert_eq!(kind(kept), "preference", "a kept fact's plain label");
    assert_eq!(kind(later), kind(kept), "{facts}");
}

/// Finding N10 on #3504 (lamp-06c1, 27 Oct): a misheard "What is my
/// favorite dream?" was kept as "Asked what their favorite dream is.". A
/// proposal resting only on a question, or saying "Asked", keeps nothing;
/// a statement with a question tag is kept.
#[test]
fn a_question_is_never_kept_as_a_fact() {
    let scratch = Scratch::new("only-asked");
    let mut store = scratch.open(30, NOW);
    let room = Person::everyone();
    for (conversation, heard, proposed, kept_count) in [
        (
            "question",
            "What is my favorite dream?",
            "Favorite dream is a question about dreams",
            0,
        ),
        (
            "asked",
            "Hey, mom. My favorite dream is flying.",
            "Asked what their favorite dream is.",
            0,
        ),
        ("tag", "I love tea, you know?", "Loves tea", 1),
    ] {
        let line = store.append(&room, NOW, heard, "Mm", &[]).unwrap();
        let job = store
            .conversation_end(&room, conversation, &[line])
            .unwrap();
        let kept = store
            .tidy_apply(&room, &job.job, &output(proposed, &[line], None))
            .unwrap();
        assert_eq!(kept.len(), kept_count, "{heard} / {proposed}");
    }
    assert!(found(&mut store, &room, "asked").is_empty());
}

/// Finding N11 on #3504 (lamp-06c1, 27 Oct): the pass's "Works at a
/// library now, not the bakery." was kept beside "works at a bakery",
/// which stayed current. With the voice's `replaces` the kept thing is
/// retired by a replacement edge, as a `remember` correction retires it;
/// one no longer current is left as it is and the observation kept beside;
/// two observations, or one with a supersession too, are refused.
#[test]
fn an_observation_replacing_a_kept_thing_retires_it() {
    let scratch = Scratch::new("observation-replaces");
    let mut store = scratch.open(30, NOW);
    let olivia = person("p-olivia");
    let bakery = store.remember(&olivia, NOW, "works at a bakery").unwrap();
    let line = store
        .append(
            &olivia,
            NOW,
            "I work at a library now. Not the bakery.",
            "That's a change",
            &[],
        )
        .unwrap();
    let job = store.conversation_end(&olivia, "library", &[line]).unwrap();
    let mut correction = proposal("Works at a library now, not the bakery.", &[line], None);
    correction.replaces = Some(bakery);
    let ids = store
        .tidy_apply(
            &olivia,
            &job.job,
            &Output {
                observations: vec![correction.clone()],
            },
        )
        .unwrap();
    assert_eq!(ids.len(), 1);
    let old = store.record(&olivia, bakery).unwrap().unwrap();
    assert!(!old.active, "{old:?}");
    let new = store.record(&olivia, ids[0]).unwrap().unwrap();
    assert!(new.active && new.supersedes == Some(bakery), "{new:?}");
    assert!(!found(&mut store, &olivia, "bakery").contains(&bakery));

    // Already replaced: kept plainly beside, nothing else changes.
    let again = store
        .append(&olivia, NOW, "The library, not the bakery", "Right", &[])
        .unwrap();
    let job = store.conversation_end(&olivia, "again", &[again]).unwrap();
    let mut late = proposal("Library job now, not the bakery", &[again], None);
    late.replaces = Some(bakery);
    let ids = store
        .tidy_apply(
            &olivia,
            &job.job,
            &Output {
                observations: vec![late],
            },
        )
        .unwrap();
    let plain = store.record(&olivia, ids[0]).unwrap().unwrap();
    assert!(plain.active && plain.supersedes.is_none(), "{plain:?}");

    // Two naming one kept thing, or a supersession beside it: refused.
    let tea = store.remember(&olivia, NOW, "likes tea").unwrap();
    let line = store
        .append(&olivia, NOW, "Coffee now, not tea", "Ok", &[])
        .unwrap();
    let job = store.conversation_end(&olivia, "two", &[line]).unwrap();
    let mut first = proposal("Drinks coffee now, not tea", &[line], None);
    first.replaces = Some(tea);
    let second = Proposal {
        text: "No longer drinks tea".into(),
        ..first.clone()
    };
    assert!(
        store
            .tidy_apply(
                &olivia,
                &job.job,
                &Output {
                    observations: vec![first, second],
                },
            )
            .is_err()
    );
    assert!(store.record(&olivia, tea).unwrap().unwrap().active);
}
