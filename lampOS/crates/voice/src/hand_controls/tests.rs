use super::*;
use lamp_bus::Header;
use lamp_msgs::{ClockStamp, ControlSource, Gesture, HoldTier};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::atomic::AtomicUsize};

const CLOCK: &str = "test-boot";
const SESSION: &str = "0123456789abcdef";
const START: u64 = 1_000_000;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = PathBuf::from(format!(
            "/tmp/lamp-hand-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("mute.json")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options(stop: bool, mute: Option<&Path>) -> ControlsOptions {
    ControlsOptions {
        stop,
        mute_record: mute.map(Path::to_path_buf),
        bus_root: None,
        head_touch: false,
        wait: false,
    }
}

fn fixture(options: ControlsOptions) -> (HandControls, SyncSender<Queued>) {
    from_record(options, MuteRecord::new(CLOCK).unwrap(), START)
}

fn from_record(
    options: ControlsOptions,
    record: MuteRecord,
    at_us: u64,
) -> (HandControls, SyncSender<Queued>) {
    let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
    let owner = HandControls::from_parts(
        options,
        CLOCK,
        record,
        input,
        Arc::new(Shared::default()),
        at_us,
    )
    .unwrap();
    (owner, sender)
}

fn gesture(intent: ControlIntent, sequence: u64, at_us: u64) -> ControlGesture {
    let (source, gesture, duration_us) = match intent {
        ControlIntent::Stop => (ControlSource::Button, Gesture::Tap, 100_000),
        ControlIntent::SoftwareMute => (ControlSource::Button, Gesture::LongPress, 2_000_000),
        ControlIntent::Pet => (ControlSource::Headpad, Gesture::Pet, 100_000),
    };
    ControlGesture {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence,
        },
        session_id: SESSION.into(),
        source,
        name: "primary".into(),
        gesture,
        intent,
        duration_us,
        released_us: at_us,
        enabled: true,
    }
}

fn state(sequence: u64, at_us: u64, event: Option<ControlGesture>) -> ControlsState {
    let mut state = ControlsState {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence,
        },
        session_id: SESSION.into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: vec![],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: None,
        latest_mute: None,
        event_sequence: event.as_ref().map_or(0, |event| event.stamp.sequence),
        rejected_inputs: 0,
        privacy: None,
        no_privacy_slide: false,
    };
    match event {
        Some(event) if event.intent == ControlIntent::Stop => state.latest_stop = Some(event),
        Some(event) if event.intent == ControlIntent::SoftwareMute => {
            state.latest_mute = Some(event)
        }
        _ => {}
    }
    state.check().unwrap();
    state
}

fn send(owner: &HandControls, sender: &SyncSender<Queued>, input: Input) {
    assert!(enqueue(sender, &owner.shared, input));
}

fn observe(
    owner: &mut HandControls,
    sender: &SyncSender<Queued>,
    state: ControlsState,
    now_us: u64,
) -> Observation {
    send(owner, sender, Input::State(Box::new(state)));
    owner.poll_at(now_us).unwrap()
}

fn muted_with_clear_pending(directory: &Directory) -> (HandControls, SyncSender<Queued>) {
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    let first = observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute.clone())),
        4_000_000,
    );
    assert!(first.muted && first.mute_changed);
    owner.persist_muted().unwrap();
    // These heartbeat observations matter: a missing/stale controls owner
    // during the hold must never qualify a clear, even with a fresh release.
    for index in 1..=21 {
        let at_us = 4_000_000 + index * 100_000;
        observe(
            &mut owner,
            &sender,
            state(index + 1, at_us, Some(mute.clone())),
            at_us,
        );
    }
    let clear = gesture(ControlIntent::SoftwareMute, 2, 6_200_000);
    let observed = observe(
        &mut owner,
        &sender,
        state(23, 6_200_000, Some(clear)),
        6_200_000,
    );
    assert!(observed.muted && observed.mute_changed);
    assert!(owner.receiver.clear_pending());
    (owner, sender)
}

#[test]
fn disabled_defaults_do_not_touch_paths_or_read_a_boot_clock() {
    let directory = Directory::new();
    let missing = directory.0.join("not-created");
    let options = ControlsOptions {
        bus_root: Some(missing.clone()),
        ..Default::default()
    };
    assert!(!options.enabled());
    assert!(HandControls::restore(&options).unwrap().is_none());
    assert!(!missing.exists());
    // In particular this passes on macOS without /proc/sys/kernel/random/boot_id.
}

#[test]
fn option_validation_does_not_accept_ambiguous_paths() {
    assert!(
        options(true, Some(Path::new("relative.json")))
            .validate()
            .is_err()
    );
    assert!(options(false, Some(Path::new("/"))).validate().is_err());
    assert!(
        ControlsOptions {
            stop: true,
            bus_root: Some("relative".into()),
            mute_record: None,
            head_touch: false,
            wait: false,
        }
        .validate()
        .is_err()
    );
    assert!(options(true, None).validate().is_ok());
}

#[test]
fn pump_subscriptions_open_both_declared_topics_with_a_valid_bus_name() {
    let directory = Directory::new();
    let bus = Bus::new(&directory.0).unwrap();
    let _state = bus
        .publisher_declared(
            ControlsState::TOPIC,
            &ControlsState::declaration(),
            "controls",
        )
        .unwrap();
    let _gestures = bus
        .publisher_declared(
            ControlGesture::TOPIC,
            &ControlGesture::declaration(),
            "controls",
        )
        .unwrap();
    // Exercise the production connection helper on macOS as well as Linux.
    // A hyphenated subscriber name used to make every pump attempt fail
    // before it created any socket; that must fail here with the actual error.
    let (subscribed, mute) =
        subscriptions(&directory.0, false, false).expect("hand controls bus subscriptions");
    assert!(mute.is_none());
    fn expects(subscribed: &[(Subscriber, Topic)]) -> Vec<(Option<&str>, Topic)> {
        subscribed
            .iter()
            .map(|(subscriber, topic)| (subscriber.expects(), *topic))
            .collect()
    }
    assert_eq!(
        expects(&subscribed),
        [
            (Some(ControlsState::TYPE), Topic::State),
            (Some(ControlGesture::TYPE), Topic::Gesture)
        ]
    );
    assert_eq!(bus.subscribers(ControlsState::TOPIC).unwrap().len(), 1);
    assert_eq!(bus.subscribers(ControlGesture::TOPIC).unwrap().len(), 1);
    assert!(
        bus.subscribers(TouchState::TOPIC)
            .unwrap_or_default()
            .is_empty()
    );
    drop(subscribed);
    // The head pads' topics only with LAMP_HEAD_TOUCH=on (#711), and
    // `voice/mute` said only where sleep from the head is on.
    let (subscribed, mute) =
        subscriptions(&directory.0, true, false).expect("head touch subscriptions");
    assert_eq!(
        expects(&subscribed)[2..],
        [
            (Some(TouchState::TYPE), Topic::TouchState),
            (Some(TouchEvent::TYPE), Topic::Touch)
        ]
    );
    assert!(mute.is_none());
    drop(subscribed);
    let (_, mute) = subscriptions(&directory.0, true, true).expect("voice mute publisher");
    assert!(mute.is_some());
    assert_eq!(bus.publishers(VoiceMute::TOPIC).unwrap().len(), 1);
}

#[test]
fn stop_is_independent_of_mute_and_records_both_latency_boundaries() {
    let (mut owner, sender) = fixture(options(true, None));
    let event = gesture(ControlIntent::Stop, 1, 2_000_000);
    let first = observe(
        &mut owner,
        &sender,
        state(1, 2_000_000, Some(event.clone())),
        2_005_000,
    );
    assert_eq!(
        first,
        Observation {
            stop: true,
            observed_at_us: Some(2_005_000),
            acquired_at_us: Some(2_000_000),
            ..Default::default()
        }
    );
    let duplicate = observe(
        &mut owner,
        &sender,
        state(2, 2_010_000, Some(event)),
        2_010_000,
    );
    assert!(!duplicate.stop);
    let mute = gesture(ControlIntent::SoftwareMute, 2, 4_000_000);
    let disabled = observe(
        &mut owner,
        &sender,
        state(3, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(!disabled.stop && !disabled.muted && !disabled.mute_changed);
}

#[test]
fn mute_without_stop_never_treats_a_tap_as_a_mute() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    let tap = gesture(ControlIntent::Stop, 1, 2_000_000);
    let ignored = observe(
        &mut owner,
        &sender,
        state(1, 2_000_000, Some(tap)),
        2_000_000,
    );
    assert!(!ignored.stop && !ignored.muted);
    let mute = gesture(ControlIntent::SoftwareMute, 2, 4_000_000);
    let closed = observe(
        &mut owner,
        &sender,
        state(2, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(!closed.stop && closed.muted && closed.mute_changed);
    assert!(!directory.file().exists(), "urgent poll performed disk I/O");
    owner.persist_muted().unwrap();
    let restored = MuteRecord::load(&directory.file(), CLOCK, 4_000_001).unwrap();
    assert!(restored.muted);
    let (mut restarted, _sender) =
        from_record(options(false, Some(&directory.file())), restored, 4_000_001);
    assert!(restarted.poll_at(4_000_001).unwrap().muted);
}

#[test]
fn stale_and_retired_controls_never_stop_a_new_turn() {
    let (mut owner, sender) = fixture(options(true, None));
    let stale = gesture(ControlIntent::Stop, 1, 2_000_000);
    let mut retained = state(1, 2_600_000, Some(stale));
    assert!(!observe(&mut owner, &sender, retained.clone(), 2_600_000).stop);
    retained.stamp.at_us += 100_000;
    retained.stamp.sequence += 1;
    retained.session_id = "fedcba9876543210".into();
    retained.latest_stop = None;
    retained.event_sequence = 0;
    observe(&mut owner, &sender, retained, 2_700_000);
    let fresh_but_retired = gesture(ControlIntent::Stop, 2, 2_800_000);
    assert!(
        !observe(
            &mut owner,
            &sender,
            state(3, 2_800_000, Some(fresh_but_retired)),
            2_800_000
        )
        .stop
    );
    assert_eq!(owner.health().rejected, 1);
}

#[test]
fn pets_are_advisory_require_state_and_deduplicate() {
    let (mut owner, sender) = fixture(options(true, None));
    let pet = gesture(ControlIntent::Pet, 1, 2_000_000);
    send(&owner, &sender, Input::Pet(pet.clone()));
    assert_eq!(owner.poll_at(2_000_000).unwrap().pets, 0);
    observe(&mut owner, &sender, state(1, 2_000_000, None), 2_000_000);
    send(&owner, &sender, Input::Pet(pet.clone()));
    send(&owner, &sender, Input::Pet(pet));
    let report = owner.poll_at(2_000_000).unwrap();
    assert_eq!(report.pets, 1);
    assert!(!report.stop && !report.muted && !report.mute_changed);
}

#[test]
fn quiescent_report_preserves_a_fresh_pet_while_muted_but_not_its_duplicate() {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    record.request_mute(START);
    record.save(&directory.file()).unwrap();
    let (mut owner, sender) = from_record(options(false, Some(&directory.file())), record, START);
    let mut ready = state(1, 2_000_000, None);
    ready.event_sequence = 1;
    send(&owner, &sender, Input::State(Box::new(ready)));
    let pet = gesture(ControlIntent::Pet, 1, 2_000_000);
    send(&owner, &sender, Input::Pet(pet.clone()));
    let (allowed, report) = owner
        .quiescent_observation_with_clock(|| 2_000_000)
        .unwrap();
    assert!(!allowed);
    assert_eq!(report.pets, 1);
    assert!(report.muted);
    assert!(!report.stop && !report.mute_changed);

    send(&owner, &sender, Input::Pet(pet));
    let (allowed, duplicate) = owner
        .quiescent_observation_with_clock(|| 2_000_001)
        .unwrap();
    assert!(!allowed);
    assert_eq!(duplicate.pets, 0);
    assert!(duplicate.muted);
    assert!(!duplicate.stop && !duplicate.mute_changed);
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, 2_000_001)
            .unwrap()
            .muted
    );
}

#[test]
fn retained_mute_at_restart_closes_but_does_not_clear() {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    record.request_mute(3_000_000);
    let (mut owner, sender) =
        from_record(options(false, Some(&directory.file())), record, 5_000_000);
    let retained = gesture(ControlIntent::SoftwareMute, 1, 4_900_000);
    let seen = observe(
        &mut owner,
        &sender,
        state(1, 5_000_000, Some(retained)),
        5_000_000,
    );
    assert!(seen.muted && seen.mute_changed);
    assert!(!owner.receiver.clear_pending());
    assert!(!owner.quiescent_at(5_000_000).unwrap());
}

#[test]
fn clear_is_saved_before_live_permission_and_old_hold_cannot_remute() {
    let directory = Directory::new();
    let (mut owner, _sender) = muted_with_clear_pending(&directory);
    assert!(owner.receiver.mute_record().muted);
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, 6_200_000)
            .unwrap()
            .muted
    );
    let (allowed, report) = owner
        .quiescent_observation_with_clock(|| 6_200_000)
        .unwrap();
    assert!(allowed);
    assert!(!report.muted);
    let saved = MuteRecord::load(&directory.file(), CLOCK, 6_200_000).unwrap();
    assert!(!saved.muted);
    assert_eq!(&saved, owner.receiver.mute_record());
    assert_eq!(saved.through_us, 6_200_000);
}

#[test]
fn failed_clear_keeps_live_latch_and_blocks_any_later_open() {
    let directory = Directory::new();
    let (mut owner, _sender) = muted_with_clear_pending(&directory);
    // Persist the newer hold, then make the explicit clear transaction fail.
    owner.persist_muted().unwrap();
    fs::write(directory.0.join(".mute.json.pending"), b"incomplete").unwrap();
    assert!(owner.quiescent_at(6_200_000).is_err());
    assert!(owner.muted());
    assert!(owner.health().persistence_failed);
    assert!(owner.arm_active().is_err());
    // The next start is not refused (#4332): the stray marker reads as muted.
    let (next, note) = MuteRecord::load_noting(&directory.file(), CLOCK, 6_200_000).unwrap();
    assert!(next.muted);
    assert!(note.is_some());
}

#[test]
fn a_slow_mute_save_cannot_refresh_an_expired_clear() {
    let directory = Directory::new();
    let (mut owner, _sender) = muted_with_clear_pending(&directory);
    let mut calls = 0;
    let permitted = owner
        .quiescent_with_clock(|| {
            calls += 1;
            if calls <= 2 { 6_200_000 } else { 6_800_000 }
        })
        .unwrap();
    assert!(!permitted);
    assert!(owner.muted());
    assert!(!owner.receiver.clear_pending());
}

#[test]
fn loss_or_new_work_during_persistence_defers_clear_until_a_new_bounded_tick() {
    let directory = Directory::new();
    let (mut owner, _sender) = muted_with_clear_pending(&directory);
    owner.persist_muted().unwrap();
    // Model a continuity break published by the independent bus pump while
    // this coordinator was syncing the accepted mute.
    owner.shared.fence();
    assert!(!owner.after_persistence(|| 6_200_000).unwrap());
    assert!(owner.muted());
    assert!(!owner.receiver.clear_pending());

    let next = Directory::new();
    let (mut owner, sender) = muted_with_clear_pending(&next);
    owner.persist_muted().unwrap();
    send(
        &owner,
        &sender,
        Input::State(Box::new(state(
            24,
            6_210_000,
            Some(gesture(ControlIntent::SoftwareMute, 3, 6_210_000)),
        ))),
    );
    assert!(!owner.after_persistence(|| 6_210_000).unwrap());
    assert!(owner.muted());
    assert_eq!(owner.shared.queued.load(Ordering::Acquire), 1);
    assert!(!owner.quiescent_at(6_210_000).unwrap());
    assert_eq!(owner.shared.queued.load(Ordering::Acquire), 0);
    assert!(!owner.receiver.clear_pending());
}

