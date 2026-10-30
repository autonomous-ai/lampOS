use super::*;
use lamp_bus::{Header, Kind};
use lamp_msgs::ClockStamp;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const CLOCK: &str = "kernel-boot";
const SESSION: &str = "0123456789abcdef";
const CONTROLLER: &str = "0123456789abcdef0123456789abcdef";
const START: u64 = 1_000_000;

fn lease(sequence: u64, now: u64, permitted: bool) -> MicrophonePrivacy {
    MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us: now,
            sequence,
        },
        session_id: SESSION.into(),
        controller_boot: CONTROLLER.into(),
        microphone_generation: 1,
        observed_us: now,
        expires_at_us: now + FRESH_US,
        permitted,
        physical_muted: Some(!permitted),
        no_slide: false,
    }
}

fn envelope(lease: MicrophonePrivacy) -> Message {
    Message {
        header: Header {
            topic: MicrophonePrivacy::TOPIC.into(),
            kind: Kind::Latest,
            enc: lamp_bus::JSON.into(),
            type_name: Some(MicrophonePrivacy::TYPE.into()),
            source: "voice".into(),
            boot: lease.session_id.clone(),
            seq: lease.stamp.sequence,
            at_us: lease.stamp.at_us,
            at_unix_ms: 10_000,
            expires_ms: Some(50),
            clock: None,
        },
        payload: serde_json::to_vec(&lease).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Revoke,
    Open,
    Read,
    Discard,
    Drop,
}

struct Fake {
    clock: Rc<Cell<u64>>,
    events: Rc<RefCell<Vec<(Event, u64)>>>,
    read_us: u64,
    count: usize,
    fail_read: bool,
    fail_discard: bool,
}

impl Capture for Fake {
    fn read(&mut self, into: &mut [i16]) -> io::Result<usize> {
        self.events
            .borrow_mut()
            .push((Event::Read, self.clock.get()));
        self.clock.set(self.clock.get() + self.read_us);
        if self.fail_read {
            return Err(io::Error::other("read failed"));
        }
        let count = into.len().min(self.count);
        into[..count].fill(100);
        Ok(count)
    }
    fn discard(&mut self) -> io::Result<()> {
        self.events
            .borrow_mut()
            .push((Event::Discard, self.clock.get()));
        if self.fail_discard {
            Err(io::Error::other("discard failed"))
        } else {
            Ok(())
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.events
            .borrow_mut()
            .push((Event::Drop, self.clock.get()));
    }
}

struct Fixture {
    clock: Rc<Cell<u64>>,
    events: Rc<RefCell<Vec<(Event, u64)>>>,
    owner: CaptureOwner<Fake>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            clock: Rc::new(Cell::new(START)),
            events: Rc::new(RefCell::new(Vec::new())),
            owner: CaptureOwner::new(CLOCK.into(), Settings::default()),
        }
    }
    fn observe(&mut self, lease: MicrophonePrivacy) -> io::Result<()> {
        self.owner
            .observe(&envelope(lease), self.clock.get(), 10_000)
    }
    fn tick(
        &mut self,
        count: usize,
        open_us: u64,
        read_us: u64,
        fail_read: bool,
        fail_discard: bool,
    ) -> io::Result<Option<Emission>> {
        let clock = self.clock.clone();
        let events = self.events.clone();
        self.owner.tick(
            || clock.get(),
            || {
                events.borrow_mut().push((Event::Open, clock.get()));
                clock.set(clock.get() + open_us);
                Ok(Fake {
                    clock: clock.clone(),
                    events: events.clone(),
                    count,
                    read_us,
                    fail_read,
                    fail_discard,
                })
            },
            false,
        )
    }
    fn kinds(&self) -> Vec<Event> {
        self.events
            .borrow()
            .iter()
            .map(|(event, _)| *event)
            .collect()
    }
}

