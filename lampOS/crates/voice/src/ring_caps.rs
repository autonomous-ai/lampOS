//! Optional output ceilings, separate from light requests and their priorities.
//! One accepted Actions snapshot is retained through bus silence; receipt of a
//! frame never refreshes it. These restrictions grant no output authority.
use lamp_bus::{Bus, Message};
use lamp_msgs::OutputCaps;
use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const ENVIRONMENT: &str = "LAMP_OUTPUT_CAPS";
pub const FLAG: &str = "--output-caps";
/// At most one bounded latest-value read per 20 ms, on an intake thread.
/// A tighter accepted limit blanks the ring in that same worker pass.
pub const LOOK_EVERY_US: u64 = 20_000;

pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.into()]
    } else {
        Vec::new()
    }
}

pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Bounded, deterministic intake state. It starts dark and can loosen only
/// through a valid full snapshot or a known wall deadline of an admitted cap.
pub struct Caps {
    clock_id: String,
    current: Option<OutputCaps>,
}

impl Caps {
    pub fn new(clock_id: impl Into<String>) -> Self {
        Self {
            clock_id: clock_id.into(),
            current: None,
        }
    }

    pub fn accept(&mut self, message: &Message, now_us: u64, now_unix_ms: u64) -> bool {
        let Ok(mut next) = OutputCaps::from_message(message, &self.clock_id, now_us, now_unix_ms)
        else {
            return false;
        };
        next.entries.sort_by(|one, two| one.source.cmp(&two.source));
        if let Some(current) = &self.current
            && (next.revision < current.revision
                || (next.revision == current.revision && next.entries != current.entries)
                || next.stamp.at_us < current.stamp.at_us
                || (next.session_id == current.session_id
                    && next.stamp.sequence <= current.stamp.sequence)
                || (next.session_id != current.session_id
                    && next.stamp.at_us <= current.stamp.at_us))
        {
            return false;
        }
        self.current = Some(next);
        true
    }

    pub fn ceiling(&self, product: u16, now_unix_ms: u64) -> u16 {
        ceiling(self.current.as_ref(), product, now_unix_ms)
    }
}

fn ceiling(current: Option<&OutputCaps>, product: u16, now_unix_ms: u64) -> u16 {
    let Some(current) = current else {
        return 0;
    };
    current
        .entries
        .iter()
        .filter(|entry| entry.active_at(now_unix_ms))
        .filter_map(|entry| entry.light_channel_max)
        .fold(product, u16::min)
}

#[derive(Default)]
struct Found {
    state: Mutex<FoundState>,
    stop: AtomicBool,
}

#[derive(Default)]
struct FoundState {
    latest: Option<OutputCaps>,
    clock: Option<(u64, u64)>,
}

/// The ordinary ring's optional cap intake. Failure to open the input remains
/// an enabled, dark consumer; it must never become the default-off path.
pub struct RingCaps {
    found: Arc<Found>,
    current: Option<OutputCaps>,
    clock: Option<(u64, u64)>,
}

impl RingCaps {
    pub fn on(root: &Path, clock_id: &str) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let found = Arc::new(Found::default());
        let reading = Arc::clone(&found);
        let mut caps = Caps::new(clock_id);
        thread::Builder::new()
            .name("voice-ring-caps".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    let message = bus.latest(OutputCaps::TOPIC).ok().flatten();
                    let wall = initialized_unix_ms();
                    let at_us = lamp_ipc::monotonic_us();
                    if let Some(message) = message {
                        caps.accept(&message, at_us, wall.unwrap_or(0));
                    }
                    if let Ok(mut slot) = reading.state.lock() {
                        slot.latest.clone_from(&caps.current);
                        slot.clock = wall.map(|wall| (at_us, wall));
                    }
                    thread::sleep(Duration::from_micros(LOOK_EVERY_US));
                }
            })?;
        Ok(Self {
            found,
            current: None,
            clock: None,
        })
    }

    pub fn dark() -> Self {
        Self {
            found: Arc::new(Found::default()),
            current: None,
            clock: None,
        }
    }

    /// No filesystem access or waiting on the ring's control/output loop.
    /// A delayed intake retains the last complete snapshot and its deadlines.
    pub fn tick(&mut self, now_us: u64) -> u16 {
        if let Ok(slot) = self.found.state.try_lock() {
            if slot
                .latest
                .as_ref()
                .map(|value| (&value.session_id, &value.stamp))
                != self
                    .current
                    .as_ref()
                    .map(|value| (&value.session_id, &value.stamp))
            {
                self.current.clone_from(&slot.latest);
            }
            self.clock = slot.clock;
        }
        let wall = self
            .clock
            .and_then(|(at_us, wall)| {
                let age = now_us.checked_sub(at_us)?;
                (age < lamp_msgs::OUTPUT_CAPS_FRESH_US).then_some(())?;
                wall.checked_add(age / 1_000)
            })
            .unwrap_or(0);
        ceiling(self.current.as_ref(), 120, wall)
    }
}