#[test]
fn optional_bus_loss_or_backlog_does_not_inhibit_unmuted_voice() {
    let directory = Directory::new();
    for configured in [options(true, None), options(false, Some(&directory.file()))] {
        let (mut owner, sender) = fixture(configured);
        owner.shared.fence();
        assert!(owner.after_persistence(|| START).unwrap());
        for sequence in 1..=QUEUE_CAPACITY {
            send(
                &owner,
                &sender,
                Input::State(Box::new(state(sequence as u64, START, None))),
            );
        }
        assert!(owner.quiescent_at(START).unwrap());
        assert!(!owner.muted());
        assert_eq!(owner.shared.queued.load(Ordering::Acquire), POLL_LIMIT);
    }
}

#[test]
fn exhausted_receiver_fences_are_reported_without_changing_the_saved_preference() {
    let directory = Directory::new();
    for muted in [false, true] {
        let mut record = MuteRecord::new(CLOCK).unwrap();
        if muted {
            record.request_mute(START);
        }
        let (mut owner, sender) =
            from_record(options(true, Some(&directory.file())), record, START);
        for index in 0..=33 {
            let at_us = START + index * 1_000;
            let mut value = state(1, at_us, None);
            value.session_id = format!("{index:016x}");
            observe(&mut owner, &sender, value, at_us);
        }
        assert!(owner.health().rejected > 0);
        let before = owner.health().rejected;
        owner.fence_clear().unwrap();
        assert_eq!(owner.health().rejected, before + 1);
        owner.shared.fence();
        assert_eq!(owner.after_persistence(|| START + 34_000).unwrap(), !muted);
        assert_eq!(owner.health().rejected, before + 2);
        assert_eq!(owner.poll_at(START + 35_000).unwrap().muted, muted);
        assert_eq!(owner.quiescent_at(START + 36_000).unwrap(), !muted);
    }
}

#[test]
fn overflow_and_full_poll_batches_invalidate_pending_clear() {
    let directory = Directory::new();
    let (mut owner, _sender) = muted_with_clear_pending(&directory);
    owner.shared.fence();
    assert!(!owner.quiescent_at(6_200_000).unwrap());
    assert!(!owner.receiver.clear_pending());

    let shared = Shared::default();
    let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
    for sequence in 1..=QUEUE_CAPACITY + 1 {
        assert!(enqueue(
            &sender,
            &shared,
            Input::State(Box::new(state(sequence as u64, START, None)))
        ));
    }
    assert_eq!(shared.lost.load(Ordering::Relaxed), 1);
    assert_eq!(shared.discontinuities.load(Ordering::Acquire), 1);
    let mut owner = HandControls::from_parts(
        options(true, None),
        CLOCK,
        MuteRecord::new(CLOCK).unwrap(),
        input,
        Arc::new(shared),
        START,
    )
    .unwrap();
    owner.poll_at(START).unwrap();
    let mut remaining = 0;
    while owner.input.try_recv().is_ok() {
        remaining += 1;
    }
    assert_eq!(remaining, QUEUE_CAPACITY - POLL_LIMIT);
}

#[test]
fn active_guard_changes_restart_state_but_not_live_preference() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    assert!(!owner.muted());
    owner.arm_active().unwrap();
    assert!(!owner.muted());
    let guarded = MuteRecord::load(&directory.file(), CLOCK, START).unwrap();
    assert!(guarded.muted);
    assert_eq!(guarded.through_us, owner.receiver.mute_record().through_us);
    let other_boot = MuteRecord::load(&directory.file(), "later-boot", 0).unwrap();
    assert!(other_boot.muted);
    let (mut restarted, _sender) =
        from_record(options(false, Some(&directory.file())), guarded, START);
    assert!(restarted.poll_at(START).unwrap().muted);
    assert!(restarted.arm_active().is_err());
    owner.disarm_after_cleanup().unwrap();
    assert!(
        !MuteRecord::load(&directory.file(), CLOCK, START)
            .unwrap()
            .muted
    );
}

/// #960: releasing the guard writes the live latch as it was, so a lamp
/// muted by its person stays muted after a requested stop, and one that was
/// listening comes back listening.
#[test]
fn releasing_the_guard_keeps_a_mute_the_person_chose() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.arm_active().unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(owner.muted());
    owner.disarm_after_cleanup().unwrap();
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, 4_000_000)
            .unwrap()
            .muted,
        "a chosen mute survives the stop"
    );
}

/// #960 point 2, `LAMP_RESTORED_MUTE=on`: a restart after a fault knows its
/// mute is the guard's; a mute the person chose is never taken for one.
#[test]
fn only_a_mute_left_by_the_restart_guard_reads_as_restored_after_a_fault() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let restart = |mark_guard: bool, now_us: u64| {
        let record = MuteRecord::load(&directory.file(), CLOCK, now_us).unwrap();
        let (mut restarted, sender) =
            from_record(options(false, Some(&directory.file())), record, now_us);
        restarted.mark_guard = mark_guard;
        restarted.restore_guard_marker(stored_identity(&directory.file()));
        (restarted, sender)
    };

    // A fault while listening: the guard's mute, with its marker.
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    assert!(marker.is_file());
    drop(owner);
    let (restored, _sender) = restart(true, START);
    assert!(restored.muted() && restored.restored_after_fault());
    // Without the switch the same lamp reads as plainly muted, and the
    // marker is dropped rather than kept for a later run.
    let (plain, _sender) = restart(false, START);
    assert!(plain.muted() && !plain.restored_after_fault());
    assert!(!marker.exists());

    // The person mutes while listening, then a fault: their mute.
    fs::remove_file(directory.file()).unwrap();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    owner.persist_muted().unwrap();
    assert!(!marker.exists());
    drop(owner);
    let (chosen, _sender) = restart(true, 4_000_000);
    assert!(chosen.muted() && !chosen.restored_after_fault());
}

/// #4332: what the pump says on `voice/mute` names who or what muted the
/// voice, for `mic.get`'s `muted_by`: a hand's sleep, the person's hold, the
/// restart guard after a fault, and a record it could not trust. Open says
/// none. A mute the person chooses after the guard's or an untrusted one is
/// theirs.
#[test]
fn voice_mute_says_who_muted_it() {
    let said = |owner: &HandControls| {
        latch_message(owner.shared.latch.load(Ordering::Acquire)).expect("a latch")
    };
    let by = |owner: &HandControls| said(owner).muted_by;

    // Open: no one.
    let directory = Directory::new();
    let (open, _sender) = fixture(head_options(Some(&directory)));
    assert_eq!(
        said(&open),
        VoiceMute {
            muted: false,
            reason: None,
            muted_by: None,
        }
    );
    drop(open);

    // The person's hold.
    let mut record = MuteRecord::new(CLOCK).unwrap();
    assert!(record.request_mute(START));
    let (held, _sender) = from_record(head_options(Some(&directory)), record, START);
    assert_eq!(by(&held), Some(MutedBy::Button));
    assert!(said(&held).check().is_ok());
    drop(held);

    // A hand's sleep.
    let mut record = MuteRecord::new(CLOCK).unwrap();
    assert!(record.request_sleep(START));
    let (asleep, _sender) = from_record(head_options(Some(&directory)), record, START);
    assert_eq!(
        said(&asleep),
        VoiceMute {
            muted: true,
            reason: Some(MuteReason::Sleep),
            muted_by: Some(MutedBy::Sleep),
        }
    );
    assert!(said(&asleep).check().is_ok());
    drop(asleep);

    // The restart guard after a fault, read back as a start does.
    let (mut owner, _sender) = fixture(head_options(Some(&directory)));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    let record = MuteRecord::load(&directory.file(), CLOCK, START).unwrap();
    let (mut guarded, _sender) = from_record(head_options(Some(&directory)), record, START);
    guarded.mark_guard = true;
    guarded.restore_guard_marker(stored_identity(&directory.file()));
    guarded.share_latch();
    assert!(guarded.restored_after_fault());
    assert_eq!(by(&guarded), Some(MutedBy::RestartGuard));
    assert!(said(&guarded).check().is_ok());
    // The person's own mute over it is theirs.
    guarded.mute_chosen().unwrap();
    guarded.share_latch();
    assert_eq!(by(&guarded), Some(MutedBy::Button));
    drop(guarded);

    // A record the voice could not trust: a newer build's (#1505) and an
    // unfinished write's (#4378), through the start's own path.
    for cut in [false, true] {
        let directory = Directory::new();
        let path = directory.file();
        if cut {
            MuteRecord::new(CLOCK).unwrap().save(&path).unwrap();
            fs::write(directory.0.join(".mute.json.pending"), "cut").unwrap();
        } else {
            fs::write(
                &path,
                r#"{"version":1,"clock_id":"test-boot","muted":false,"through_us":5,"later":true}"#,
            )
            .unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let restored = restored_record(&path, CLOCK, START).unwrap();
        assert!(restored.unreadable, "cut {cut}");
        let (_sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mut untrusted = HandControls::with_restored(
            &head_options(Some(&directory)),
            CLOCK,
            restored,
            input,
            Arc::new(Shared::default()),
            START,
        )
        .unwrap();
        assert!(untrusted.unreadable(), "cut {cut}");
        assert_eq!(by(&untrusted), Some(MutedBy::Unreadable), "cut {cut}");
        assert!(said(&untrusted).check().is_ok());
        untrusted.mute_chosen().unwrap();
        untrusted.share_latch();
        assert_eq!(by(&untrusted), Some(MutedBy::Button), "cut {cut}");
    }

    // A record read as it was is trusted.
    let directory = Directory::new();
    MuteRecord::new(CLOCK)
        .unwrap()
        .save(&directory.file())
        .unwrap();
    assert!(
        !restored_record(&directory.file(), CLOCK, START)
            .unwrap()
            .unreadable
    );
}

/// A restart as `restore` does it, on the boot `clock`: the identity as
/// stored, the record loaded on this boot's clock and saved, a guard of an
/// earlier boot released ([`released_after_power_cut`]), then the marker read.
fn restart_as_restore(
    directory: &Directory,
    clock: &str,
    now_us: u64,
) -> (HandControls, SyncSender<Queued>, bool) {
    let path = directory.file();
    let mut restored = Restored {
        stored: stored_identity(&path),
        record: MuteRecord::load(&path, clock, now_us).unwrap(),
        late_sleep: None,
        unreadable: false,
    };
    restored.record.save(&path).unwrap();
    let released = released_after_power_cut(&path, true, &mut restored).unwrap();
    let stored = restored.stored.clone();
    let (mut restarted, sender) = from_record(options(false, Some(&path)), restored.record, now_us);
    restarted.mark_guard = true;
    restarted.restore_guard_marker(stored);
    (restarted, sender, released)
}

/// #4332, lamp-220c: unplugged while it listened, the lamp came back muted
/// by the restart guard, deaf until a tap. A guard of an earlier boot is a
/// power cut: the lamp starts listening, the marker gone, and stays so at
/// the next restart of that boot.
#[test]
fn a_power_cut_while_listening_starts_listening_with_no_tap() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    // The plug pulled: the next boot has another clock.
    let (rebooted, _sender, released) = restart_as_restore(&directory, "after-power-cut", 1_000);
    assert!(released);
    assert!(!rebooted.muted() && !rebooted.restored_after_fault());
    assert!(!marker.exists());
    assert!(
        !MuteRecord::load(&directory.file(), "after-power-cut", 1_000)
            .unwrap()
            .muted,
        "saved as the person left it"
    );
    drop(rebooted);
    let (updated, _sender, released) = restart_as_restore(&directory, "after-power-cut", 2_000);
    assert!(!released && !updated.muted());
}

/// #4332: a guard of this boot (a crash, a voice that ended before its stop)
/// still restores its mute, known as the guard's, for a tap to clear.
#[test]
fn a_guard_of_this_boot_still_restores_its_mute() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    let (restarted, _sender, released) = restart_as_restore(&directory, CLOCK, START);
    assert!(!released);
    assert!(restarted.muted() && restarted.restored_after_fault());
    assert!(marker.is_file());
}

/// #4332: the person's own mute survives a power cut, whether they muted
/// before the voice ran or while it listened.
#[test]
fn a_power_cut_keeps_a_mute_the_person_chose() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    owner.persist_muted().unwrap();
    drop(owner);
    let (rebooted, _sender, released) = restart_as_restore(&directory, "after-power-cut", 1_000);
    assert!(!released);
    assert!(rebooted.muted() && !rebooted.restored_after_fault());
    // And again on the boot after: a chosen mute is never released.
    drop(rebooted);
    let (again, _sender, released) = restart_as_restore(&directory, "a-third-boot", 1_000);
    assert!(!released && again.muted());
}

/// #4332: without `LAMP_RESTORED_MUTE=on` no marker is trusted, so a muted
/// record from an earlier boot keeps its mute.
#[test]
fn without_the_marker_switch_a_power_cut_keeps_the_mute() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    let path = directory.file();
    let mut restored = Restored {
        stored: stored_identity(&path),
        record: MuteRecord::load(&path, "after-power-cut", 1_000).unwrap(),
        late_sleep: None,
        unreadable: false,
    };
    assert!(!released_after_power_cut(&path, false, &mut restored).unwrap());
    assert!(restored.record.muted);
}

/// #4332, lamp-0c89 (29 Oct 15:22): the app's Restart stopped the voice
/// clean, and the next start read the guard's mute again. A clean stop ends
/// the guard's mute: the next start of the same boot listens.
#[test]
fn a_clean_stop_ends_a_mute_the_restart_guard_restored() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    let (mut restored, _sender, _) = restart_as_restore(&directory, CLOCK, START);
    assert!(restored.restored_after_fault());
    assert!(restored.release_restored_on_stop().unwrap());
    assert!(!restored.muted() && !marker.exists());
    drop(restored);
    // The release is saved at the voice's own clock.
    let (restarted, _sender, _) = restart_as_restore(&directory, CLOCK, monotonic_us());
    assert!(!restarted.muted() && !restarted.restored_after_fault());
}

/// #4332: the person's own mute survives a fault and a clean restart after
/// it, and a guard's mute with a press in flight keeps for the next start.
#[test]
fn a_clean_stop_keeps_a_chosen_mute_and_a_guard_with_a_press_in_flight() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    owner.persist_muted().unwrap();
    // A fault: the voice ends with no stop.
    drop(owner);
    let (mut chosen, _sender, _) = restart_as_restore(&directory, CLOCK, 5_000_000);
    assert!(chosen.muted() && !chosen.restored_after_fault());
    assert!(!chosen.release_restored_on_stop().unwrap());
    drop(chosen);
    let (again, _sender, _) = restart_as_restore(&directory, CLOCK, 6_000_000);
    assert!(
        again.muted(),
        "the person's mute survives the clean restart"
    );

    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    let (mut restored, _sender, _) = restart_as_restore(&directory, CLOCK, START);
    restored.queued_for_test().store(1, Ordering::Release);
    assert!(!restored.release_restored_on_stop().unwrap());
    drop(restored);
    let (kept, _sender, _) = restart_as_restore(&directory, CLOCK, START + 1_000);
    assert!(kept.muted() && kept.restored_after_fault());
}

