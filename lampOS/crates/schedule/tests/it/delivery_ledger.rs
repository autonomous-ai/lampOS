//! Authored crash/receipt cases with the real in-process Schedule engine.
//! No socket, provider, speaker, wall-clock change, or lamp is involved.
use lamp_schedule::{
    delivery::{
        AbsentEvidence, AckAttempt, AckEvidence, Correlation, DeliveryKey, DeliveryLedger,
        Evidence, ItemEvidence, MAX_ENTRIES, PendingEvidence, PreparedToken, QualifiedCompletion,
        QualifiedRetirement, Reconciled, Resolution, RetirementBasis,
    },
    engine::{Engine, Persistence},
    protocol::{Guard, Request, refusal},
    store::{MAX_STATE_BYTES, Store},
    time::{ClockSample, When},
};
use serde_json::{Value, json};
use std::{
    io,
    sync::{Arc, Mutex},
};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "11111111111111111111111111111111";
const LEDGER: &str = "abcdef0123456789abcdef0123456789";
const WALL: i64 = 1_791_964_800_000;
const PRIVATE_TEXT: &str = "Private reminder text is not ledger metadata";

#[derive(Clone, Copy)]
enum Failure {
    Before,
    After,
}
#[derive(Default)]
struct Data {
    bytes: Option<Vec<u8>>,
    failure: Option<Failure>,
    attempts: usize,
    writes: usize,
    kept: Vec<(String, Vec<u8>)>,
    refuse_set_aside: bool,
}
#[derive(Clone, Default)]
struct Disk(Arc<Mutex<Data>>);
impl Disk {
    fn fail(&self, failure: Failure) {
        self.0.lock().unwrap().failure = Some(failure);
    }
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().bytes.clone().unwrap()
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes()).unwrap()
    }
    fn replace(&self, value: &Value) {
        self.0.lock().unwrap().bytes = Some(serde_json::to_vec(value).unwrap());
    }
    fn writes(&self) -> usize {
        self.0.lock().unwrap().writes
    }
}
impl Persistence for Disk {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().bytes.clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut data = self.0.lock().unwrap();
        data.attempts += 1;
        let failure = data.failure.take();
        if matches!(failure, Some(Failure::Before)) {
            return Err(io::Error::other("authored precommit failure"));
        }
        data.bytes = Some(bytes.into());
        data.writes += 1;
        if matches!(failure, Some(Failure::After)) {
            return Err(io::Error::other("authored postcommit sync failure"));
        }
        Ok(())
    }
    fn set_aside(&mut self, label: &str) -> io::Result<String> {
        let mut data = self.0.lock().unwrap();
        if data.refuse_set_aside {
            return Err(io::Error::other("authored preservation failure"));
        }
        let name = format!("schedule.sqlite3.{label}");
        let bytes = data.bytes.take().unwrap();
        data.kept.push((name.clone(), bytes));
        Ok(name)
    }
}
fn now(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: "authored-boot".into(),
        wall_trusted: true,
    }
}
fn ledger(disk: Disk) -> DeliveryLedger<Disk> {
    DeliveryLedger::open(disk, LEDGER).unwrap()
}
fn engine(disk: Disk) -> Engine<Disk> {
    Engine::open(disk, "Etc/UTC", &now(0), STORE).unwrap()
}
fn guard(engine: &Engine<Disk>, request_id: &str) -> Guard {
    Guard {
        store_id: engine.store_id().into(),
        revision: engine.revision(),
        request_id: request_id.into(),
    }
}
fn set(engine: &mut Engine<Disk>, at: u64, when: When) -> u64 {
    engine.tick(&now(at)).unwrap();
    let response = engine
        .answer(
            Request::Set {
                person: None,
                guard: guard(engine, &format!("create-{}", engine.revision())),
                text: PRIVATE_TEXT.into(),
                when,
            },
            &now(at),
        )
        .unwrap();
    response["id"].as_u64().unwrap()
}
fn get(engine: &mut Engine<Disk>, id: u64, at: u64) -> Value {
    engine.answer(Request::Get { id }, &now(at)).unwrap()
}
fn pending(engine: &mut Engine<Disk>, id: u64, at: u64) -> PendingEvidence {
    PendingEvidence::from_get(&Request::Get { id }, &get(engine, id, at)).unwrap()
}
fn timer() -> (Engine<Disk>, PendingEvidence) {
    let mut engine = engine(Disk::default());
    let id = set(&mut engine, 0, When::After { seconds: 1 });
    let pending = pending(&mut engine, id, 1000);
    (engine, pending)
}
fn correlation(sequence: u64) -> Correlation {
    Correlation {
        consumer_session: "consumer-one".into(),
        sequence,
    }
}
fn proof(prepared: &PreparedToken) -> QualifiedCompletion {
    QualifiedCompletion {
        correlation: prepared.correlation.clone(),
        owner_boot: [7; 16],
        turn: 1,
        microphone_generation: 2,
        provider_session: 3,
        playback_sequence: 4,
        final_epoch: 0,
        final_frame: 4800,
        retired_generations: 2,
    }
}
fn retirement(prepared: &PreparedToken) -> QualifiedRetirement {
    QualifiedRetirement {
        correlation: prepared.correlation.clone(),
        retired_by_session: "new-owner".into(),
        basis: RetirementBasis::PreviousProcessExited {},
    }
}
fn prepare_ack(
    ledger: &mut DeliveryLedger<Disk>,
    pending: &PendingEvidence,
) -> (PreparedToken, AckAttempt) {
    let prepared = ledger.prepare(pending, correlation(1)).unwrap();
    let complete = ledger.complete(&prepared, &proof(&prepared)).unwrap();
    let ack = ledger.plan_ack(&complete, pending).unwrap();
    (prepared, ack)
}
fn absence(engine: &mut Engine<Disk>, key: &DeliveryKey, at: u64) -> AbsentEvidence {
    let request = Request::List {
        after_id: key.id - 1,
        limit: 1,
        pending_only: false,
    };
    let response = engine.answer(request.clone(), &now(at)).unwrap();
    AbsentEvidence::from_list(key, &request, &response).unwrap()
}

#[test]
fn fresh_ledger_initializes_tagged_snapshot_and_reopen_preserves_identity() {
    let disk = Disk::default();
    let first = ledger(disk.clone());
    assert_eq!(first.revision(), 0);
    assert!(first.entries().unwrap().is_empty());
    assert_eq!(disk.writes(), 1);
    let bytes = disk.bytes();
    drop(first);
    let second = DeliveryLedger::open(disk.clone(), OTHER).unwrap();
    assert_eq!(second.revision(), 0);
    assert_eq!(disk.bytes(), bytes);
    assert_eq!(disk.json()["kind"], "lamp_schedule_delivery_ledger");
    assert_eq!(disk.json()["ledger_id"], LEDGER);
}

