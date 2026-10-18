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
    assert!(MuteRecord::load(&directory.file(), CLOCK, 6_200_000).is_err());
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

/// The first builder on #997: after a reboot restore saves the record rebased
/// to the new clock, and a second restart in that boot (an update's clean
/// stop) must still know the mute is the guard's.
#[test]
fn a_guard_mute_is_still_known_as_the_guards_after_a_reboot_and_another_restart() {
    let directory = Directory::new();
    let marker = directory.0.join(format!("mute.json{GUARD_MARKER_SUFFIX}"));
    // A restart as `restore` does it: the identity as stored, then the
    // record loaded on this boot's clock and saved, then the marker read.
    let restart = |clock: &str, now_us: u64| {
        let stored = stored_identity(&directory.file());
        let record = MuteRecord::load(&directory.file(), clock, now_us).unwrap();
        record.save(&directory.file()).unwrap();
        let (mut restarted, sender) =
            from_record(options(false, Some(&directory.file())), record, now_us);
        restarted.mark_guard = true;
        restarted.restore_guard_marker(stored);
        (restarted, sender)
    };
    let (mut owner, _sender) = fixture(options(false, Some(&directory.file())));
    owner.mark_guard = true;
    owner.arm_active().unwrap();
    drop(owner);
    // A power cut: the next boot has another clock, and the record is rebased.
    let before = stored_identity(&directory.file()).unwrap();
    let (rebooted, _sender) = restart("after-power-cut", 1_000);
    assert!(rebooted.muted() && rebooted.restored_after_fault());
    let rebased = stored_identity(&directory.file()).unwrap();
    assert_ne!(rebased, before, "the reboot rebased the record");
    // The marker now names the record as saved.
    assert_eq!(
        marker_identity(&fs::read_to_string(&marker).unwrap()),
        Some(rebased)
    );
    drop(rebooted);
    // An update stops it cleanly in the same boot: still the guard's.
    let (updated, _sender) = restart("after-power-cut", 2_000);
    assert!(updated.muted() && updated.restored_after_fault());
    assert!(marker.is_file());
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
    let saved = MuteRecord::load(&directory.file(), CLOCK, START + 5_600_000).unwrap();
    assert!(!saved.muted && saved.reason.is_none());
}

/// #960, `LAMP_RESTORED_MUTE=on`: a lamp brought up muted by its restart
/// guard after a crash wakes by a head tap, as sleep does; a person's own
/// mute does not, and without the switch neither does the guard's.
#[test]
fn a_tap_wakes_a_mute_left_by_the_restart_guard_but_never_a_persons() {
    let tapped = |guard: bool, mark_guard: bool| {
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
        let mut ticks = head.run(START + 700_000, true);
        ticks.extend(head.run(START + 1_200_000, false));
        let seen = feed(&mut owner, &sender, ticks, true);
        assert!(
            seen.iter()
                .any(|(_, seen, _)| seen.head_tap_at_us.is_some())
        );
        let opened = seen.iter().any(|(_, _, allowed)| *allowed);
        let saved = MuteRecord::load(&directory.file(), CLOCK, START + 1_300_000).unwrap();
        assert_eq!(saved.muted, !opened);
        (opened, owner.restored_after_fault())
    };
    assert_eq!(tapped(true, true), (true, false), "the guard's mute wakes");
    assert_eq!(tapped(false, true), (false, false), "a person's mute stays");
    assert_eq!(tapped(true, false), (false, false), "off: as before");
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
    let worker = thread::spawn(move || pump(pump_root, true, true, sender, &pump_shared));
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
            reason: None
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

#[test]
#[ignore = "manual host probe: a head touch on a real bus to the coordinator's poll; no lamp claim"]
fn head_touch_bus_to_poll_latency() {
    let directory = Directory::new();
    let mut elapsed_us = Vec::with_capacity(100);
    for round in 0..100 {
        // A fresh bus, publishers, listener and pump each round; all of it,
        // and the recognizer's own work, is outside the measured boundary.
        let root = directory.0.join(format!("b{round}"));
        let bus = Bus::new(&root).unwrap();
        let declared = |topic, declaration| bus.publisher_declared(topic, &declaration, "controls");
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
        let (pump_shared, pump_root) = (Arc::clone(&shared), root.clone());
        let worker = thread::spawn(move || pump(pump_root, true, false, sender, &pump_shared));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while bus.subscribers(TouchEvent::TOPIC).unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "pump never subscribed"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let (state, touch) = test_head_touch(CLOCK, lamp_ipc::monotonic_us() - 40_000);
        let published = lamp_ipc::monotonic_us();
        states.publish_json(published, &state).unwrap();
        events.publish_json(published, &touch[0]).unwrap();
        loop {
            if owner.poll().unwrap().head_touch_at_us.is_some() {
                elapsed_us.push(lamp_ipc::monotonic_us() - published);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the touch never arrived: {:?}",
                owner.health()
            );
            thread::yield_now();
        }
        shared.stop.store(true, Ordering::Release);
        worker.thread().unpark();
        worker.join().unwrap();
    }
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
    let directory = Directory::new();
    let options = ControlsOptions {
        stop: true,
        mute_record: Some(directory.file()),
        bus_root: Some(directory.0.join("b")),
        head_touch: true,
    };
    let _owner = HandControls::restore(&options).unwrap().unwrap();
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
        "idle pump: {used} ticks in 10 s, {:.1}% of a core",
        used as f64 / 10.0
    );
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
    };
    let owner = HandControls::restore(&options).unwrap().unwrap();
    assert!(owner.muted());
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.file()).unwrap()).unwrap();
    assert_eq!(saved["muted"], true);
    assert!(saved.get("later").is_none(), "{saved}");
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
    owner.shared.queued.fetch_add(1, Ordering::AcqRel);
    assert!(!owner.release_never_listened().unwrap(), "an input unread");
    owner.shared.queued.fetch_sub(1, Ordering::AcqRel);
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