/// The marker never outlives its guard: a clean release removes it, and one
/// left beside an unmuted record (a fault between the two writes) is dropped.
#[test]
fn the_restart_guard_marker_goes_with_its_guard() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    owner.disarm_after_cleanup().unwrap();
    assert!(!marker.exists());

    fs::write(&marker, b"").unwrap();
    let record = MuteRecord::load(&directory.file(), CLOCK, START).unwrap();
    assert!(!record.muted);
    let (mut restarted, _sender) =
        from_record(options(false, Some(&directory.file())), record, START);
    restarted.mark_guard = true;
    restarted.restore_guard_marker(stored_identity(&directory.file()));
    assert!(!restarted.restored_after_fault());
    assert!(!marker.exists());
}

#[test]
fn arm_failure_refuses_audio_and_failed_new_mute_keeps_durable_guard() {
    let directory = Directory::new();
    let missing = directory.0.join("missing").join("mute.json");
    let (mut invalid_path, _sender) = fixture(options(false, Some(&missing)));
    assert!(invalid_path.arm_active().is_err());
    assert!(invalid_path.health().persistence_failed);
    assert!(invalid_path.arm_active().is_err());

    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.arm_active().unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(owner.persist_muted().is_err());
    assert!(owner.muted());
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, 4_000_000)
            .unwrap()
            .muted
    );
    assert!(owner.disarm_after_cleanup().is_err());
}

fn envelope(state_topic: bool, payload: impl serde::Serialize) -> Message {
    Message {
        header: Header {
            topic: if state_topic {
                ControlsState::TOPIC
            } else {
                ControlGesture::TOPIC
            }
            .into(),
            kind: if state_topic {
                Kind::Latest
            } else {
                Kind::Event
            },
            enc: lamp_bus::JSON.into(),
            type_name: Some(
                if state_topic {
                    ControlsState::TYPE
                } else {
                    ControlGesture::TYPE
                }
                .into(),
            ),
            source: "controls".into(),
            boot: SESSION.into(),
            seq: 1,
            at_us: 123, // Deliberately not the shared monotonic clock.
            at_unix_ms: 10_000,
            expires_ms: Some(if state_topic { 300 } else { 500 }),
        },
        payload: serde_json::to_vec(&payload).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn envelopes_require_exact_source_type_kind_expiry_and_state_owner() {
    let good = envelope(true, state(1, START, None));
    assert!(parse(good.clone(), Topic::State, 10_000).is_ok());
    let corruptions: &[fn(&mut Message)] = &[
        |m| m.header.topic = "controls/gesture".into(),
        |m| m.header.source = "replay".into(),
        |m| m.header.type_name = None,
        |m| m.header.kind = Kind::Event,
        |m| m.header.enc = lamp_bus::NONE.into(),
        |m| m.header.seq = 0,
        |m| m.header.boot = "fedcba9876543210".into(),
        |m| m.header.boot = "not-a-boot".into(),
        |m| m.header.expires_ms = None,
        |m| m.header.expires_ms = Some(10_000),
        |m| m.expired = true,
        |m| m.header.at_unix_ms = 20_000,
        |m| m.payload = b"{}".to_vec(),
    ];
    for corrupt in corruptions {
        let mut bad = good.clone();
        corrupt(&mut bad);
        assert!(parse(bad, Topic::State, 10_000).is_err());
    }
    assert!(parse(good, Topic::State, 10_351).is_err());
    let mut pet = envelope(false, gesture(ControlIntent::Pet, 1, 2_000_000));
    // The event publisher has its own boot; payload session still names state.
    pet.header.boot = "fedcba9876543210".into();
    assert!(parse(pet, Topic::Gesture, 10_000).is_ok());
}

fn head_touch() -> (TouchState, Vec<TouchEvent>) {
    test_head_touch(CLOCK, START)
}

#[test]
fn a_head_touch_is_taken_once_fresh_and_only_with_lamp_head_touch_on() {
    let (state, events) = head_touch();
    assert_eq!(events.len(), 1);
    let touched = events[0].observed_us;
    assert_eq!(touched, START + 20_000);
    let mut switched = options(false, None);
    switched.head_touch = true;
    let (mut owner, sender) = fixture(switched);
    send(&owner, &sender, Input::TouchState(Box::new(state.clone())));
    send(&owner, &sender, Input::Touch(Box::new(events[0].clone())));
    let seen = owner.poll_at(START + 31_000).unwrap();
    assert_eq!(seen.head_touch_at_us, Some(touched));
    assert_eq!(seen.observed_at_us, Some(START + 31_000));
    assert!(!seen.stop, "a head touch is not the button's stop");
    // The same touch again, and its state, ask nothing more.
    send(&owner, &sender, Input::TouchState(Box::new(state.clone())));
    send(&owner, &sender, Input::Touch(Box::new(events[0].clone())));
    assert_eq!(
        owner.poll_at(START + 32_000).unwrap().head_touch_at_us,
        None
    );

    // Too late: a touch older than its freshness never stops a later turn.
    let mut switched = options(false, None);
    switched.head_touch = true;
    let (mut owner, sender) = fixture(switched);
    send(&owner, &sender, Input::TouchState(Box::new(state.clone())));
    send(&owner, &sender, Input::Touch(Box::new(events[0].clone())));
    assert_eq!(
        owner.poll_at(START + 400_000).unwrap().head_touch_at_us,
        None
    );

    // Off: nothing is taken from the head.
    let (mut owner, sender) = fixture(options(true, None));
    send(&owner, &sender, Input::TouchState(Box::new(state)));
    send(&owner, &sender, Input::Touch(Box::new(events[0].clone())));
    assert_eq!(
        owner.poll_at(START + 31_000).unwrap().head_touch_at_us,
        None
    );
}

/// Head touch on and the durable mute at `directory`, or not.
fn head_options(directory: Option<&Directory>) -> ControlsOptions {
    let file = directory.map(Directory::file);
    let mut options = options(false, file.as_deref());
    options.head_touch = true;
    options
}

/// Feeds each step's state and events, as the pump would, and polls at its
/// time: through the quiescent transaction when `quiescent`. Returns each
/// step's observation and whether a quiescent poll allowed opening.
fn feed(
    owner: &mut HandControls,
    sender: &SyncSender<Queued>,
    ticks: Vec<(u64, TouchState, Vec<TouchEvent>)>,
    quiescent: bool,
) -> Vec<(u64, Observation, bool)> {
    ticks
        .into_iter()
        .map(|(at, state, events)| {
            send(owner, sender, Input::TouchState(Box::new(state)));
            for event in events {
                send(owner, sender, Input::Touch(Box::new(event)));
            }
            if quiescent {
                let (allowed, seen) = owner.quiescent_observation_with_clock(|| at).unwrap();
                (at, seen, allowed)
            } else {
                (at, owner.poll_at(at).unwrap(), false)
            }
        })
        .collect()
}

#[test]
fn a_hand_held_on_the_head_puts_it_to_sleep_and_a_tap_wakes_it_once_saved() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(head_options(Some(&directory)));
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_OPEN);
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), false);
    // A hand from 1.51 s, kept 3.2 s: at 3 s it is asleep, as a mute.
    let held = feed(
        &mut owner,
        &sender,
        head.run(START + 3_700_000, true),
        false,
    );
    let (slept_at, seen, _) = held
        .iter()
        .find(|(_, seen, _)| seen.mute_changed)
        .copied()
        .expect("the hold puts it to sleep");
    assert!(seen.muted && seen.asleep);
    assert_eq!(seen.acquired_at_us, Some(START + 510_000 + 3_000_000));
    assert_eq!(slept_at, START + 510_000 + 3_000_000);
    assert!(
        held.iter()
            .all(|(at, seen, _)| (*at < slept_at) != seen.muted)
    );
    // The hand's time is told as it grows, one contact all along (#1362).
    let times: Vec<(u64, u64)> = held
        .iter()
        .filter_map(|(_, seen, _)| seen.head_held)
        .collect();
    assert!(
        times
            .windows(2)
            .all(|w| w[0].0 == w[1].0 && w[0].1 <= w[1].1)
    );
    assert!(
        times
            .iter()
            .any(|(_, held)| crate::head_cue::sighs_at(*held))
    );
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_ASLEEP);
    // Saved before anything else, as any mute; a restart keeps it asleep.
    owner.persist_muted().unwrap();
    let saved = MuteRecord::load(&directory.file(), CLOCK, START + 3_800_000).unwrap();
    assert!(saved.asleep());
    // Let go, a second's quiet, then a tap of 0.2 s: it wakes once the
    // clear is saved, in the quiescent wait.
    let quiet = feed(
        &mut owner,
        &sender,
        head.run(START + 4_800_000, false),
        true,
    );
    assert!(
        quiet
            .iter()
            .all(|(_, seen, allowed)| seen.asleep && !allowed)
    );
    let mut ticks = head.run(START + 5_000_000, true);
    ticks.extend(head.run(START + 5_500_000, false));
    let tapped = feed(&mut owner, &sender, ticks, true);
    let (woke_at, seen, _) = tapped
        .iter()
        .find(|(_, _, allowed)| *allowed)
        .copied()
        .expect("the tap wakes it");
    assert!(!seen.muted && !seen.asleep);
    assert!(seen.head_tap_at_us.is_some());
    assert!(woke_at < START + 5_500_000);
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_OPEN);
    // The person woke it: the next conversation's "hm?" (#1362), once, and
    // only while fresh.
    assert_eq!(owner.take_woken(woke_at), seen.head_tap_at_us);
    assert_eq!(owner.take_woken(woke_at), None, "once");
    owner.woken = seen.head_tap_at_us;
    assert_eq!(
        owner.take_woken(seen.head_tap_at_us.unwrap() + WOKEN_FRESH_US),
        None,
        "a wake held back past its time is let go"
    );
    let saved = MuteRecord::load(&directory.file(), CLOCK, START + 5_600_000).unwrap();
    assert!(!saved.muted && saved.reason.is_none());
}

/// #3747 (the lead on #1711): while the onboarding walk is under way, a
/// hand kept 3.5 s on the head asks no sleep: no mute, nothing saved, the
/// latch open, so the voice keeps saying it is open and Expression doesn't
/// fold. Once the walk is over, the same hold sleeps as before.
#[test]
fn a_hold_while_onboarding_is_under_way_asks_no_sleep() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(head_options(Some(&directory)));
    owner.set_onboarding(true);
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), false);
    // A hand from 1.51 s, kept 3.5 s: Controls asks for sleep at 4.51 s.
    let held = feed(
        &mut owner,
        &sender,
        head.run(START + 4_000_000, true),
        false,
    );
    assert!(
        held.iter().all(|(_, seen, _)| !seen.mute_changed
            && !seen.muted
            && !seen.asleep
            && seen.acquired_at_us.is_none()),
        "{held:?}"
    );
    assert!(
        held.iter()
            .any(|(_, seen, _)| seen.head_touch_at_us.is_some())
    );
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_OPEN);
    assert_eq!(
        latch_message(owner.shared.latch.load(Ordering::Acquire)),
        Some(VoiceMute {
            muted: false,
            reason: None,
            muted_by: None,
        }),
        "said open"
    );
    owner.persist_muted().unwrap();
    assert!(!directory.file().exists(), "no mute to save");
    let lifted = feed(
        &mut owner,
        &sender,
        head.run(START + 5_000_000, false),
        true,
    );
    assert!(
        lifted
            .iter()
            .all(|(_, seen, allowed)| *allowed && !seen.muted)
    );
    // The walk over: the same hold sleeps at its three seconds.
    owner.set_onboarding(false);
    let held = feed(
        &mut owner,
        &sender,
        head.run(START + 8_500_000, true),
        false,
    );
    let (slept_at, seen, _) = held
        .iter()
        .find(|(_, seen, _)| seen.mute_changed)
        .copied()
        .expect("the hold puts it to sleep");
    assert!(seen.muted && seen.asleep);
    assert_eq!(slept_at, START + 5_010_000 + 3_000_000);
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_ASLEEP);
}

/// #3748: a sleep the person asked for aloud (`go_to_sleep`) is the same
/// sleep a hand's hold asks: the durable mute with the reason `sleep`,
/// saved and woken as that one is. Never while onboarding is under way,
/// never over a mute already in force, never without head sleep (a tap
/// on the head is how it is woken), and asked once.
#[test]
fn a_sleep_asked_aloud_is_a_hands_sleep_but_never_during_onboarding() {
    // Without head touch, nothing could wake it as it says: not taken.
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.ask_sleep();
    let seen = owner.poll_at(START + 500_000).unwrap();
    assert!(!seen.mute_changed && !seen.muted, "{seen:?}");
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(head_options(Some(&directory)));
    // Under way, it is not taken, and nothing is left asked after.
    owner.set_onboarding(true);
    owner.ask_sleep();
    let seen = owner.poll_at(START + 1_000_000).unwrap();
    assert!(
        !seen.mute_changed && !seen.muted && !seen.asleep,
        "{seen:?}"
    );
    owner.set_onboarding(false);
    let seen = owner.poll_at(START + 1_100_000).unwrap();
    assert!(!seen.mute_changed, "{seen:?}");
    // Asked with the walk over: asleep at once.
    owner.ask_sleep();
    let seen = owner.poll_at(START + 2_000_000).unwrap();
    assert!(seen.mute_changed && seen.muted && seen.asleep, "{seen:?}");
    assert_eq!(seen.acquired_at_us, Some(START + 2_000_000));
    owner.persist_muted().unwrap();
    assert!(
        directory.file().exists(),
        "the sleep is saved as a hand's is"
    );
    owner.poll_at(START + 2_100_000).unwrap();
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_ASLEEP);
    // Asked again while asleep: nothing changes.
    owner.ask_sleep();
    let seen = owner.poll_at(START + 3_000_000).unwrap();
    assert!(!seen.mute_changed && seen.muted && seen.asleep, "{seen:?}");
}

/// #3747: a sleeping lamp wakes at a hand kept a second on its head, at the
/// stroke, not at its letting go, once the clear is saved, so a person who
/// holds it to bring it back does; and the same hand, kept past three
/// seconds, never sends it back to sleep. Its first touch alone (20 ms)
/// never wakes it: a lone edge is noise.
#[test]
fn a_hand_kept_on_a_sleeping_lamp_wakes_it_at_the_stroke_once_saved() {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    assert!(record.request_sleep(START / 2));
    record.save(&directory.file()).unwrap();
    let (mut owner, sender) = from_record(head_options(Some(&directory)), record, START);
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_ASLEEP);
    let mut head = TestHead::new(CLOCK, START);
    let settled = feed(&mut owner, &sender, head.run(START + 500_000, false), true);
    assert!(
        settled
            .iter()
            .all(|(_, seen, allowed)| seen.asleep && !allowed)
    );
    // A hand from 1.51 s, kept 3.5 s: its touch at 1.53 s, its stroke at
    // 2.51 s, its hold at 4.51 s, its letting go at 5.01 s.
    let mut ticks = head.run(START + 4_000_000, true);
    ticks.extend(head.run(START + 4_500_000, false));
    let kept = feed(&mut owner, &sender, ticks, true);
    let touched = kept
        .iter()
        .find(|(_, seen, _)| seen.head_touch_at_us.is_some())
        .expect("the touch is taken");
    assert_eq!(touched.0, START + 530_000);
    assert!(
        touched.1.asleep && !touched.2,
        "the touch alone: still asleep"
    );
    let (woke_at, seen, _) = kept
        .iter()
        .find(|(_, _, allowed)| *allowed)
        .copied()
        .expect("the kept hand wakes it");
    let stroke = START + 510_000 + 1_000_000;
    assert_eq!(woke_at, stroke, "at the stroke, the hand still there");
    assert!(!seen.muted && !seen.asleep);
    assert_eq!(seen.acquired_at_us, Some(stroke));
    assert!(seen.head_tap_at_us.is_none());
    assert!(
        kept.iter()
            .all(|(at, seen, allowed)| (*at < stroke) == (seen.asleep && !*allowed)),
        "awake from the stroke on, through the hold: {kept:?}"
    );
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_OPEN);
    let saved = MuteRecord::load(&directory.file(), CLOCK, START + 4_600_000).unwrap();
    assert!(!saved.muted && saved.reason.is_none());
    // The person woke it: the next conversation's "hm?" (#1362), once.
    assert_eq!(owner.take_woken(woke_at), Some(stroke));
    assert_eq!(owner.take_woken(woke_at), None, "once");
}

