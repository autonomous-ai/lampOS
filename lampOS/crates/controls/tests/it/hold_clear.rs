use lamp_controls::{
    mute::MuteRecord,
    receiver::{Effects, Receiver},
};
use lamp_msgs::{
    ClockStamp, ControlFault, ControlGesture, ControlIntent, ControlSource, ControlsState, Gesture,
    HoldTier,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const START: u64 = 3_000_000;
const STEP: u64 = 100_000;
const HOLD: u64 = 2_000_000;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "controls-hold-clear-{}-{}",
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

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn record(muted: bool) -> MuteRecord {
    let mut record = MuteRecord::new("boot").unwrap();
    if muted {
        record.request_mute(1_000_000);
    }
    record
}

fn state() -> ControlsState {
    ControlsState {
        privacy: None,
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: START,
            sequence: 0,
        },
        session_id: "first".into(),
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
        event_sequence: 0,
        rejected_inputs: 0,
        no_privacy_slide: false,
    }
}

struct Rig {
    receiver: Receiver,
    sample: ControlsState,
    now: u64,
}

impl Rig {
    fn new(muted: bool, hold_clear: bool) -> Self {
        let receiver = if hold_clear {
            Receiver::with_hold_clear(record(muted), START).unwrap()
        } else {
            Receiver::new(record(muted)).unwrap()
        };
        let mut rig = Self {
            receiver,
            sample: state(),
            now: START,
        };
        rig.tick(START);
        rig
    }

    fn tick(&mut self, at_us: u64) -> Effects {
        assert!(at_us >= self.now);
        self.now = at_us;
        self.sample.stamp.sequence += 1;
        self.sample.stamp.at_us = at_us;
        self.receiver.observe(at_us, &self.sample).unwrap()
    }

    fn through(&mut self, at_us: u64) {
        while self.now < at_us {
            self.tick((self.now + STEP).min(at_us));
        }
    }

    fn publish_hold(&mut self, pressed_us: u64, released_us: u64, published_us: u64) -> Effects {
        self.sample.event_sequence += 1;
        self.sample.latest_mute = Some(ControlGesture {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: published_us,
                sequence: self.sample.event_sequence,
            },
            session_id: self.sample.session_id.clone(),
            source: ControlSource::Button,
            name: "primary".into(),
            gesture: Gesture::LongPress,
            intent: ControlIntent::SoftwareMute,
            duration_us: released_us - pressed_us,
            released_us,
            enabled: true,
        });
        self.tick(published_us)
    }

    fn hold(&mut self) -> Effects {
        let pressed = self.now + STEP;
        self.through(pressed + HOLD - STEP);
        self.publish_hold(pressed, pressed + HOLD, pressed + HOLD)
    }

    fn stop(&mut self) -> Effects {
        let at_us = self.now + STEP;
        self.sample.event_sequence += 1;
        self.sample.latest_stop = Some(ControlGesture {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us,
                sequence: self.sample.event_sequence,
            },
            session_id: self.sample.session_id.clone(),
            source: ControlSource::Button,
            name: "primary".into(),
            gesture: Gesture::Tap,
            intent: ControlIntent::Stop,
            duration_us: 1_000,
            released_us: at_us,
            enabled: true,
        });
        self.tick(at_us)
    }

    fn change_session(&mut self) {
        self.sample.session_id = "replacement".into();
        self.sample.stamp.sequence = 0;
        self.sample.event_sequence = 0;
        self.sample.latest_mute = None;
        self.sample.latest_stop = None;
        self.tick(self.now + STEP);
    }
}

#[test]
fn legacy_receiver_and_independent_stop_effect_remain_one_way() {
    let mut rig = Rig::new(false, false);
    for _ in 0..3 {
        let effect = rig.hold();
        assert!(effect.stop && effect.mute_changed && effect.software_muted);
        assert!(!effect.stop_requested && !effect.clear_requested);
        assert!(!rig.receiver.clear_pending());
    }
    rig.receiver.fence_hold_clear(rig.now).unwrap();
    assert!(!rig.hold().clear_requested);
    let effect = rig.stop();
    assert!(effect.stop && effect.stop_requested && effect.software_muted);
    assert!(!effect.mute_changed && !effect.clear_requested);
}

#[test]
fn only_second_fresh_hold_proposes_clear_and_persistence_precedes_live_change() {
    let root = Root::new();
    let mut rig = Rig::new(false, true);
    let first = rig.hold();
    assert!(first.mute_changed && first.software_muted && first.stop);
    assert!(!first.clear_requested && !first.stop_requested);
    rig.receiver.mute_record().save(&root.file()).unwrap();
    let second = rig.hold();
    assert!(second.clear_requested && second.mute_changed && second.software_muted);
    assert!(!second.stop_requested);
    let prepared = rig.receiver.prepare_clear(rig.now).unwrap();
    assert!(!prepared.record().muted);
    assert!(rig.receiver.mute_record().muted);
    assert!(rig.receiver.prepare_clear(rig.now).is_err());
    prepared.record().save(&root.file()).unwrap();
    rig.receiver.commit_clear(prepared).unwrap();
    assert!(!rig.receiver.mute_record().muted);
    assert!(!rig.tick(rig.now + STEP).software_muted);
    assert_eq!(
        MuteRecord::load(&root.file(), "boot", rig.now).unwrap(),
        *rig.receiver.mute_record()
    );
    // The same retained release cannot undo a committed clear after restart.
    let mut restarted = Receiver::with_hold_clear(
        MuteRecord::load(&root.file(), "boot", rig.now).unwrap(),
        rig.now,
    )
    .unwrap();
    let effect = restarted.observe(rig.now, &rig.sample).unwrap();
    assert!(!effect.mute_changed && !effect.software_muted && !effect.clear_requested);
}