#[test]
fn prepare_complete_and_ack_intent_each_commit_before_return_without_text() {
    let (mut engine, pending) = timer();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    let prepared = ledger.prepare(&pending, correlation(1)).unwrap();
    assert_eq!(disk.writes(), 2);
    assert!(ledger.pending_ack(pending.key()).unwrap().is_none());
    let complete = ledger.complete(&prepared, &proof(&prepared)).unwrap();
    assert_eq!(disk.writes(), 3);
    let ack = ledger.plan_ack(&complete, &pending).unwrap();
    assert_eq!(disk.writes(), 4);
    assert_eq!(
        ledger.pending_ack(pending.key()).unwrap(),
        Some(ack.clone())
    );
    let response = engine.answer(ack.request.clone(), &now(1000)).unwrap();
    let evidence = AckEvidence::from_reply(&ack, &response).unwrap();
    let resolved = ledger
        .reconcile(pending.key(), &Evidence::Ack(evidence))
        .unwrap();
    assert_eq!(
        resolved,
        Reconciled::Resolved(Resolution::AckConfirmed {
            revision: response["revision"].as_u64().unwrap()
        })
    );
    assert_eq!(disk.writes(), 5);
    assert!(
        !String::from_utf8(disk.bytes())
            .unwrap()
            .contains(PRIVATE_TEXT)
    );
    assert_eq!(ledger.compact_resolved().unwrap(), 1);
    assert!(ledger.entries().unwrap().is_empty());
    assert_eq!(
        ledger.prepare(&pending, correlation(2)).unwrap_err().code,
        "stale_revision"
    );
}

#[test]
fn duplicate_prepare_never_returns_another_admission_and_completion_is_idempotent() {
    let (_, pending) = timer();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    let prepared = ledger.prepare(&pending, correlation(1)).unwrap();
    assert_eq!(
        ledger.prepare(&pending, correlation(2)).unwrap_err().code,
        "already_known"
    );
    let proof = proof(&prepared);
    let complete = ledger.complete(&prepared, &proof).unwrap();
    let writes = disk.writes();
    assert_eq!(ledger.complete(&prepared, &proof).unwrap(), complete);
    assert_eq!(disk.writes(), writes);
    let mut conflicting = proof.clone();
    conflicting.final_frame += 1;
    assert_eq!(
        ledger.complete(&prepared, &conflicting).unwrap_err().code,
        "completion_conflict"
    );
    assert_eq!(disk.writes(), writes);
}

#[test]
fn completion_requires_nonzero_actual_receipt_fields_and_exact_correlation() {
    let (_, pending) = timer();
    let mut ledger = ledger(Disk::default());
    let prepared = ledger.prepare(&pending, correlation(1)).unwrap();
    for field in [
        "turn",
        "microphone_generation",
        "provider_session",
        "playback_sequence",
        "final_frame",
        "retired_generations",
    ] {
        let mut value = serde_json::to_value(proof(&prepared)).unwrap();
        value[field] = json!(0);
        let malformed: QualifiedCompletion = serde_json::from_value(value).unwrap();
        assert_eq!(
            ledger.complete(&prepared, &malformed).unwrap_err().code,
            "invalid_completion",
            "{field}"
        );
    }
    let mut malformed = proof(&prepared);
    malformed.owner_boot = [0; 16];
    assert_eq!(
        ledger.complete(&prepared, &malformed).unwrap_err().code,
        "invalid_completion"
    );
    malformed = proof(&prepared);
    malformed.correlation.sequence += 1;
    assert_eq!(
        ledger.complete(&prepared, &malformed).unwrap_err().code,
        "invalid_completion"
    );
    assert!(ledger.pending_ack(pending.key()).unwrap().is_none());
}