/// #3748 (the reviewer's P1 on #4478): with `LAMP_SLEEP_LINE=on`, a hand
/// kept on a sleeping lamp wakes it at the stroke, and the same hand kept
/// past three seconds owes a sleep the receiver refuses: a hand that woke
/// the lamp cannot send it back, however late its owed sleep lands.
#[test]
fn with_the_sleep_line_a_hand_that_woke_the_lamp_cannot_send_it_back() {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    assert!(record.request_sleep(START / 2));
    record.save(&directory.file()).unwrap();
    let (mut owner, sender) = from_record(head_options(Some(&directory)), record, START);
    owner.set_sleep_line(true);
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), true);
    // A hand from 1.51 s, kept 3.5 s: its stroke at 2.51 s, its hold at
    // 4.51 s; then a second without it, for the owed sleep to land.
    let mut ticks = head.run(START + 4_000_000, true);
    ticks.extend(head.run(START + 5_500_000, false));
    let kept = feed(&mut owner, &sender, ticks, true);
    let stroke = START + 510_000 + 1_000_000;
    let (woke_at, _, _) = kept
        .iter()
        .find(|(_, _, allowed)| *allowed)
        .copied()
        .expect("the kept hand wakes it");
    assert_eq!(woke_at, stroke);
    assert!(
        owner.hold_owed.is_none(),
        "the hold's sleep was owed and asked"
    );
    assert!(
        kept.iter()
            .filter(|(at, _, _)| *at >= stroke)
            .all(|(_, seen, _)| !seen.muted && !seen.asleep && !seen.mute_changed),
        "awake from the stroke on: {kept:?}"
    );
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_OPEN);
    let saved = MuteRecord::load(&directory.file(), CLOCK, START + 5_600_000).unwrap();
    assert!(!saved.muted && saved.reason.is_none());
}

/// #3748 (the reviewer's P2 on #4478): with `LAMP_SLEEP_LINE=on`, the Hold
/// and the letting go read in one poll: the cue is not cut as lifted, and
/// the owed sleep lands once it is no longer sounding. A let go after the
/// Hold changes nothing.
#[test]
fn with_the_sleep_line_a_hold_and_its_letting_go_in_one_poll_still_sleep() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(head_options(Some(&directory)));
    owner.set_sleep_line(true);
    // Pads with short gaps of 50 ms, so the hand is seen gone while its
    // Hold is still fresh.
    let mut head = TestHead::with_gap(CLOCK, START, Some(50));
    feed(&mut owner, &sender, head.run(START + 500_000, false), false);
    // A hand from 1.51 s, its Hold at 4.51 s, let go at once: read
    // through the first state that says it has left.
    let hold_at = START + 510_000 + 3_000_000;
    let mut ticks = head.run(hold_at, true);
    let off = head.run(hold_at + 500_000, false);
    let left = off
        .iter()
        .position(|(_, state, _)| head_lifted(state))
        .expect("the hand is seen gone");
    ticks.extend(off.into_iter().take(left + 1));
    let (before, together): (Vec<_>, Vec<_>) =
        ticks.into_iter().partition(|(at, _, _)| *at < hold_at);
    let seen = feed(&mut owner, &sender, before, false);
    assert!(
        seen.iter()
            .all(|(_, seen, _)| !seen.muted && !seen.head_lifted)
    );
    // One poll: the newest state only, the hand gone, then the events, as
    // a pump that keeps only the newest state would give them.
    let (at, state, _) = together.last().cloned().unwrap();
    assert!(head_lifted(&state), "the hand has left in the newest state");
    let events: Vec<TouchEvent> = together
        .iter()
        .flat_map(|(_, _, events)| events.clone())
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event.gesture == TouchGesture::Hold),
        "the Hold is in the same poll"
    );
    send(&owner, &sender, Input::TouchState(Box::new(state)));
    for event in events {
        send(&owner, &sender, Input::Touch(Box::new(event)));
    }
    owner.sleep_cue_sounding(at);
    let seen = owner.poll_at(at).unwrap();
    assert!(
        owner.hold_owed.is_some(),
        "the Hold is taken, its sleep owed"
    );
    assert!(!seen.head_lifted, "the Hold came first: the cue is not cut");
    assert!(!seen.muted, "owed while its cue sounds");
    // The cue no longer said to be sounding: the owed sleep lands.
    let seen = owner.poll_at(at + 10_000).unwrap();
    assert!(seen.mute_changed && seen.asleep, "{seen:?}");
}

/// #960, `LAMP_RESTORED_MUTE=on`: a lamp brought up muted by its restart
/// guard after a crash wakes by a head tap, as sleep does; a person's own
/// mute does not, and without the switch neither does the guard's.
#[test]
fn a_tap_wakes_a_mute_left_by_the_restart_guard_but_never_a_persons() {
    let tapped = |guard: bool, mark_guard: bool| {
        let (opened, restored, seen) = a_hand_on_a_muted_lamp(guard, mark_guard, START + 700_000);
        assert!(
            seen.iter()
                .any(|(_, seen, _)| seen.head_tap_at_us.is_some())
        );
        (opened.is_some(), restored)
    };
    assert_eq!(tapped(true, true), (true, false), "the guard's mute wakes");
    assert_eq!(tapped(false, true), (false, false), "a person's mute stays");
    assert_eq!(tapped(true, false), (false, false), "off: as before");
}

/// #3747 (the Reviewer-lead on #3767): a hand kept a second wakes the
/// restart guard's mute at its stroke, as it wakes sleep, the hand still
/// there; a person's mute still stays, and without the switch the guard's.
#[test]
fn a_hand_kept_a_second_wakes_a_mute_left_by_the_restart_guard_but_never_a_persons() {
    let kept = |guard: bool, mark_guard: bool| {
        let (opened, restored, seen) = a_hand_on_a_muted_lamp(guard, mark_guard, START + 2_000_000);
        assert!(
            seen.iter()
                .all(|(_, seen, _)| seen.head_tap_at_us.is_none())
        );
        (opened, restored)
    };
    let stroke = START + 510_000 + 1_000_000;
    assert_eq!(
        kept(true, true),
        (Some(stroke), false),
        "the guard's mute wakes"
    );
    assert_eq!(kept(false, true), (None, false), "a person's mute stays");
    assert_eq!(kept(true, false), (None, false), "off: as before");
}

/// A lamp brought up muted, by its restart guard (`guard`) or by a person,
/// with `LAMP_RESTORED_MUTE` as `mark_guard`, and a hand on its head from
/// 1.51 s to `lifted_us`, then off for half a second, through the
/// quiescent wait: when it opened, if it did, saved; whether it still says
/// it was restored after a fault; and each step's observation.
fn a_hand_on_a_muted_lamp(
    guard: bool,
    mark_guard: bool,
    lifted_us: u64,
) -> (Option<u64>, bool, Vec<(u64, Observation, bool)>) {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    if guard {
        // As `arm_active` writes it: the live record, muted.
        record.muted = true;
        fs::write(
            directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}")),
            format!("{CLOCK}\n{}\n", record.through_us),
        )
        .unwrap();
    } else {
        assert!(record.request_mute(START / 2));
    }
    record.save(&directory.file()).unwrap();
    let (mut owner, sender) = from_record(head_options(Some(&directory)), record, START);
    owner.mark_guard = mark_guard;
    owner.restore_guard_marker(stored_identity(&directory.file()));
    assert_eq!(owner.restored_after_fault(), guard && mark_guard);
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), true);
    let mut ticks = head.run(lifted_us, true);
    ticks.extend(head.run(lifted_us + 500_000, false));
    let seen = feed(&mut owner, &sender, ticks, true);
    let opened = seen
        .iter()
        .find(|(_, _, allowed)| *allowed)
        .map(|(at, _, _)| *at);
    let saved = MuteRecord::load(&directory.file(), CLOCK, lifted_us + 600_000).unwrap();
    assert_eq!(saved.muted, opened.is_none());
    // Not a hand's sleep: no "hm?" for it (#1362).
    assert_eq!(owner.take_woken(lifted_us + 600_000), None);
    (opened, owner.restored_after_fault(), seen)
}

/// The reviewer's probe on #997 (P1): a person mutes with the button while
/// the lamp listens, the guard's marker still on disk, and the voice dies in
/// teardown before the mute is saved. The coordinator removed the marker
/// when the mute was accepted (`mute_chosen`), so after the restart the
/// disk's mute is not taken for the guard's, and a head tap leaves it shut.
#[test]
fn a_persons_mute_left_unsaved_by_a_crash_is_never_opened_by_a_tap() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, sender) = fixture(head_options(Some(&directory)));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    assert!(marker.is_file());
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    let seen = observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(seen.muted && seen.mute_changed);
    // What `run_inner` does when the loop returns the mute, before teardown.
    owner.mute_chosen().unwrap();
    assert!(!marker.exists(), "gone before teardown");
    // A crash in teardown: no persist_muted, no disarm.
    drop(owner);
    let record = MuteRecord::load(&directory.file(), CLOCK, 4_000_000).unwrap();
    assert!(record.muted, "the guard's mute is what the disk holds");
    let (mut restarted, sender) = from_record(head_options(Some(&directory)), record, 4_000_000);
    restarted.mark_guard = true;
    restarted.restore_guard_marker(stored_identity(&directory.file()));
    assert!(!restarted.restored_after_fault());
    let mut head = TestHead::new(CLOCK, 4_000_000);
    feed(&mut restarted, &sender, head.run(4_500_000, false), true);
    let mut ticks = head.run(4_700_000, true);
    ticks.extend(head.run(5_200_000, false));
    let tapped = feed(&mut restarted, &sender, ticks, true);
    assert!(
        tapped
            .iter()
            .any(|(_, seen, _)| seen.head_tap_at_us.is_some())
    );
    assert!(
        tapped
            .iter()
            .all(|(_, seen, allowed)| seen.muted && !allowed)
    );
}

/// The reviewer's P2 on #997: the marker is what lets a tap clear a mute,
/// so a removal that fails fails closed, as a failed save does.
#[test]
fn a_guard_marker_that_cannot_be_removed_fails_closed() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    // A marker that cannot be unlinked as a file.
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    fs::write(marker.join("held"), b"").unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(owner.mute_chosen().is_err());
    assert!(owner.health().persistence_failed);
    assert!(owner.quiescent_at(4_100_000).is_err(), "nothing opens");
    assert!(owner.disarm_after_cleanup().is_err());
}

/// The reviewer's P2 on #997, across the restart a fault causes: a guard's
/// marker (a regular file naming the guard record) left beside a person's
/// saved mute never counts, since the person's mute moved `through_us`
/// forward; the restarted lamp's tap leaves it shut. And `persist_muted`
/// saves the person's mute even when the marker cannot be removed, returning
/// the fault after.
#[test]
fn a_marker_left_beside_a_persons_saved_mute_never_lets_a_tap_open_it() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    // The guard as `arm_active` writes it, then a person's mute saved over
    // it, the marker still there (as if its removal had failed).
    let mut guard = MuteRecord::new(CLOCK).unwrap();
    guard.muted = true;
    guard.save(&directory.file()).unwrap();
    fs::write(&marker, format!("{CLOCK}\n{}\n", guard.through_us)).unwrap();
    let mut person = MuteRecord::new(CLOCK).unwrap();
    assert!(person.request_mute(4_000_000));
    person.save(&directory.file()).unwrap();
    // The restart.
    let record = MuteRecord::load(&directory.file(), CLOCK, 4_000_000).unwrap();
    let (mut restarted, sender) = from_record(head_options(Some(&directory)), record, 4_000_000);
    restarted.mark_guard = true;
    restarted.restore_guard_marker(stored_identity(&directory.file()));
    assert!(!restarted.restored_after_fault());
    let mut head = TestHead::new(CLOCK, 4_000_000);
    feed(&mut restarted, &sender, head.run(4_500_000, false), true);
    let mut ticks = head.run(4_700_000, true);
    ticks.extend(head.run(5_200_000, false));
    let tapped = feed(&mut restarted, &sender, ticks, true);
    assert!(
        tapped
            .iter()
            .any(|(_, seen, _)| seen.head_tap_at_us.is_some())
    );
    assert!(
        tapped
            .iter()
            .all(|(_, seen, allowed)| seen.muted && !allowed)
    );
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, 5_300_000)
            .unwrap()
            .muted
    );

    // A removal that fails: the person's mute is still saved, then the fault.
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    fs::write(marker.join("held"), b"").unwrap();
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(owner.persist_muted().is_err());
    let saved = MuteRecord::load(&directory.file(), CLOCK, 4_000_000).unwrap();
    assert!(
        saved.muted && saved.through_us == 4_000_000,
        "the person's mute"
    );
}

#[test]
fn the_head_never_opens_a_privacy_mute_and_sleeps_only_with_the_durable_mute() {
    // Muted by the button's hold: a hold of the head and a tap change
    // nothing; the mute stays a person's privacy.
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    assert!(record.request_mute(START / 2));
    record.save(&directory.file()).unwrap();
    let (mut owner, sender) = from_record(head_options(Some(&directory)), record, START);
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_MUTED);
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), true);
    let mut ticks = head.run(START + 3_700_000, true);
    ticks.extend(head.run(START + 4_500_000, false));
    ticks.extend(head.run(START + 4_700_000, true));
    ticks.extend(head.run(START + 5_200_000, false));
    let seen = feed(&mut owner, &sender, ticks, true);
    assert!(
        seen.iter()
            .any(|(_, seen, _)| seen.head_tap_at_us.is_some())
    );
    assert!(
        seen.iter()
            .all(|(_, seen, allowed)| seen.muted && !seen.asleep && !allowed)
    );
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_MUTED);

    // Head touch without `--controls-mute`: a hold is not a sleep, and
    // `voice/mute` is never said.
    let (mut owner, sender) = fixture(head_options(None));
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_NONE);
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), false);
    let held = feed(
        &mut owner,
        &sender,
        head.run(START + 3_700_000, true),
        false,
    );
    assert!(
        held.iter()
            .all(|(_, seen, _)| !seen.muted && !seen.mute_changed)
    );
    assert_eq!(owner.shared.latch.load(Ordering::Acquire), LATCH_NONE);
}

#[test]
fn the_pump_says_the_latch_on_voice_mute_when_it_changes_and_every_second() {
    let directory = Directory::new();
    let root = directory.0.join("bus");
    let bus = Bus::new(&root).unwrap();
    let mut heard = bus
        .subscribe_typed(VoiceMute::TOPIC, "mute_test", VoiceMute::TYPE)
        .unwrap();
    let (sender, _input) = mpsc::sync_channel(QUEUE_CAPACITY);
    let shared = Arc::new(Shared::default());
    shared.latch.store(LATCH_OPEN, Ordering::Release);
    let (pump_shared, pump_root) = (Arc::clone(&shared), root.clone());
    let worker = thread::spawn(move || pump(pump_root, true, true, None, sender, &pump_shared));
    let next = |heard: &mut Subscriber| -> VoiceMute {
        let message = heard
            .recv(Duration::from_secs(2))
            .unwrap()
            .expect("voice/mute is said");
        assert_eq!(message.header.source, "voice");
        assert_eq!(message.header.expires_ms, Some(3_000));
        message.parse().unwrap()
    };
    assert_eq!(
        next(&mut heard),
        VoiceMute {
            muted: false,
            reason: None,
            muted_by: None,
        }
    );
    shared.latch.store(LATCH_ASLEEP, Ordering::Release);
    let started = std::time::Instant::now();
    assert!(next(&mut heard).asleep());
    assert!(started.elapsed() < Duration::from_millis(500));
    // Unchanged, it is said again within about a second.
    assert!(next(&mut heard).asleep());
    shared.stop.store(true, Ordering::Release);
    worker.thread().unpark();
    worker.join().unwrap();
}

