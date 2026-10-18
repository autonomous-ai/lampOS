//! Real anonymous IPC and claim transfer around the parent state owner.
//! No Schedule, provider, microphone or acoustic outcome is inferred here.
use super::*;
use lamp_schedule::delivery::{Correlation, DeliveryKey, RetirementBasis};
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const AT: u64 = 10_000_000;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Test {
    parent: Reminders,
    worker: transport::WorkerTransport,
    directory: PathBuf,
}
impl Test {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "schedule-parent-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let (transport, worker) = transport::test_pair(&directory.join("claim")).unwrap();
        let parent = Reminders {
            transport,
            offer: None,
            active: None,
            unadmitted: None,
            sent: false,
            failed: false,
            by_skill: false,
            owed_told: None,
        };
        Self {
            parent,
            worker,
            directory,
        }
    }
    fn send(&mut self, message: FromWorker, at: u64) {
        assert!(self.worker.send(&message).unwrap());
        self.parent.poll(at);
        assert!(!self.parent.failed);
    }
    fn offer(&mut self, offer: Offer, at: u64) {
        self.send(FromWorker::Offer { offer }, at);
    }
    fn command(&mut self) -> Option<ToWorker> {
        self.worker.receive().unwrap()
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}
fn offer() -> Offer {
    Offer {
        token: PreparedToken {
            key: DeliveryKey {
                store_id: "0123456789abcdef0123456789abcdef".into(),
                id: 3,
                delivery: 2,
            },
            attempt: 1,
            correlation: Correlation {
                consumer_session: "parent-test".into(),
                sequence: 1,
            },
        },
        text: "The kettle timer is due.".into(),
        late: false,
        late_by_ms: None,
        observed_at_us: AT,
        expires_at_us: AT + LEASE_US,
    }
}
fn owner(boot: u8) -> (TurnOwner, u64) {
    let at = MonoTime::from_micros(AT);
    let until = MonoTime::from_micros(AT + LEASE_US);
    let mut controller = Controller::new(BootId::new([boot; 16]).unwrap(), at);
    controller
        .set_microphone_permission(at, Permission::Allowed)
        .unwrap();
    controller
        .set_capture(at, CaptureState::RetainingUntil(until))
        .unwrap();
    controller
        .set_admission(at, AdmissionState::OpenUntil(until))
        .unwrap();
    let turn = controller.admit(at, AdmittedInput::NewTurn).unwrap();
    (
        turn,
        controller.snapshot(at).unwrap().microphone_generation(),
    )
}

#[test]
fn offered_lease_is_bounded_and_does_not_gain_age_from_parent_poll() {
    let mut test = Test::new();
    let valid = offer();
    test.offer(valid.clone(), AT);
    assert!(test.parent.offer(AT + LEASE_US - 1).is_some());
    test.parent.poll(AT + LEASE_US);
    assert!(test.parent.offer(AT + LEASE_US).is_none());
    assert!(test.command().is_none());
    for index in 0..7 {
        let mut bad = valid.clone();
        match index {
            0 => bad.expires_at_us += 1,
            1 => bad.observed_at_us += 1,
            2 => bad.token.attempt = 0,
            3 => bad.text = "x".repeat(301),
            4 => bad.text = "  ".into(),
            5 => bad.text = "line\nbreak".into(),
            _ => bad.token.key.delivery = 0,
        }
        test.offer(bad, AT);
        assert!(test.parent.offer(AT).is_none());
    }
}

#[test]
fn withdrawal_before_admission_retires_exact_attempt_and_waits_for_settlement() {
    let mut test = Test::new();
    let first = offer();
    test.offer(first.clone(), AT);
    test.send(
        FromWorker::Withdraw {
            token: first.token.clone(),
        },
        AT,
    );
    let Some(ToWorker::Retire { token, proof }) = test.command() else {
        panic!("missing retirement")
    };
    assert_eq!(token, first.token);
    assert_eq!(proof.correlation, token.correlation);
    assert_eq!(proof.retired_by_session, token.correlation.consumer_session);
    assert!(matches!(proof.basis, RetirementBasis::NeverAdmitted {}));
    test.offer(first.clone(), AT);
    assert!(test.parent.offer(AT).is_none());
    assert!(
        test.command().is_none(),
        "one settlement packet per attempt"
    );
    let mut wrong = first.token.clone();
    wrong.attempt += 1;
    test.send(FromWorker::Settled { token: wrong }, AT);
    assert!(test.parent.unadmitted.is_some());
    test.send(FromWorker::Settled { token: first.token }, AT);
    assert!(test.parent.unadmitted.is_none());
    assert!(
        test.parent.offer(AT).is_none(),
        "old lease is discarded after settlement"
    );
}