impl Drop for RingCaps {
    /// As the head intake, one reader stops on its next look. Stop and
    /// authority expiry never wait for a bus filesystem operation.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

fn initialized_unix_ms() -> Option<u64> {
    let wall = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let wall = u64::try_from(wall.as_millis()).ok()?;
    lamp_msgs::output_caps_clock_known(wall).then_some(wall)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::{Header, Kind};
    use lamp_msgs::{ClockStamp, OUTPUT_CAPS_FRESH_US, OutputCap};

    const WALL: u64 = 1_767_225_600_000;
    const HOST: &str = "host-boot";
    const SESSION: &str = "0123456789abcdef";

    fn snapshot(revision: u64, sequence: u64, at_us: u64, limit: Option<u16>) -> OutputCaps {
        OutputCaps {
            stamp: ClockStamp {
                clock_id: HOST.into(),
                at_us,
                sequence,
            },
            session_id: SESSION.into(),
            revision,
            entries: limit
                .map(|limit| OutputCap {
                    source: "daily-rest".into(),
                    token: "chance-1".into(),
                    issued_unix_ms: WALL,
                    valid_until_unix_ms: WALL + 60_000,
                    light_channel_max: Some(limit),
                    volume_raw_max: None,
                })
                .into_iter()
                .collect(),
        }
    }

    fn message(value: &OutputCaps) -> Message {
        Message {
            header: Header {
                topic: OutputCaps::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(OutputCaps::TYPE.into()),
                source: OutputCaps::SOURCE.into(),
                boot: value.session_id.clone(),
                seq: value.stamp.sequence,
                at_us: value.stamp.at_us,
                at_unix_ms: WALL,
                expires_ms: Some(3_000),
                clock: None,
            },
            payload: serde_json::to_vec(value).unwrap(),
            gap: 0,
            publisher_restarted: false,
            expired: false,
        }
    }

    #[test]
    fn startup_is_dark_then_all_caps_combine_with_the_product_minimum() {
        let mut caps = Caps::new(HOST);
        assert_eq!(caps.ceiling(40, WALL), 0);
        let mut value = snapshot(1, 1, 100, Some(30));
        let mut other = value.entries[0].clone();
        other.source = "quiet-hour".into();
        other.light_channel_max = Some(12);
        value.entries.push(other);
        assert!(caps.accept(&message(&value), 100, WALL));
        assert_eq!(caps.ceiling(40, WALL), 12);
        assert_eq!(caps.ceiling(8, WALL), 8);
        assert_eq!(caps.ceiling(0, WALL), 0);
    }

    /// Actions holding its caps back (a store from a later build) publishes
    /// an empty snapshot at revision 0 each second. A ring that starts with
    /// it takes the product's ceiling, not dark. A ring still holding a later
    /// revision keeps it until its caps expire by wall time (Jarvis and the
    /// reviewer on #2434).
    #[test]
    fn a_held_back_empty_snapshot_lights_a_starting_ring_and_leaves_a_running_one() {
        let mut starting = Caps::new(HOST);
        assert!(starting.accept(&message(&snapshot(0, 1, 100, None)), 100, WALL));
        assert_eq!(starting.ceiling(120, WALL), 120);
        let mut running = Caps::new(HOST);
        assert!(running.accept(&message(&snapshot(3, 1, 100, Some(12))), 100, WALL));
        assert!(!running.accept(&message(&snapshot(0, 2, 200, None)), 200, WALL));
        assert_eq!(running.ceiling(120, WALL), 12);
        assert_eq!(running.ceiling(120, WALL + 60_000), 120);
    }

    #[test]
    fn invalid_stale_future_and_mismatched_snapshots_never_loosen() {
        let mut caps = Caps::new(HOST);
        assert!(caps.accept(&message(&snapshot(2, 2, 100, Some(12))), 100, WALL));
        let release = message(&snapshot(3, 3, 200, None));
        for change in [
            |m: &mut Message| m.header.source = "other".into(),
            |m: &mut Message| m.header.boot = "fedcba9876543210".into(),
            |m: &mut Message| m.header.seq += 1,
            |m: &mut Message| m.header.at_us += 1,
            |m: &mut Message| m.header.kind = Kind::Event,
            |m: &mut Message| m.header.type_name = Some("lamp_msgs/Light".into()),
            |m: &mut Message| m.header.at_unix_ms += 10_000,
            |m: &mut Message| m.header.expires_ms = None,
            |m: &mut Message| m.expired = true,
        ] {
            let mut invalid = release.clone();
            change(&mut invalid);
            assert!(!caps.accept(&invalid, 200, WALL));
            assert_eq!(caps.ceiling(120, WALL), 12);
        }
        assert!(!caps.accept(&release, 199, WALL));
        assert!(!caps.accept(&release, 200 + OUTPUT_CAPS_FRESH_US, WALL));
        let mut foreign = snapshot(3, 3, 200, None);
        foreign.stamp.clock_id = "another-boot".into();
        assert!(!caps.accept(&message(&foreign), 200, WALL));
        assert!(!caps.accept(&message(&snapshot(1, 3, 200, None)), 200, WALL));
        assert!(!caps.accept(&message(&snapshot(2, 3, 200, None)), 200, WALL));
        assert!(!caps.accept(&message(&snapshot(3, 2, 200, None)), 200, WALL));
        assert!(!caps.accept(&message(&snapshot(3, 3, 99, None)), 200, WALL));
        assert_eq!(caps.ceiling(120, WALL), 12);
        assert!(caps.accept(&release, 200, WALL));
        assert_eq!(caps.ceiling(40, WALL), 40);
    }

    #[test]
    fn a_new_session_must_preserve_revision_and_cannot_replay_an_old_stamp() {
        let mut caps = Caps::new(HOST);
        let value = snapshot(7, 50, 100, Some(12));
        assert!(caps.accept(&message(&value), 100, WALL));
        let mut restart = value.clone();
        restart.session_id = "fedcba9876543210".into();
        restart.stamp.sequence = 1;
        restart.stamp.at_us = 101;
        restart.revision = 6;
        assert!(!caps.accept(&message(&restart), 101, WALL));
        restart.revision = 7;
        assert!(caps.accept(&message(&restart), 101, WALL));
        assert!(!caps.accept(&message(&value), 102, WALL));
        assert_eq!(caps.ceiling(120, WALL), 12);
    }

    #[test]
    fn silence_keeps_caps_until_wall_expiry_and_unknown_time_keeps_them() {
        let mut caps = Caps::new(HOST);
        assert!(caps.accept(&message(&snapshot(1, 1, 100, Some(12))), 100, WALL));
        assert_eq!(caps.ceiling(40, WALL + 30_000), 12);
        assert_eq!(caps.ceiling(40, 0), 12);
        assert_eq!(caps.ceiling(40, u64::MAX), 12);
        assert_eq!(caps.ceiling(40, WALL - 1), 12);
        assert_eq!(caps.ceiling(40, WALL + 60_000), 40);
        // A later explicit full release still works with an unset wall clock.
        assert!(caps.accept(&message(&snapshot(2, 2, 200, None)), 200, 0));
        assert_eq!(caps.ceiling(40, 0), 40);
    }

    #[test]
    fn the_switch_is_off_by_default_and_forwarded_as_one_explicit_flag() {
        for value in [None, Some("off"), Some("true"), Some("")] {
            assert!(arguments(value).is_empty());
        }
        assert_eq!(arguments(Some("on")), [FLAG]);
        let flags = [FLAG.into(), "--next".into()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    #[test]
    fn an_unknown_or_stale_clock_sample_cannot_expire_caps() {
        let mut reader = RingCaps::dark();
        {
            let mut slot = reader.found.state.lock().unwrap();
            slot.latest = Some(snapshot(1, 1, 100, Some(12)));
            slot.clock = Some((100, WALL + 60_000));
        }
        assert_eq!(reader.tick(100), 120);
        assert_eq!(reader.tick(100 + OUTPUT_CAPS_FRESH_US), 12);
        assert_eq!(reader.tick(99), 12);
        reader.found.state.lock().unwrap().clock = None;
        assert_eq!(reader.tick(100), 12);
    }

    #[test]
    fn real_bus_intake_keeps_restrictions_across_silence_and_accepts_a_new_session_release() {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let bus = Bus::new(&directory.path).unwrap();
        let mut reader = RingCaps::on(&directory.path, HOST).unwrap();
        assert_eq!(reader.tick(lamp_ipc::monotonic_us()), 0);
        let mut publisher = bus
            .publisher_declared(
                OutputCaps::TOPIC,
                &OutputCaps::declaration(),
                OutputCaps::SOURCE,
            )
            .unwrap();
        let wall = initialized_unix_ms().unwrap();
        let mut value = snapshot(3, 1, lamp_ipc::monotonic_us(), Some(12));
        value.session_id = publisher.boot().into();
        value.entries[0].issued_unix_ms = wall;
        value.entries[0].valid_until_unix_ms = wall + 60_000;
        publisher.publish_json(value.stamp.at_us, &value).unwrap();
        let wait = |reader: &mut RingCaps, wanted| {
            let until = std::time::Instant::now() + Duration::from_secs(1);
            loop {
                if reader.tick(lamp_ipc::monotonic_us()) == wanted {
                    break;
                }
                assert!(
                    std::time::Instant::now() < until,
                    "cap intake did not reach {wanted}"
                );
                thread::sleep(Duration::from_millis(2));
            }
        };
        wait(&mut reader, 12);
        drop(publisher);
        std::fs::remove_file(directory.path.join(OutputCaps::TOPIC).join("latest")).unwrap();
        thread::sleep(Duration::from_millis(2 * LOOK_EVERY_US / 1_000));
        assert_eq!(reader.tick(lamp_ipc::monotonic_us()), 12);
        let mut next = bus
            .publisher_declared(
                OutputCaps::TOPIC,
                &OutputCaps::declaration(),
                OutputCaps::SOURCE,
            )
            .unwrap();
        let mut release = snapshot(4, 1, lamp_ipc::monotonic_us(), None);
        release.session_id = next.boot().into();
        next.publish_json(release.stamp.at_us, &release).unwrap();
        wait(&mut reader, 120);
    }

    /// The feelings skill's Glad (#3691): warmth and a 6 s breath at the
    /// person's level, here the brightest (5), asked of the ring under a
    /// night cap of 24. The cap's ceiling holds every channel of every frame
    /// through two breaths, and the breath still shows.
    #[test]
    fn a_glad_breath_at_the_persons_brightest_draws_nothing_over_a_cap() {
        use crate::ring_face::{Face, Lit, Mood};
        use lamp_msgs::{LightPattern, PatternKind};
        let mut caps = Caps::new(HOST);
        assert!(caps.accept(&message(&snapshot(1, 1, 100, Some(24))), 100, WALL));
        let ceiling = u8::try_from(caps.ceiling(120, WALL)).unwrap();
        assert_eq!(ceiling, 24);
        let mut face = Face::new();
        face.light(Lit::of(true, "warm", 5).unwrap());
        face.pattern(Some((
            LightPattern {
                kind: PatternKind::Breathe,
                period_ms: 6_000,
                low: 4,
            },
            0,
        )));
        let mut brightest = 0;
        for frame in 0..600u64 {
            for pixel in face.render(Mood::Resting, 0, frame * 20_000, ceiling) {
                let most = pixel.red.max(pixel.green).max(pixel.blue);
                assert!(most <= ceiling, "frame {frame}: {pixel:?} over {ceiling}");
                brightest = brightest.max(most);
            }
        }
        assert!(brightest > 0, "the breath must still show");
    }
}