/// With `LAMP_CONTROLS_WAIT=on`: the pipe the owner wakes the pump with, its
/// write end in `shared` and its read end for the pump.
fn waking(shared: &Shared) -> std::os::fd::OwnedFd {
    let (read, write) = wake_pipe().unwrap();
    shared.wake.set(write).unwrap();
    read
}

/// Readiness is an observed completed drain after subscriptions, not a delay.
fn pump_ready_before(shared: &Shared, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while !shared.pump_ready.load(Ordering::Acquire) {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_micros(100));
    }
    true
}

/// Join before any fixture assertion, including a readiness failure. A stuck
/// join belongs to the outer test watchdog; never detach a still-running pump.
fn stop_idle_pump(shared: &Shared, worker: thread::JoinHandle<()>) {
    shared.stop.store(true, Ordering::Release);
    shared.wake();
    worker.thread().unpark();
    let stopping = std::time::Instant::now();
    let joined = worker.join();
    let elapsed = stopping.elapsed();
    assert!(joined.is_ok(), "the idle pump panicked");
    assert!(
        elapsed < Duration::from_secs(2),
        "the pump took {elapsed:?} to stop and join (limit 2 s)"
    );
}

/// Holding the worker before pump entry cannot satisfy readiness. Release the
/// same worker and require an actual subscribed drain, then clean up before
/// checking either result. This establishes startup ordering, not throughput.
#[test]
fn an_idle_pump_is_ready_only_after_its_first_subscribed_drain() {
    let directory = Directory::new();
    let root = directory.0.join("bus");
    let (sender, _input) = mpsc::sync_channel(QUEUE_CAPACITY);
    let shared = Arc::new(Shared::default());
    let (release, held) = mpsc::sync_channel(1);
    let pump_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || {
        if held.recv().is_ok() {
            pump(root, true, false, None, sender, &pump_shared);
        }
    });
    let prematurely_ready = pump_ready_before(&shared, Duration::from_millis(20));
    let passes_while_held = shared.passes.load(Ordering::Relaxed);
    let released = release.send(()).is_ok();
    let ready = pump_ready_before(&shared, Duration::from_secs(2));
    stop_idle_pump(&shared, worker);
    assert!(!prematurely_ready, "held startup was reported ready");
    assert_eq!(passes_while_held, 0, "held startup completed a pass");
    assert!(released, "the held worker exited before release");
    assert!(ready, "the released pump did not finish its first drain");
}

/// Read only after the worker joined. Progress is sampled before stop so
/// stop's unpark cannot manufacture a completed idle wait for the test.
#[derive(Debug)]
struct TickWaitReceipt {
    progressed: bool,
    completed: u64,
    shortest: Duration,
    longest: Duration,
}

/// A pump on a bus with no publishers: how many passes it makes over
/// `for_ms`, and then, with the wait, how long after a wake it comes round
/// again, the wake given 20 ms into a 200 ms wait.
fn idle_pump(wait: bool, for_ms: u64) -> (u64, Option<Duration>, Option<TickWaitReceipt>) {
    let directory = Directory::new();
    let root = directory.0.join("bus");
    let (sender, _input) = mpsc::sync_channel(QUEUE_CAPACITY);
    let shared = Arc::new(Shared::default());
    let woken = wait.then(|| waking(&shared));
    let (pump_shared, pump_root) = (Arc::clone(&shared), root.clone());
    let worker = thread::spawn(move || pump(pump_root, true, false, woken, sender, &pump_shared));
    if !pump_ready_before(&shared, Duration::from_secs(2)) {
        stop_idle_pump(&shared, worker);
        panic!("the idle pump did not finish its first subscribed drain");
    }
    let before = shared.passes.load(Ordering::Relaxed);
    let ticks_before = shared.tick_waits.count.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(for_ms));
    let passes = shared.passes.load(Ordering::Relaxed) - before;
    let next_pass = |after: u64| {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while shared.passes.load(Ordering::Relaxed) == after {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_micros(100));
        }
        true
    };
    // Under load the wake can come after the wait has ended, and the pass
    // timed is then the wait's own: that can hide a pump that ignores the
    // wake, never fail one that heeds it (the lead's reviewer on #2274).
    let woken = wait.then(|| {
        if !next_pass(shared.passes.load(Ordering::Relaxed)) {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
        let waking = std::time::Instant::now();
        let at = shared.passes.load(Ordering::Relaxed);
        shared.wake();
        next_pass(at).then(|| waking.elapsed())
    });
    // Observe actual off-mode wait progress, not a host throughput floor.
    // Record a missing observation, then stop and join before asserting it.
    let tick_progress = (!wait).then(|| {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while shared.tick_waits.count.load(Ordering::Acquire) == ticks_before
            && std::time::Instant::now() < deadline
        {
            thread::sleep(Duration::from_micros(100));
        }
        shared.tick_waits.count.load(Ordering::Acquire) > ticks_before
    });
    // Subscription teardown can be slow beside SQLite stores syncing. Keep
    // its existing two-second bound, checking it only after the owned join.
    stop_idle_pump(&shared, worker);
    let woken = woken.map(|observed| observed.expect("the pump stopped passing"));
    let ticks = tick_progress.map(|progressed| TickWaitReceipt {
        progressed,
        completed: shared.tick_waits.count.load(Ordering::Acquire),
        shortest: Duration::from_nanos(shared.tick_waits.shortest_ns.load(Ordering::Relaxed)),
        longest: Duration::from_nanos(shared.tick_waits.longest_ns.load(Ordering::Relaxed)),
    });
    (passes, woken, ticks)
}

/// #705, the lamp's heat: with nothing coming, the waiting pump wakes about
/// five times a second (the bus's 200 ms rescan). The off-mode park still
/// requests 2 ms, without claiming a loaded host completes 100 passes/s.
/// A stop still ends it at once. A stop is a wake: the
/// wake brings the pump round at once, not at its wait's end, and the pass it
/// brings finds the stop. That is judged by the pump's next pass; its
/// thread's end is only bounded loosely, at 2 s: ending, the pump lets go of
/// its subscriptions on disk, and beside four SQLite-like stores syncing
/// (`F_FULLFSYNC`) a woken stop took a median of 225 to 234 ms on a Mac, up
/// to 265 ms, while the wake's pass came within 1.5 ms (#1569 row 35).
#[test]
fn with_the_wait_an_idle_pump_wakes_five_times_a_second_and_stops_at_once() {
    let (waiting, woken, _) = idle_pump(true, 1_000);
    assert!(waiting <= 10, "{waiting} passes in 1 s");
    let woken = woken.unwrap();
    assert!(woken < Duration::from_millis(100), "{woken:?}");
    let (ticking, _, ticks) = idle_pump(false, 1_000);
    let ticks = ticks.expect("off-mode wait receipt after stop and join");
    eprintln!("idle pump: {ticking} off-mode passes in the sample; {ticks:?}");
    assert!(
        ticks.progressed,
        "no completed off-mode park before stop: {ticks:?}"
    );
    assert!(ticks.completed > 0, "{ticks:?}");
    assert_eq!(ticks.shortest, Duration::from_millis(2), "{ticks:?}");
    assert_eq!(ticks.longest, Duration::from_millis(2), "{ticks:?}");
}

/// An owner with head sleep on (`voice/mute` said) and the wait, its pump
/// running on a bus in `directory`, as `restore` makes one but on the test
/// clock, so it runs where the host has no boot id (macOS); and a sender
/// into its queue, as the pump's.
fn waiting_owner_with_head_sleep(directory: &Directory) -> (HandControls, SyncSender<Queued>) {
    let mut options = head_options(Some(directory));
    options.bus_root = Some(directory.0.join("b"));
    options.wait = true;
    assert!(options.head_sleep());
    let (mut owner, sender) = fixture(options);
    owner.start_pump(sender.clone()).unwrap();
    (owner, sender)
}

/// The reviewer on #1456: with head sleep on, the owner shares its latch on
/// every poll, and the coordinator polls every 2 to 5 ms. An unchanged latch
/// does not wake a waiting pump: polled every 2 ms for a second, it passes
/// no more than its own waits make it.
#[test]
fn with_the_wait_an_owner_polling_every_2_ms_does_not_wake_the_pump() {
    let directory = Directory::new();
    let (mut owner, _sender) = waiting_owner_with_head_sleep(&directory);
    thread::sleep(Duration::from_millis(300));
    let before = owner.shared.passes.load(Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut at_us = START;
    while started.elapsed() < Duration::from_secs(1) {
        at_us += 2_000;
        owner.poll_at(at_us).unwrap();
        thread::sleep(Duration::from_millis(2));
    }
    let passes = owner.shared.passes.load(Ordering::Relaxed) - before;
    assert!(passes <= 10, "{passes} passes in 1 s");
}

/// The reviewer on #1456: a mute the owner takes through its poll wakes the
/// waiting pump, so `voice/mute` says it at once, not at the wait's end
/// (up to 200 ms later).
#[test]
fn with_the_wait_a_mute_taken_through_the_owners_poll_is_said_at_once() {
    let directory = Directory::new();
    let bus = Bus::new(directory.0.join("b")).unwrap();
    let mut heard = bus
        .subscribe_typed(VoiceMute::TOPIC, "mute_test", VoiceMute::TYPE)
        .unwrap();
    let (mut owner, sender) = waiting_owner_with_head_sleep(&directory);
    let first = heard.recv(Duration::from_secs(2)).unwrap().expect("said");
    assert!(!first.parse::<VoiceMute>().unwrap().muted);
    // Just after the pump's pass that said it, well inside its wait.
    thread::sleep(Duration::from_millis(30));
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    let muted = std::time::Instant::now();
    let seen = observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(seen.muted && seen.mute_changed);
    let message = heard.recv(Duration::from_secs(2)).unwrap().expect("said");
    assert!(message.parse::<VoiceMute>().unwrap().muted);
    assert!(
        muted.elapsed() < Duration::from_millis(100),
        "{:?}",
        muted.elapsed()
    );
}

/// With the wait, a latch the owner changes is said at once: the owner
/// wakes the pump, which does not wait out its 200 ms.
#[test]
fn with_the_wait_a_changed_latch_is_said_at_once() {
    let directory = Directory::new();
    let root = directory.0.join("bus");
    let bus = Bus::new(&root).unwrap();
    let mut heard = bus
        .subscribe_typed(VoiceMute::TOPIC, "mute_test", VoiceMute::TYPE)
        .unwrap();
    let (sender, _input) = mpsc::sync_channel(QUEUE_CAPACITY);
    let shared = Arc::new(Shared::default());
    let woken = Some(waking(&shared));
    shared.latch.store(LATCH_OPEN, Ordering::Release);
    let (pump_shared, pump_root) = (Arc::clone(&shared), root.clone());
    let worker = thread::spawn(move || pump(pump_root, true, true, woken, sender, &pump_shared));
    let first = heard.recv(Duration::from_secs(2)).unwrap().expect("said");
    assert!(!first.parse::<VoiceMute>().unwrap().muted);
    // Well inside its 1 s repeat, so only the wake can say it this soon.
    thread::sleep(Duration::from_millis(250));
    shared.latch.store(LATCH_ASLEEP, Ordering::Release);
    shared.wake();
    let changed = std::time::Instant::now();
    let message = heard.recv(Duration::from_secs(2)).unwrap().expect("said");
    assert!(message.parse::<VoiceMute>().unwrap().asleep());
    assert!(
        changed.elapsed() < Duration::from_millis(100),
        "{:?}",
        changed.elapsed()
    );
    shared.stop.store(true, Ordering::Release);
    shared.wake();
    worker.join().unwrap();
}

/// Test-owned workers are stopped and joined even when a fixture assertion
/// unwinds. This does not change the service's bounded shutdown policy.
struct JoinedHeadPump {
    shared: Arc<Shared>,
    worker: Option<thread::JoinHandle<()>>,
}

impl JoinedHeadPump {
    fn stop_and_join(&mut self) -> thread::Result<()> {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.wake();
        match self.worker.take() {
            Some(worker) => {
                worker.thread().unpark();
                worker.join()
            }
            None => Ok(()),
        }
    }
}

impl Drop for JoinedHeadPump {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

#[test]
fn the_head_touch_fixture_joins_its_worker_before_a_failed_check_returns() {
    let shared = Arc::new(Shared::default());
    let stopped = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&stopped);
    let pump_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || {
        while !pump_shared.stop.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(2));
        }
        finished.store(true, Ordering::Release);
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _worker = JoinedHeadPump {
            shared,
            worker: Some(worker),
        };
        panic!("controlled fixture failure");
    }));
    assert!(result.is_err());
    assert!(
        stopped.load(Ordering::Acquire),
        "worker was joined before return"
    );
}