/// A lamp with no slide: Voice's lease bound to the Controls publication
/// that said so, with no physical level.
fn no_slide(sequence: u64, now: u64, observed_us: u64, permitted: bool) -> MicrophonePrivacy {
    MicrophonePrivacy {
        observed_us,
        expires_at_us: observed_us + FRESH_US,
        physical_muted: None,
        no_slide: true,
        permitted,
        ..lease(sequence, now, permitted)
    }
}

#[test]
fn only_exact_on_selects_the_new_boundary() {
    assert!(!enabled(None));
    assert!(!enabled(Some("off")));
    assert!(enabled(Some("on")));
    for value in ["", "ON", "true", " on", "NO-SLIDE", "no_slide", " no-slide"] {
        assert!(!enabled(Some(value)));
    }
}

/// `no-slide` selects the same boundary: Ears captures only under Voice's
/// lease, with its lineage, epoch and fence, whatever grants it.
#[test]
fn no_slide_selects_the_same_boundary() {
    assert!(enabled(Some("no-slide")));
}

/// A no-slide lease opens MIC1 and ends 50 ms after its Controls
/// publication, as a slide's ends 50 ms after its reading.
#[test]
fn a_no_slide_lease_opens_capture_and_expires_at_its_observation() {
    let mut fixture = Fixture::new();
    let lease = no_slide(1, START, START - 10_000, true);
    lease.check().unwrap();
    fixture.observe(lease).unwrap();
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_some());
    assert!(fixture.owner.is_open());
    fixture.clock.set(START + 39_999);
    assert!(
        fixture
            .owner
            .maintain(fixture.clock.get())
            .unwrap()
            .is_some()
    );
    fixture.clock.set(START + 40_000);
    assert!(
        fixture
            .owner
            .maintain(fixture.clock.get())
            .unwrap()
            .is_none()
    );
    assert!(!fixture.owner.is_open());
    assert_eq!(
        fixture.kinds(),
        [Event::Open, Event::Read, Event::Discard, Event::Drop]
    );
    // Denied by Voice (the software mute): nothing opens.
    let mut denied = Fixture::new();
    denied.observe(no_slide(1, START, START, false)).unwrap();
    assert!(denied.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    assert!(denied.kinds().is_empty());
}

/// A later no-slide lease cannot carry an older Controls publication than
/// the last: its observation is fenced as a switch reading is.
#[test]
fn a_no_slide_observation_regression_is_refused() {
    let mut fixture = Fixture::new();
    fixture.observe(no_slide(1, START, START, true)).unwrap();
    fixture.tick(BLOCK / 2, 0, 0, false, false).unwrap();
    assert!(fixture.owner.is_open());
    fixture.clock.set(START + 10_000);
    let regressed = no_slide(2, START + 10_000, START - 1, true);
    regressed.check().unwrap();
    assert!(fixture.observe(regressed).is_err());
    assert!(!fixture.owner.is_open());
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    // The same publication renewed is no regression.
    let mut renewed = Fixture::new();
    renewed.observe(no_slide(1, START, START, true)).unwrap();
    renewed.clock.set(START + 10_000);
    renewed
        .observe(no_slide(2, START + 10_000, START, true))
        .unwrap();
    assert!(renewed.tick(BLOCK, 0, 0, false, false).unwrap().is_some());
}

#[test]
fn missing_and_denied_never_open_or_retry_a_capture() {
    let mut fixture = Fixture::new();
    for _ in 0..20 {
        assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    }
    fixture.observe(lease(1, START, false)).unwrap();
    for _ in 0..20 {
        assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    }
    assert!(fixture.kinds().is_empty());
}

