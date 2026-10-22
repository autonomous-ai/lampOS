use super::*;
use crate::{
    face_repair::{
        AccountGeneration, Candidate, ConsentProof, Controller, FrameObservation, SpeechRequest,
        Ticket,
    },
    who::{MOST_PRINTS, PRINT, SET_ASIDE},
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{atomic::AtomicU64, mpsc},
    time::Duration,
};

const BOOT: &str = "0123456789abcdef";

struct Fixture {
    root: PathBuf,
    people: Option<People>,
    other: Value,
    target_id: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "eyes-repair-writer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut people = People::open(root.join("people.json"))
            .unwrap()
            .with_stable_ids(true);
        people
            .remember("Fixture Target", vec![print(0)], 10)
            .unwrap();
        people
            .remember("Fixture Other", vec![print(1)], 11)
            .unwrap();
        people.tell_visits("Fixture Other", true).unwrap();
        let target_id = people.id_of("Fixture Target").unwrap();
        let other = read_people(&root)
            .into_iter()
            .find(|p| p["name"] == "Fixture Other")
            .unwrap();
        Self {
            root,
            people: Some(people),
            other,
            target_id,
        }
    }

    fn work(&self, at: Instant) -> (Controller, Ticket, CommitWork) {
        // Complete the approval fixture before the writer's independent clock
        // begins. This module does not manufacture or revalidate authority.
        let at = at - Duration::from_secs(2);
        let mut controller = Controller::new(true, BOOT).unwrap();
        let target = self
            .people
            .as_ref()
            .unwrap()
            .repair_target("Fixture Target")
            .unwrap();
        let ticket = controller.start(account(), target, at).unwrap();
        let face = Candidate::from_source(BOOT, "f1").unwrap();
        controller
            .observe(
                account(),
                FrameObservation::from_source(Some(face), 1, at),
                true,
                at,
            )
            .unwrap();
        let proof = ConsentProof::from_trusted_ingress(
            ticket,
            SpeechRequest::from_trusted_ingress([3; 16], 1, 1).unwrap(),
            face,
            at,
            at + Duration::from_secs(30),
        )
        .unwrap();
        controller
            .accept_consent(account(), ticket, proof, at)
            .unwrap();
        for index in 1..=MOST_PRINTS {
            let now = at + Duration::from_millis(index as u64 * 100);
            controller
                .observe(
                    account(),
                    FrameObservation::from_source(Some(face), 1, now),
                    true,
                    now,
                )
                .unwrap();
            controller
                .push_print(account(), ticket, face, now, print(2), now)
                .unwrap();
        }
        let work = controller
            .begin_commit(account(), ticket, true, at + Duration::from_secs(1))
            .unwrap();
        (controller, ticket, work)
    }

    fn start(
        &mut self,
        clock: Clock,
        hook: impl FnMut(Checkpoint) -> io::Result<()> + Send + 'static,
    ) -> Writer {
        Writer::start_with(self.people.take().unwrap(), clock, hook).unwrap()
    }

    fn assert_only_other(&self) {
        assert_eq!(read_people(&self.root), vec![self.other.clone()]);
        assert!(!self.root.join("people.json.repair.new").exists());
        assert!(!self.root.join("people.json.new").exists());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn account() -> AccountGeneration {
    AccountGeneration::from_trusted_ingress([1; 16]).unwrap()
}
fn print(axis: usize) -> Vec<f32> {
    let mut value = vec![0.0; PRINT];
    value[axis] = 1.0;
    value
}
fn read_people(root: &std::path::Path) -> Vec<Value> {
    let kept: Value =
        serde_json::from_slice(&std::fs::read(root.join("people.json")).unwrap()).unwrap();
    kept["people"].as_array().unwrap().clone()
}
fn clock(at: Instant) -> (Clock, Arc<AtomicU64>) {
    let elapsed = Arc::new(AtomicU64::new(0));
    let read = elapsed.clone();
    (
        Arc::new(move || at + Duration::from_millis(read.load(Ordering::SeqCst))),
        elapsed,
    )
}
fn forget_id(value: u64) -> ForgetId {
    ForgetId::from_trusted_runtime(value).unwrap()
}
fn until(mut ready: impl FnMut() -> bool) {
    let limit = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(
            Instant::now() < limit,
            "worker did not reach the test boundary"
        );
        std::thread::yield_now();
    }
}
// Contention is an explicit, retryable API result, not a fixture scheduling
// assumption. No real storage deadline is advanced by these bounded retries.
fn submit(
    writer: &Writer,
    mut work: CommitWork,
    deadline: Instant,
) -> Result<RepairId, RejectedRepair> {
    let limit = Instant::now() + Duration::from_secs(3);
    loop {
        match writer.submit_repair(work, deadline) {
            Err(rejected) if rejected.reason == AdmissionError::Contended => {
                assert!(Instant::now() < limit, "admission remained contended");
                work = *rejected.work;
                std::thread::yield_now();
            }
            other => return other,
        }
    }
}
fn queue(
    writer: &Writer,
    id: ForgetId,
    name: &str,
    asked: u64,
    deadline: Instant,
) -> Result<Admission, AdmissionError> {
    let mut result = Err(AdmissionError::Contended);
    until(|| {
        result = writer.queue_forget(id, name, asked, deadline);
        result != Err(AdmissionError::Contended)
    });
    result
}
fn read<T>(mut action: impl FnMut() -> Result<T, ReadError>) -> Result<T, ReadError> {
    let limit = Instant::now() + Duration::from_secs(3);
    loop {
        match action() {
            Err(ReadError::Contended) => {
                assert!(Instant::now() < limit, "read remained contended");
                std::thread::yield_now();
            }
            other => return other,
        }
    }
}
fn repair_status(writer: &Writer, id: &RepairId) -> Result<RepairStatus, ReadError> {
    read(|| writer.repair_status(id))
}
fn forget_status(writer: &Writer, id: ForgetId) -> Result<ForgetStatus, ReadError> {
    read(|| writer.forget_status(id))
}
fn take_repair(writer: &Writer, id: &RepairId) -> RepairCompletion {
    let mut result = None;
    until(|| {
        match writer.take_repair(id) {
            Ok(value) => result = value,
            Err(ReadError::Contended) => {}
            Err(error) => panic!("unexpected read: {error:?}"),
        }
        result.is_some()
    });
    result.unwrap()
}
fn take_forget(writer: &Writer, id: ForgetId) -> Result<ForgetCompletion, ForgetError> {
    let mut result = None;
    until(|| {
        match writer.take_forget(id) {
            Ok(value) => result = value,
            Err(ReadError::Contended) => {}
            Err(error) => panic!("unexpected read: {error:?}"),
        }
        result.is_some()
    });
    result.unwrap()
}
fn close(writer: &Writer) {
    writer.close();
    until(|| writer.stopped());
}