/// A head touch published on a real bus to the owner's poll accepting it,
/// `rounds` times: each round's time, in µs.
fn head_touch_rounds(wait: bool, rounds: usize) -> Vec<u64> {
    let directory = Directory::new();
    let mut elapsed_us = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let mut attempt = 0;
        loop {
            // A fresh bus, publishers, listener and pump each round; all of it,
            // and the recognizer's own work, is outside the measured boundary.
            let root = directory.0.join(format!("b{round}-{attempt}"));
            // How long the two publications themselves took, for the retry below.
            let mut publish_took = 0;
            let bus = Bus::new(&root).unwrap();
            let declared =
                |topic, declaration| bus.publisher_declared(topic, &declaration, "controls");
            let mut states = declared(TouchState::TOPIC, TouchState::declaration()).unwrap();
            let mut events = declared(TouchEvent::TOPIC, TouchEvent::declaration()).unwrap();
            let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
            let shared = Arc::new(Shared::default());
            let mut owner = HandControls::from_parts(
                ControlsOptions {
                    head_touch: true,
                    ..Default::default()
                },
                CLOCK,
                MuteRecord::new(CLOCK).unwrap(),
                input,
                Arc::clone(&shared),
                lamp_ipc::monotonic_us(),
            )
            .unwrap();
            let woken = wait.then(|| waking(&shared));
            let (pump_shared, pump_root) = (Arc::clone(&shared), root.clone());
            let worker =
                thread::spawn(move || pump(pump_root, true, false, woken, sender, &pump_shared));
            let mut worker = JoinedHeadPump {
                shared: Arc::clone(&shared),
                worker: Some(worker),
            };
            let result = (|| -> Result<u64, String> {
                let ready_until = std::time::Instant::now() + Duration::from_secs(2);
                while !shared.pump_ready.load(Ordering::Acquire) {
                    if std::time::Instant::now() >= ready_until {
                        return Err("pump never completed its first subscribed drain".into());
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                let (state, touch) = test_head_touch(CLOCK, lamp_ipc::monotonic_us() - 40_000);
                let published = lamp_ipc::monotonic_us();
                states
                    .publish_json(published, &state)
                    .map_err(|error| format!("state publication: {error}"))?;
                let state_sent = lamp_ipc::monotonic_us();
                events
                    .publish_json(published, &touch[0])
                    .map_err(|error| format!("event publication: {error}"))?;
                let event_sent = lamp_ipc::monotonic_us();
                publish_took = event_sent.saturating_sub(published);
                let receipt_until = std::time::Instant::now() + Duration::from_secs(2);
                loop {
                    if owner
                        .poll()
                        .map_err(|error| format!("owner poll: {error}"))?
                        .head_touch_at_us
                        .is_some()
                    {
                        return Ok(lamp_ipc::monotonic_us() - published);
                    }
                    if std::time::Instant::now() >= receipt_until {
                        let now = lamp_ipc::monotonic_us();
                        return Err(format!(
                            "touch did not arrive; round={round}, state_publish_us={}, event_publish_us={}, state_age_at_publish_us={}, event_age_at_publish_us={}, receipt_wait_us={}, health={:?}",
                            state_sent.saturating_sub(published),
                            event_sent.saturating_sub(state_sent),
                            published.saturating_sub(state.stamp.at_us),
                            published.saturating_sub(touch[0].stamp.at_us),
                            now.saturating_sub(event_sent),
                            owner.health()
                        ));
                    }
                    thread::yield_now();
                }
            })();
            // Preserve failure evidence, then stop and join before asserting it.
            worker.stop_and_join().expect("head-touch pump joined");
            // The touch is stamped as it is published, and the owner takes it
            // only while its source's reading is under `TOUCH_INPUT_TTL_US`
            // (100 ms) old. On a loaded Mac the publication alone took 93 to
            // 299 ms (30 Oct, load 13), so the event was rightly refused as
            // stale and the round could not pass by design. A round whose own
            // publications took half that bound is the host's, not the voice's:
            // it is run again, at most `ROUND_TRIES` times, and said. Any other
            // failure fails at once.
            if result.is_err()
                && publish_took >= lamp_msgs::TOUCH_INPUT_TTL_US / 2
                && attempt + 1 < ROUND_TRIES
            {
                eprintln!(
                    "head-touch round {round}: the host took {publish_took} µs to publish; run again"
                );
                attempt += 1;
                continue;
            }
            elapsed_us.push(result.unwrap_or_else(|error| panic!("{error}")));
            break;
        }
    }
    elapsed_us
}

/// How often one head-touch round is run again when the host stalled its
/// own publications past half the touch input's freshness bound.
const ROUND_TRIES: usize = 5;

/// #705, the lamp's heat: with the wait, a head touch reaches the owner as
/// it is published, not at the wait's 200 ms end. The bound is loose, for a
/// loaded or emulated runner (2.7 ms under qemu, #1456): only a wait that
/// the touch's datagram does not end can fail it. The latency itself is
/// `head_touch_bus_to_poll_latency`'s (ignored), 0.25 ms on a laptop.
#[test]
fn with_the_wait_a_head_touch_is_read_as_it_arrives() {
    let mut waiting = head_touch_rounds(true, 21);
    waiting.sort_unstable();
    assert!(
        waiting[10] < 100_000,
        "median {} µs: {waiting:?}",
        waiting[10]
    );
}

#[test]
#[ignore = "manual host probe: a head touch on a real bus to the coordinator's poll; no lamp claim"]
fn head_touch_bus_to_poll_latency() {
    for wait in [false, true] {
        head_touch_latency(wait);
    }
}

fn head_touch_latency(wait: bool) {
    let elapsed_us = head_touch_rounds(wait, 100);
    let first_us = elapsed_us[0];
    let mut repeated = elapsed_us[1..].to_vec();
    repeated.sort_unstable();
    let count = repeated.len();
    println!(
        "{}",
        serde_json::json!({
            "measurement":"head_touch_bus_to_poll",
            "host_os":std::env::consts::OS,
            "host_arch":std::env::consts::ARCH,
            "boundary":"touch state then event published on a real bus to the listener's poll accepting the head touch",
            "excludes":["the hand on the pad", "GPIO acquisition and Controls' 20 ms qualification", "the coordinator loop's own tick", "hand::cancel to the speaker (local_hand_stop_dispatch)", "device playback", "acoustic silence"],
            "pump_tick_us":PUMP_TICK.as_micros(),
            "controls_wait":wait,
            "samples":elapsed_us.len(),
            "first_us":first_us,
            "repeated_median_us":repeated[(count - 1) / 2],
            "repeated_p95_us":repeated[(count * 95).div_ceil(100) - 1],
            "repeated_worst_us":repeated[count - 1],
            "lamp_measurement":false
        })
    );
}

#[test]
fn disconnected_bus_input_does_not_reopen_or_fail_healthy_voice() {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    record.request_mute(START);
    let (mut owner, sender) = from_record(options(true, Some(&directory.file())), record, START);
    drop(sender);
    let state = owner.poll_at(START + 100_000).unwrap();
    assert!(state.muted && !state.stop);
    assert!(!owner.quiescent_at(START + 100_000).unwrap());
}

#[cfg(target_os = "linux")]
mod live_bus {
    use super::*;
    use lamp_bus::Publisher;
    use std::{os::unix::net::UnixDatagram, time::Instant};

    fn publish(
        publisher: &mut Publisher,
        clock_id: &str,
        sequence: &mut u64,
        event: Option<ControlGesture>,
    ) {
        *sequence += 1;
        let at_us = lamp_ipc::monotonic_us();
        let mut value = state(*sequence, at_us, event);
        value.stamp.clock_id = clock_id.into();
        value.session_id = publisher.boot().into();
        for event in [&mut value.latest_stop, &mut value.latest_mute]
            .into_iter()
            .flatten()
        {
            event.stamp.clock_id = clock_id.into();
            event.session_id.clone_from(&value.session_id);
        }
        value.check().unwrap();
        publisher.publish_json(at_us, &value).unwrap();
    }

    fn wait_for(
        owner: &mut HandControls,
        mut condition: impl FnMut(Observation, Health) -> bool,
    ) -> Observation {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let observed = owner.poll().unwrap();
            let health = owner.health();
            if condition(observed, health) {
                return observed;
            }
            assert!(Instant::now() < deadline, "controls deadline: {health:?}");
            thread::sleep(PUMP_TICK);
        }
    }

    /// A message the pump refuses is a look that found something, as one it
    /// takes is: the good one behind it is read in the same pass, not a
    /// tick later. Only a look at an empty socket is `Empty`.
    #[test]
    fn a_refused_message_is_read_and_so_is_the_good_one_behind_it() {
        let directory = Directory::new();
        let bus = Bus::new(directory.0.join("b"))
            .unwrap()
            .rescan_every(Duration::from_millis(5));
        let mut subscriber = bus
            .subscribe_typed(ControlsState::TOPIC, SUBSCRIBER, ControlsState::TYPE)
            .unwrap();
        let mut publisher = bus
            .publisher_declared(
                ControlsState::TOPIC,
                &ControlsState::declaration(),
                "controls",
            )
            .unwrap();
        let clock_id = lamp_controls::runtime::clock_id(None).unwrap();
        let (sender, _input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Shared::default();
        let mut sequence = 0;
        // Until the subscriber is found, and then nothing is left.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            publish(&mut publisher, &clock_id, &mut sequence, None);
            thread::sleep(Duration::from_millis(10));
            let read = read_one(&mut subscriber, Topic::State, &sender, &shared).unwrap();
            if read == Pumped::Read {
                break;
            }
            assert!(Instant::now() < deadline, "never delivered");
        }
        while read_one(&mut subscriber, Topic::State, &sender, &shared).unwrap() != Pumped::Empty {}
        let (received, rejected) = (
            shared.received.load(Ordering::Relaxed),
            shared.rejected.load(Ordering::Relaxed),
        );

        publisher
            .publish_json(
                lamp_ipc::monotonic_us(),
                &serde_json::json!({"not": "a state"}),
            )
            .unwrap();
        publish(&mut publisher, &clock_id, &mut sequence, None);
        thread::sleep(Duration::from_millis(20));
        let looks: Vec<_> = (0..3)
            .map(|_| read_one(&mut subscriber, Topic::State, &sender, &shared).unwrap())
            .collect();
        assert_eq!(looks, [Pumped::Read, Pumped::Read, Pumped::Empty]);
        assert_eq!(shared.rejected.load(Ordering::Relaxed), rejected + 1);
        assert_eq!(shared.received.load(Ordering::Relaxed), received + 1);
    }

    #[test]
    fn actual_bus_pump_restarts_publisher_fences_old_owner_and_keeps_mute() {
        let directory = Directory::new();
        let bus_root = directory.0.join("b");
        let options = ControlsOptions {
            stop: true,
            mute_record: Some(directory.file()),
            bus_root: Some(bus_root.clone()),
            head_touch: false,
            wait: false,
        };
        let clock_id = lamp_controls::runtime::clock_id(None).unwrap();
        let mut owner = HandControls::restore(&options).unwrap().unwrap();
        assert!(directory.file().is_file());
        assert!(!owner.muted());
        owner.arm_active().unwrap();
        let bus = Bus::new(&bus_root)
            .unwrap()
            .rescan_every(Duration::from_millis(5));
        let mut first = bus
            .publisher_declared(
                ControlsState::TOPIC,
                &ControlsState::declaration(),
                "controls",
            )
            .unwrap();
        let mut sequence = 0;
        publish(&mut first, &clock_id, &mut sequence, None);
        wait_for(&mut owner, |_, health| health.received >= 1);
        assert_eq!(owner.health().rejected, 0);

        let stop = gesture(ControlIntent::Stop, 1, lamp_ipc::monotonic_us());
        publish(&mut first, &clock_id, &mut sequence, Some(stop.clone()));
        let stopped = wait_for(&mut owner, |observation, _| observation.stop);
        assert_eq!(stopped.acquired_at_us, Some(stop.released_us));
        assert!(stopped.observed_at_us.unwrap() >= stop.released_us);
        let received = owner.health().received;
        publish(&mut first, &clock_id, &mut sequence, Some(stop));
        let duplicate = wait_for(&mut owner, |_, health| health.received > received);
        assert!(!duplicate.stop);
        let mut old_message = bus.latest(ControlsState::TOPIC).unwrap().unwrap();
        drop(first);

        let mut replacement = bus
            .publisher_declared(
                ControlsState::TOPIC,
                &ControlsState::declaration(),
                "controls",
            )
            .unwrap();
        assert_ne!(replacement.boot(), old_message.header.boot);
        sequence = 0;
        let received = owner.health().received;
        publish(&mut replacement, &clock_id, &mut sequence, None);
        wait_for(&mut owner, |_, health| health.received > received);
        publish(
            &mut replacement,
            &clock_id,
            &mut sequence,
            Some(gesture(ControlIntent::Stop, 1, lamp_ipc::monotonic_us())),
        );
        wait_for(&mut owner, |observation, _| observation.stop);

        // A delayed datagram from the old valid publisher uses a fresh payload
        // here so rejection proves the session fence, not simply stale age.
        let mut old_state: ControlsState = old_message.parse().unwrap();
        let now_us = lamp_ipc::monotonic_us();
        old_state.stamp.at_us = now_us;
        old_state.stamp.sequence += 1;
        let mut late_stop = gesture(ControlIntent::Stop, 2, now_us);
        late_stop.stamp.clock_id.clone_from(&clock_id);
        late_stop.session_id.clone_from(&old_state.session_id);
        old_state.latest_stop = Some(late_stop);
        old_state.event_sequence = 2;
        old_state.check().unwrap();
        old_message.header.seq += 1;
        old_message.header.at_unix_ms = wall_ms();
        old_message.payload = serde_json::to_vec(&old_state).unwrap();
        let mut datagram = serde_json::to_vec(&old_message.header).unwrap();
        datagram.push(b'\n');
        datagram.extend_from_slice(&old_message.payload);
        let rejected = owner.health().rejected;
        let socket = UnixDatagram::unbound().unwrap();
        let listeners = bus.subscribers(ControlsState::TOPIC).unwrap();
        assert!(!listeners.is_empty());
        for listener in listeners {
            let path = bus_root
                .join(ControlsState::TOPIC)
                .join("subs")
                .join(format!("{}.sock", listener.socket));
            socket.send_to(&datagram, path).unwrap();
        }
        let retired = wait_for(&mut owner, |_, health| health.rejected > rejected);
        assert!(!retired.stop);

        publish(
            &mut replacement,
            &clock_id,
            &mut sequence,
            Some(gesture(
                ControlIntent::SoftwareMute,
                2,
                lamp_ipc::monotonic_us(),
            )),
        );
        wait_for(&mut owner, |observation, _| observation.muted);
        owner.persist_muted().unwrap();
        drop(replacement);
        let deadline = Instant::now() + Duration::from_millis(350);
        while Instant::now() < deadline {
            let observed = owner.poll().unwrap();
            assert!(observed.muted && !observed.stop);
            thread::sleep(PUMP_TICK);
        }
        assert!(!owner.quiescent().unwrap());
        assert!(
            MuteRecord::load(&directory.file(), &clock_id, lamp_ipc::monotonic_us())
                .unwrap()
                .muted
        );
        assert!(owner.health().pump_running);
        drop(owner); // Requests bounded shutdown of this one persistent pump.
    }
}

/// The pump's pass (#705, 08:37): idle, one look at each subscriber, not
/// eight; a burst still taken in one pass, up to its bound; a closed queue
/// or an error ends it at once.
#[test]
fn the_pump_looks_once_a_pass_when_idle_and_drains_a_burst() {
    let mut subscribed = vec![(0usize, 'a'), (0, 'b'), (0, 'c'), (0, 'd')];
    let mut looks = 0;
    assert_eq!(
        drain(&mut subscribed, |_, _| {
            looks += 1;
            Ok(Pumped::Empty)
        })
        .unwrap(),
        Drained::Done
    );
    assert_eq!(looks, 4, "one look at each of four, idle");
    // Three messages waiting on 'b': three rounds read, a fourth finds none.
    let mut waiting = 3;
    let mut looks = 0;
    drain(&mut subscribed, |_, topic| {
        looks += 1;
        Ok(if topic == 'b' && waiting > 0 {
            waiting -= 1;
            Pumped::Read
        } else {
            Pumped::Empty
        })
    })
    .unwrap();
    assert_eq!((waiting, looks), (0, 16));
    // A flood is bounded: POLL_LIMIT / 2 rounds a pass, as before.
    let mut looks = 0;
    drain(&mut subscribed, |_, _| {
        looks += 1;
        Ok(Pumped::Read)
    })
    .unwrap();
    assert_eq!(looks, 4 * POLL_LIMIT / 2);
    // Closed ends the pass where it is.
    let mut looks = 0;
    assert_eq!(
        drain(&mut subscribed, |_, _| {
            looks += 1;
            Ok(Pumped::Closed)
        })
        .unwrap(),
        Drained::Closed
    );
    assert_eq!(looks, 1);
    assert!(drain(&mut subscribed, |_, _| Err(io::Error::other("gone"))).is_err());
}

/// What the pump costs with nothing happening (#705, 08:37): the real pump on
/// an idle bus with all four subscriptions, its thread's CPU over 10 s from
/// `/proc/self/task`. A measure, not a check: run with `--ignored
/// --nocapture`, on the host or a lamp.
#[test]
#[cfg(target_os = "linux")]
#[ignore = "a 10 s measure of the idle pump's CPU, on the real clock"]
fn measure_the_idle_pumps_cpu() {
    for wait in [false, true] {
        measure_the_idle_pump(wait);
    }
}