#[test]
fn a_worker_that_reported_muted_cannot_permit_again() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK / 2, 0, 0, false, false).unwrap();
    fixture.clock.set(START + 10_000);
    fixture.observe(lease(2, START + 10_000, false)).unwrap();
    fixture.clock.set(START + 20_000);
    assert!(fixture.observe(lease(3, START + 20_000, true)).is_err());
    assert!(!fixture.owner.is_open());
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    // A later contiguous heartbeat from the same worker is still refused.
    fixture.clock.set(START + 30_000);
    assert!(fixture.observe(lease(4, START + 30_000, true)).is_err());
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    // A new worker may grant again.
    fixture.clock.set(START + 40_000);
    let mut next = lease(1, START + 40_000, true);
    next.session_id = "fedcba9876543210".into();
    fixture.observe(next).unwrap();
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_some());
}

#[test]
fn mute_discards_and_drops_before_another_read_or_reopen() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK / 2, 0, 0, false, false).unwrap();
    fixture.clock.set(START + 10_000);
    fixture.observe(lease(2, START + 10_000, false)).unwrap();
    assert!(!fixture.owner.is_open());
    assert_eq!(
        fixture.kinds(),
        [Event::Open, Event::Read, Event::Discard, Event::Drop]
    );
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    fixture.clock.set(START + 20_000);
    // Voice ends a worker that saw the switch closed; a new one reopens.
    let mut reopened = lease(1, START + 20_000, true);
    reopened.session_id = "fedcba9876543210".into();
    reopened.microphone_generation = 3;
    fixture.observe(reopened).unwrap();
    let emission = fixture.tick(BLOCK, 0, 0, false, false).unwrap().unwrap();
    assert!(
        emission.heard.level.is_none(),
        "partial old block and listener window were discarded"
    );
    assert_eq!(fixture.kinds()[4..], [Event::Open, Event::Read]);
}

#[test]
fn lease_expiry_closes_without_waiting_for_sound() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(0, 0, 0, false, false).unwrap();
    fixture.clock.set(START + FRESH_US);
    assert!(
        fixture
            .owner
            .maintain(fixture.clock.get())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture.kinds(),
        [Event::Open, Event::Read, Event::Discard, Event::Drop]
    );
    assert_eq!(fixture.events.borrow().last().unwrap().1 - START, FRESH_US);
}

#[test]
fn slow_open_and_read_never_hand_over_expired_sound() {
    for (open_us, read_us, expected) in [
        (FRESH_US, 0, vec![Event::Open, Event::Discard, Event::Drop]),
        (
            0,
            FRESH_US,
            vec![Event::Open, Event::Read, Event::Discard, Event::Drop],
        ),
    ] {
        let mut fixture = Fixture::new();
        fixture.observe(lease(1, START, true)).unwrap();
        assert!(
            fixture
                .tick(BLOCK, open_us, read_us, false, false)
                .unwrap()
                .is_none()
        );
        assert!(!fixture.owner.is_open());
        assert_eq!(fixture.kinds(), expected);
    }
}

#[test]
fn read_and_discard_failures_still_drop_the_actual_handle() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    assert!(fixture.tick(BLOCK, 0, 0, true, false).is_err());
    assert!(!fixture.owner.is_open());
    assert_eq!(
        fixture.kinds(),
        [Event::Open, Event::Read, Event::Discard, Event::Drop]
    );
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 0, false, true).unwrap();
    assert!(fixture.owner.invalidate().is_err());
    assert!(!fixture.owner.is_open());
    assert_eq!(
        fixture.kinds(),
        [Event::Open, Event::Read, Event::Discard, Event::Drop]
    );
}