#[test]
fn a_fresh_retained_hold_in_the_first_state_cannot_clear_a_restored_mute() {
    let mut rig = Rig::new(true, true);
    let release = START + HOLD + STEP;
    rig.through(release - STEP);
    let effect = rig.publish_hold(START + STEP, release, release);
    assert!(effect.clear_requested);
    let restored = rig.receiver.mute_record().clone();
    let mut restarted = Receiver::with_hold_clear(restored, release).unwrap();
    let effect = restarted.observe(release, &rig.sample).unwrap();
    assert!(effect.software_muted);
    assert!(!effect.clear_requested);
    assert!(restarted.prepare_clear(release).is_err());
}

#[test]
fn a_hold_started_before_receiver_restart_cannot_clear() {
    let mut rig = Rig::new(true, true);
    let pressed = START - STEP;
    let release = pressed + HOLD;
    rig.through(release - STEP);
    let effect = rig.publish_hold(pressed, release, release);
    assert!(effect.mute_changed && effect.software_muted);
    assert!(!effect.clear_requested);
    assert!(rig.hold().clear_requested);
}

#[test]
fn original_release_must_be_fresh_even_when_publication_is_fresh() {
    let mut rig = Rig::new(true, true);
    let pressed = START + STEP;
    let release = pressed + HOLD;
    let publish = release + 500_000;
    rig.through(publish - STEP);
    let effect = rig.publish_hold(pressed, release, publish);
    assert!(effect.mute_changed && effect.software_muted);
    assert!(!effect.clear_requested);
    assert!(rig.receiver.prepare_clear(publish).is_err());
}

#[test]
fn duplicate_identity_or_reflected_old_release_cannot_clear() {
    let mut rig = Rig::new(true, true);
    assert!(rig.hold().clear_requested);
    rig.receiver.fence_hold_clear(rig.now).unwrap();
    let original = rig.sample.latest_mute.clone().unwrap();
    assert!(!rig.tick(rig.now + STEP).clear_requested);
    // Freshening publication alone must not freshen the original local act.
    rig.sample.latest_mute.as_mut().unwrap().stamp.at_us = rig.now + STEP;
    assert!(!rig.tick(rig.now + STEP).clear_requested);
    // Reuse of the same ID with a later release remains ineligible for clear.
    rig.through(rig.now + HOLD + STEP);
    let retimed = rig.sample.latest_mute.as_mut().unwrap();
    retimed.released_us = rig.now;
    retimed.stamp.at_us = rig.now;
    assert!(!rig.tick(rig.now).clear_requested);
    // Repackaging the original hold in a new publisher cannot clear either.
    rig.change_session();
    let mut reflected = original;
    reflected.session_id = rig.sample.session_id.clone();
    reflected.stamp.sequence = 1;
    reflected.stamp.at_us = rig.now;
    rig.sample.latest_mute = Some(reflected);
    rig.sample.event_sequence = 1;
    assert!(!rig.tick(rig.now).clear_requested);
    assert!(rig.receiver.mute_record().muted);
}

#[test]
fn publisher_restart_fault_or_observation_gap_fences_a_hold_in_progress() {
    for interruption in 0..3 {
        let mut rig = Rig::new(true, true);
        let pressed = START + STEP;
        let release = pressed + HOLD;
        rig.through(pressed + 500_000);
        match interruption {
            0 => rig.change_session(),
            1 => {
                rig.sample.ready = false;
                rig.sample.fault = Some(ControlFault::Missing);
                rig.tick(rig.now + STEP);
                rig.sample.ready = true;
                rig.sample.fault = None;
                rig.tick(rig.now + STEP);
            }
            _ => {
                rig.tick(rig.now + 300_000);
            }
        }
        rig.through(release - STEP);
        let effect = rig.publish_hold(pressed, release, release);
        assert!(effect.software_muted);
        assert!(!effect.clear_requested, "interruption {interruption}");
        assert!(rig.hold().clear_requested);
    }
}