#[cfg(target_os = "linux")]
fn measure_the_idle_pump(wait: bool) {
    let directory = Directory::new();
    let options = ControlsOptions {
        stop: true,
        mute_record: Some(directory.file()),
        bus_root: Some(directory.0.join("b")),
        head_touch: true,
        wait,
    };
    let owner = HandControls::restore(&options).unwrap().unwrap();
    let ticks = || -> u64 {
        fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|task| {
                fs::read_to_string(task.path().join("comm"))
                    .is_ok_and(|name| name.trim() == "voice-hand-cont")
            })
            .map(|task| {
                let stat = fs::read_to_string(task.path().join("stat")).unwrap();
                let fields: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..]
                    .split_whitespace()
                    .collect();
                fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
            })
            .sum()
    };
    std::thread::sleep(Duration::from_secs(1));
    let before = ticks();
    std::thread::sleep(Duration::from_secs(10));
    let used = ticks() - before;
    // Linux's USER_HZ is 100 on the lamps and on this host.
    println!(
        "idle pump (LAMP_CONTROLS_WAIT {}): {used} ticks in 10 s, {:.1}% of a core",
        if wait { "on" } else { "off" },
        used as f64 / 10.0
    );
    drop(owner);
}

/// #1505, harm 1: a mute record a newer build wrote (here a field this
/// build does not know, saying unmuted) no longer stops the voice: it
/// restores, muted, and the record is saved back as this build's own.
/// Linux only, as the live bus tests: `restore` reads the kernel's boot id.
#[test]
#[cfg(target_os = "linux")]
fn a_mute_record_from_a_newer_build_restores_muted() {
    let directory = Directory::new();
    fs::write(
        directory.file(),
        r#"{"version":1,"clock_id":"boot","muted":false,"through_us":5,"later":true}"#,
    )
    .unwrap();
    fs::set_permissions(directory.file(), fs::Permissions::from_mode(0o600)).unwrap();
    let options = ControlsOptions {
        stop: true,
        mute_record: Some(directory.file()),
        bus_root: Some(directory.0.join("b")),
        head_touch: false,
        wait: false,
    };
    let owner = HandControls::restore(&options).unwrap().unwrap();
    assert!(owner.muted());
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.file()).unwrap()).unwrap();
    assert_eq!(saved["muted"], true);
    assert!(saved.get("later").is_none(), "{saved}");
}

/// #4332, lamp-06c1 on 29 Oct: a plug pulled just after a save leaves the
/// record's `.pending` marker behind. The voice still starts: a marker that
/// is the record's own inode is a finished write and the record restores as
/// it is; any other marker restores muted. Either way the start's own save
/// works, or `restore` would fail. Linux only, as the live bus tests:
/// `restore` reads the boot id.
#[test]
#[cfg(target_os = "linux")]
fn a_mute_write_cut_by_a_power_cut_never_stops_the_start() {
    for own_inode in [true, false] {
        let directory = Directory::new();
        let mut record = MuteRecord::new("boot").unwrap();
        record.clear_after_explicit_user_request(5).unwrap();
        record.save(&directory.file()).unwrap();
        let name = directory
            .file()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let pending = directory.0.join(format!(".{name}.pending"));
        if own_inode {
            fs::hard_link(directory.file(), &pending).unwrap();
        } else {
            fs::write(&pending, "cut").unwrap();
        }
        let options = ControlsOptions {
            stop: true,
            mute_record: Some(directory.file()),
            bus_root: Some(directory.0.join("b")),
            head_touch: false,
            wait: false,
        };
        let owner = HandControls::restore(&options).unwrap().unwrap();
        assert_eq!(owner.muted(), !own_inode);
        assert!(!pending.exists());
    }
}

/// #1567, B: with `LAMP_GUARD_NEVER_LISTENED=on`, a run that ends before its
/// capture gave any audio, with nothing in flight, releases the guard: the
/// record is written as the latch was (listening), and the count beside it
/// goes up by one. The first block a later run hears ends the count. A run
/// that heard something keeps the guard.
#[test]
fn a_run_that_heard_nothing_releases_the_guard_and_is_counted() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.never_listened = true;
    let count = || fs::read_to_string(owner_count_path(&directory)).ok();
    owner.arm_active().unwrap();
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, START)
            .unwrap()
            .muted
    );
    assert!(owner.release_never_listened().unwrap());
    assert!(
        !MuteRecord::load(&directory.file(), CLOCK, START)
            .unwrap()
            .muted
    );
    assert_eq!(owner.could_not_start(), 1);
    assert_eq!(count().as_deref(), Some("1\n"));
    // The next run fails the same way: two in a row.
    owner.arm_active().unwrap();
    assert!(owner.release_never_listened().unwrap());
    assert_eq!(owner.could_not_start(), 2);
    assert_eq!(count().as_deref(), Some("2\n"));
    // A run that hears: the count is over, and a fault now keeps the guard.
    owner.arm_active().unwrap();
    owner.heard_audio();
    assert_eq!(owner.could_not_start(), 0);
    assert_eq!(count(), None);
    assert!(!owner.release_never_listened().unwrap());
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, START)
            .unwrap()
            .muted,
        "heard, so the guard stands"
    );
    // Each run starts unheard.
    owner.disarm_after_cleanup().unwrap();
    owner.arm_active().unwrap();
    assert!(owner.release_never_listened().unwrap());
}

fn owner_count_path(directory: &Directory) -> PathBuf {
    let mut name = directory.file().file_name().unwrap().to_os_string();
    name.push(NEVER_LISTENED_SUFFIX);
    directory.file().with_file_name(name)
}

/// #1567, B's limits: off unless set; and a control input unread or a mute
/// change unsaved keeps the guard, so a person's mute in the startup is
/// never lost; and a run never armed has nothing to release.
#[test]
fn the_guard_stands_when_off_or_with_a_control_in_flight() {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    assert!(!owner.release_never_listened().unwrap(), "never armed");
    owner.arm_active().unwrap();
    assert!(
        !owner.release_never_listened().unwrap(),
        "the switch is off"
    );
    owner.never_listened = true;
    // An unread input that could carry a press (#1632).
    owner.shared.pressing.fetch_add(1, Ordering::AcqRel);
    assert!(!owner.release_never_listened().unwrap(), "an input unread");
    owner.shared.pressing.fetch_sub(1, Ordering::AcqRel);
    let mute = gesture(ControlIntent::SoftwareMute, 1, 4_000_000);
    observe(
        &mut owner,
        &sender,
        state(1, 4_000_000, Some(mute)),
        4_000_000,
    );
    assert!(owner.dirty);
    assert!(!owner.release_never_listened().unwrap(), "a mute unsaved");
    assert!(
        MuteRecord::load(&directory.file(), CLOCK, START)
            .unwrap()
            .muted
    );
    assert_eq!(owner.could_not_start(), 0);
}

/// The count beside the record, as a restart reads it: a number, else none.
#[test]
fn the_count_of_runs_that_could_not_start_is_read_back_or_none() {
    let directory = Directory::new();
    let (owner, _sender) = fixture(options(false, Some(&directory.file())));
    let path = owner_count_path(&directory);
    assert_eq!(owner.read_could_not_start(), 0);
    fs::write(&path, "3\n").unwrap();
    assert_eq!(owner.read_could_not_start(), 3);
    fs::write(&path, "three").unwrap();
    assert_eq!(owner.read_could_not_start(), 0);
    fs::write(&path, "1234567890123").unwrap();
    assert_eq!(owner.read_could_not_start(), 0);
}

/// The reviewer on #1592: the count is read only with the switch on, and a
/// FIFO or a large file where it is kept never holds the restore up.
#[test]
fn the_count_file_is_read_only_as_a_small_regular_file() {
    let directory = Directory::new();
    let (owner, _sender) = fixture(options(false, Some(&directory.file())));
    let path = owner_count_path(&directory);
    fs::write(&path, "x".repeat(17)).unwrap();
    assert_eq!(owner.read_could_not_start(), 0, "too large");
    fs::remove_file(&path).unwrap();
    // rustix has no mkfifoat on Apple platforms, so the tool, as lamp-body's
    // FIFO test does.
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(owner.read_could_not_start(), 0, "a FIFO is never opened");
}

/// The reviewer's P3 on #1592: the count is loaded at restore only with the
/// switch on; with it off, a count left over says nothing.
#[test]
fn the_count_is_loaded_only_with_the_switch_on() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    fs::write(owner_count_path(&directory), "3\n").unwrap();
    owner.load_could_not_start();
    assert_eq!(owner.could_not_start(), 0, "off");
    owner.never_listened = true;
    owner.load_could_not_start();
    assert_eq!(owner.could_not_start(), 3, "on");
}