#[test]
fn malformed_foreign_future_and_stale_input_close_an_open_handle() {
    let mutations: &[fn(&mut Message)] = &[
        |message| message.header.source = "controls".into(),
        |message| message.header.kind = Kind::Event,
        |message| message.header.type_name = None,
        |message| message.header.expires_ms = Some(300),
        |message| message.header.boot = "fedcba9876543210".into(),
        |message| message.header.seq += 1,
        |message| message.header.at_us += 1,
        |message| message.expired = true,
        |message| message.payload = b"{}".to_vec(),
    ];
    for mutate in mutations {
        let mut fixture = Fixture::new();
        fixture.observe(lease(1, START, true)).unwrap();
        fixture.tick(BLOCK, 0, 0, false, false).unwrap();
        let mut message = envelope(lease(2, START, true));
        mutate(&mut message);
        assert!(fixture.owner.observe(&message, START, 10_000).is_err());
        assert!(!fixture.owner.is_open());
    }
    for (mut value, now) in [
        (lease(2, START + 1, true), START),
        (lease(2, START, true), START + FRESH_US),
    ] {
        let mut fixture = Fixture::new();
        fixture.observe(lease(1, START, true)).unwrap();
        fixture.tick(BLOCK, 0, 0, false, false).unwrap();
        assert!(
            fixture
                .owner
                .observe(&envelope(value.clone()), now, 10_000)
                .is_err()
        );
        value.stamp.clock_id = "another-kernel".into();
        assert!(
            fixture
                .owner
                .observe(&envelope(value), now, 10_000)
                .is_err()
        );
        assert!(!fixture.owner.is_open());
    }
}