#[test]
fn active_lease_is_exact_and_does_not_revoke_an_unrelated_person_turn() {
    let mut test = Test::new();
    let first = offer();
    test.offer(first.clone(), AT);
    let (turn, privacy) = owner(1);
    let receipt = test.parent.admitted(first.clone(), turn, privacy, 7);
    assert!(test.parent.offer(AT).is_none());
    assert!(test.parent.active_valid(turn, AT));
    let mut later = first.clone();
    later.observed_at_us += 100;
    later.expires_at_us += 100;
    test.offer(later.clone(), AT + 100);
    assert!(test.parent.active_valid(turn, AT + LEASE_US + 1));
    later.token.correlation.sequence += 1;
    later.observed_at_us += 100;
    later.expires_at_us += 100;
    test.offer(later.clone(), AT + 200);
    assert!(!test.parent.active_valid(turn, AT + LEASE_US + 100));
    assert!(test.parent.active_valid(owner(2).0, AT + LEASE_US + 100));
    test.send(FromWorker::Withdraw { token: later.token }, AT + 200);
    assert!(
        test.parent.active_valid(turn, AT + 200),
        "unrelated withdrawal has no authority"
    );
    test.send(
        FromWorker::Withdraw {
            token: first.token.clone(),
        },
        AT + 200,
    );
    assert!(!test.parent.active_valid(turn, AT + 200));
    assert!(
        test.command().is_none(),
        "live Reply still owns its receipt"
    );
    drop(receipt);
    test.parent.poll(AT + 200);
    assert!(matches!(test.command(), Some(ToWorker::Abandon { token }) if token == first.token));
}

#[test]
fn completion_waits_for_reply_drop_and_durable_worker_settlement() {
    let mut test = Test::new();
    let first = offer();
    test.offer(first.clone(), AT);
    let (turn, privacy) = owner(1);
    let mut receipt = test.parent.admitted(first.clone(), turn, privacy, 7);
    receipt.audio();
    receipt.retired(12, 0, 480);
    receipt.completed();
    test.parent.poll(AT);
    assert!(
        test.command().is_none(),
        "live Reply has not finished teardown"
    );
    drop(receipt);
    test.parent.poll(AT);
    let Some(ToWorker::Complete { token, proof }) = test.command() else {
        panic!("missing completion")
    };
    assert_eq!(token, first.token);
    assert_eq!(proof.provider_session, 7);
    assert_eq!(proof.microphone_generation, privacy);
    assert_eq!(
        (
            proof.playback_sequence,
            proof.final_epoch,
            proof.final_frame
        ),
        (12, 0, 480)
    );
    let mut second = first.clone();
    second.token.key.id += 1;
    test.offer(second.clone(), AT);
    assert!(
        test.parent.offer(AT).is_none(),
        "a sent packet is not a durable receipt"
    );
    test.parent.poll(AT);
    assert!(test.command().is_none());
    test.send(
        FromWorker::Settled {
            token: second.token.clone(),
        },
        AT,
    );
    assert!(test.parent.active.is_some());
    test.send(FromWorker::Settled { token: first.token }, AT);
    assert!(test.parent.active.is_none());
    test.offer(second, AT);
    assert!(test.parent.offer(AT).is_some());
}

#[test]
fn failed_worker_invalidates_only_the_due_owner_and_does_not_restart() {
    let mut test = Test::new();
    let first = offer();
    test.offer(first.clone(), AT);
    let (turn, privacy) = owner(1);
    let receipt = test.parent.admitted(first, turn, privacy, 7);
    assert!(
        test.worker
            .send(&serde_json::json!({"unknown":"packet"}))
            .unwrap()
    );
    test.parent.poll(AT);
    assert!(test.parent.failed);
    assert!(!test.parent.active_valid(turn, AT));
    assert!(test.parent.active_valid(owner(2).0, AT));
    assert!(test.parent.transport.claim_held());
    drop(receipt);
    for index in 0..100 {
        test.parent.poll(AT + index);
    }
    assert!(test.parent.failed && test.parent.offer(AT).is_none());
    assert!(test.command().is_none());
}