/// Jarvis's P3 on #1592: a link where the count is kept is never followed:
/// its target is left as it was, written or removed.
#[test]
fn a_link_where_the_count_is_kept_is_never_followed() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.never_listened = true;
    let target = directory.0.join("elsewhere");
    fs::write(&target, "kept\n").unwrap();
    std::os::unix::fs::symlink(&target, owner_count_path(&directory)).unwrap();
    owner.arm_active().unwrap();
    assert!(owner.release_never_listened().unwrap());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "kept\n",
        "not written through"
    );
    owner.arm_active().unwrap();
    owner.heard_audio();
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "kept\n",
        "not removed through"
    );
    assert!(
        owner_count_path(&directory)
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

/// #1607 (Jarvis's R2 on #1592, flipped): the voice before last looks for
/// input at 1.6 s; a hand held on the head from 1.51 s for 3.2 s, so the
/// hold is reached at 4.51 s, while no voice runs; the next voice starts at
/// 5.9 s. What it sees, with the late sleep `window` or none. Controls'
/// own state comes first, as the window waits for it.
fn a_hold_while_no_voice_ran(
    window: Option<LateSleep>,
    record: MuteRecord,
) -> Vec<(u64, Observation, bool)> {
    let directory = Directory::new();
    let mut head = TestHead::new(CLOCK, START);
    let _ = head.run(START + 500_000, false);
    let _ = head.run(START + 3_700_000, true);
    let _ = head.run(START + 4_900_000, false);
    // The next voice starts as the head's last step ends.
    let at = START + 4_900_000;
    let (mut next, sender) = from_record(head_options(Some(&directory)), record, at);
    if let Some(window) = window {
        next.allow_late_sleep(window);
    }
    send(&next, &sender, Input::State(Box::new(state(1, at, None))));
    feed(
        &mut next,
        &sender,
        head.run(START + 6_000_000, false),
        false,
    )
}

/// The window [`restored_record`] would open for a voice starting at `at`
/// whose record's watermark is `through` and whose voice before looked last
/// at `looked`.
fn window(through: u64, looked: u64, at: u64) -> LateSleep {
    LateSleep {
        began_after_us: through,
        reached_after_us: through.max(looked),
        reached_before_us: at,
    }
}

fn asleep(seen: &[(u64, Observation, bool)]) -> bool {
    seen.iter().all(|(_, seen, _)| seen.muted && seen.asleep)
}

fn listening(seen: &[(u64, Observation, bool)]) -> bool {
    seen.iter().all(|(_, seen, _)| !seen.muted)
}

/// The hold is taken when it began after the watermark, was reached after
/// the voice before last looked, and before this one started. A hand that
/// came before the look and reached 3 s after it is taken: that voice never
/// had the hold (the reviewer's P2 on #1622).
#[test]
fn a_head_hold_made_while_no_voice_ran_puts_the_next_one_to_sleep() {
    let fresh = MuteRecord::new(CLOCK).unwrap();
    let through = fresh.through_us;
    let at = START + 4_900_000;
    let hold = |window| a_hold_while_no_voice_ran(window, fresh.clone());
    // Looked at 0.5 s, before the hand came: taken.
    assert!(asleep(&hold(Some(window(through, START + 500_000, at)))));
    // Looked at 1.6 s, after the hand came at 1.51 s but before 3 s of it.
    let seen = hold(Some(window(through, START + 600_000, at)));
    assert!(asleep(&seen), "{seen:?}");
    // As before: no window, a look after the hold was reached, a watermark
    // after the hand came, or a start before it was reached: the lamp
    // listens.
    assert!(listening(&hold(None)));
    assert!(listening(&hold(Some(window(
        through,
        START + 3_600_000,
        at
    )))));
    assert!(listening(&hold(Some(window(
        START + 520_000,
        START + 520_000,
        at
    )))));
    assert!(listening(&hold(Some(window(
        through,
        START + 500_000,
        START + 3_000_000
    )))));
}

/// The reviewer's P3 on #1622: a hold, then a tap, both while no voice ran:
/// the person's last act woke it, so the next voice stays awake.
#[test]
fn a_hold_then_a_tap_while_no_voice_ran_leaves_the_next_one_awake() {
    let directory = Directory::new();
    let mut head = TestHead::new(CLOCK, START);
    let _ = head.run(START + 500_000, false);
    let _ = head.run(START + 3_700_000, true);
    let _ = head.run(START + 4_000_000, false);
    let _ = head.run(START + 4_150_000, true);
    let _ = head.run(START + 4_900_000, false);
    let at = START + 4_900_000;
    let fresh = MuteRecord::new(CLOCK).unwrap();
    let through = fresh.through_us;
    let (mut next, sender) = from_record(head_options(Some(&directory)), fresh, at);
    next.allow_late_sleep(window(through, START + 500_000, at));
    send(&next, &sender, Input::State(Box::new(state(1, at, None))));
    let seen = feed(
        &mut next,
        &sender,
        head.run(START + 6_000_000, false),
        false,
    );
    assert!(listening(&seen), "{seen:?}");
}

/// The reviewer's P3 on #1622: the window waits for Controls' own state.
/// Touch states before it take no late hold; the first touch state after
/// it does.
#[test]
fn the_late_window_waits_for_the_first_controls_state() {
    let directory = Directory::new();
    let mut head = TestHead::new(CLOCK, START);
    let _ = head.run(START + 500_000, false);
    let _ = head.run(START + 3_700_000, true);
    let _ = head.run(START + 4_900_000, false);
    let at = START + 4_900_000;
    let fresh = MuteRecord::new(CLOCK).unwrap();
    let through = fresh.through_us;
    let (mut next, sender) = from_record(head_options(Some(&directory)), fresh, at);
    next.allow_late_sleep(window(through, START + 500_000, at));
    let before = feed(
        &mut next,
        &sender,
        head.run(START + 5_400_000, false),
        false,
    );
    assert!(listening(&before), "no Controls state yet: {before:?}");
    send(
        &next,
        &sender,
        Input::State(Box::new(state(1, START + 5_400_000, None))),
    );
    let after = feed(
        &mut next,
        &sender,
        head.run(START + 6_000_000, false),
        false,
    );
    assert!(
        after
            .last()
            .is_some_and(|(_, seen, _)| seen.muted && seen.asleep),
        "{after:?}"
    );
}

/// #1607: the late path is Sleep only. A lamp put to sleep, then tapped on
/// the head while no voice ran, wakes only by a tap its voice sees: the
/// next one stays asleep.
#[test]
fn a_tap_made_while_no_voice_ran_never_wakes_the_next_one() {
    let directory = Directory::new();
    let mut record = MuteRecord::new(CLOCK).unwrap();
    assert!(record.request_sleep(START + 400_000));
    let mut head = TestHead::new(CLOCK, START);
    let _ = head.run(START + 500_000, false);
    let _ = head.run(START + 650_000, true);
    let _ = head.run(START + 4_900_000, false);
    // The next voice starts as the head's last step ends.
    let at = START + 4_900_000;
    let through = record.through_us;
    let (mut next, sender) = from_record(head_options(Some(&directory)), record, at);
    next.allow_late_sleep(window(through, START + 450_000, at));
    send(&next, &sender, Input::State(Box::new(state(1, at, None))));
    let seen = feed(
        &mut next,
        &sender,
        head.run(START + 6_000_000, false),
        false,
    );
    assert!(asleep(&seen), "{seen:?}");
}

/// The record's time `seconds_before` the next start at `at_us`, as the
/// voice before left it: [`restored_record`] reads it as that voice's last
/// look.
fn left_at(path: &Path, at_us: u64, looked_us: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    let ago = std::time::Duration::from_micros(at_us - looked_us);
    file.set_modified(SystemTime::now() - ago).unwrap();
}

/// The reviewer's P2 on #1622, through the start's own reading of the
/// record: its time, the voice before's last look, is the floor; a hold
/// reached after it is taken, one reached before it is not.
#[test]
fn a_hold_reached_after_the_last_look_is_taken_through_the_records_time() {
    // The next voice starts at 8 s, so a look can come well after the hold
    // is reached: the time between setting the record's time and reading it
    // back only moves the floor earlier, by ms on a loaded host.
    let at = START + 8_000_000;
    let start = |looked: u64| {
        let directory = Directory::new();
        let path = directory.file();
        MuteRecord::new(CLOCK).unwrap().save(&path).unwrap();
        left_at(&path, at, looked);
        let restored = restored_record(&path, CLOCK, at).unwrap();
        let window = restored
            .late_sleep
            .expect("a window from the record's time");
        // Read back about the look; this start is the bound.
        assert!(
            window.reached_after_us.abs_diff(looked) < 500_000,
            "{window:?}"
        );
        assert_eq!(window.reached_before_us, at);
        let mut head = TestHead::new(CLOCK, START);
        let _ = head.run(START + 500_000, false);
        let _ = head.run(START + 3_700_000, true);
        let _ = head.run(at, false);
        let (mut next, sender) = from_record(head_options(Some(&directory)), restored.record, at);
        next.allow_late_sleep(window);
        send(&next, &sender, Input::State(Box::new(state(1, at, None))));
        feed(&mut next, &sender, head.run(at + 1_100_000, false), false)
    };
    // The hand came at 1.51 s and reached 3 s at 4.51 s.
    assert!(asleep(&start(START + 600_000)));
    assert!(listening(&start(START + 6_500_000)));
}

/// The first builder's question on #1622: a sleep the voice took, which the
/// person then woke, never comes back at the next start. The sleep is saved
/// as a mute with its hold's watermark, the wake is saved with the clear's,
/// and the tap after the hold asks to wake: each alone refuses the hold.
#[test]
fn a_sleep_the_person_woke_from_never_comes_back_at_the_next_start() {
    let directory = Directory::new();
    let path = directory.file();
    let (mut owner, sender) = fixture(head_options(Some(&directory)));
    let mut head = TestHead::new(CLOCK, START);
    feed(&mut owner, &sender, head.run(START + 500_000, false), false);
    let held = feed(
        &mut owner,
        &sender,
        head.run(START + 3_700_000, true),
        false,
    );
    assert!(held.iter().any(|(_, seen, _)| seen.asleep));
    owner.persist_muted().unwrap();
    feed(
        &mut owner,
        &sender,
        head.run(START + 4_800_000, false),
        true,
    );
    let mut ticks = head.run(START + 5_000_000, true);
    ticks.extend(head.run(START + 5_500_000, false));
    let tapped = feed(&mut owner, &sender, ticks, true);
    assert!(
        tapped
            .iter()
            .any(|(_, seen, allowed)| *allowed && !seen.muted)
    );
    // The run stops; the next starts at 6 s, the record's time either the
    // wake's save or (as if not kept) before the hold.
    let at = START + 6_000_000;
    for looked in [START + 5_400_000, START + 500_000] {
        left_at(&path, at, looked);
        let restored = restored_record(&path, CLOCK, at).unwrap();
        assert!(!restored.record.muted);
        let (mut next, next_sender) =
            from_record(head_options(Some(&directory)), restored.record, at);
        next.allow_late_sleep(restored.late_sleep.unwrap());
        send(
            &next,
            &next_sender,
            Input::State(Box::new(state(1, at, None))),
        );
        let seen = feed(
            &mut next,
            &next_sender,
            head.run(at + 500_000, false),
            false,
        );
        assert!(listening(&seen), "looked at {looked}: {seen:?}");
    }
}

/// #1607 (the first builder's 21:39): while a release holds the press
/// queue, from its look for inputs in flight through its save, a press
/// waits; it is queued once the save is done, never between.
#[test]
fn a_press_waits_while_a_release_looks_and_saves() {
    let directory = Directory::new();
    let (owner, sender) = fixture(options(false, Some(&directory.file())));
    let shared = Arc::clone(&owner.shared);
    let held = owner.quiet_for_test();
    let pressed = std::thread::spawn(move || {
        let mute = gesture(ControlIntent::Pet, 1, 4_000_000);
        enqueue(&sender, &shared, Input::Pet(mute))
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(owner.shared.queued.load(Ordering::Acquire), 0, "waits");
    drop(held);
    assert!(pressed.join().unwrap());
    assert_eq!(
        owner.shared.queued.load(Ordering::Acquire),
        1,
        "queued after"
    );
}

/// A release of an armed guard, with `queued` sent before its look (in the
/// queue, not yet taken) and `during` sent while it has looked and is
/// saving, `hold` long. Whether it released, and whether the record on disk
/// is muted (the guard kept).
fn release_with(
    queued: Vec<Input>,
    during: Option<Input>,
    hold: std::time::Duration,
) -> (bool, bool) {
    release_after(vec![], queued, during, hold)
}

/// The same, with `taken` sent and taken in the run first.
fn release_after(
    taken: Vec<Input>,
    queued: Vec<Input>,
    during: Option<Input>,
    hold: std::time::Duration,
) -> (bool, bool) {
    let directory = Directory::new();
    let (mut owner, sender) = fixture(options(false, Some(&directory.file())));
    for input in taken {
        send(&owner, &sender, input);
    }
    owner.poll_at(START + 4_100_000).unwrap();
    owner.arm_active().unwrap();
    for input in queued {
        send(&owner, &sender, input);
    }
    let shared = Arc::clone(&owner.shared);
    let (sent_tx, sent_rx) = mpsc::channel();
    // Only an input sent during the save is waited for: with none, the
    // sender stays in a hold the release may never run, and the wait below
    // ran its whole 5 s for nothing (#1128).
    let sending = during.is_some();
    owner.while_held = Some(Box::new(move || {
        if let Some(input) = during {
            let waiting = Arc::clone(&shared);
            let sender = sender.clone();
            std::thread::spawn(move || {
                let _ = sent_tx.send(enqueue(&sender, &waiting, input));
            });
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while shared.waited.load(Ordering::Acquire) == 0 {
                assert!(std::time::Instant::now() < until, "the input never waited");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        std::thread::sleep(hold);
    }));
    let released = owner.release_if_still_quiet().unwrap();
    let saved = MuteRecord::load(&directory.file(), CLOCK, START).unwrap();
    if sending && let Ok(sent) = sent_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        assert!(sent, "queued once the save was done");
    }
    (released, saved.muted)
}

/// A Controls state with a new mute in it, the kind of input that carries a
/// button's press.
fn controls_mute(sequence: u64) -> Input {
    let mute = gesture(ControlIntent::SoftwareMute, sequence, 4_000_000);
    Input::State(Box::new(state(sequence + 1, 4_000_000, Some(mute))))
}

/// The reviewer's P2 on #1622 (the lock changes an outcome), and Fourth's
/// and the reviewer's on #1632 (only what could carry a press): an input
/// that could carry a press, made while a release has looked and is saving,
/// keeps the guard, so the next start is muted; the routine Controls and
/// touch states republished every 100 ms, and a pet, which never changes
/// the mute, do not.
#[test]
fn only_a_press_during_the_releases_save_keeps_the_guard() {
    let none = std::time::Duration::ZERO;
    let mut head = TestHead::new(CLOCK, START);
    let ticks = head.run(START + 500_000, false);
    let (_, routine_touch, _) = ticks.last().cloned().unwrap();
    let mut touched = head.run(START + 700_000, true);
    let hand = touched
        .iter_mut()
        .find_map(|(_, _, events)| events.pop())
        .expect("a head event");
    let pet = Input::Pet(gesture(ControlIntent::Pet, 1, 4_000_000));
    let routine_controls = Input::State(Box::new(state(1, START + 100_000, None)));
    // Kept: a new mute in Controls' state, or a head event.
    assert_eq!(
        release_with(vec![], Some(controls_mute(1)), none),
        (false, true)
    );
    assert_eq!(
        release_with(vec![], Some(Input::Touch(Box::new(hand))), none),
        (false, true)
    );
    // Released: the routine states, and a pet.
    assert_eq!(
        release_with(vec![], Some(routine_controls), none),
        (true, false)
    );
    assert_eq!(
        release_with(
            vec![],
            Some(Input::TouchState(Box::new(routine_touch))),
            none
        ),
        (true, false)
    );
    assert_eq!(release_with(vec![], Some(pet), none), (true, false));
    // And with nothing at all.
    assert_eq!(release_with(vec![], None, none), (true, false));
}

/// A Controls state from `session` whose newest event is a stop or a mute
/// at `sequence`.
fn controls_in(session: &str, sequence: u64, intent: ControlIntent) -> Input {
    let mut state = state(
        sequence + 1,
        4_000_000,
        Some(gesture(intent, sequence, 4_000_000)),
    );
    state.session_id = session.into();
    for event in [&mut state.latest_stop, &mut state.latest_mute]
        .into_iter()
        .flatten()
    {
        event.session_id = session.into();
    }
    state.check().unwrap();
    Input::State(Box::new(state))
}

/// Fourth's P3 on #1632: after Controls restarts, its new session's events
/// count from nothing, so its first mute during the save is a press and
/// keeps the guard, though its number is below the old session's; a lower
/// number from the same session, a state already passed, is not one.
#[test]
fn a_new_controls_sessions_first_mute_is_a_press() {
    let none = std::time::Duration::ZERO;
    let restarted = "fedcba9876543210";
    let seen = || vec![controls_in(SESSION, 9, ControlIntent::Stop)];
    assert_eq!(
        release_after(
            seen(),
            vec![],
            Some(controls_in(restarted, 1, ControlIntent::SoftwareMute)),
            none
        ),
        (false, true),
        "a new session's first mute keeps the guard"
    );
    assert_eq!(
        release_after(
            seen(),
            vec![],
            Some(controls_in(SESSION, 5, ControlIntent::Stop)),
            none
        ),
        (true, false),
        "a number already passed in the same session is not a press"
    );
}

/// The reviewer's P3 on #1632: a state the full queue drops never reached
/// the owner, so its number is forgotten; the same state sent again while
/// the release saves is a press, and keeps the guard.
#[test]
fn a_state_the_full_queue_dropped_counts_again() {
    let none = std::time::Duration::ZERO;
    let mut queued: Vec<Input> = (0..QUEUE_CAPACITY)
        .map(|sequence| Input::Pet(gesture(ControlIntent::Pet, sequence as u64 + 1, 4_000_000)))
        .collect();
    queued.push(controls_in(SESSION, 7, ControlIntent::Stop));
    assert_eq!(
        release_with(
            queued,
            Some(controls_in(SESSION, 7, ControlIntent::Stop)),
            none
        ),
        (false, true)
    );
}

/// The reviewer on #1632, the look: routine states still queued at the end
/// of a run, which nothing takes before the release looks, do not keep the
/// guard; a state with a new mute in it does.
#[test]
fn routine_states_queued_at_the_look_do_not_keep_the_guard() {
    let none = std::time::Duration::ZERO;
    let mut head = TestHead::new(CLOCK, START);
    let (_, routine_touch, _) = head.run(START + 500_000, false).pop().unwrap();
    let routine = vec![
        Input::State(Box::new(state(1, START + 100_000, None))),
        Input::State(Box::new(state(2, START + 200_000, None))),
        Input::TouchState(Box::new(routine_touch)),
    ];
    assert_eq!(release_with(routine, None, none), (true, false));
    assert_eq!(
        release_with(vec![controls_mute(1)], None, none),
        (false, true)
    );
}

/// The reviewer's P3 on #1632: the release sets the record's time to its
/// look, not to its save; the next start reads that as its floor.
#[test]
fn the_records_time_is_the_releases_look_not_its_save() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.arm_active().unwrap();
    owner.while_held = Some(Box::new(|| {
        std::thread::sleep(std::time::Duration::from_millis(300))
    }));
    // Against the moment just before the release, not the file's age after
    // it, so a slow save can neither pass without the time nor fail with it
    // (the reviewer on #1658).
    let released = SystemTime::now();
    assert!(owner.release_if_still_quiet().unwrap());
    let after = record_time(&directory.file())
        .duration_since(released)
        .unwrap_or_default();
    assert!(
        after < std::time::Duration::from_millis(250),
        "{after:?} after the release began: the save's time, not the look's"
    );
}

/// How long before `released` the record at `path` is dated; nothing if it
/// is dated after.
fn dated_before(released: SystemTime, path: &Path) -> std::time::Duration {
    released
        .duration_since(record_time(path))
        .unwrap_or_default()
}

fn record_time(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

/// The reviewer's follow-up on #1632: a clean stop sets the record's time
/// to the conversation's last read of the press queue, not to its save
/// after the teardown, so a head hold reached during the teardown is the
/// next start's. With no read at all, the time is the save's.
#[test]
fn a_clean_stops_record_time_is_the_last_poll_not_its_save() {
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.arm_active().unwrap();
    owner.poll_at(START).unwrap();
    // The teardown.
    std::thread::sleep(std::time::Duration::from_millis(300));
    // Against the moment just before the release, not the file's age after
    // it, so a slow save can neither pass without the time nor fail with it
    // (the reviewer on #1658).
    let released = SystemTime::now();
    owner.disarm_after_clean_stop().unwrap();
    let after_poll = dated_before(released, &directory.file());
    assert!(
        after_poll >= std::time::Duration::from_millis(250),
        "{after_poll:?} before the release: the save's time, not the last poll's"
    );
    let directory = Directory::new();
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.arm_active().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let released = SystemTime::now();
    owner.disarm_after_clean_stop().unwrap();
    let unread = dated_before(released, &directory.file());
    assert!(
        unread < std::time::Duration::from_millis(250),
        "{unread:?} before the release: no poll, so the save's time"
    );
}

/// Fourth's and the reviewer's P3 on #1632: the start gives the window it
/// read from the record to the controls it makes.
#[test]
fn the_start_gives_the_records_window_to_its_controls() {
    let directory = Directory::new();
    let path = directory.file();
    let at = START + 4_900_000;
    MuteRecord::new(CLOCK).unwrap().save(&path).unwrap();
    left_at(&path, at, START + 600_000);
    let restored = restored_record(&path, CLOCK, at).unwrap();
    let window = restored.late_sleep.expect("a window");
    let (_sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
    let controls = HandControls::with_restored(
        &head_options(Some(&directory)),
        CLOCK,
        restored,
        input,
        Arc::new(Shared::default()),
        at,
    )
    .unwrap();
    assert_eq!(controls.late_sleep, Some(window));
}

/// #1607, the receiver itself: of what the head retained while no voice ran,
/// the late window returns a Sleep hold and nothing else: never a tap, which
/// asks to wake, nor a touch.
#[test]
fn the_late_window_returns_only_a_sleep_hold() {
    let late = |hand_on_until: u64| {
        let mut head = TestHead::new(CLOCK, START);
        let _ = head.run(START + 500_000, false);
        let _ = head.run(hand_on_until, true);
        let _ = head.run(START + 4_900_000, false);
        let next = head.run(START + 4_910_000, false);
        let (at, state, _) = &next[0];
        let mut receiver = lamp_controls::touch_receiver::TouchReceiver::new(CLOCK).unwrap();
        receiver.allow_late_sleep(window(0, START + 500_000, START + 4_900_000));
        receiver
            .observe(*at, state)
            .unwrap()
            .into_iter()
            .map(|event| event.gesture)
            .collect::<Vec<_>>()
    };
    assert_eq!(late(START + 3_700_000), [lamp_msgs::TouchGesture::Hold]);
    assert!(
        late(START + 650_000).is_empty(),
        "a tap and its touch: none"
    );
}