#[test]
fn gaps_duplicates_and_clock_regression_cannot_keep_capture_open() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    assert!(fixture.observe(lease(1, START, true)).is_err());
    assert!(!fixture.owner.is_open());
    fixture.observe(lease(2, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    assert!(fixture.observe(lease(4, START, true)).is_err());
    assert!(!fixture.owner.is_open());
    fixture.observe(lease(5, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    assert!(fixture.owner.maintain(START - 1).is_err());
    assert!(!fixture.owner.is_open());
    assert!(fixture.observe(lease(6, START, true)).is_err());
}

#[test]
fn every_capture_lineage_change_closes_first_and_recent_retired_publishers_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    let mut value = lease(2, START, true);
    value.microphone_generation = 2;
    fixture.observe(value.clone()).unwrap();
    assert!(!fixture.owner.is_open());
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    value.stamp.sequence = 3;
    value.controller_boot = "abcdef0123456789abcdef0123456789".into();
    value.microphone_generation = 1;
    fixture.observe(value.clone()).unwrap();
    assert!(!fixture.owner.is_open());
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    value.session_id = "fedcba9876543210".into();
    value.stamp.sequence = 1;
    fixture.observe(value).unwrap();
    assert!(!fixture.owner.is_open());
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    assert!(fixture.observe(lease(4, START, true)).is_err());
    assert!(!fixture.owner.is_open());
    assert_eq!(
        fixture
            .kinds()
            .iter()
            .filter(|event| **event == Event::Open)
            .count(),
        4
    );
    assert_eq!(
        fixture
            .kinds()
            .iter()
            .filter(|event| **event == Event::Drop)
            .count(),
        4
    );
}

#[test]
fn listener_history_and_queued_metadata_do_not_cross_a_mute() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    let mut emission = None;
    for _ in 0..100 {
        emission = fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    }
    assert!(fixture.owner.floor_db().is_some());
    let permit = emission.unwrap().permit;
    let publications = PublicationGate::default();
    publications.update(Some(permit), 0);
    assert!(publications.allows(permit, 0, START));
    let generation = publications.fence();
    assert!(!publications.allows(permit, 0, START));
    fixture.observe(lease(2, START, false)).unwrap();
    assert!(fixture.owner.floor_db().is_none());
    let mut renewed = lease(1, START, true);
    renewed.session_id = "fedcba9876543210".into();
    fixture.observe(renewed).unwrap();
    let next = fixture
        .tick(BLOCK, 0, 0, false, false)
        .unwrap()
        .unwrap()
        .permit;
    publications.update(Some(next), generation);
    assert!(!publications.allows(permit, generation, START));
    assert!(publications.allows(next, generation, START));
    assert!(!publications.allows(next, generation, next.until_us));
}

#[test]
fn old_physical_acquisition_is_not_refreshed_by_a_new_publication() {
    let mut fixture = Fixture::new();
    let mut value = lease(1, START, true);
    value.observed_us = START - 40_000;
    value.expires_at_us = START + 10_000;
    fixture.observe(value).unwrap();
    fixture.tick(0, 0, 0, false, false).unwrap();
    assert!(fixture.owner.maintain(START + 10_000).unwrap().is_none());
    assert!(!fixture.owner.is_open());
}

#[test]
fn an_invalid_new_session_does_not_erase_the_sequence_high_water() {
    let mut gate = LeaseGate::new(CLOCK.into());
    gate.observe(&envelope(lease(10, START, true)), START, 10_000)
        .unwrap();
    let mut bad = lease(1, START - 1, true);
    bad.session_id = "fedcba9876543210".into();
    assert!(gate.observe(&envelope(bad), START, 10_000).is_err());
    assert!(
        gate.observe(&envelope(lease(10, START, true)), START, 10_000)
            .is_err()
    );
    assert!(gate.permit(START).unwrap().is_none());
    gate.observe(&envelope(lease(11, START, true)), START, 10_000)
        .unwrap();
    assert!(gate.permit(START).unwrap().is_some());
}

#[test]
fn retired_session_capacity_closes_capture_and_recovers_after_the_horizon() {
    let mut fixture = Fixture::new();
    for session in 0..=MAX_RETIRED {
        let mut value = lease(1, START, true);
        value.session_id = format!("{session:016x}");
        fixture.observe(value).unwrap();
        fixture.tick(BLOCK, 0, 0, false, false).unwrap();
        assert!(fixture.owner.gate.retired.len() <= MAX_RETIRED);
    }
    for now in [START, START + FRESH_US - 1] {
        fixture.clock.set(now);
        let mut value = lease(1, now, true);
        value.session_id = format!("{:016x}", MAX_RETIRED + 1);
        assert!(fixture.observe(value).is_err());
        assert_eq!(fixture.owner.gate.retired.len(), MAX_RETIRED);
        assert!(!fixture.owner.gate.exhausted);
        assert!(fixture.owner.maintain(now).unwrap().is_none());
        assert!(!fixture.owner.is_open());
        assert_eq!(fixture.kinds().last(), Some(&Event::Drop));
    }
    let now = START + FRESH_US;
    fixture.clock.set(now);
    let mut value = lease(1, now, true);
    value.session_id = format!("{:016x}", MAX_RETIRED + 1);
    fixture.observe(value).unwrap();
    assert_eq!(fixture.owner.gate.retired.len(), 1);
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    assert!(fixture.owner.is_open());
}

#[test]
fn ordinary_service_publisher_cycles_do_not_exhaust_session_history() {
    let mut fixture = Fixture::new();
    for session in 0..128u64 {
        let now = START + session * (FRESH_US + 1);
        fixture.clock.set(now);
        let mut value = lease(1, now, true);
        value.session_id = format!("{session:016x}");
        fixture.observe(value).unwrap();
        assert!(!fixture.owner.is_open());
        if session > 0 {
            let events = fixture.events.borrow();
            assert_eq!(
                &events[events.len() - 2..],
                &[(Event::Discard, now), (Event::Drop, now)]
            );
        }
        assert!(fixture.owner.gate.retired.len() <= 1);
        assert!(
            fixture
                .tick(BLOCK / 2, 0, 0, false, false)
                .unwrap()
                .is_none()
        );
        assert!(fixture.owner.is_open());
    }
    assert!(!fixture.owner.gate.exhausted);
    assert_eq!(
        fixture
            .kinds()
            .iter()
            .filter(|event| **event == Event::Open)
            .count(),
        128
    );
}

#[test]
fn recent_retired_replay_and_late_old_replay_both_stay_closed() {
    let mut fixture = Fixture::new();
    let old = lease(1, START, true);
    fixture.observe(old.clone()).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    let mut current = lease(1, START, true);
    current.session_id = "fedcba9876543210".into();
    fixture.observe(current).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    assert_eq!(
        fixture.owner.gate.retired.get(SESSION),
        Some(&(START + FRESH_US))
    );

    fixture.clock.set(START + FRESH_US - 1);
    assert!(fixture.observe(old.clone()).is_err());
    assert!(fixture.owner.gate.retired.contains_key(SESSION));
    assert!(!fixture.owner.is_open());

    fixture.clock.set(START + FRESH_US);
    fixture.owner.maintain(fixture.clock.get()).unwrap();
    assert!(!fixture.owner.gate.retired.contains_key(SESSION));
    // Forgetting the identity does not refresh the packet's original times.
    assert!(fixture.observe(old).is_err());
    assert!(fixture.tick(BLOCK, 0, 0, false, false).unwrap().is_none());
    assert!(!fixture.owner.is_open());
}

#[test]
fn pruning_retired_ids_keeps_the_global_publication_high_water() {
    let mut gate = LeaseGate::new(CLOCK.into());
    gate.observe(&envelope(lease(1, START, true)), START, 10_000)
        .unwrap();
    let mut current = lease(1, START + 10_000, true);
    current.session_id = "fedcba9876543210".into();
    gate.observe(&envelope(current.clone()), START + 10_000, 10_000)
        .unwrap();
    current.stamp.sequence = 2;
    current.stamp.at_us = START + FRESH_US;
    current.observed_us = current.stamp.at_us;
    current.expires_at_us = current.stamp.at_us + FRESH_US;
    gate.observe(&envelope(current), START + FRESH_US, 10_000)
        .unwrap();
    let now = START + 10_000 + FRESH_US;
    gate.permit(now).unwrap();
    assert!(!gate.retired.contains_key(SESSION));
    assert_eq!(gate.at_us, START + FRESH_US);
    let delayed = lease(2, START + FRESH_US - 1, true);
    assert!(gate.observe(&envelope(delayed), now, 10_000).is_err());
    assert!(gate.permit(now).unwrap().is_none());
}

#[test]
fn counter_and_retirement_clock_exhaustion_remain_terminal() {
    let mut gate = LeaseGate::new(CLOCK.into());
    gate.epoch = u64::MAX;
    gate.close();
    assert!(gate.permit(START + FRESH_US).is_err());
    assert!(
        gate.observe(
            &envelope(lease(1, START + FRESH_US, true)),
            START + FRESH_US,
            10_000
        )
        .is_err()
    );

    let mut gate = LeaseGate::new(CLOCK.into());
    gate.observe(&envelope(lease(1, START, true)), START, 10_000)
        .unwrap();
    let mut value = lease(1, START, true);
    value.session_id = "fedcba9876543210".into();
    value.stamp.at_us = u64::MAX - 1;
    value.observed_us = value.stamp.at_us;
    value.expires_at_us = u64::MAX;
    assert!(
        gate.observe(&envelope(value), u64::MAX - 1, 10_000)
            .is_err()
    );
    assert!(gate.permit(u64::MAX).is_err());
}

#[test]
fn explicit_restart_notice_closes_even_if_the_body_keeps_the_same_lineage() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 0, false, false).unwrap();
    let mut message = envelope(lease(2, START, true));
    message.publisher_restarted = true;
    fixture.owner.observe(&message, START, 10_000).unwrap();
    assert!(!fixture.owner.is_open());
    assert_eq!(fixture.kinds().last(), Some(&Event::Drop));
}