#[test]
fn fresh_hold_must_start_strictly_after_owner_fence_and_mute_watermark() {
    let mut rig = Rig::new(true, true);
    rig.receiver.fence_hold_clear(rig.now).unwrap();
    let pressed = rig.now;
    rig.through(pressed + HOLD - STEP);
    assert!(
        !rig.publish_hold(pressed, pressed + HOLD, pressed + HOLD)
            .clear_requested
    );
    let watermark = rig.receiver.mute_record().through_us;
    rig.through(watermark + HOLD - STEP);
    assert!(
        !rig.publish_hold(watermark, watermark + HOLD, watermark + HOLD)
            .clear_requested
    );
    assert!(rig.hold().clear_requested);
}

#[test]
fn expired_or_failed_persistence_never_releases_the_live_latch() {
    let mut rig = Rig::new(true, true);
    assert!(rig.hold().clear_requested);
    let prepared = rig.receiver.prepare_clear(rig.now).unwrap();
    assert!(prepared.record().save(Path::new("relative.json")).is_err());
    drop(prepared);
    assert!(rig.receiver.mute_record().muted);
    assert!(!rig.receiver.clear_pending());
    assert!(rig.receiver.prepare_clear(rig.now).is_err());
    assert!(rig.hold().clear_requested);
    rig.now += 500_000;
    assert!(rig.receiver.prepare_clear(rig.now).is_err());
    assert!(rig.receiver.mute_record().muted);
    assert!(!rig.receiver.clear_pending());
}

#[test]
fn newer_owner_fence_stop_mute_fault_or_session_invalidates_prepared_clear() {
    for change in 0..6 {
        let mut rig = Rig::new(true, true);
        assert!(rig.hold().clear_requested);
        let prepared = rig.receiver.prepare_clear(rig.now).unwrap();
        match change {
            0 => rig.receiver.fence_hold_clear(rig.now).unwrap(),
            1 => {
                assert!(rig.stop().stop_requested);
            }
            2 => {
                assert!(rig.hold().clear_requested);
            }
            3 => {
                rig.sample.ready = false;
                rig.sample.fault = Some(ControlFault::Missing);
                rig.tick(rig.now + STEP);
            }
            4 => rig.change_session(),
            _ => {
                assert!(rig.receiver.observe(rig.now - 1, &rig.sample).is_err());
            }
        }
        assert!(
            rig.receiver.commit_clear(prepared).is_err(),
            "change {change}"
        );
        assert!(rig.receiver.mute_record().muted);
    }
}

#[test]
fn rejected_state_cancels_pending_clear_without_changing_mute() {
    for malformed in 0..4 {
        let mut rig = Rig::new(true, true);
        assert!(rig.hold().clear_requested);
        let mut rejected = rig.sample.clone();
        match malformed {
            0 => rejected.enabled = false,
            1 => rejected.stamp.clock_id = "other-boot".into(),
            2 => rejected.stamp.at_us = rig.now + 1,
            _ => rejected.stamp.sequence = 0,
        }
        assert!(rig.receiver.observe(rig.now, &rejected).is_err());
        assert!(!rig.receiver.clear_pending());
        assert!(rig.receiver.mute_record().muted);
    }
}

#[test]
fn later_stop_cancels_pending_clear_but_older_retained_stop_does_not() {
    let mut rig = Rig::new(true, true);
    assert!(rig.stop().stop_requested);
    assert!(rig.hold().clear_requested);
    assert!(rig.receiver.clear_pending());
    assert!(rig.stop().stop_requested);
    assert!(!rig.receiver.clear_pending());
    assert!(rig.receiver.prepare_clear(rig.now).is_err());
    assert!(rig.receiver.mute_record().muted);
}

#[test]
fn a_coalesced_fresh_stop_does_not_hide_a_later_clear_hold() {
    let mut rig = Rig::new(true, true);
    let pressed = rig.now + STEP;
    let release = pressed + HOLD;
    rig.through(release - STEP);
    rig.sample.event_sequence = 1;
    rig.sample.latest_stop = Some(ControlGesture {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: release - STEP,
            sequence: 1,
        },
        session_id: rig.sample.session_id.clone(),
        source: ControlSource::Button,
        name: "secondary".into(),
        gesture: Gesture::Tap,
        intent: ControlIntent::Stop,
        duration_us: 1_000,
        released_us: release - STEP,
        enabled: true,
    });
    let effect = rig.publish_hold(pressed, release, release);
    assert!(effect.stop_requested && effect.clear_requested && effect.software_muted);
    assert!(rig.receiver.clear_pending());
}

#[test]
fn a_prepared_clear_belongs_only_to_its_original_receiver() {
    let mut first = Rig::new(true, true);
    let mut second = Rig::new(true, true);
    assert!(first.hold().clear_requested);
    assert!(second.hold().clear_requested);
    assert_eq!(first.receiver.mute_record(), second.receiver.mute_record());
    let prepared = first.receiver.prepare_clear(first.now).unwrap();
    assert!(second.receiver.commit_clear(prepared).is_err());
    assert!(second.receiver.mute_record().muted);
}

#[test]
fn receiver_refuses_a_start_before_its_saved_watermark() {
    let mut record = record(true);
    record.request_mute(START + 1);
    assert!(Receiver::with_hold_clear(record, START).is_err());
}
