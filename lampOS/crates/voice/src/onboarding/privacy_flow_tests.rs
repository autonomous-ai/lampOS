use super::*;
use lamp_msgs::{ClockStamp, PrivacySwitch};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const T: u64 = 1_000_000;
fn epoch() -> privacy::Epoch {
    privacy::Epoch {
        device_boot: "device-boot".into(),
        clock_id: "clock".into(),
    }
}
fn receipt(seq: u64, at: u64, muted: bool) -> PrivacyReceipt {
    PrivacyReceipt {
        controls_session: "controls".into(),
        physical: PrivacySwitch {
            stamp: ClockStamp {
                clock_id: "clock".into(),
                at_us: at,
                sequence: seq,
            },
            reader_session_id: "reader".into(),
            muted: Some(muted),
            fault: None,
        },
    }
}
fn face() -> Option<Framing> {
    Some(Framing {
        size: 0.2,
        x: 0.0,
        y: 0.0,
    })
}
struct Store {
    path: PathBuf,
    owner: storage::ServiceStorage,
}
impl Store {
    fn new(hook: Option<Box<dyn FnMut(storage::Tag) -> io::Result<()> + Send>>) -> Self {
        let path = std::env::temp_dir().join(format!(
            "onboarding-flow-{}",
            crate::process::boot_text(crate::process::new_boot().unwrap())
        ));
        std::fs::create_dir(&path).unwrap();
        let paths = storage::Paths {
            progress: path.join("progress.json"),
            checkpoint: path.join("privacy-pending.json"),
            mailbox: path.join("progress.json.told"),
            near_call: path.join("near-call.json"),
            room_sounds: path.join("room-sounds.json"),
            room_sounds_group: None,
        };
        let owner = match hook {
            Some(hook) => storage::ServiceStorage::start_with_hook(paths, hook).unwrap(),
            None => storage::ServiceStorage::start(paths).unwrap(),
        };
        Self { path, owner }
    }
    fn cycle(&self, now: u64, seq: u64) -> Onboarding {
        let mut walk =
            Onboarding::with_storage(self.path.join("progress.json"), self.owner.client());
        context(&mut walk, now, seq, false);
        walk
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        if self.owner.finish(Duration::from_secs(2)).ok()
            == Some(storage::StorageShutdown::Finished)
        {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
fn context(walk: &mut Onboarding, now: u64, seq: u64, muted: bool) {
    walk.set_privacy_context(
        epoch(),
        true,
        Some(receipt(seq, now, muted)),
        Some("at its verified profile location"),
    );
}
// Explicit fixture waits, never called by the production runner.
fn pump(walk: &mut Onboarding, now: u64) {
    for _ in 0..3 {
        walk.drain_storage_for_test(now);
        walk.maintain_privacy(now, true).unwrap();
    }
    walk.drain_storage_for_test(now);
}
fn walk() -> (Store, Onboarding) {
    let store = Store::new(None);
    let mut walk = store.cycle(T, 1);
    pump(&mut walk, T);
    (store, walk)
}
fn offer(walk: &mut Onboarding, now: u64, seq: u64) -> u64 {
    context(walk, now, seq, false);
    walk.drain_storage_for_test(now);
    assert_eq!(
        walk.tick(now, START_QUIET_US, true, false, face(), true),
        None
    );
    let token = walk.privacy_pending().invitations[0].invitation;
    assert!(!walk.privacy_pending().invitations[0].ready);
    pump(walk, now);
    assert!(walk.privacy_pending().invitations[0].ready);
    token
}
fn deliver(walk: &mut Onboarding, token: u64, now: u64) {
    let boot = BootId::new([7; 16]).unwrap();
    assert!(
        walk.take_privacy_invitation(token, now)
            .unwrap()
            .contains("Close it, then open it again.")
    );
    assert!(walk.bind_privacy_invitation(token, boot, 4));
    walk.privacy_invitation_finished(boot, 4, Delivery::Whole, now);
    assert_eq!(
        walk.privacy_flow.attempt.as_ref().unwrap().phase(),
        privacy::Phase::Armed
    );
}
fn close(store: &Store, walk: &mut Onboarding, seq: u64, now: u64) {
    let checkpoint = walk
        .take_close_candidate(&receipt(seq, now, true), now)
        .unwrap();
    assert!(matches!(
        store.owner.client().persist_closed(checkpoint).unwrap(),
        storage::Submit::Queued(_)
    ));
}

#[test]
fn privacy_is_first_and_only_the_current_committed_core_offer_gets_words() {
    let (_store, mut walk) = walk();
    let token = offer(&mut walk, T, 1);
    assert_eq!(walk.led.as_ref().unwrap().step, Step::Privacy);
    assert!(
        walk.take_privacy_invitation(token.wrapping_add(1), T)
            .is_none()
    );
    assert!(walk.take_privacy_invitation(token, T).is_some());
    assert!(walk.take_privacy_invitation(token, T).is_none());
    assert!(!walk.privacy_pending().invitations[0].ready);
    assert_eq!(walk.take_gesture(), None);
}

#[test]
fn unplayed_invitation_never_arms_and_restarts_use_only_one_retry() {
    let (store, mut first) = walk();
    let token = offer(&mut first, T, 1);
    first.take_privacy_invitation(token, T).unwrap();
    assert!(
        first
            .take_close_candidate(&receipt(2, T + 10_000, true), T + 10_000)
            .is_none()
    );
    let mut second = store.cycle(2 * T, 3);
    pump(&mut second, 2 * T);
    let next = offer(&mut second, 2 * T, 3);
    assert_ne!(token, next);
    second.privacy_invitation_refused(token, 2 * T);
    assert_eq!(second.privacy_pending().invitations[0].invitation, next);
    second.privacy_invitation_refused(next, 2 * T);
    let mut third = store.cycle(3 * T, 4);
    pump(&mut third, 3 * T);
    third.tick(3 * T, START_QUIET_US, true, false, face(), true);
    pump(&mut third, 3 * T);
    // Both invitations spent with no close seen: later (#3996).
    assert_eq!(third.kept().steps["privacy"].outcome, Outcome::Later);
}

#[test]
fn cancellation_retries_once_and_model_success_is_refused() {
    let (_store, mut walk) = walk();
    let token = offer(&mut walk, T, 1);
    let boot = BootId::new([7; 16]).unwrap();
    walk.take_privacy_invitation(token, T).unwrap();
    assert!(walk.bind_privacy_invitation(token, boot, 5));
    walk.privacy_invitation_finished(boot, 999, Delivery::Whole, T);
    assert!(
        walk.privacy_flow.attempt.is_some(),
        "armed as the turn began"
    );
    walk.privacy_invitation_finished(boot, 5, Delivery::Cut, T);
    assert!(walk.privacy_flow.attempt.is_none());
    assert_ne!(offer(&mut walk, 2 * T, 2), token);
    let call = ToolCall {
        id: "proof".into(),
        name: STEP.into(),
        args: json!({"step":"privacy","outcome":"done"}),
    };
    assert!(run(&walk.file, &call).0.get("error").is_some());
    walk.apply_mailbox("{\"step\":\"privacy\",\"outcome\":\"done\"}\n", 2 * T);
    assert!(!walk.kept().steps.contains_key("privacy"));
}

#[test]
fn verified_close_requires_new_ready_open_and_done_commit_precedes_ack() {
    let (store, mut first) = walk();
    let token = offer(&mut first, T, 1);
    deliver(&mut first, token, T);
    close(&store, &mut first, 2, T + 10_000);
    let mut second = store.cycle(T + 20_000, 3);
    assert!(!second.kept().steps.contains_key("privacy"));
    second.drain_storage_for_test(T + 20_000);
    second.maintain_privacy(T + 20_000, false).unwrap();
    assert!(!second.kept().steps.contains_key("privacy"));
    second.maintain_privacy(T + 20_000, true).unwrap(); // loads RAM checkpoint, not evidence
    assert!(!second.kept().steps.contains_key("privacy"));
    let after_io = T + 20_000 + lamp_msgs::PRIVACY_SWITCH_TTL_US;
    second.maintain_privacy(after_io, true).unwrap();
    assert!(!second.kept().steps.contains_key("privacy"));
    context(&mut second, after_io + 1, 4, true);
    second.maintain_privacy(after_io + 1, true).unwrap();
    assert!(!second.kept().steps.contains_key("privacy"));
    context(&mut second, after_io + 2, 5, false);
    second.maintain_privacy(after_io + 2, true).unwrap();
    assert!(
        !second.kept().steps.contains_key("privacy"),
        "desired Done is not committed Done"
    );
    assert_eq!(
        second.tick(after_io + 2, START_QUIET_US, false, false, face(), true),
        None
    );
    second.drain_storage_for_test(after_io + 2);
    assert_eq!(second.kept().steps["privacy"].outcome, Outcome::Done);
    assert_eq!(second.take_gesture(), None);
    let mut third = store.cycle(after_io + 3, 6);
    pump(&mut third, after_io + 3);
    assert_eq!(
        third.owed, None,
        "new cycle must not repeat durable acknowledgement"
    );
    assert!(!store.path.join("privacy-pending.json").exists());
}

#[test]
fn no_person_means_no_privacy_invitation_and_missing_profile_is_unavailable() {
    let (_store, mut walk) = walk();
    walk.tick(T, START_QUIET_US, true, false, None, true);
    assert!(walk.privacy_pending().invitations.is_empty());
    walk.set_privacy_context(epoch(), true, Some(receipt(2, T, false)), None);
    walk.tick(T, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(T);
    assert_eq!(walk.kept().steps["privacy"].outcome, Outcome::NotAvailable);
}

#[test]
fn without_a_receipt_source_privacy_is_unavailable_and_the_walk_goes_on() {
    // The product's GPIO worker sends no receipt and its unit sets no
    // LAMP_PRIVACY_CONTROLS: a verified location alone must not stall the walk.
    let (_store, mut walk) = walk();
    walk.set_privacy_context(epoch(), false, None, Some("on its base"));
    walk.tick(T, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(T);
    assert_eq!(walk.kept().steps["privacy"].outcome, Outcome::NotAvailable);
    assert!(walk.privacy_pending().invitations.is_empty());
    walk.tick(T + 1, START_QUIET_US, true, false, face(), true);
    assert_eq!(walk.led.as_ref().unwrap().step, Step::WhereYouSit);
}

// Third's P3 on #1792: a receipt source that never shows an open, fresh
// switch is bounded by the sitting step's give-up time.
fn stale_tick(walk: &mut Onboarding, now: u64) {
    walk.drain_storage_for_test(now);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(now);
}

#[test]
fn a_receipt_never_open_and_fresh_leaves_privacy_not_available_after_the_bound() {
    let (_store, mut walk) = walk();
    // The fixture's receipt (stamped T) is long stale here.
    let start = 100 * T;
    stale_tick(&mut walk, start);
    stale_tick(&mut walk, start + SIT_GIVE_UP_US - 1);
    assert!(!walk.kept().steps.contains_key("privacy"));
    assert!(walk.privacy_pending().invitations.is_empty());
    stale_tick(&mut walk, start + SIT_GIVE_UP_US);
    assert_eq!(walk.kept().steps["privacy"].outcome, Outcome::NotAvailable);
    assert!(
        walk.records
            .iter()
            .any(|row| row["why"] == "no_open_privacy_receipt")
    );
    stale_tick(&mut walk, start + SIT_GIVE_UP_US + 1);
    assert_eq!(walk.led.as_ref().unwrap().step, Step::WhereYouSit);
    // An open, fresh receipt afterwards offers privacy again.
    let later = start + SIT_GIVE_UP_US + T;
    offer(&mut walk, later, 2);
    assert_eq!(walk.led.as_ref().unwrap().step, Step::Privacy);
}

#[test]
fn an_open_fresh_receipt_before_the_bound_is_offered_as_usual() {
    let (_store, mut walk) = walk();
    let start = 100 * T;
    stale_tick(&mut walk, start);
    let token = offer(&mut walk, start + SIT_GIVE_UP_US - 1, 2);
    stale_tick(&mut walk, start + SIT_GIVE_UP_US + T);
    assert!(!walk.kept().steps.contains_key("privacy"));
    assert_eq!(walk.privacy_pending().invitations[0].invitation, token);
}

#[test]
fn a_close_before_the_invitation_began_is_not_its_answer() {
    let (_store, mut walk) = walk();
    let token = offer(&mut walk, T, 1);
    // Offered but its words not yet taken: nothing is armed to answer.
    assert!(
        walk.take_close_candidate(&receipt(2, T + 10_000, true), T + 10_000)
            .is_none()
    );
    // The open read (stamped T) is still fresh when the invitation begins
    // 40 ms later; the slide was read 10 ms before that.
    deliver(&mut walk, token, T + 40_000);
    let checkpoint = walk
        .take_close_candidate(&receipt(2, T + 30_000, true), T + 41_000)
        .unwrap();
    assert!(checkpoint.is_discard());
    assert!(walk.privacy_flow.pending_result.is_none());
    pump(&mut walk, T + 41_000);
    assert!(!walk.kept().steps.contains_key("privacy"));
    assert_ne!(offer(&mut walk, 3 * T, 3), token);
}

#[test]
fn expiry_is_checked_only_when_the_physical_switch_reopens() {
    let (store, mut first) = walk();
    let token = offer(&mut first, T, 1);
    deliver(&mut first, token, T);
    close(&store, &mut first, 2, T + 10_000);
    let late = T + privacy::ATTEMPT_US;
    let mut second = store.cycle(late, 3);
    context(&mut second, late, 3, true);
    second.drain_storage_for_test(late);
    second.maintain_privacy(late, false).unwrap();
    assert!(!second.kept().steps.contains_key("privacy"));
    context(&mut second, late + 1, 4, false);
    pump(&mut second, late + 1);
    assert_eq!(second.kept().steps["privacy"].outcome, Outcome::Failed);
}

struct Release(Option<mpsc::SyncSender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.try_send(());
        }
    }
}
fn stalled_store(stall_initialize: bool) -> (Store, Arc<AtomicBool>, mpsc::Receiver<()>, Release) {
    let armed = Arc::new(AtomicBool::new(stall_initialize));
    let gate = armed.clone();
    let (entered, seen) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let store = Store::new(Some(Box::new(move |_| {
        if gate.swap(false, Ordering::SeqCst) {
            let _ = entered.try_send(());
            wait.recv().map_err(io::Error::other)?;
        }
        Ok(())
    })));
    (store, armed, seen, Release(Some(release)))
}

#[test]
fn stalled_initialization_keeps_tutorial_loading_without_blocking_input_ticks() {
    let (store, _armed, seen, release) = stalled_store(true);
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut walk = store.cycle(T, 1);
    let start = Instant::now();
    for offset in 0..100 {
        assert_eq!(
            walk.tick(T + offset, START_QUIET_US, true, false, face(), true),
            None
        );
        walk.maintain_privacy(T + offset, true).unwrap();
    }
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!walk.initialized && !walk.storage_failed);
    assert!(walk.kept().steps.is_empty() && walk.privacy_pending().invitations.is_empty());
    assert_eq!(
        store
            .owner
            .client()
            .commit(Progress::default(), false)
            .unwrap(),
        storage::Submit::Busy
    );
    drop(release);
    pump(&mut walk, T + 100);
    assert!(walk.initialized);
}

#[test]
fn stalled_old_done_cannot_overwrite_redo_or_publish_its_acknowledgement() {
    let (store, armed, seen, release) = stalled_store(false);
    let mut walk = store.cycle(T, 1);
    pump(&mut walk, T);
    walk.desired.steps.insert(
        "privacy".into(),
        Kept {
            outcome: Outcome::Done,
            at_unix_s: 1,
            framing: None,
            offered_at_unix_s: None,
            later_offers: 0,
        },
    );
    walk.desired.privacy_done_attempt = Some(BootId::new([9; 16]).unwrap());
    walk.owed = Some(PRIVACY_DONE.into());
    walk.save();
    armed.store(true, Ordering::SeqCst);
    walk.poll_storage(T);
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    walk.apply_mailbox("{\"redo\":true}\n", T + 1);
    for offset in 2..100 {
        assert_eq!(
            walk.tick(T + offset, START_QUIET_US, true, false, face(), true),
            None
        );
    }
    assert!(walk.kept().steps.is_empty());
    assert_eq!(walk.owed, None);
    assert!(walk.barrier.is_some() && walk.flight.is_none());
    drop(release);
    walk.drain_storage_for_test(T + 100);
    assert!(walk.kept().steps.is_empty() && walk.kept().privacy_done_attempt.is_none());
    let disk: Progress = serde_json::from_slice(&std::fs::read(&walk.file).unwrap()).unwrap();
    assert_eq!(disk, *walk.kept());
    assert!(
        walk.take_records()
            .iter()
            .all(|row| row["outcome"] != "done")
    );
}

#[test]
fn older_completion_never_replaces_a_newer_desired_snapshot() {
    let (store, armed, seen, release) = stalled_store(false);
    let mut walk = store.cycle(T, 1);
    pump(&mut walk, T);
    walk.desired.steps.insert(
        "who_you_are".into(),
        Kept {
            outcome: Outcome::Skipped,
            at_unix_s: 1,
            framing: None,
            offered_at_unix_s: None,
            later_offers: 0,
        },
    );
    walk.save();
    armed.store(true, Ordering::SeqCst);
    walk.poll_storage(T);
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    walk.desired.steps.insert(
        "preferences".into(),
        Kept {
            outcome: Outcome::Skipped,
            at_unix_s: 2,
            framing: None,
            offered_at_unix_s: None,
            later_offers: 0,
        },
    );
    walk.save();
    assert_eq!(walk.desired.steps.len(), 2);
    drop(release);
    walk.drain_storage_for_test(T + 1);
    assert_eq!(walk.kept().steps.len(), 2);
    let disk: Progress = serde_json::from_slice(&std::fs::read(&walk.file).unwrap()).unwrap();
    assert_eq!(disk, *walk.kept());
}

#[test]
fn pause_then_physical_reclose_orders_a_durable_discard_after_old_closed() {
    let (store, mut first) = walk();
    let token = offer(&mut first, T, 1);
    deliver(&mut first, token, T);
    close(&store, &mut first, 2, T + 10_000);
    let mut next = store.cycle(T + 20_000, 3);
    next.drain_storage_for_test(T + 20_000);
    next.maintain_privacy(T + 20_000, true).unwrap();
    next.tick(T + 20_001, START_QUIET_US, false, true, face(), true);
    let discard = next
        .take_close_candidate(&receipt(4, T + 30_000, true), T + 30_000)
        .unwrap();
    assert!(matches!(
        store.owner.client().persist_closed(discard).unwrap(),
        storage::Submit::Queued(_)
    ));
    let mut reopened = store.cycle(T + 40_000, 5);
    pump(&mut reopened, T + 40_000);
    assert!(
        !reopened
            .kept()
            .steps
            .get("privacy")
            .is_some_and(|step| step.outcome == Outcome::Done)
    );
    assert!(!store.path.join("privacy-pending.json").exists());
}

#[test]
fn fast_reopen_cannot_use_a_closed_proof_while_its_storage_is_stalled() {
    let (store, armed, seen, release) = stalled_store(false);
    let mut first = store.cycle(T, 1);
    pump(&mut first, T);
    let token = offer(&mut first, T, 1);
    deliver(&mut first, token, T);
    armed.store(true, Ordering::SeqCst);
    close(&store, &mut first, 2, T + 10_000);
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut next = store.cycle(T + 20_000, 3);
    for offset in 0..100 {
        next.maintain_privacy(T + 20_000 + offset, true).unwrap();
        assert!(!next.kept().steps.contains_key("privacy"));
    }
    assert!(!next.initialized);
    drop(release);
    next.drain_storage_for_test(T + 20_100);
    let stale = T + 20_000 + lamp_msgs::PRIVACY_SWITCH_TTL_US;
    next.maintain_privacy(stale, true).unwrap();
    next.maintain_privacy(stale, true).unwrap();
    assert!(!next.kept().steps.contains_key("privacy"));
    context(&mut next, stale + 1, 4, false);
    pump(&mut next, stale + 1);
    assert_eq!(next.kept().steps["privacy"].outcome, Outcome::Done);
}

#[test]
fn a_storage_failure_retries_no_write_until_an_explicit_mailbox_redo() {
    let fail = Arc::new(AtomicBool::new(false));
    let failure = fail.clone();
    let store = Store::new(Some(Box::new(move |_| {
        if failure.swap(false, Ordering::SeqCst) {
            return Err(io::Error::other("fixture storage unavailable"));
        }
        Ok(())
    })));
    let mut walk = store.cycle(T, 1);
    pump(&mut walk, T);
    walk.desired.steps.insert(
        "preferences".into(),
        Kept {
            outcome: Outcome::Done,
            at_unix_s: 1,
            framing: None,
            offered_at_unix_s: None,
            later_offers: 0,
        },
    );
    walk.save();
    fail.store(true, Ordering::SeqCst);
    walk.poll_storage(T);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !walk.storage_failed {
        walk.poll_storage(T);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..100 {
        walk.poll_storage(T);
    }
    assert!(!walk.kept().steps.contains_key("preferences"));
    assert_eq!(
        walk.take_records()
            .iter()
            .filter(|row| row["kind"] == "onboarding_storage_failed")
            .count(),
        1
    );
    let redo = ToolCall {
        id: "redo".into(),
        name: REDO.into(),
        args: json!({"step": "all"}),
    };
    assert_eq!(run(&walk.file, &redo).0, json!({"ok":true}));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        walk.poll_storage(2 * T);
        if !walk.storage_failed && walk.storage_idle() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "redo did not recover the ordered storage barrier"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(walk.kept().steps.is_empty());
    assert_eq!(
        serde_json::from_slice::<Progress>(&std::fs::read(&walk.file).unwrap()).unwrap(),
        *walk.kept()
    );
}

#[test]
fn a_consumed_redo_mailbox_batch_is_not_replayed_by_the_next_voice_cycle() {
    let (store, mut first) = walk();
    let redo = ToolCall {
        id: "redo".into(),
        name: REDO.into(),
        args: json!({"step": "all"}),
    };
    assert_eq!(run(&first.file, &redo).0, json!({"ok":true}));
    first.drain_storage_for_test(2 * T);
    first.desired.steps.insert(
        "who_you_are".into(),
        Kept {
            outcome: Outcome::Skipped,
            at_unix_s: 2,
            framing: None,
            offered_at_unix_s: None,
            later_offers: 0,
        },
    );
    first.save();
    first.drain_storage_for_test(2 * T);
    let mut second = store.cycle(3 * T, 3);
    second.drain_storage_for_test(3 * T);
    assert_eq!(second.kept().steps["who_you_are"].outcome, Outcome::Skipped);
}

/// How a close's camera receipt comes, for [`camera_flow`].
enum Eyes {
    /// No eyes publisher on the bus: the camera is not available.
    Absent,
    /// The eyes say a receipt for the asked close, offset by this.
    Receipt { token_offset: u64, released: bool },
    /// The eyes are there and say nothing.
    Silent,
}

/// One verified close and reopen with eyes told; the step's outcome and
/// the words owed for it.
fn camera_flow(eyes: Eyes) -> (Outcome, String) {
    let (store, mut first) = walk();
    let root = store.path.join("bus");
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut publisher = match eyes {
        Eyes::Absent => None,
        _ => Some(
            bus.publisher_declared(
                lamp_msgs::CameraClosed::TOPIC,
                &lamp_msgs::CameraClosed::declaration(),
                "eyes",
            )
            .unwrap(),
        ),
    };
    first.eyes = Some(lamp_bus::Bus::new(&root).unwrap());
    let token = offer(&mut first, T, 1);
    deliver(&mut first, token, T);
    // A socket path must be short: the store's directory may not be.
    let socket = std::env::temp_dir().join(format!(
        "lcr-{}-{}",
        std::process::id(),
        crate::process::boot_text(crate::process::new_boot().unwrap())
            .get(..8)
            .unwrap()
    ));
    let permit = std::os::unix::net::UnixDatagram::bind(&socket).unwrap();
    permit.set_nonblocking(true).unwrap();
    let mut link = crate::body_link::EyesLink::to(socket.clone()).unwrap();
    link.close();
    let started = Instant::now();
    let (checkpoint, records) = first.take_close_candidate_telling(
        &receipt(2, T + 10_000, true),
        T + 10_000,
        Some(&mut link),
    );
    let checkpoint = checkpoint.unwrap();
    // Never waits on the eyes: the mute returns at once.
    assert!(started.elapsed() < Duration::from_millis(100));
    // The camera's record comes back with the candidate, for the mute to
    // write: the cycle ends before the loop would take it (lamp-52e6).
    let camera_state = records
        .iter()
        .find(|line| line["kind"] == "onboarding_privacy_camera")
        .map(|line| line["state"].clone());
    let wanted = match &eyes {
        Eyes::Absent => "not_available",
        _ => "asked",
    };
    assert_eq!(camera_state, Some(json!(wanted)));
    assert!(first.take_records().is_empty(), "handed back, not kept");
    let mut datagram = [0u8; 64];
    let mut told = Vec::new();
    while let Ok(length) = permit.recv(&mut datagram) {
        told.push(String::from_utf8_lossy(&datagram[..length]).into_owned());
    }
    assert_eq!(told[0], r#"{"camera":false}"#, "no comes first");
    let asked = checkpoint.camera();
    match (&eyes, asked) {
        (Eyes::Absent, None) => assert_eq!(told.len(), 1),
        (Eyes::Absent, Some(_)) => panic!("no eyes on the bus, yet a receipt asked"),
        (_, None) => panic!("eyes on the bus, no receipt asked"),
        (_, Some(close)) => {
            assert_eq!(told[1], format!(r#"{{"close_receipt":{}}}"#, close.token));
        }
    }
    assert!(matches!(
        store.owner.client().persist_closed(checkpoint).unwrap(),
        storage::Submit::Queued(_)
    ));
    if let (
        Eyes::Receipt {
            token_offset,
            released,
        },
        Some(close),
        Some(publisher),
    ) = (&eyes, asked, publisher.as_mut())
    {
        publisher
            .publish_json(
                1,
                &lamp_msgs::CameraClosed {
                    close: close.token.wrapping_add(*token_offset),
                    released: *released,
                },
            )
            .unwrap();
    }
    let mut second = store.cycle(T + 20_000, 3);
    second.eyes = Some(lamp_bus::Bus::new(&root).unwrap());
    second.drain_storage_for_test(T + 20_000);
    second.maintain_privacy(T + 20_000, true).unwrap();
    let after_io = T + 20_000 + lamp_msgs::PRIVACY_SWITCH_TTL_US;
    second.maintain_privacy(after_io, true).unwrap();
    context(&mut second, after_io + 2, 5, false);
    second.maintain_privacy(after_io + 2, true).unwrap();
    let _ = std::fs::remove_file(&socket);
    let outcome = second.desired.steps["privacy"].outcome;
    let owed = second.owed.clone().unwrap();
    drop(publisher);
    (outcome, owed)
}

#[test]
fn with_eyes_that_give_receipts_done_needs_the_receipt_for_this_close() {
    assert_eq!(
        camera_flow(Eyes::Receipt {
            token_offset: 0,
            released: true
        }),
        (Outcome::Done, PRIVACY_DONE_CAMERA.into())
    );
    // A receipt for another close, one that says the device may be held,
    // or none at all: the camera is not proven and the check has failed.
    for eyes in [
        Eyes::Receipt {
            token_offset: 1,
            released: true,
        },
        Eyes::Receipt {
            token_offset: 0,
            released: false,
        },
        Eyes::Silent,
    ] {
        assert_eq!(camera_flow(eyes), (Outcome::Failed, PRIVACY_FAILED.into()));
    }
}

#[test]
fn without_eyes_on_the_bus_done_claims_the_microphones_only() {
    assert_eq!(
        camera_flow(Eyes::Absent),
        (Outcome::Done, PRIVACY_DONE.into())
    );
}

#[test]
fn the_mute_reads_only_what_the_last_look_at_the_bus_kept() {
    let (store, mut first) = walk();
    let root = store.path.join("bus");
    let bus = lamp_bus::Bus::new(&root).unwrap();
    first.eyes = Some(lamp_bus::Bus::new(&root).unwrap());
    let token = offer(&mut first, T, 1);
    deliver(&mut first, token, T);
    assert!(!first.privacy_flow.eyes_give_receipts, "no eyes when armed");
    assert!(
        first.privacy_flow.camera_token.is_some(),
        "token made when armed"
    );
    // Eyes that give receipts come up after the arm: the mute doesn't look,
    // so this close asks none.
    let publisher = bus
        .publisher_declared(
            lamp_msgs::CameraClosed::TOPIC,
            &lamp_msgs::CameraClosed::declaration(),
            "eyes",
        )
        .unwrap();
    let socket = std::env::temp_dir().join(format!(
        "lcm-{}-{}",
        std::process::id(),
        crate::process::boot_text(crate::process::new_boot().unwrap())
            .get(..8)
            .unwrap()
    ));
    let _permit = std::os::unix::net::UnixDatagram::bind(&socket).unwrap();
    let mut link = crate::body_link::EyesLink::to(socket.clone()).unwrap();
    let (checkpoint, records) = first.take_close_candidate_telling(
        &receipt(2, T + 10_000, true),
        T + 10_000,
        Some(&mut link),
    );
    assert_eq!(checkpoint.unwrap().camera(), None);
    assert!(records.iter().any(
        |line| line["kind"] == "onboarding_privacy_camera" && line["state"] == "not_available"
    ));
    drop(publisher);
    let _ = std::fs::remove_file(&socket);
}

#[test]
fn a_receipt_said_later_than_its_bound_or_by_another_source_proves_nothing() {
    let root = std::env::temp_dir().join(format!(
        "onboarding-camera-{}",
        crate::process::boot_text(crate::process::new_boot().unwrap())
    ));
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let close = privacy::CameraClose {
        token: 9,
        asked_unix_ms: unix_ms(),
    };
    let receipt = lamp_msgs::CameraClosed {
        close: 9,
        released: true,
    };
    let declaration = lamp_msgs::CameraClosed::declaration();
    let mut other = bus
        .publisher_declared(lamp_msgs::CameraClosed::TOPIC, &declaration, "body")
        .unwrap();
    other.publish_json(1, &receipt).unwrap();
    assert_eq!(camera_receipt(Some(&bus), close), Err("not_the_eyes"));
    let mut eyes = bus
        .publisher_declared(lamp_msgs::CameraClosed::TOPIC, &declaration, "eyes")
        .unwrap();
    eyes.publish_json(1, &receipt).unwrap();
    assert_eq!(camera_receipt(Some(&bus), close), Ok(()));
    let asked_long_ago = privacy::CameraClose {
        asked_unix_ms: close.asked_unix_ms - CAMERA_RECEIPT_MS - 1_000,
        ..close
    };
    assert_eq!(camera_receipt(Some(&bus), asked_long_ago), Err("late"));
    assert_eq!(camera_receipt(None, close), Err("no_bus"));
    drop((other, eyes));
    let _ = std::fs::remove_dir_all(&root);
}

// The lead's 52e6 run (21 October): the person closed the switch while the
// lamp was still saying the invitation, and it was offered again.
#[test]
fn a_close_while_the_invitation_is_said_counts_without_another_offer() {
    let (store, mut first) = walk();
    let token = offer(&mut first, T, 1);
    let boot = BootId::new([7; 16]).unwrap();
    first.take_privacy_invitation(token, T).unwrap();
    assert!(first.bind_privacy_invitation(token, boot, 4));
    // No turn end: the mute cuts the words and ends this voice cycle.
    let checkpoint = first
        .take_close_candidate(&receipt(2, T + 10_000, true), T + 10_000)
        .unwrap();
    assert!(!checkpoint.is_discard());
    assert!(matches!(
        store.owner.client().persist_closed(checkpoint).unwrap(),
        storage::Submit::Queued(_)
    ));
    drop(first);
    let mut second = store.cycle(T + 20_000, 3);
    pump(&mut second, T + 20_000);
    second.tick(T + 20_000, START_QUIET_US, false, false, face(), true);
    second.drain_storage_for_test(T + 20_000);
    assert_eq!(second.kept().steps["privacy"].outcome, Outcome::Done);
    assert_eq!(second.kept().privacy_invitation_tries, 1);
}

// The reviewer-lead's P3 on #2448: an arm that fails while the words are
// already being said does not spend a try; the same try is offered again,
// so a close during its words has an attempt to answer.
#[test]
fn an_arm_that_fails_while_the_invitation_is_said_spends_no_try() {
    let (store, mut first) = walk();
    let token = offer(&mut first, T, 1);
    let boot = BootId::new([7; 16]).unwrap();
    first.take_privacy_invitation(token, T).unwrap();
    // The open receipt is gone by the bind: the arm fails.
    context(&mut first, T, 2, true);
    assert!(!first.bind_privacy_invitation(token, boot, 4));
    assert!(first.privacy_flow.attempt.is_none());
    assert_eq!(first.privacy_flow.failures, 0);
    assert_eq!(first.privacy_pending().invitations[0].invitation, token);
    // The switch reads open again: the same try is offered and armed.
    let now = 2 * T;
    context(&mut first, now, 3, false);
    first.drain_storage_for_test(now);
    first.tick(now, START_QUIET_US, true, false, face(), true);
    assert!(first.privacy_pending().invitations[0].ready);
    first.take_privacy_invitation(token, now).unwrap();
    assert!(first.bind_privacy_invitation(token, boot, 5));
    close(&store, &mut first, 4, now + 10_000);
    drop(first);
    let mut second = store.cycle(now + 20_000, 5);
    pump(&mut second, now + 20_000);
    second.tick(now + 20_000, START_QUIET_US, false, false, face(), true);
    second.drain_storage_for_test(now + 20_000);
    assert_eq!(second.kept().steps["privacy"].outcome, Outcome::Done);
    assert_eq!(second.kept().privacy_invitation_tries, 1);
}

#[test]
fn the_invitation_is_short_with_the_location() {
    let (_store, mut walk) = walk();
    let token = offer(&mut walk, T, 1);
    let note = walk.take_privacy_invitation(token, T).unwrap();
    let said = note.split('"').nth(1).unwrap();
    assert_eq!(
        said,
        "Find my privacy switch at its verified profile location. Close it, then open it again."
    );
    // About 3-5 s spoken: where it is, then what to do.
    assert!(said.split_whitespace().count() <= 16);
    assert_eq!(said.matches('.').count(), 2);
}