#[test]
fn a_receiver_fence_during_open_or_read_closes_before_handing_on_sound() {
    for during_open in [true, false] {
        let mut fixture = Fixture::new();
        fixture.observe(lease(1, START, true)).unwrap();
        let clock = fixture.clock.clone();
        let events = fixture.events.clone();
        let result = fixture
            .owner
            .tick_while(
                || clock.get(),
                || {
                    events.borrow_mut().push((Event::Open, clock.get()));
                    if during_open {
                        clock.set(START + 1);
                    }
                    Ok(Fake {
                        clock: clock.clone(),
                        events: events.clone(),
                        count: BLOCK,
                        read_us: 1,
                        fail_read: false,
                        fail_discard: false,
                    })
                },
                false,
                || clock.get() == START,
            )
            .unwrap();
        assert!(result.is_none());
        assert!(!fixture.owner.is_open());
        let kinds = fixture.kinds();
        assert_eq!(kinds.last(), Some(&Event::Drop));
        assert_eq!(kinds.contains(&Event::Read), !during_open);
    }
}

#[test]
fn pcm_export_requires_a_complete_current_read_and_keeps_its_source_time() {
    for (allowed, count, read_us, fence_stays) in [
        (false, BLOCK, 1, true),
        (true, BLOCK / 2, 1, true),
        (true, BLOCK, FRESH_US, true),
        (true, BLOCK, 1, false),
        (true, BLOCK, 1, true),
    ] {
        let mut fixture = Fixture::new();
        fixture.observe(lease(1, START, allowed)).unwrap();
        let clock = fixture.clock.clone();
        let events = fixture.events.clone();
        let mut handed = Vec::new();
        fixture
            .owner
            .tick_export_while(
                || clock.get(),
                || {
                    Ok(Fake {
                        clock: clock.clone(),
                        events: events.clone(),
                        read_us,
                        count,
                        fail_read: false,
                        fail_discard: false,
                    })
                },
                false,
                || fence_stays || clock.get() == START,
                |samples, permit, sequence, at_us| {
                    assert_eq!(samples, &[100; BLOCK]);
                    handed.push((permit, sequence, at_us));
                },
            )
            .unwrap();
        let valid = allowed && count == BLOCK && read_us < FRESH_US && fence_stays;
        assert_eq!(handed.len(), usize::from(valid));
        if valid {
            assert_eq!(handed[0].1, 0);
            assert_eq!(handed[0].2, START + read_us);
            assert!(handed[0].0.until_us <= START + FRESH_US);
            assert_eq!(fixture.owner.samples, [0; BLOCK]);
        }
    }
}