struct Barrier {
    entered: mpsc::Receiver<()>,
    release: Option<mpsc::Sender<()>>,
}
impl Barrier {
    fn at(
        point: Checkpoint,
        fail: bool,
    ) -> (Self, impl FnMut(Checkpoint) -> io::Result<()> + Send) {
        let (arrived, entered) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let mut reached = false;
        let hook = move |at| {
            if at == point && !reached {
                reached = true;
                arrived.send(()).unwrap();
                released
                    .recv_timeout(Duration::from_secs(3))
                    .map_err(|_| io::Error::other("test barrier was not released"))?;
                if fail {
                    return Err(io::Error::other("synthetic storage failure"));
                }
            }
            Ok(())
        };
        (
            Self {
                entered,
                release: Some(release),
            },
            hook,
        )
    }
    fn entered(&self) {
        self.entered.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
impl Drop for Barrier {
    fn drop(&mut self) {
        self.release();
    }
}

#[test]
fn late_before_rename_is_not_written_and_the_priority_forget_still_runs() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (mut controller, ticket, work) = fixture.work(at);
    let (clock, elapsed) = clock(at);
    let (mut barrier, hook) = Barrier::at(Checkpoint::Repair(RepairStep::Rename), false);
    let writer = fixture.start(clock, hook);
    let id = submit(&writer, work, at + Duration::from_millis(10))
        .ok()
        .unwrap();
    barrier.entered();
    elapsed.store(10, Ordering::SeqCst);
    assert_eq!(
        repair_status(&writer, &id).unwrap(),
        RepairStatus {
            deadline_exceeded: true,
            storage: RepairStorage::Pending
        }
    );
    assert_eq!(
        controller.invalidate(at + Duration::from_secs(2)),
        Ok(crate::face_repair::Status::Committing)
    );
    let forget = forget_id(1);
    assert_eq!(
        queue(
            &writer,
            forget,
            "Fixture Target",
            606,
            at + Duration::from_millis(5)
        ),
        Ok(Admission::Accepted)
    );
    assert_eq!(
        queue(&writer, forget, "Fixture Target", 606, at),
        Ok(Admission::Existing)
    );
    assert_eq!(
        queue(&writer, forget, "Fixture Other", 606, at),
        Err(AdmissionError::Conflict)
    );
    assert_eq!(
        queue(&writer, forget_id(2), "Fixture Other", 606, at),
        Err(AdmissionError::Busy)
    );
    assert_eq!(
        forget_status(&writer, forget).unwrap().storage,
        ForgetStorage::Pending
    );
    assert_eq!(
        read_people(&fixture.root).len(),
        2,
        "nothing forgotten at admission"
    );
    barrier.release();
    let completion = take_repair(&writer, &id);
    assert_eq!(completion.result, Err(RepairError::NotWritten));
    assert!(completion.deadline_exceeded);
    assert_eq!(
        controller.complete(completion.completion, completion.result),
        Ok(crate::face_repair::Status::NotWritten)
    );
    assert!(
        read(|| writer.take_repair(&id)).unwrap().is_none(),
        "completion consumed once"
    );
    let forgotten = take_forget(&writer, forget).ok().unwrap();
    assert_eq!(
        forgotten.result,
        Forgot {
            forgot: true,
            unsearched: 0
        }
    );
    assert_eq!(
        forgotten.stable_id.as_deref(),
        Some(fixture.target_id.as_str())
    );
    assert!(forgotten.name_id_free);
    assert!(forgotten.deadline_exceeded);
    assert_eq!(
        controller.status(account(), ticket, at + Duration::from_secs(3)),
        Some(crate::face_repair::Status::NotWritten)
    );
    fixture.assert_only_other();
    close(&writer);
}

#[test]
fn late_after_rename_keeps_actual_success_or_indeterminate_before_forgetting() {
    for fail in [false, true] {
        let mut fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, _, work) = fixture.work(at);
        let (clock, elapsed) = clock(at);
        let (mut barrier, hook) = Barrier::at(Checkpoint::Repair(RepairStep::SyncDirectory), fail);
        let writer = fixture.start(clock, hook);
        let id = submit(&writer, work, at + Duration::from_millis(10))
            .ok()
            .unwrap();
        barrier.entered();
        let target = read_people(&fixture.root)
            .into_iter()
            .find(|p| p["name"] == "Fixture Target")
            .unwrap();
        assert_eq!(
            target["prints"][0],
            serde_json::json!(print(2)),
            "rename is already visible"
        );
        elapsed.store(20, Ordering::SeqCst);
        let forget = forget_id(1);
        assert_eq!(
            queue(&writer, forget, "Fixture Target", 606, at),
            Ok(Admission::Accepted)
        );
        assert_eq!(
            repair_status(&writer, &id).unwrap().storage,
            RepairStorage::Pending
        );
        assert_eq!(
            forget_status(&writer, forget).unwrap().storage,
            ForgetStorage::Pending
        );
        barrier.release();
        let completion = take_repair(&writer, &id);
        assert_eq!(
            completion.result,
            if fail {
                Err(RepairError::Indeterminate)
            } else {
                Ok(())
            }
        );
        assert!(completion.deadline_exceeded);
        assert_eq!(
            controller.complete(completion.completion, completion.result),
            Ok(if fail {
                crate::face_repair::Status::Indeterminate
            } else {
                crate::face_repair::Status::Committed
            })
        );
        let forgotten = take_forget(&writer, forget).ok().unwrap();
        assert!(forgotten.result.forgot);
        assert_eq!(
            forgotten.stable_id.as_deref(),
            Some(fixture.target_id.as_str())
        );
        fixture.assert_only_other();
        close(&writer);
    }
}

