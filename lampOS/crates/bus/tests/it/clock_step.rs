//! A wall clock that a time server sets forward or back while the lamp
//! runs (chrony steps lamp-06c1's by about +20 s some 46 s into every cold
//! boot). A header that says its `at_us` is this boot's CLOCK_MONOTONIC is
//! judged by that, so the step makes no silence and ages no value; one that
//! says nothing is judged by the wall clock, as before.
use lamp_bus::{Bus, Declaration, Kind, Notice};
use serde_json::json;
use std::{
    fs,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const BOOT: &str = "6a1c2b3d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
const EARLIER_BOOT: &str = "0f1e2d3c-4b5a-4968-8776-655443322110";
const STEP_MS: u64 = 20_000;
const WAIT: Duration = Duration::from_secs(5);

/// A root of this file's own, removed when the test ends.
struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static MADE: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "k{}_{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The host's clocks as a test plays them: both move on together, and the
/// wall clock alone is stepped.
#[derive(Clone)]
struct Clock {
    wall_ms: Arc<AtomicU64>,
    monotonic_us: Arc<AtomicU64>,
}

impl Clock {
    fn new() -> Self {
        Self {
            wall_ms: Arc::new(AtomicU64::new(1_791_878_400_000)),
            monotonic_us: Arc::new(AtomicU64::new(46_000_000)),
        }
    }

    fn bus(&self, root: &Root, boot: Option<&str>) -> Bus {
        let (wall, monotonic) = (Arc::clone(&self.wall_ms), Arc::clone(&self.monotonic_us));
        Bus::new(&root.0)
            .unwrap()
            .rescan_every(Duration::ZERO)
            .boot_id(boot)
            .clocks(
                move || wall.load(Ordering::SeqCst),
                move || monotonic.load(Ordering::SeqCst),
            )
    }

    fn pass_ms(&self, ms: u64) {
        self.wall_ms.fetch_add(ms, Ordering::SeqCst);
        self.monotonic_us.fetch_add(ms * 1_000, Ordering::SeqCst);
    }

    fn step_forward(&self) {
        self.wall_ms.fetch_add(STEP_MS, Ordering::SeqCst);
    }

    fn step_back(&self) {
        self.wall_ms.fetch_sub(STEP_MS, Ordering::SeqCst);
    }

    fn monotonic_us(&self) -> u64 {
        self.monotonic_us.load(Ordering::SeqCst)
    }
}

/// A topic sent every `period` whose values last `lifespan`, as Controls'
/// state is.
fn declared(period: Duration, lifespan: Duration) -> Declaration {
    Declaration::new("lamp_msgs/ControlsState", 1, Kind::Latest)
        .every(period)
        .lasting(lifespan)
}

#[test]
fn a_step_of_twenty_seconds_either_way_is_no_silence_and_ages_no_value_of_a_live_publisher() {
    let root = Root::new();
    let clock = Clock::new();
    let bus = clock.bus(&root, Some(BOOT));
    // A period long against the test's own run, so that no real silence
    // is ever two of them, and Controls' lifespan of 300 ms.
    let topic = "controls/state";
    let declaration = declared(Duration::from_secs(10), Duration::from_millis(300));
    let mut follow = bus.subscribe(topic, "voice").unwrap();
    let mut controls = bus
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    assert!(controls.stamp_monotonic_clock());
    let mut seq = 0;
    let mut publish = |clock: &Clock| {
        seq += 1;
        controls
            .publish_json(clock.monotonic_us(), &json!({"seq": seq}))
            .unwrap();
    };
    publish(&clock);
    assert!(follow.recv(WAIT).unwrap().is_some());
    for step in [Clock::step_forward as fn(&Clock), Clock::step_back] {
        clock.pass_ms(5_000);
        publish(&clock);
        // Sent before the step and read after it.
        step(&clock);
        let message = follow.recv(WAIT).unwrap().expect("not expired by the step");
        assert!(!message.expired);
        clock.pass_ms(5_000);
        // Sent after it: half a period after the one before, by `at_us`.
        publish(&clock);
        let message = follow.recv(WAIT).unwrap().expect("in time");
        assert!(!message.expired);
        assert_eq!(follow.notices(), vec![], "a step is no silence");
        assert!(!follow.latest().unwrap().unwrap().expired);
        assert!(!bus.latest(topic).unwrap().unwrap().expired);
    }
    assert_eq!(follow.expired(), 0);
}

#[test]
fn a_dead_publisher_across_a_step_is_late_after_two_periods_and_its_value_expires_on_time() {
    for forward in [true, false] {
        let root = Root::new();
        let clock = Clock::new();
        let bus = clock.bus(&root, Some(BOOT));
        let topic = "controls/state";
        let period = Duration::from_millis(60);
        let declaration = declared(period, Duration::from_millis(300));
        let mut follow = bus.subscribe(topic, "voice").unwrap();
        let mut controls = bus
            .publisher_declared(topic, &declaration, "controls")
            .unwrap();
        assert!(controls.stamp_monotonic_clock());
        let sent_us = clock.monotonic_us();
        controls
            .publish_json(sent_us, &json!({"muted": true}))
            .unwrap();
        let took = Instant::now();
        assert!(follow.recv(WAIT).unwrap().is_some());
        drop(controls);
        if forward {
            clock.step_forward();
        } else {
            clock.step_back();
        }
        // The silence is the subscriber's own clock's to measure.
        let mut said = Vec::new();
        while !said.contains(&Notice::Late) {
            assert!(took.elapsed() < WAIT, "never late");
            said.extend(follow.notices());
            thread::sleep(Duration::from_millis(2));
        }
        let late = took.elapsed();
        assert!(
            late > 2 * period && late < 2 * period + Duration::from_secs(1),
            "{late:?}"
        );
        // Its value lasts its lifespan and 50 ms by `at_us`, not a moment
        // more or less, whichever way the wall clock went.
        clock.pass_ms(350);
        assert!(
            !follow.latest().unwrap().unwrap().expired,
            "forward {forward}"
        );
        assert!(!bus.latest(topic).unwrap().unwrap().expired);
        clock.pass_ms(1);
        assert!(
            follow.latest().unwrap().unwrap().expired,
            "forward {forward}"
        );
        assert!(bus.latest(topic).unwrap().unwrap().expired);
    }
}

#[test]
fn privacy_ask_a_a_kept_value_expires_at_its_own_lifespan_after_a_step_back_then_forward() {
    let root = Root::new();
    let clock = Clock::new();
    let bus = clock.bus(&root, Some(BOOT));
    let topic = "controls/state";
    let declaration = declared(Duration::from_millis(100), Duration::from_millis(300));
    let mut controls = bus
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    assert!(controls.stamp_monotonic_clock());
    let mut follow = bus.subscribe(topic, "voice").unwrap();
    for step in [Clock::step_back as fn(&Clock), Clock::step_forward] {
        controls
            .publish_json(clock.monotonic_us(), &json!({"muted": false}))
            .unwrap();
        assert!(follow.recv(WAIT).unwrap().is_some());
        step(&clock);
        // 300 ms and the 50 ms grace by `at_us`: then, and only then.
        for passed in [0, 300, 50] {
            clock.pass_ms(passed);
            assert!(!follow.latest().unwrap().unwrap().expired);
            assert!(!bus.latest(topic).unwrap().unwrap().expired);
        }
        clock.pass_ms(1);
        assert!(follow.latest().unwrap().unwrap().expired, "fails open");
        assert!(bus.latest(topic).unwrap().unwrap().expired, "fails open");
    }
}

#[test]
fn privacy_ask_b_a_this_boot_value_already_past_its_lifespan_is_expired_on_arrival_and_in_the_file()
{
    let root = Root::new();
    let clock = Clock::new();
    let bus = clock.bus(&root, Some(BOOT));
    let topic = "controls/state";
    let declaration = declared(Duration::from_millis(100), Duration::from_millis(300));
    let mut follow = bus.subscribe(topic, "voice").unwrap();
    // A value sent with a stamp already 351 ms old: the wall clock says it
    // was sent this moment, and it is not handed over.
    let mut controls = bus
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    assert!(controls.stamp_monotonic_clock());
    controls
        .publish_json(clock.monotonic_us() - 351_000, &json!({"muted": false}))
        .unwrap();
    assert!(follow.recv(Duration::from_millis(30)).unwrap().is_none());
    assert_eq!(follow.expired(), 1);
    assert!(follow.latest().unwrap().unwrap().expired);
    // The file a publisher kept before it was restarted in this same boot,
    // with the wall clock set back since: it does not come back live to a
    // subscriber that comes after.
    controls
        .publish_json(clock.monotonic_us(), &json!({"muted": false}))
        .unwrap();
    drop(controls);
    clock.pass_ms(351);
    clock.step_back();
    let mut after = bus.subscribe(topic, "after").unwrap();
    assert!(after.recv(Duration::from_millis(30)).unwrap().is_none());
    assert_eq!(after.expired(), 1);
    assert!(after.latest().unwrap().unwrap().expired);
    assert!(bus.latest(topic).unwrap().unwrap().expired);
}

#[test]
fn a_value_kept_in_an_earlier_boot_or_stamped_ahead_is_expired() {
    let root = Root::new();
    let clock = Clock::new();
    let topic = "controls/state";
    let declaration = declared(Duration::from_millis(100), Duration::from_secs(60));
    // Kept by a publisher of an earlier boot, with a life far from over by
    // any clock's numbers.
    let earlier = clock.bus(&root, Some(EARLIER_BOOT));
    let mut controls = earlier
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    assert!(controls.stamp_monotonic_clock());
    controls
        .publish_json(clock.monotonic_us(), &json!({"muted": false}))
        .unwrap();
    drop(controls);
    let bus = clock.bus(&root, Some(BOOT));
    let mut follow = bus.subscribe(topic, "voice").unwrap();
    assert!(follow.recv(Duration::from_millis(30)).unwrap().is_none());
    assert_eq!(follow.expired(), 1);
    assert!(follow.latest().unwrap().unwrap().expired);
    assert!(bus.latest(topic).unwrap().unwrap().expired);

    // By hand: one of this boot stamped 51 ms ahead of its clock, and one of
    // another boot, are passed over; one of this boot 50 ms ahead is not.
    let path = fs::read_dir(root.0.join(topic).join("subs"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|end| end == "sock"))
        .unwrap();
    let stranger = UnixDatagram::unbound().unwrap();
    let now_us = clock.monotonic_us();
    let wall = clock.wall_ms.load(Ordering::SeqCst);
    let send = |seq: u64, at_us: u64, boot: &str| {
        let datagram = format!(
            "{{\"topic\":\"{topic}\",\"kind\":\"latest\",\"enc\":\"json\",\"source\":\"hand\",\
             \"boot\":\"0123456789abcdef\",\"seq\":{seq},\"at_us\":{at_us},\
             \"at_unix_ms\":{wall},\"expires_ms\":60000,\"clock\":\"{boot}\"}}\n{seq}"
        );
        stranger.send_to(datagram.as_bytes(), &path).unwrap();
    };
    send(1, now_us + 51_000, BOOT);
    send(2, now_us, EARLIER_BOOT);
    send(3, now_us + 50_000, BOOT);
    let message = follow.recv(WAIT).unwrap().expect("50 ms ahead is in time");
    assert_eq!((message.header.seq, message.gap), (3, 0));
    assert_eq!(follow.expired(), 3);
}

#[test]
fn a_header_without_a_clock_is_judged_by_the_wall_clock_as_before() {
    let root = Root::new();
    let clock = Clock::new();
    let bus = clock.bus(&root, Some(BOOT));
    let topic = "controls/state";
    let declaration = declared(Duration::from_secs(5), Duration::from_secs(1));
    let mut follow = bus.subscribe(topic, "voice").unwrap();
    // A publisher that does not say its `at_us` is the monotonic clock.
    let mut controls = bus
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    controls
        .publish_json(clock.monotonic_us(), &json!({"seq": 1}))
        .unwrap();
    let message = follow.recv(WAIT).unwrap().unwrap();
    assert_eq!(message.header.clock, None);
    controls
        .publish_json(clock.monotonic_us(), &json!({"seq": 2}))
        .unwrap();
    // Read after a step forward: older than its second by the wall clock.
    clock.step_forward();
    assert!(follow.recv(Duration::from_millis(30)).unwrap().is_none());
    assert_eq!(follow.expired(), 1);
    // And the next, twenty seconds after the last one handed over by the
    // wall clock, is a silence of more than two periods.
    controls
        .publish_json(clock.monotonic_us(), &json!({"seq": 3}))
        .unwrap();
    assert!(follow.recv(WAIT).unwrap().is_some());
    assert_eq!(follow.notices(), vec![Notice::Late, Notice::OnTime]);
    // A step back keeps a value the wall clock's 20 s longer.
    clock.step_back();
    clock.step_back();
    clock.pass_ms(1_051);
    assert!(!follow.latest().unwrap().unwrap().expired);
}

#[test]
fn a_bus_that_knows_no_boot_stamps_no_clock_and_judges_a_stamped_header_by_the_wall() {
    let root = Root::new();
    let clock = Clock::new();
    let topic = "controls/state";
    let declaration = declared(Duration::from_secs(10), Duration::from_secs(1));
    let unknown = clock.bus(&root, None);
    let mut controls = unknown
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    assert!(!controls.stamp_monotonic_clock());
    controls
        .publish_json(clock.monotonic_us(), &json!({"seq": 1}))
        .unwrap();
    assert_eq!(unknown.latest(topic).unwrap().unwrap().header.clock, None);
    // Stamped by a bus that knows its boot, read by one that knows none:
    // the wall clock's rule, which has it still in time.
    let known = clock.bus(&root, Some(BOOT));
    let mut controls = known
        .publisher_declared(topic, &declaration, "controls")
        .unwrap();
    assert!(controls.stamp_monotonic_clock());
    controls.publish_json(0, &json!({"seq": 1})).unwrap();
    let kept = unknown.latest(topic).unwrap().unwrap();
    assert_eq!(kept.header.clock.as_deref(), Some(BOOT));
    assert!(!kept.expired);
    assert!(known.latest(topic).unwrap().unwrap().expired);
}