#[test]
fn queued_completion_survives_privacy_cycle_state_dropping_without_new_admission() {
    let mut test = Test::new();
    let first = offer();
    test.offer(first.clone(), AT);
    let (turn, privacy) = owner(1);
    let mut receipt = test.parent.admitted(first.clone(), turn, privacy, 7);
    receipt.audio();
    receipt.retired(1, 2, 240);
    receipt.completed();
    drop(receipt);
    // The manager belongs to run_source, so polling it during the privacy
    // wait may persist already-completed audio without granting a new turn.
    test.parent.poll(AT + LEASE_US + 1);
    assert!(
        matches!(test.command(), Some(ToWorker::Complete { token, .. }) if token == first.token)
    );
    assert!(test.parent.offer(AT + LEASE_US + 1).is_none());
    test.send(
        FromWorker::Settled { token: first.token },
        AT + LEASE_US + 1,
    );
    assert!(test.parent.active.is_none());
}

/// #939, (c): `skill` turns Schedule speech on as `on` does; the due
/// reminder then also waits for a skill's ask. Nothing else turns it on.
#[test]
fn on_and_skill_turn_schedule_speech_on_and_nothing_else_does() {
    assert!(speaking(Ok("on")) && speaking(Ok("skill")));
    for off in ["off", "ON", "SKILL", "skills", " skill", ""] {
        assert!(!speaking(Ok(off)), "{off:?}");
    }
    assert!(!speaking(Err(&std::env::VarError::NotPresent)));
}