#[test]
fn a_forget_admitted_after_store_return_precedes_any_new_work_or_owner_poll() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (_, _, work) = fixture.work(at);
    let (_, _, extra) = fixture.work(at);
    let (clock, _) = clock(at);
    let (mut barrier, hook) = Barrier::at(Checkpoint::RepairReturned, false);
    let writer = fixture.start(clock, hook);
    let id = submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    barrier.entered();
    let forget = forget_id(1);
    assert_eq!(
        queue(
            &writer,
            forget,
            "Fixture Target",
            606,
            at + Duration::from_secs(30)
        ),
        Ok(Admission::Accepted)
    );
    let rejected = submit(&writer, extra, at + Duration::from_secs(30))
        .err()
        .unwrap();
    assert_eq!(rejected.reason, AdmissionError::Busy);
    barrier.release();
    let forgotten = take_forget(&writer, forget).ok().unwrap();
    assert!(
        forgotten.result.forgot,
        "no repair-completion poll needed to dispatch forget"
    );
    fixture.assert_only_other();
    assert_eq!(take_repair(&writer, &id).result, Ok(()));
    close(&writer);
}

#[test]
fn dropping_the_request_owner_does_not_cancel_an_accepted_forget() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (_, _, work) = fixture.work(at);
    let (clock, _) = clock(at);
    let (mut barrier, hook) = Barrier::at(Checkpoint::Repair(RepairStep::SyncDirectory), false);
    let writer = fixture.start(clock, hook);
    submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    barrier.entered();
    assert_eq!(
        queue(&writer, forget_id(1), "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    let shared = writer.shared.clone();
    drop(writer);
    barrier.release();
    until(|| shared.stopped.load(Ordering::Acquire));
    fixture.assert_only_other();
}

#[test]
fn bounded_receipts_never_replay_an_evicted_forget_or_evict_live_work() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (_, _, work) = fixture.work(at);
    let (clock, _) = clock(at);
    let calls = Arc::new(AtomicU64::new(0));
    let counted = calls.clone();
    let (mut barrier, mut held_forget) = Barrier::at(Checkpoint::Forget, false);
    let writer = fixture.start(clock, move |step| {
        if step == Checkpoint::Forget && counted.fetch_add(1, Ordering::SeqCst) == 7 {
            held_forget(step)?;
        }
        Ok(())
    });
    for number in 1..=7 {
        let id = forget_id(number);
        assert_eq!(
            queue(&writer, id, "Fixture Absent", 606, at),
            Ok(Admission::Accepted)
        );
        assert!(!take_forget(&writer, id).ok().unwrap().result.forgot);
        assert_eq!(
            queue(&writer, id, "Fixture Absent", 606, at),
            Err(AdmissionError::StaleId)
        );
        assert!(read(|| writer.take_forget(id)).unwrap().is_none());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 7);
    assert_eq!(
        queue(&writer, forget_id(1), "Fixture Absent", 606, at),
        Err(AdmissionError::StaleId)
    );
    assert_eq!(
        writer.shared.state.lock().unwrap().receipts.len(),
        MOST_RECEIPTS
    );
    assert_eq!(read_people(&fixture.root).len(), 2);
    let repair = submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    until(|| {
        matches!(
            repair_status(&writer, &repair).unwrap().storage,
            RepairStorage::Finished(_)
        )
    });
    let live = forget_id(8);
    assert_eq!(
        queue(&writer, live, "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    barrier.entered();
    // Acknowledging the repair evicts an old terminal receipt, never the live
    // forget, even while that forget is in the worker rather than the queue.
    assert_eq!(take_repair(&writer, &repair).result, Ok(()));
    assert_eq!(
        writer.shared.state.lock().unwrap().receipts.len(),
        MOST_RECEIPTS
    );
    assert_eq!(
        forget_status(&writer, live).unwrap().storage,
        ForgetStorage::Pending
    );
    assert_eq!(
        queue(&writer, live, "Fixture Target", 606, at),
        Ok(Admission::Existing)
    );
    assert_eq!(
        queue(&writer, forget_id(9), "Fixture Other", 606, at),
        Err(AdmissionError::Busy)
    );
    barrier.release();
    assert!(take_forget(&writer, live).ok().unwrap().result.forgot);
    fixture.assert_only_other();
    close(&writer);
}

#[test]
fn acknowledged_forget_retains_only_status_and_never_replays_any_label() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (clock, _) = clock(at);
    let calls = Arc::new(AtomicU64::new(0));
    let counted = calls.clone();
    let writer = fixture.start(clock, move |step| {
        if step == Checkpoint::Forget {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    });
    let id = forget_id(1);
    let deadline = at + Duration::from_secs(30);
    assert_eq!(
        queue(&writer, id, "Fixture Target", 606, deadline),
        Ok(Admission::Accepted)
    );
    until(|| {
        matches!(
            forget_status(&writer, id).unwrap().storage,
            ForgetStorage::Finished(_)
        )
    });
    assert_eq!(
        queue(&writer, id, "Fixture Target", 606, deadline),
        Ok(Admission::Existing),
        "completed but unacknowledged is still pinned"
    );
    assert_eq!(
        queue(&writer, id, "Fixture Other", 606, deadline),
        Err(AdmissionError::Conflict)
    );
    assert_eq!(
        queue(&writer, forget_id(2), "Fixture Other", 606, deadline),
        Err(AdmissionError::Busy)
    );
    assert!(take_forget(&writer, id).ok().unwrap().result.forgot);
    let status = forget_status(&writer, id).unwrap();
    assert_eq!(
        status.storage,
        ForgetStorage::Finished(Ok(Forgot {
            forgot: true,
            unsearched: 0
        }))
    );
    {
        let state = writer.shared.state.lock().unwrap();
        assert!(
            state.forget.is_none(),
            "no live record keeps the forgotten name"
        );
        assert_eq!(state.receipts.len(), 1);
        // This owned Copy value and exhaustive field pattern guard the receipt
        // shape: no String, biometric snapshot or private stable ID is retained.
        let receipt = *state.receipts.back().unwrap();
        match receipt {
            Receipt::Forget {
                id: kept,
                status: kept_status,
            } => {
                assert!(kept == id);
                assert_eq!(kept_status, status);
            }
            Receipt::Repair { .. } => panic!("wrong receipt kind"),
        }
    }
    for name in [
        "Fixture Target",
        "Fixture Other",
        "not/a/name",
        &"x".repeat(161),
    ] {
        assert_eq!(
            queue(&writer, id, name, 707, at),
            Err(AdmissionError::StaleId)
        );
    }
    assert_eq!(forget_status(&writer, id).unwrap(), status);
    assert!(read(|| writer.take_forget(id)).unwrap().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fixture.assert_only_other();
    close(&writer);
}

#[test]
fn a_failed_priority_forget_before_worker_start_still_cancels_the_queued_repair() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (_, _, work) = fixture.work(at);
    let before = read_people(&fixture.root);
    let (clock, _) = clock(at);
    let (mut barrier, hook) = Barrier::at(Checkpoint::Started, false);
    let writer = fixture.start(clock, hook);
    barrier.entered();
    let repair = submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    let forget = forget_id(1);
    std::fs::create_dir(fixture.root.join("people.json.repair.new")).unwrap();
    assert_eq!(
        queue(&writer, forget, "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    barrier.release();
    assert!(matches!(
        take_forget(&writer, forget),
        Err(ForgetError::Storage)
    ));
    assert_eq!(
        take_repair(&writer, &repair).result,
        Err(RepairError::NotWritten)
    );
    assert_eq!(read_people(&fixture.root), before);
    close(&writer);
}

#[test]
fn stale_store_reservation_is_an_actual_refusal_and_keeps_the_controller_completion() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (mut controller, _, work) = fixture.work(at);
    fixture
        .people
        .as_mut()
        .unwrap()
        .tell_visits("Fixture Other", false)
        .unwrap();
    let before = read_people(&fixture.root);
    let (clock, _) = clock(at);
    let writer = fixture.start(clock, |_| Ok(()));
    let id = submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    let done = take_repair(&writer, &id);
    assert_eq!(done.result, Err(RepairError::TargetChanged));
    assert_eq!(
        controller.complete(done.completion, done.result),
        Ok(crate::face_repair::Status::Refused)
    );
    assert_eq!(read_people(&fixture.root), before);
    close(&writer);
}

#[test]
fn a_repair_read_key_cannot_take_another_writers_completion() {
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    let at = Instant::now();
    let (_, _, first_work) = first.work(at);
    let (_, _, second_work) = second.work(at);
    let (first_clock, _) = clock(at);
    let (second_clock, _) = clock(at);
    let first_writer = first.start(first_clock, |_| Ok(()));
    let second_writer = second.start(second_clock, |_| Ok(()));
    let first_id = submit(&first_writer, first_work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    let second_id = submit(&second_writer, second_work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    assert_eq!(
        first_writer.repair_status(&second_id),
        Err(ReadError::Unknown)
    );
    assert!(matches!(
        second_writer.take_repair(&first_id),
        Err(ReadError::Unknown)
    ));
    assert_eq!(take_repair(&first_writer, &first_id).result, Ok(()));
    assert_eq!(take_repair(&second_writer, &second_id).result, Ok(()));
    close(&first_writer);
    close(&second_writer);
}

#[test]
fn invalid_names_conflicting_retries_and_closed_admission_do_not_write() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (clock, _) = clock(at);
    let writer = fixture.start(clock, |_| Ok(()));
    assert!(ForgetId::from_trusted_runtime(0).is_none());
    for name in ["", "../private", "Fixture 2", &"x".repeat(161)] {
        assert_eq!(
            queue(&writer, forget_id(1), name, 606, at),
            Err(AdmissionError::InvalidName)
        );
    }
    close(&writer);
    assert_eq!(
        queue(&writer, forget_id(1), "Fixture Target", 606, at),
        Err(AdmissionError::Closed)
    );
    assert_eq!(read_people(&fixture.root).len(), 2);
}

#[test]
fn a_real_forget_cleanup_error_is_reported_without_a_false_forgotten_result() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let unexpected = fixture.root.join("people.json.repair.new");
    std::fs::create_dir(&unexpected).unwrap();
    std::fs::write(unexpected.join("keep"), b"synthetic retained copy").unwrap();
    let original = read_people(&fixture.root);
    let (clock, _) = clock(at);
    let writer = fixture.start(clock, |_| Ok(()));
    let id = forget_id(1);
    assert_eq!(
        queue(&writer, id, "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    assert!(matches!(
        take_forget(&writer, id),
        Err(ForgetError::Storage)
    ));
    assert_eq!(
        forget_status(&writer, id).unwrap().storage,
        ForgetStorage::Finished(Err(ForgetError::Storage))
    );
    assert_eq!(read_people(&fixture.root), original);
    assert_eq!(
        std::fs::read(unexpected.join("keep")).unwrap(),
        b"synthetic retained copy"
    );
    close(&writer);
}

#[test]
fn actual_set_aside_forget_keeps_unsearched_and_stable_id_semantics() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let aside = fixture.root.join(SET_ASIDE);
    std::fs::create_dir(&aside).unwrap();
    std::fs::write(
        aside.join("synthetic.json"),
        b"unsearchable synthetic store",
    )
    .unwrap();
    let (clock, _) = clock(at);
    let writer = fixture.start(clock, |_| Ok(()));
    let id = forget_id(1);
    assert_eq!(
        queue(&writer, id, "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    let result = take_forget(&writer, id).ok().unwrap();
    assert_eq!(
        result.result,
        Forgot {
            forgot: true,
            unsearched: 1
        }
    );
    assert_eq!(
        result.stable_id.as_deref(),
        Some(fixture.target_id.as_str())
    );
    assert!(result.name_id_free);
    fixture.assert_only_other();
    assert!(aside.join("synthetic.json").exists());
    close(&writer);
}

#[test]
fn actual_forget_keeps_another_renamed_persons_by_name_memory_guard() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let people = fixture.people.as_mut().unwrap();
    let other_id = people.id_of("Fixture Other").unwrap();
    people
        .remember_one("Fixture Renamed", vec![print(0)], 20, true, false)
        .unwrap();
    people
        .remember_one("Fixture Target", vec![print(1)], 21, true, false)
        .unwrap();
    assert_eq!(
        people.id_of("Fixture Target").as_deref(),
        Some(other_id.as_str())
    );
    assert_eq!(
        people.id_of("Fixture Renamed").as_deref(),
        Some(fixture.target_id.as_str())
    );
    fixture.other = read_people(&fixture.root)
        .into_iter()
        .find(|person| person["name"] == "Fixture Renamed")
        .unwrap();
    let (clock, _) = clock(at);
    let writer = fixture.start(clock, |_| Ok(()));
    let id = forget_id(1);
    assert_eq!(
        queue(&writer, id, "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    let result = take_forget(&writer, id).ok().unwrap();
    assert!(result.result.forgot);
    assert_eq!(result.stable_id.as_deref(), Some(other_id.as_str()));
    assert!(
        !result.name_id_free,
        "the renamed person's by-name memory must stay"
    );
    fixture.assert_only_other();
    close(&writer);
}

#[test]
fn worker_failure_is_unknown_and_never_starts_a_second_writer_or_reports_forget() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (_, _, work) = fixture.work(at);
    let (clock, _) = clock(at);
    let (mut barrier, mut hook) = Barrier::at(Checkpoint::Repair(RepairStep::Rename), false);
    let writer = fixture.start(clock, move |step| {
        hook(step)?;
        assert_ne!(
            step,
            Checkpoint::Repair(RepairStep::Rename),
            "synthetic worker failure"
        );
        Ok(())
    });
    let id = submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    barrier.entered();
    let forget = forget_id(1);
    assert_eq!(
        queue(&writer, forget, "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    barrier.release();
    until(|| writer.stopped());
    assert_eq!(
        repair_status(&writer, &id).unwrap().storage,
        RepairStorage::Unknown
    );
    assert_eq!(
        forget_status(&writer, forget).unwrap().storage,
        ForgetStorage::Unknown
    );
    assert!(matches!(
        writer.take_repair(&id),
        Err(ReadError::WorkerFailed)
    ));
    assert!(matches!(
        writer.take_forget(forget),
        Err(ReadError::WorkerFailed)
    ));
    assert_eq!(
        queue(&writer, forget_id(2), "Fixture Other", 606, at),
        Err(AdmissionError::WorkerFailed)
    );
    assert_eq!(read_people(&fixture.root).len(), 2);
}

#[test]
fn status_and_admission_do_not_wait_for_a_blocked_filesystem_or_state_lock() {
    let mut fixture = Fixture::new();
    let at = Instant::now();
    let (_, _, work) = fixture.work(at);
    let (clock, _) = clock(at);
    let (mut barrier, hook) = Barrier::at(Checkpoint::Repair(RepairStep::SyncDirectory), false);
    let writer = fixture.start(clock, hook);
    let id = submit(&writer, work, at + Duration::from_secs(30))
        .ok()
        .unwrap();
    barrier.entered();
    let first = Instant::now();
    assert_eq!(
        repair_status(&writer, &id).unwrap().storage,
        RepairStorage::Pending
    );
    let first_ns = first.elapsed().as_nanos();
    let mut maximum_ns = 0;
    for _ in 0..100 {
        let start = Instant::now();
        assert_eq!(
            repair_status(&writer, &id).unwrap().storage,
            RepairStorage::Pending
        );
        maximum_ns = maximum_ns.max(start.elapsed().as_nanos());
    }
    let guard = writer.shared.state.lock().unwrap();
    let start = Instant::now();
    assert_eq!(writer.repair_status(&id), Err(ReadError::Contended));
    assert_eq!(
        writer.queue_forget(forget_id(1), "Fixture Target", 606, at),
        Err(AdmissionError::Contended)
    );
    assert!(
        start.elapsed() < Duration::from_millis(100),
        "try_lock must not wait"
    );
    drop(guard);
    let start = Instant::now();
    assert_eq!(
        queue(&writer, forget_id(1), "Fixture Target", 606, at),
        Ok(Admission::Accepted)
    );
    let admission_ns = start.elapsed().as_nanos();
    eprintln!(
        "repair writer host status/admission nanoseconds: first={first_ns} repeated_max={maximum_ns} admission={admission_ns}; target=1000000, filesystem held"
    );
    barrier.release();
    take_repair(&writer, &id);
    take_forget(&writer, forget_id(1)).ok().unwrap();
    close(&writer);
}

#[test]
fn already_expired_work_is_not_written_and_completed_on_time_stays_on_time() {
    for expired in [true, false] {
        let mut fixture = Fixture::new();
        let at = Instant::now();
        let (_, _, work) = fixture.work(at);
        let (clock, elapsed) = clock(at);
        let calls = Arc::new(AtomicU64::new(0));
        let counted = calls.clone();
        let writer = fixture.start(clock, move |point| {
            if matches!(point, Checkpoint::Repair(_)) {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        });
        let id = submit(
            &writer,
            work,
            if expired {
                at
            } else {
                at + Duration::from_secs(1)
            },
        )
        .ok()
        .unwrap();
        let result = take_repair(&writer, &id);
        assert_eq!(
            result.result,
            if expired {
                Err(RepairError::NotWritten)
            } else {
                Ok(())
            }
        );
        assert_eq!(result.deadline_exceeded, expired);
        if expired {
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
        elapsed.store(2000, Ordering::SeqCst);
        assert_eq!(
            repair_status(&writer, &id).unwrap().deadline_exceeded,
            expired,
            "a later read cannot reclassify actual completion timing"
        );
        close(&writer);
    }
}