#[test]
fn prepared_restart_is_uncertain_and_retry_requires_retirement_then_new_durable_attempt() {
    let (_, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    let prepared = first.prepare(&pending, correlation(1)).unwrap();
    drop(first);
    let mut second = ledger(disk.clone());
    assert_eq!(
        second.view(pending.key()).unwrap().unwrap().prepared,
        prepared
    );
    assert_eq!(
        second.prepare(&pending, correlation(2)).unwrap_err().code,
        "already_known"
    );
    let mut wrong = retirement(&prepared);
    wrong.correlation.sequence += 1;
    assert_eq!(
        second
            .retry_interrupted(&prepared, &pending, &wrong, correlation(2))
            .unwrap_err()
            .code,
        "invalid_retirement"
    );
    let replacement = second
        .retry_interrupted(&prepared, &pending, &retirement(&prepared), correlation(2))
        .unwrap();
    assert!(replacement.attempt > prepared.attempt);
    assert_eq!(
        ledger(disk).view(pending.key()).unwrap().unwrap().prepared,
        replacement
    );
    assert_eq!(
        second
            .complete(&prepared, &proof(&prepared))
            .unwrap_err()
            .code,
        "stale_attempt"
    );
    second.complete(&replacement, &proof(&replacement)).unwrap();
    assert_eq!(
        second
            .retry_interrupted(
                &replacement,
                &pending,
                &retirement(&replacement),
                correlation(3)
            )
            .unwrap_err()
            .code,
        "not_prepared"
    );
}

#[test]
fn never_admitted_and_explicit_owner_fence_retry_are_typed_not_timeouts() {
    for basis in [
        RetirementBasis::NeverAdmitted {},
        RetirementBasis::OwnerFenced {
            owner_boot: [3; 16],
            turn: 5,
            microphone_generation: 7,
        },
    ] {
        let (_, pending) = timer();
        let mut ledger = ledger(Disk::default());
        let prepared = ledger.prepare(&pending, correlation(1)).unwrap();
        let retired = QualifiedRetirement {
            basis,
            ..retirement(&prepared)
        };
        ledger
            .retry_interrupted(&prepared, &pending, &retired, correlation(2))
            .unwrap();
    }
    assert!(
        serde_json::from_value::<RetirementBasis>(
            json!({"kind":"previous_process_exited","timeout":true})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<RetirementBasis>(json!({"kind":"never_admitted","extra":0}))
            .is_err()
    );
}

#[test]
fn all_ack_and_retry_requests_remain_exact_after_reopen() {
    let (_, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    let (_, ack) = prepare_ack(&mut first, &pending);
    let bytes = serde_json::to_vec(&ack.request).unwrap();
    drop(first);
    let mut reopened = ledger(disk);
    let recovered = reopened.pending_ack(pending.key()).unwrap().unwrap();
    assert_eq!(serde_json::to_vec(&recovered.request).unwrap(), bytes);
    let complete = reopened
        .view(pending.key())
        .unwrap()
        .unwrap()
        .completed_token()
        .unwrap();
    assert_eq!(reopened.plan_ack(&complete, &pending).unwrap(), recovered);
    assert_eq!(
        reopened.prepare(&pending, correlation(9)).unwrap_err().code,
        "already_known"
    );
}

#[test]
fn lost_ack_reply_survives_server_restart_and_due_revision_change() {
    let schedule_disk = Disk::default();
    let mut schedule = engine(schedule_disk.clone());
    let id = set(&mut schedule, 0, When::After { seconds: 1 });
    set(&mut schedule, 0, When::After { seconds: 2 });
    let pending = pending(&mut schedule, id, 1000);
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    let (_, ack) = prepare_ack(&mut first, &pending);
    let lost_reply = schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    drop(first);
    drop(schedule);
    let mut schedule = Engine::open(schedule_disk, "Etc/UTC", &now(2000), OTHER).unwrap();
    let replayed = schedule.answer(ack.request.clone(), &now(2000)).unwrap();
    assert_eq!(replayed, lost_reply);
    assert!(schedule.revision() > replayed["revision"].as_u64().unwrap());
    let status = schedule.answer(Request::Status {}, &now(2000)).unwrap();
    let receipt = AckEvidence::from_status(&ack, &status).unwrap();
    let mut recovered = ledger(disk);
    assert!(matches!(
        recovered
            .reconcile(pending.key(), &Evidence::Ack(receipt))
            .unwrap(),
        Reconciled::Resolved(Resolution::AckConfirmed { .. })
    ));
    assert_eq!(recovered.schedule_revision(), Some(schedule.revision()));
}

#[test]
fn repeated_matching_status_receipt_still_persists_its_newer_revision_fence() {
    let mut schedule = engine(Disk::default());
    let id = set(&mut schedule, 0, When::After { seconds: 1 });
    set(&mut schedule, 0, When::After { seconds: 2 });
    let pending = pending(&mut schedule, id, 1000);
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    let (_, ack) = prepare_ack(&mut ledger, &pending);
    let response = schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    let evidence = AckEvidence::from_reply(&ack, &response).unwrap();
    ledger
        .reconcile(pending.key(), &Evidence::Ack(evidence))
        .unwrap();
    let earlier = ledger.schedule_revision().unwrap();
    let status = schedule.answer(Request::Status {}, &now(2000)).unwrap();
    let repeated = AckEvidence::from_status(&ack, &status).unwrap();
    ledger
        .reconcile(pending.key(), &Evidence::Ack(repeated))
        .unwrap();
    assert!(ledger.schedule_revision().unwrap() > earlier);
    assert_eq!(
        self::ledger(disk).schedule_revision(),
        Some(schedule.revision())
    );
}

#[test]
fn unrelated_mutation_before_ack_requires_explicit_persisted_rebase_of_same_logical_id() {
    let (mut schedule, pending) = timer();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    let (prepared, original) = prepare_ack(&mut ledger, &pending);
    set(&mut schedule, 1000, When::After { seconds: 10 });
    assert_eq!(
        schedule
            .answer(original.request.clone(), &now(1000))
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        ledger.rebase_ack(pending.key(), &pending).unwrap_err().code,
        "not_newer"
    );
    let refreshed = self::pending(&mut schedule, pending.key().id, 1000);
    let complete = ledger.complete(&prepared, &proof(&prepared)).unwrap();
    assert_eq!(
        ledger.plan_ack(&complete, &refreshed).unwrap_err().code,
        "rebase_required"
    );
    let rebased = ledger.rebase_ack(pending.key(), &refreshed).unwrap();
    assert_ne!(original.request, rebased.request);
    assert_eq!(
        original.request.guard().unwrap().request_id,
        rebased.request.guard().unwrap().request_id
    );
    assert_eq!(
        self::ledger(disk).pending_ack(pending.key()).unwrap(),
        Some(rebased.clone())
    );
    let response = schedule
        .answer(rebased.request.clone(), &now(1000))
        .unwrap();
    assert!(AckEvidence::from_reply(&original, &response).is_err());
    let evidence = AckEvidence::from_reply(&rebased, &response).unwrap();
    ledger
        .reconcile(pending.key(), &Evidence::Ack(evidence))
        .unwrap();
}

#[test]
fn overwritten_committed_ack_receipt_resolves_absence_as_gone_not_ack_confirmed() {
    let (mut schedule, pending) = timer();
    let mut ledger = ledger(Disk::default());
    let (_, ack) = prepare_ack(&mut ledger, &pending);
    schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    set(&mut schedule, 1000, When::After { seconds: 10 });
    let status = schedule.answer(Request::Status {}, &now(1000)).unwrap();
    assert_eq!(
        AckEvidence::from_status(&ack, &status).unwrap_err().code,
        "no_matching_receipt"
    );
    assert_eq!(
        schedule.answer(ack.request, &now(1000)).unwrap_err().code,
        "conflict"
    );
    let gone = absence(&mut schedule, pending.key(), 1000);
    assert!(matches!(
        ledger
            .reconcile(pending.key(), &Evidence::Absent(gone))
            .unwrap(),
        Reconciled::Resolved(Resolution::Gone { .. })
    ));
}

#[test]
fn snooze_advancement_is_superseded_and_old_completion_cannot_ack_next_token() {
    let (mut schedule, pending) = timer();
    let mut ledger = ledger(Disk::default());
    let (_, ack) = prepare_ack(&mut ledger, &pending);
    schedule
        .answer(
            Request::Snooze {
                asker: None,
                guard: guard(&schedule, "snooze"),
                id: pending.key().id,
                delivery: 1,
                seconds: 10,
            },
            &now(1000),
        )
        .unwrap();
    let item = ItemEvidence::from_get(
        &Request::Get {
            id: pending.key().id,
        },
        &get(&mut schedule, pending.key().id, 1000),
    )
    .unwrap();
    assert_eq!(item.key().delivery, 2);
    assert!(!item.is_pending());
    assert!(matches!(
        ledger
            .reconcile(pending.key(), &Evidence::Item(item))
            .unwrap(),
        Reconciled::Resolved(Resolution::Superseded { delivery: 2, .. })
    ));
    assert!(ledger.pending_ack(pending.key()).unwrap().is_none());
    assert_eq!(
        schedule.answer(ack.request, &now(1000)).unwrap_err().code,
        "conflict"
    );
    let next = self::pending(&mut schedule, pending.key().id, 11_000);
    let next_prepared = ledger.prepare(&next, correlation(2)).unwrap();
    assert!(ledger.pending_ack(&next_prepared.key).unwrap().is_none());
}

#[test]
fn exact_old_ack_replay_never_consumes_a_later_daily_occurrence() {
    let mut schedule = engine(Disk::default());
    let id = set(
        &mut schedule,
        0,
        When::Daily {
            time: "00:01".into(),
            zone: Some("Etc/UTC".into()),
        },
    );
    let first_at = u64::try_from(
        get(&mut schedule, id, 0)["item"]["due_unix_ms"]
            .as_i64()
            .unwrap()
            - WALL,
    )
    .unwrap();
    let next_at = first_at + 86_400_000;
    let first = pending(&mut schedule, id, first_at);
    let mut ledger = ledger(Disk::default());
    let (_, ack) = prepare_ack(&mut ledger, &first);
    let receipt = schedule
        .answer(ack.request.clone(), &now(first_at))
        .unwrap();
    let second = pending(&mut schedule, id, next_at);
    assert_eq!(second.key().delivery, 2);
    assert_eq!(
        schedule.answer(ack.request.clone(), &now(next_at)).unwrap(),
        receipt
    );
    assert_eq!(get(&mut schedule, id, next_at)["item"]["state"], "pending");
    assert_eq!(
        ledger.rebase_ack(first.key(), &second).unwrap_err().code,
        "wrong_delivery"
    );
    let evidence = AckEvidence::from_reply(&ack, &receipt).unwrap();
    ledger
        .reconcile(first.key(), &Evidence::Ack(evidence))
        .unwrap();
    ledger.prepare(&second, correlation(2)).unwrap();
}

#[test]
fn status_plus_unbound_not_found_never_proves_absence_in_the_original_store() {
    let (mut schedule, pending) = timer();
    let mut ledger = ledger(Disk::default());
    let prepared = ledger.prepare(&pending, correlation(1)).unwrap();
    let status = schedule.answer(Request::Status {}, &now(1000)).unwrap();
    assert_eq!(status["store_id"], STORE);
    let mut switched = Engine::open(Disk::default(), "Etc/UTC", &now(1000), OTHER).unwrap();
    let refused = switched
        .answer(
            Request::Get {
                id: pending.key().id,
            },
            &now(1000),
        )
        .unwrap_err();
    let unbound = refusal(refused.code, refused.message);
    assert!(
        ItemEvidence::from_get(
            &Request::Get {
                id: pending.key().id
            },
            &unbound
        )
        .is_err()
    );
    let query = Request::List {
        after_id: pending.key().id - 1,
        limit: 1,
        pending_only: false,
    };
    let foreign_list = switched.answer(query.clone(), &now(1000)).unwrap();
    assert!(AbsentEvidence::from_list(pending.key(), &query, &foreign_list).is_err());
    assert_eq!(
        ledger.view(pending.key()).unwrap().unwrap().prepared,
        prepared
    );
    assert!(
        ledger
            .view(pending.key())
            .unwrap()
            .unwrap()
            .resolution
            .is_none()
    );
}

#[test]
fn list_absence_requires_exact_unfiltered_page_and_valid_cursor() {
    let (mut schedule, pending) = timer();
    schedule
        .answer(
            Request::Cancel {
                asker: None,
                guard: guard(&schedule, "cancel"),
                id: pending.key().id,
            },
            &now(1000),
        )
        .unwrap();
    set(&mut schedule, 1000, When::After { seconds: 30 });
    let query = Request::List {
        after_id: pending.key().id - 1,
        limit: 1,
        pending_only: false,
    };
    let reply = schedule.answer(query.clone(), &now(1000)).unwrap();
    assert!(AbsentEvidence::from_list(pending.key(), &query, &reply).is_ok());
    for wrong in [
        Request::Status {},
        Request::List {
            after_id: pending.key().id,
            limit: 1,
            pending_only: false,
        },
        Request::List {
            after_id: pending.key().id - 1,
            limit: 1,
            pending_only: true,
        },
    ] {
        assert_eq!(
            AbsentEvidence::from_list(pending.key(), &wrong, &reply)
                .unwrap_err()
                .code,
            "wrong_query"
        );
    }
    let mut corrupt = reply.clone();
    corrupt["next_after_id"] = json!(999);
    assert!(AbsentEvidence::from_list(pending.key(), &query, &corrupt).is_err());
    corrupt = reply;
    corrupt["items"][0]["id"] = json!(pending.key().id);
    assert_eq!(
        AbsentEvidence::from_list(pending.key(), &query, &corrupt)
            .unwrap_err()
            .code,
        "not_absent"
    );
}

#[test]
fn store_switch_and_revision_rollback_preserve_all_existing_evidence() {
    let (mut schedule, pending) = timer();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    ledger.prepare(&pending, correlation(1)).unwrap();
    let id = set(&mut schedule, 1000, When::After { seconds: 1 });
    let newer = self::pending(&mut schedule, id, 2000);
    ledger.prepare(&newer, correlation(2)).unwrap();
    let old = ItemEvidence::from_get(
        &Request::Get {
            id: pending.key().id,
        },
        &{
            let mut value = get(&mut schedule, pending.key().id, 2000);
            value["revision"] = json!(pending.revision());
            value
        },
    )
    .unwrap();
    let before = disk.bytes();
    assert_eq!(
        ledger
            .reconcile(pending.key(), &Evidence::Item(old))
            .unwrap_err()
            .code,
        "stale_revision"
    );
    let mut foreign = get(&mut schedule, id, 2000);
    foreign["store_id"] = json!(OTHER);
    let foreign = PendingEvidence::from_get(&Request::Get { id }, &foreign).unwrap();
    assert_eq!(
        ledger.prepare(&foreign, correlation(3)).unwrap_err().code,
        "store_changed"
    );
    assert_eq!(disk.bytes(), before);
}

#[test]
fn identity_bound_but_equal_revision_absence_cannot_contradict_pending_evidence() {
    let (_, pending) = timer();
    let mut ledger = ledger(Disk::default());
    ledger.prepare(&pending, correlation(1)).unwrap();
    let query = Request::List {
        after_id: pending.key().id - 1,
        limit: 1,
        pending_only: false,
    };
    let value = json!({"ok":true,"store_id":STORE,"revision":pending.revision(),"items":[],"next_after_id":null});
    let evidence = AbsentEvidence::from_list(pending.key(), &query, &value).unwrap();
    assert_eq!(
        ledger
            .reconcile(pending.key(), &Evidence::Absent(evidence))
            .unwrap_err()
            .code,
        "inconsistent_revision"
    );
}

#[test]
fn full_unresolved_ledger_never_evicts_and_only_durable_resolution_frees_capacity() {
    let mut schedule = engine(Disk::default());
    for _ in 0..MAX_ENTRIES {
        set(&mut schedule, 0, When::After { seconds: 1 });
    }
    schedule.tick(&now(1000)).unwrap();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    let mut keys = Vec::new();
    for id in 1..=MAX_ENTRIES as u64 {
        let observation = pending(&mut schedule, id, 1000);
        keys.push(observation.key().clone());
        ledger.prepare(&observation, correlation(id)).unwrap();
    }
    assert_eq!(ledger.compact_resolved().unwrap(), 0);
    assert_eq!(ledger.entries().unwrap().len(), MAX_ENTRIES);
    schedule
        .answer(
            Request::Cancel {
                asker: None,
                guard: guard(&schedule, "cancel-first"),
                id: 1,
            },
            &now(1000),
        )
        .unwrap();
    let id = set(&mut schedule, 1000, When::After { seconds: 1 });
    let extra = pending(&mut schedule, id, 2000);
    assert_eq!(
        ledger.prepare(&extra, correlation(129)).unwrap_err().code,
        "full"
    );
    assert_eq!(ledger.entries().unwrap().len(), MAX_ENTRIES);
    let gone = absence(&mut schedule, &keys[0], 2000);
    ledger.reconcile(&keys[0], &Evidence::Absent(gone)).unwrap();
    assert_eq!(ledger.compact_resolved().unwrap(), 1);
    let prepared = ledger.prepare(&extra, correlation(129)).unwrap();
    assert_eq!(prepared.attempt, 129);
    assert_eq!(self::ledger(disk).entries().unwrap().len(), MAX_ENTRIES);
}

#[test]
fn every_durable_transition_poisoned_on_before_or_after_commit_error_recovers_only_from_disk() {
    for stage in [
        "prepare", "complete", "ack", "retry", "rebase", "resolve", "compact",
    ] {
        for failure in [Failure::Before, Failure::After] {
            let (mut schedule, pending) = timer();
            let disk = Disk::default();
            let mut ledger = ledger(disk.clone());
            let prepared = if stage == "prepare" {
                None
            } else {
                Some(ledger.prepare(&pending, correlation(1)).unwrap())
            };
            let completed = if matches!(stage, "ack" | "rebase" | "resolve" | "compact") {
                let prepared = prepared.as_ref().unwrap();
                Some(ledger.complete(prepared, &proof(prepared)).unwrap())
            } else {
                None
            };
            let ack = if matches!(stage, "rebase" | "resolve" | "compact") {
                Some(
                    ledger
                        .plan_ack(completed.as_ref().unwrap(), &pending)
                        .unwrap(),
                )
            } else {
                None
            };
            let mut refreshed = None;
            if stage == "rebase" {
                set(&mut schedule, 1000, When::After { seconds: 30 });
                refreshed = Some(self::pending(&mut schedule, pending.key().id, 1000));
            }
            let receipt = if matches!(stage, "resolve" | "compact") {
                let ack = ack.as_ref().unwrap();
                Some(
                    AckEvidence::from_reply(
                        ack,
                        &schedule.answer(ack.request.clone(), &now(1000)).unwrap(),
                    )
                    .unwrap(),
                )
            } else {
                None
            };
            if stage == "compact" {
                ledger
                    .reconcile(
                        pending.key(),
                        &Evidence::Ack(receipt.as_ref().unwrap().clone()),
                    )
                    .unwrap();
            }
            let previous = disk.bytes();
            let revision = ledger.revision();
            disk.fail(failure);
            let error = match stage {
                "prepare" => ledger.prepare(&pending, correlation(1)).unwrap_err(),
                "complete" => {
                    let prepared = prepared.as_ref().unwrap();
                    ledger.complete(prepared, &proof(prepared)).unwrap_err()
                }
                "ack" => ledger
                    .plan_ack(completed.as_ref().unwrap(), &pending)
                    .unwrap_err(),
                "retry" => {
                    let prepared = prepared.as_ref().unwrap();
                    ledger
                        .retry_interrupted(
                            prepared,
                            &pending,
                            &retirement(prepared),
                            correlation(2),
                        )
                        .unwrap_err()
                }
                "rebase" => ledger
                    .rebase_ack(pending.key(), refreshed.as_ref().unwrap())
                    .unwrap_err(),
                "resolve" => ledger
                    .reconcile(pending.key(), &Evidence::Ack(receipt.unwrap()))
                    .unwrap_err(),
                "compact" => ledger.compact_resolved().unwrap_err(),
                _ => unreachable!(),
            };
            assert_eq!(error.code, "storage", "{stage}");
            assert!(ledger.failed());
            assert_eq!(ledger.revision(), revision);
            assert_eq!(ledger.entries().unwrap_err().code, "storage_failed");
            assert_eq!(
                ledger.compact_resolved().unwrap_err().code,
                "storage_failed"
            );
            assert_eq!(
                ledger.prepare(&pending, correlation(99)).unwrap_err().code,
                "storage_failed"
            );
            let recovered = self::ledger(disk.clone());
            match failure {
                Failure::Before => {
                    assert_eq!(disk.bytes(), previous, "{stage}");
                    assert_eq!(recovered.revision(), revision);
                }
                Failure::After => {
                    assert_ne!(disk.bytes(), previous, "{stage}");
                    assert_eq!(recovered.revision(), revision + 1);
                }
            }
        }
    }
}

#[test]
fn initial_commit_failure_never_returns_a_usable_ledger() {
    for failure in [Failure::Before, Failure::After] {
        let disk = Disk::default();
        disk.fail(failure);
        assert_eq!(
            DeliveryLedger::open(disk.clone(), LEDGER)
                .err()
                .unwrap()
                .code,
            "storage"
        );
        assert!(ledger(disk).entries().unwrap().is_empty());
    }
}

#[test]
fn snapshot_rejects_wrong_kind_missing_duplicate_and_invalid_fields_without_writes() {
    let (_, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    prepare_ack(&mut first, &pending);
    let valid = disk.json();
    let mut variants = Vec::new();
    for (field, value) in [
        ("kind", json!("schedule")),
        ("schema", json!(0)),
        ("next_attempt", json!(0)),
        ("ledger_id", json!("bad")),
    ] {
        let mut next = valid.clone();
        next[field] = value;
        variants.push(next);
    }
    for field in ["schedule", "entries", "next_attempt"] {
        let mut next = valid.clone();
        next.as_object_mut().unwrap().remove(field);
        variants.push(next);
    }
    for field in ["completion", "ack", "resolution"] {
        let mut next = valid.clone();
        next["entries"][0]["view"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        variants.push(next);
    }
    let mut duplicate = valid.clone();
    duplicate["entries"]
        .as_array_mut()
        .unwrap()
        .push(valid["entries"][0].clone());
    variants.push(duplicate);
    let mut wrong_ack = valid.clone();
    wrong_ack["entries"][0]["view"]["ack"]["request"]["guard"]["request_id"] =
        json!("not-this-ledger");
    variants.push(wrong_ack);
    let mut no_completion = valid.clone();
    no_completion["entries"][0]["view"]["completion"] = Value::Null;
    variants.push(no_completion);
    let mut wrong_attempt = valid.clone();
    wrong_attempt["entries"][0]["view"]["prepared"]["attempt"] = json!(200);
    variants.push(wrong_attempt);
    let mut missing_retirement = valid.clone();
    missing_retirement["entries"][0]["first_attempt"] = json!(0);
    variants.push(missing_retirement);
    let schedule_disk = Disk::default();
    let _ = engine(schedule_disk.clone());
    variants.push(schedule_disk.json());
    for (i, snapshot) in variants.iter().enumerate() {
        disk.replace(snapshot);
        let before = disk.bytes();
        let writes = disk.writes();
        assert!(
            DeliveryLedger::open(disk.clone(), LEDGER).is_err(),
            "variant {i}"
        );
        assert_eq!(disk.bytes(), before);
        assert_eq!(disk.writes(), writes);
    }
    let duplicate = String::from_utf8(serde_json::to_vec(&valid).unwrap())
        .unwrap()
        .replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
    disk.0.lock().unwrap().bytes = Some(duplicate.into_bytes());
    assert!(DeliveryLedger::open(disk.clone(), LEDGER).is_err());
    disk.0.lock().unwrap().bytes = Some(vec![b' '; MAX_STATE_BYTES + 1]);
    assert!(DeliveryLedger::open(disk, LEDGER).is_err());
}

#[test]
fn malformed_or_misbound_get_and_ack_responses_never_create_evidence() {
    let (mut schedule, pending) = timer();
    let query = Request::Get {
        id: pending.key().id,
    };
    let original = get(&mut schedule, pending.key().id, 1000);
    for field in ["ok", "store_id", "revision", "item"] {
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(ItemEvidence::from_get(&query, &missing).is_err());
    }
    let mut bad = original.clone();
    bad["item"]["delivery"] = json!(0);
    assert!(ItemEvidence::from_get(&query, &bad).is_err());
    bad = original.clone();
    bad["item"]["state"] = json!("done");
    assert!(ItemEvidence::from_get(&query, &bad).is_err());
    bad = original;
    bad["item"]["text"] = json!("x".repeat(9000));
    assert_eq!(
        ItemEvidence::from_get(&query, &bad).unwrap_err().code,
        "too_large"
    );
    let mut ledger = ledger(Disk::default());
    let (_, ack) = prepare_ack(&mut ledger, &pending);
    let original = schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    for (field, value) in [
        ("ok", json!(false)),
        ("store_id", json!(OTHER)),
        ("id", json!(99)),
        ("delivery", json!(2)),
        ("acknowledged", json!(false)),
        ("revision", json!(999)),
        ("extra", json!(true)),
    ] {
        let mut bad = original.clone();
        bad[field] = value;
        assert!(AckEvidence::from_reply(&ack, &bad).is_err(), "{field}");
    }
}

#[test]
fn counters_refuse_overflow_without_wrapping_or_writing() {
    let (_, pending) = timer();
    for counter in ["revision", "next_attempt"] {
        let disk = Disk::default();
        let mut first = ledger(disk.clone());
        let prepared = first.prepare(&pending, correlation(1)).unwrap();
        let mut state = disk.json();
        if counter == "revision" {
            state["revision"] = json!(u64::MAX);
        } else {
            state["revision"] = json!(u64::MAX - 1);
            state["next_attempt"] = json!(u64::MAX);
        }
        disk.replace(&state);
        let mut recovered = ledger(disk.clone());
        let before = disk.bytes();
        let error = if counter == "revision" {
            recovered
                .complete(&prepared, &proof(&prepared))
                .unwrap_err()
        } else {
            recovered
                .retry_interrupted(&prepared, &pending, &retirement(&prepared), correlation(2))
                .unwrap_err()
        };
        assert_eq!(error.code, "exhausted");
        assert_eq!(disk.bytes(), before);
        assert!(!recovered.failed());
    }
}

#[test]
fn reopened_private_sqlite_store_preserves_completion_and_exact_ack_intent() {
    let (_, pending) = timer();
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("consumer-ledger");
    let mut ledger = DeliveryLedger::open(Store::open(&directory).unwrap(), LEDGER).unwrap();
    let prepared = ledger.prepare(&pending, correlation(1)).unwrap();
    let complete = ledger.complete(&prepared, &proof(&prepared)).unwrap();
    let ack = ledger.plan_ack(&complete, &pending).unwrap();
    drop(ledger);
    let ledger = DeliveryLedger::open(Store::open(&directory).unwrap(), OTHER).unwrap();
    assert_eq!(ledger.pending_ack(pending.key()).unwrap(), Some(ack));
    assert_eq!(
        ledger.view(pending.key()).unwrap().unwrap().completion,
        Some(proof(&prepared))
    );
}

/// #1505: a ledger as a later build writes it, with a field this build doesn't
/// know at its top and in an entry. It opens as the same ledger without them,
/// so the voice's due reminders do not wait for the file to be fixed.
#[test]
fn a_ledger_from_a_later_build_is_read() {
    let (_engine, pending) = timer();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    ledger.prepare(&pending, correlation(1)).unwrap();
    let revision = ledger.revision();
    drop(ledger);
    assert_eq!(
        DeliveryLedger::open(disk.clone(), LEDGER)
            .unwrap()
            .revision(),
        revision
    );
    let mut later = disk.json();
    later["a_field_from_a_later_build"] = json!(true);
    later["entries"][0]["a_field_from_a_later_build"] = json!(true);
    disk.replace(&later);
    assert_eq!(
        DeliveryLedger::open(disk, LEDGER).unwrap().revision(),
        revision
    );
}

#[test]
fn later_metadata_preserves_completed_delivery_and_exact_ack_without_replay() {
    let (mut schedule, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    let old = first.prepare(&pending, correlation(1)).unwrap();
    let prepared = first
        .retry_interrupted(&old, &pending, &retirement(&old), correlation(2))
        .unwrap();
    let completed = first.complete(&prepared, &proof(&prepared)).unwrap();
    let ack = first.plan_ack(&completed, &pending).unwrap();
    let response = schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    first
        .reconcile(
            pending.key(),
            &Evidence::Ack(AckEvidence::from_reply(&ack, &response).unwrap()),
        )
        .unwrap();
    let expected = first.entries().unwrap();
    let mut snapshot = disk.json();
    for path in [
        "",
        "/schedule",
        "/entries/0",
        "/entries/0/view",
        "/entries/0/view/prepared",
        "/entries/0/view/prepared/key",
        "/entries/0/view/prepared/correlation",
        "/entries/0/view/completion",
        "/entries/0/view/completion/correlation",
        "/entries/0/view/ack",
        "/entries/0/view/ack/key",
        "/entries/0/view/resolution",
        "/entries/0/previous",
        "/entries/0/previous/proof",
        "/entries/0/previous/proof/correlation",
        "/entries/0/previous/proof/basis",
    ] {
        snapshot.pointer_mut(path).unwrap()["later_metadata"] = json!({"version": 2});
    }
    disk.replace(&snapshot);
    let before = disk.bytes();
    let writes = disk.writes();
    let mut reopened = ledger(disk.clone());
    assert_eq!(reopened.entries().unwrap(), expected);
    assert_eq!(disk.bytes(), before);
    assert_eq!(disk.writes(), writes);
    assert!(reopened.set_aside().is_none());
    assert!(reopened.pending_ack(pending.key()).unwrap().is_none());
    assert!(reopened.prepare(&pending, correlation(3)).is_err());
    assert_eq!(reopened.compact_resolved().unwrap(), 1);
    assert!(
        !String::from_utf8(disk.bytes())
            .unwrap()
            .contains("later_metadata")
    );
    assert_eq!(disk.json()["ledger_id"], LEDGER);
}

#[test]
fn later_metadata_does_not_strand_an_unfinished_due_delivery() {
    let (mut schedule, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    let old = first.prepare(&pending, correlation(1)).unwrap();
    let mut snapshot = disk.json();
    snapshot["later_metadata"] = json!(true);
    snapshot["entries"][0]["view"]["later_metadata"] = json!(true);
    disk.replace(&snapshot);
    let mut reopened = ledger(disk);
    let prepared = reopened
        .retry_interrupted(&old, &pending, &retirement(&old), correlation(2))
        .unwrap();
    let completed = reopened.complete(&prepared, &proof(&prepared)).unwrap();
    let ack = reopened.plan_ack(&completed, &pending).unwrap();
    let response = schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    assert!(AckEvidence::from_reply(&ack, &response).is_ok());
    assert!(
        schedule
            .answer(
                Request::Get {
                    id: pending.key().id
                },
                &now(1000)
            )
            .is_err()
    );
}

#[test]
fn unknown_saved_enum_values_and_changed_exact_requests_are_refused_without_writes() {
    let (_, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    let old = first.prepare(&pending, correlation(1)).unwrap();
    let prepared = first
        .retry_interrupted(&old, &pending, &retirement(&old), correlation(2))
        .unwrap();
    let completed = first.complete(&prepared, &proof(&prepared)).unwrap();
    first.plan_ack(&completed, &pending).unwrap();
    let valid = disk.json();
    for (path, value) in [
        ("/entries/0/previous/proof/basis/kind", json!("later_kind")),
        (
            "/entries/0/view/resolution",
            json!({"kind": "later_kind", "revision": 3}),
        ),
        ("/entries/0/view/ack/request/op", json!("later_op")),
        (
            "/entries/0/view/ack/request",
            json!({"op": "ack", "guard": valid["entries"][0]["view"]["ack"]["request"]["guard"], "id": pending.key().id, "delivery": pending.key().delivery, "later_field": true}),
        ),
    ] {
        let mut snapshot = valid.clone();
        *snapshot.pointer_mut(path).unwrap() = value;
        disk.replace(&snapshot);
        let before = disk.bytes();
        let writes = disk.writes();
        assert_eq!(
            DeliveryLedger::open(disk.clone(), LEDGER)
                .err()
                .unwrap()
                .code,
            "invalid_snapshot",
            "{path}"
        );
        assert_eq!(disk.bytes(), before);
        assert_eq!(disk.writes(), writes);
        assert!(disk.0.lock().unwrap().kept.is_empty());
    }
}

/// The ledger saves the exact Ack request and refuses one with a field it
/// doesn't know (the test above). So a field added to `Request::Ack` or
/// `Guard` needs a `delivery::SCHEMA` bump: without one, an older build
/// refuses the whole ledger instead of setting it aside. These are the keys
/// it saves today, so a new one fails here first (Ten on #1559).
#[test]
fn the_saved_ack_request_has_exactly_these_keys() {
    use std::collections::BTreeSet;
    fn keys(value: &Value) -> BTreeSet<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }
    let (_, pending) = timer();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    let (_, ack) = prepare_ack(&mut ledger, &pending);
    assert!(matches!(ack.request, Request::Ack { .. }));
    let request = serde_json::to_value(&ack.request).unwrap();
    assert_eq!(
        keys(&request),
        BTreeSet::from(["op", "guard", "id", "delivery"])
    );
    assert_eq!(
        keys(&request["guard"]),
        BTreeSet::from(["store_id", "revision", "request_id"])
    );
    assert_eq!(disk.json()["entries"][0]["view"]["ack"]["request"], request);
}

#[test]
fn newer_schema_is_preserved_once_and_current_pending_reminders_can_complete() {
    let (mut schedule, pending) = timer();
    let disk = Disk::default();
    let mut first = ledger(disk.clone());
    prepare_ack(&mut first, &pending);
    let mut snapshot = disk.json();
    snapshot["schema"] = json!(u64::MAX);
    snapshot["entries"][0]["view"]["ack"]["request"]["op"] = json!("later_op");
    disk.replace(&snapshot);
    let newer_bytes = disk.bytes();
    let mut reopened = DeliveryLedger::open(disk.clone(), OTHER).unwrap();
    let kept = reopened.set_aside().unwrap().to_owned();
    assert!(kept.contains("newer-18446744073709551615-"));
    assert_eq!(disk.0.lock().unwrap().kept, [(kept, newer_bytes)]);
    assert_eq!(disk.json()["ledger_id"], OTHER);
    assert_eq!(disk.json()["schema"], 1);
    assert!(reopened.entries().unwrap().is_empty());
    assert!(reopened.view(pending.key()).unwrap().is_none());
    let (_, ack) = prepare_ack(&mut reopened, &pending);
    let response = schedule.answer(ack.request.clone(), &now(1000)).unwrap();
    assert!(AckEvidence::from_reply(&ack, &response).is_ok());
    drop(reopened);
    let current = ledger(disk.clone());
    assert!(current.set_aside().is_none());
    assert_eq!(disk.0.lock().unwrap().kept.len(), 1);
}

#[test]
fn failed_preservation_or_wrong_ledger_kind_never_overwrites_a_newer_snapshot() {
    for wrong_kind in [false, true] {
        let disk = Disk::default();
        let _ = ledger(disk.clone());
        let mut snapshot = disk.json();
        snapshot["schema"] = json!(2);
        if wrong_kind {
            snapshot["kind"] = json!("other_store");
        } else {
            disk.0.lock().unwrap().refuse_set_aside = true;
        }
        disk.replace(&snapshot);
        let before = disk.bytes();
        let writes = disk.writes();
        assert!(DeliveryLedger::open(disk.clone(), OTHER).is_err());
        assert_eq!(disk.bytes(), before);
        assert_eq!(disk.writes(), writes);
        assert!(disk.0.lock().unwrap().kept.is_empty());
    }
}

#[test]
fn failed_fresh_commit_keeps_the_newer_ledger_and_returns_no_usable_owner() {
    for failure in [Failure::Before, Failure::After] {
        let disk = Disk::default();
        let _ = ledger(disk.clone());
        let mut snapshot = disk.json();
        snapshot["schema"] = json!(2);
        disk.replace(&snapshot);
        let newer_bytes = disk.bytes();
        disk.fail(failure);
        assert_eq!(
            DeliveryLedger::open(disk.clone(), OTHER)
                .err()
                .unwrap()
                .code,
            "storage"
        );
        assert_eq!(disk.0.lock().unwrap().kept[0].1, newer_bytes);
        let reopened = DeliveryLedger::open(disk.clone(), OTHER).unwrap();
        assert!(reopened.set_aside().is_none());
        assert!(reopened.entries().unwrap().is_empty());
        assert_eq!(disk.0.lock().unwrap().kept.len(), 1);
        assert_eq!(disk.json()["ledger_id"], OTHER);
    }
}

#[test]
fn real_sqlite_preserves_newer_ledger_before_starting_a_reopenable_current_one() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("consumer-ledger");
    let bytes =
        br#"{"kind":"lamp_schedule_delivery_ledger","schema":2,"later_metadata":{"keep":"whole"}}"#;
    let mut store = Store::open(&directory).unwrap();
    store.commit(bytes).unwrap();
    drop(store);
    let reopened = DeliveryLedger::open(Store::open(&directory).unwrap(), LEDGER).unwrap();
    let kept = reopened.set_aside().unwrap().to_owned();
    let archived = rusqlite::Connection::open_with_flags(
        directory.join(&kept),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let preserved: Vec<u8> = archived
        .query_row("SELECT snapshot FROM state WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(preserved, bytes);
    drop(archived);
    drop(reopened);
    let current = DeliveryLedger::open(Store::open(&directory).unwrap(), OTHER).unwrap();
    assert!(current.set_aside().is_none());
    assert!(current.entries().unwrap().is_empty());
    assert!(directory.join(kept).is_file());
}

/// The delivery ledgers kept as fixtures (`tests/fixtures/stores/`), one for
/// each change of the ledger's shape, each written by its day's build
/// ([`write_a_delivery_ledger_fixture`]). The voice's consumer ledger
/// outlives an update in its own store, so every later build opens each as
/// it was kept: nothing set aside or rewritten, its revision and Schedule's
/// fence, and every delivery where it was, so no reminder said is said again
/// and none due is lost (the build row "Updates cover the whole lamp, V3 to
/// V5").
#[test]
fn each_kept_delivery_ledger_opens_with_every_delivery_where_it_was() {
    let kept: [(&str, &str); 1] = [(
        "2026-10-21",
        include_str!("../fixtures/stores/delivery-2026-10-21.json"),
    )];
    for (day, text) in kept {
        let saved: Value = serde_json::from_str(text).unwrap();
        // The fixture holds each stage, the retried attempt's record too.
        assert!(!saved["entries"][1]["previous"].is_null(), "{day}");
        let disk = Disk::default();
        disk.0.lock().unwrap().bytes = Some(text.as_bytes().to_vec());
        let ledger = DeliveryLedger::open(disk.clone(), OTHER)
            .unwrap_or_else(|error| panic!("{day}: {error:?}"));
        assert_eq!(ledger.set_aside(), None, "{day}");
        assert!(!ledger.failed(), "{day}");
        assert_eq!(
            disk.bytes(),
            text.as_bytes(),
            "{day}: opened, not rewritten"
        );
        assert_eq!(
            ledger.revision(),
            saved["revision"].as_u64().unwrap(),
            "{day}"
        );
        assert_eq!(
            ledger.schedule_revision(),
            saved["schedule"]["revision"].as_u64(),
            "{day}"
        );
        let views = ledger.entries().unwrap();
        let kept_views: Vec<Value> = saved["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["view"].clone())
            .collect();
        assert_eq!(
            missing(
                &Value::Array(kept_views),
                &serde_json::to_value(&views).unwrap(),
                "entries",
            ),
            None,
            "{day}"
        );
        let stages: Vec<(bool, bool)> = views
            .iter()
            .map(|view| (view.completion.is_some(), view.ack.is_some()))
            .collect();
        assert_eq!(
            stages,
            [(true, true), (false, false), (false, false)],
            "{day}"
        );
        assert!(
            ledger
                .pending_ack(&views[0].prepared.key)
                .unwrap()
                .is_some(),
            "{day}"
        );
    }
}

/// The first thing a kept record holds that `read` doesn't: a key of an
/// object missing or different, by its path, an array of another length, or
/// another value. A key `read` has beyond the kept one is a later build's
/// addition, and passes: a kept day's test is never edited (#1505; the
/// reviewer on #2486).
fn missing(kept: &serde_json::Value, read: &serde_json::Value, path: &str) -> Option<String> {
    use serde_json::Value;
    match (kept, read) {
        (Value::Object(kept), Value::Object(read)) => kept.iter().find_map(|(key, value)| {
            let path = format!("{path}.{key}");
            match read.get(key) {
                Some(got) => missing(value, got, &path),
                None => Some(format!("{path}: not read")),
            }
        }),
        (Value::Array(kept), Value::Array(read)) if kept.len() == read.len() => kept
            .iter()
            .zip(read)
            .enumerate()
            .find_map(|(i, (value, got))| missing(value, got, &format!("{path}[{i}]"))),
        _ if kept == read => None,
        _ => Some(format!("{path}: kept {kept}, read {read}")),
    }
}

/// Writes this build's delivery ledger to the path in
/// `LAMP_SCHEDULE_DELIVERY_FIXTURE`: three timers at three stages, one said
/// with its acknowledgement planned, one tried again after an interrupted
/// attempt, and one only prepared. Run by hand when the ledger's shape
/// changes, keeping the ones kept.
#[test]
#[ignore = "writes a delivery ledger fixture: run by hand with LAMP_SCHEDULE_DELIVERY_FIXTURE set"]
fn write_a_delivery_ledger_fixture() {
    let target =
        std::env::var_os("LAMP_SCHEDULE_DELIVERY_FIXTURE").expect("LAMP_SCHEDULE_DELIVERY_FIXTURE");
    let mut engine = engine(Disk::default());
    let ids: Vec<u64> = (1..=3)
        .map(|seconds| set(&mut engine, 0, When::After { seconds }))
        .collect();
    let due: Vec<PendingEvidence> = ids
        .iter()
        .map(|&id| pending(&mut engine, id, 4000))
        .collect();
    let disk = Disk::default();
    let mut ledger = ledger(disk.clone());
    // Said, its acknowledgement planned.
    let said = ledger.prepare(&due[0], correlation(1)).unwrap();
    let done = ledger.complete(&said, &proof(&said)).unwrap();
    ledger.plan_ack(&done, &due[0]).unwrap();
    // An interrupted attempt, retired and tried again.
    let cut = ledger.prepare(&due[1], correlation(2)).unwrap();
    ledger
        .retry_interrupted(&cut, &due[1], &retirement(&cut), correlation(3))
        .unwrap();
    // Only prepared.
    ledger.prepare(&due[2], correlation(4)).unwrap();
    std::fs::write(target, disk.bytes()).unwrap();
}