#[test]
fn disabled_switch_never_opens_the_requested_ledger_or_consumer() {
    const MARKER: &str = "LAMP_SCHEDULE_PARENT_TEST_DISABLED";
    if std::env::var_os(MARKER).is_some() {
        assert!(Reminders::from_environment().unwrap().is_none());
        assert!(run_consumer().is_err());
        return;
    }
    for value in [
        None,
        Some("off"),
        Some("ON"),
        Some("true"),
        Some("1"),
        Some(" on "),
        Some("SKILL"),
        Some("skills"),
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "reminders::tests::disabled_switch_never_opens_the_requested_ledger_or_consumer",
                "--exact",
            ])
            .env(MARKER, "1")
            .env(LEDGER_VARIABLE, "/a/path/that/must/not/be/opened")
            .env("LAMP_SCHEDULE_SOCKET", "relative-invalid-path");
        if let Some(value) = value {
            command.env(SPEAK_VARIABLE, value);
        } else {
            command.env_remove(SPEAK_VARIABLE);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            value,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

#[test]
#[ignore = "host-only parent polling measurement; run alone with --ignored --exact --nocapture"]
fn parent_poll_cost() {
    use serde_json::json;
    fn measure(test: &mut Test) -> u64 {
        let start = Instant::now();
        test.parent.poll(AT);
        let nanos = start.elapsed().as_nanos().try_into().unwrap();
        assert!(!test.parent.failed);
        nanos
    }
    fn fill_offers(test: &mut Test) {
        let message = FromWorker::Offer { offer: offer() };
        for _ in 0..1024 {
            if !test.worker.send(&message).unwrap() {
                return;
            }
        }
        panic!("fixture queue was not bounded");
    }
    let mut report = Vec::new();
    for mode in [
        "idle",
        "incoming_offers_full",
        "outgoing_full_completion_waiting",
    ] {
        let mut test = Test::new();
        if mode == "incoming_offers_full" {
            fill_offers(&mut test);
        }
        if mode == "outgoing_full_completion_waiting" {
            let offered = offer();
            test.offer(offered.clone(), AT);
            let (turn, privacy) = owner(1);
            let mut receipt = test.parent.admitted(offered.clone(), turn, privacy, 7);
            receipt.audio();
            receipt.retired(1, 1, 240);
            receipt.completed();
            drop(receipt);
            let packet = ToWorker::Abandon {
                token: offered.token,
            };
            let mut full = false;
            for _ in 0..1024 {
                if !test.parent.transport.send(&packet).unwrap() {
                    full = true;
                    break;
                }
            }
            assert!(full);
        }
        let first_ns = measure(&mut test);
        let mut warm_ns = Vec::with_capacity(100);
        for _ in 0..100 {
            if mode == "incoming_offers_full" {
                fill_offers(&mut test);
            }
            warm_ns.push(measure(&mut test));
        }
        if mode == "outgoing_full_completion_waiting" {
            assert!(
                !test.parent.sent,
                "withheld peer cannot establish settlement"
            );
            assert!(test.parent.active.is_some());
        }
        let mut sorted = warm_ns.clone();
        sorted.sort_unstable();
        report.push(json!({"case":mode,"first_ns":first_ns,"warm_ns":warm_ns,
            "warm_p50_ns":(sorted[49] as f64 + sorted[50] as f64)/2.0,
            "warm_p95_ns":sorted[94],"warm_max_ns":sorted[99]}));
    }
    println!(
        "{}",
        json!({"measurement":"schedule_parent_poll","os":std::env::consts::OS,
        "arch":std::env::consts::ARCH,"samples_per_case":101,"cooperative_target_us":POLL_TARGET_US,
        "boundary":"Instant immediately before Reminders::poll through return, including bounded datagram receive, JSON decoding, state checks and settlement encode/send. Idle and incoming first samples include claim receive; outgoing setup has already received it. Peer filling is outside timing. No child process, file/lock opening, Schedule I/O, ledger commit, provider, coordinator iteration, audio or device in the measured interval. Debug test binary; no hard deadline assertion.",
        "cases":report})
    );
}

/// The first builder, 2026-10-16: a reminder due an hour ago or more is
/// said only when someone is at the lamp, never into an empty room; one due
/// less long ago, or not late, begins as before.
#[test]
fn an_overdue_reminder_begins_only_with_someone_at_the_lamp() {
    let late = |late: bool, late_by_ms: Option<u64>| Offer {
        late,
        late_by_ms,
        ..offer()
    };
    let overdue = late(true, Some(12 * 3_600_000));
    assert!(overdue.overdue());
    assert!(!overdue.may_begin(false));
    assert!(overdue.may_begin(true));
    // Lateness unknown (no trusted wall clock yet after a boot): it waits
    // too, so an overdue one is never said into an empty room as due now
    // (the reviewer on #1103).
    for waits in [late(true, None), late(false, None)] {
        assert!(waits.overdue());
        assert!(!waits.may_begin(false));
        assert!(waits.may_begin(true));
    }
    for begins in [late(false, Some(0)), late(true, Some(59 * 60_000))] {
        assert!(!begins.overdue());
        assert!(begins.may_begin(false));
    }
    assert!(late(true, Some(super::OVERDUE_MS)).overdue());
}

/// Someone is here from the eyes, or from something said within two
/// minutes, and no longer; the wait is said once for each attempt.
#[test]
fn someone_is_here_for_two_minutes_after_speech_and_the_wait_is_said_once() {
    let s = 1_000_000u64;
    assert!(super::someone_here(true, None, 500 * s));
    assert!(!super::someone_here(false, None, 500 * s));
    assert!(super::someone_here(false, Some(100 * s), 219 * s));
    assert!(!super::someone_here(false, Some(100 * s), 220 * s));
    let overdue = Offer {
        late: true,
        late_by_ms: Some(2 * super::OVERDUE_MS),
        ..offer()
    };
    let mut wait = super::OverdueWait::default();
    assert!(!wait.may_begin(&overdue, false));
    assert_eq!(wait.told(), Some(&overdue.token));
    assert!(!wait.may_begin(&overdue, false));
    assert!(wait.may_begin(&overdue, true));
    let mut retried = overdue.clone();
    retried.token.attempt += 1;
    assert!(!wait.may_begin(&retried, false));
    assert_eq!(wait.told(), Some(&retried.token));
    let on_time = Offer {
        late_by_ms: Some(0),
        ..offer()
    };
    assert!(wait.may_begin(&on_time, false), "an on-time one begins");
}