#[test]
fn pcm_partial_read_is_destroyed_by_mute_and_never_replayed_after_reopen() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    assert!(
        fixture
            .tick(BLOCK / 2, 0, 1, false, false)
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.owner.filled, BLOCK / 2);
    fixture.observe(lease(2, START + 1, false)).unwrap();
    assert_eq!(fixture.owner.samples, [0; BLOCK]);
    assert_eq!(fixture.owner.filled, 0);
    let clock = fixture.clock.clone();
    let mut copies = 0;
    fixture
        .owner
        .tick_export_while(
            || clock.get(),
            || panic!("muted"),
            false,
            || true,
            |_, _, _, _| copies += 1,
        )
        .unwrap();
    assert_eq!(copies, 0);
    // A new controller session reopens from an empty partial block.
    fixture.clock.set(START + 2);
    let mut reopened = lease(1, START + 2, true);
    reopened.session_id = "fedcba9876543210".into();
    fixture.observe(reopened).unwrap();
    assert!(
        fixture
            .tick(BLOCK / 2, 0, 1, false, false)
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.owner.filled, BLOCK / 2);
    let mut stamps = Vec::new();
    fixture
        .owner
        .tick_export_while(
            || clock.get(),
            || panic!("already open"),
            false,
            || true,
            |_, permit, sequence, at| stamps.push((permit, sequence, at)),
        )
        .unwrap();
    assert_eq!(stamps.len(), 1);
    assert_eq!(stamps[0].1, 1);
    assert!(stamps[0].2 >= START + 2);
}

#[test]
fn publication_is_revoked_before_driver_discard_even_on_internal_read_failure() {
    for fail_read in [false, true] {
        let mut fixture = Fixture::new();
        fixture.observe(lease(1, START, true)).unwrap();
        let publication = std::sync::Arc::new(PublicationGate::default());
        let permit = fixture.owner.maintain(START).unwrap().unwrap();
        publication.update(Some(permit), 0);
        let gate = publication.clone();
        let events = fixture.events.clone();
        let clock = fixture.clock.clone();
        fixture.owner.before_close(move || {
            gate.update(None, 0);
            assert!(!gate.allows(permit, 0, clock.get()));
            events.borrow_mut().push((Event::Revoke, clock.get()));
        });
        let result = fixture.tick(BLOCK, 0, 1, fail_read, false);
        if fail_read {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_some());
            assert!(publication.allows(permit, 0, fixture.clock.get()));
            fixture.owner.invalidate().unwrap();
        }
        assert!(!publication.allows(permit, 0, fixture.clock.get()));
        let kinds = fixture.kinds();
        let discard = kinds
            .iter()
            .position(|event| *event == Event::Discard)
            .unwrap();
        assert_eq!(kinds[discard - 1], Event::Revoke);
    }
}

#[test]
fn closing_between_complete_blocks_leaves_one_sequence_gap_even_for_a_short_mute() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK, 0, 1, false, false).unwrap().unwrap();
    // The first complete block used sequence zero; there is no partial PCM.
    assert_eq!(fixture.owner.sequence, 1);
    assert_eq!(fixture.owner.filled, 0);
    fixture.observe(lease(2, START + 1, false)).unwrap();
    assert_eq!(fixture.owner.sequence, 2);
    for _ in 0..100 {
        fixture.owner.invalidate().unwrap();
        assert!(fixture.owner.maintain(START + 1).unwrap().is_none());
    }
    assert_eq!(fixture.owner.sequence, 2);
    fixture.clock.set(START + 2);
    let mut next = lease(1, START + 2, true);
    next.session_id = "fedcba9876543210".into();
    fixture.observe(next).unwrap();
    let clock = fixture.clock.clone();
    let events = fixture.events.clone();
    let mut emitted = Vec::new();
    fixture
        .owner
        .tick_export_while(
            || clock.get(),
            || {
                Ok(Fake {
                    clock: clock.clone(),
                    events: events.clone(),
                    count: BLOCK,
                    read_us: 1,
                    fail_read: false,
                    fail_discard: false,
                })
            },
            false,
            || true,
            |_, _, sequence, _| emitted.push(sequence),
        )
        .unwrap()
        .unwrap();
    assert_eq!(emitted, [2]);
}

#[test]
fn read_failure_reserves_a_gap_and_sequence_exhaustion_still_drops_the_handle() {
    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    assert!(fixture.tick(BLOCK, 0, 1, true, false).is_err());
    assert_eq!(fixture.owner.sequence, 1);
    assert!(!fixture.owner.is_open());
    fixture.owner.invalidate().unwrap();
    assert_eq!(fixture.owner.sequence, 1);

    let mut fixture = Fixture::new();
    fixture.observe(lease(1, START, true)).unwrap();
    fixture.tick(BLOCK / 2, 0, 1, false, true).unwrap();
    fixture.owner.sequence = u64::MAX;
    assert!(fixture.owner.invalidate().is_err());
    assert!(!fixture.owner.is_open());
    assert_eq!(fixture.owner.filled, 0);
    assert_eq!(fixture.owner.samples, [0; BLOCK]);
    assert_eq!(fixture.owner.sequence, u64::MAX);
    assert_eq!(fixture.kinds().last(), Some(&Event::Drop));
    assert!(fixture.owner.maintain(START + 1).is_err());
    let clock = fixture.clock.clone();
    assert!(
        fixture
            .owner
            .tick(
                || clock.get(),
                || panic!("exhaustion must not reopen"),
                false
            )
            .is_err()
    );
}
