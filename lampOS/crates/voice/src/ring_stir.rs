//! A stir at its name in the night's sleep, on the ring (#1672, the first
//! builder on #4285, 30 Oct 03:5x: "the ring lifts a little from its night
//! level and the head rises a few degrees, within the first half second").
//! Expression stirs the head and names it `stirring` in `expression/state`
//! (`lamp_expression` `alive.rs`); at night the voice's ring worker owns the
//! ring, so the ring's half is here: while Expression names `stirring` alone
//! the resting face lifts by up to [`crate::ring_face::STIR_LIFT`] of
//! itself, in over [`IN_US`]; once it names `stir_settling` beside it (no
//! answer in its 5 s), or no longer names it, the face eases back over
//! [`OUT_US`], with the head. Its answer wakes the lamp as always: the
//! speaking face takes no lift ([`crate::ring_face::Face::stir`]).
//!
//! Behind `LAMP_REST_THROUGH_SOUNDS=on` in the voice's unit, the switch
//! Expression and skills read; off, nothing is read and nothing changes.
//! It only shapes the face; it never decides whether the ring may show
//! anything, so a closed privacy switch or a mute is as dark or as asleep
//! as ever. As with the head's hand ([`crate::ring_head`]), the bus is read
//! by a thread of its own, never by the ring's loop.
use lamp_bus::{Bus, Message};
use lamp_msgs::ExpressionState;
use std::{
    f32::consts::PI,
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// The switch, shared with `lamp-expression` and `lamp-skills`.
pub const SWITCH: &str = "LAMP_REST_THROUGH_SOUNDS";
/// What Expression names while the head stirs, and beside it while the stir
/// settles back (`lamp_expression::STIR_SETTLING`).
pub const STIRRING: &str = "stirring";
pub const SETTLING: &str = "stir_settling";
/// The lift comes in over this once seen, eased. Expression says its state
/// every 200 ms and this looks every [`LOOK_EVERY_US`], so it begins within
/// about a quarter of a second of the stir and is whole by about half a
/// second, with the head's 0.4 s (`lamp_expression::STIR_IN_US`).
pub const IN_US: u64 = 250_000;
/// It eases back over the head's 2 s (`lamp_expression::STIR_OUT_US`).
pub const OUT_US: u64 = 2_000_000;
/// How often the thread looks.
const LOOK_EVERY_US: u64 = 40_000;
/// A state lasts this long after its stamp (`ExpressionState`'s 500 ms):
/// Expression gone quiet lets the lift go.
pub const LASTS_US: u64 = 500_000;

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether Expression's state says the stir is held (its answer awaited),
/// and until when that state holds: its stamp and [`LASTS_US`], on
/// Expression's monotonic clock, the ring's. Not from Expression, expired or not valid: none.
pub fn held(message: &Message) -> Option<(bool, u64)> {
    if message.expired || message.header.source != "expression" {
        return None;
    }
    let state = message.parse::<ExpressionState>().ok()?;
    state.check().ok()?;
    let named = |name: &str| state.behaviours.iter().any(|said| said == name);
    Some((
        named(STIRRING) && !named(SETTLING),
        state.stamp.at_us.saturating_add(LASTS_US),
    ))
}

/// How far the lift has come, eased both ways.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lift {
    /// Linear, 0 to 1.
    through: f32,
    last_us: Option<u64>,
}

impl Lift {
    /// Move on to `now_us` toward held (`true`) or back: the share of the
    /// lift to show now, 0 to 1, eased in and out.
    pub fn step(&mut self, held: bool, now_us: u64) -> f32 {
        let step = self
            .last_us
            .map_or(0, |last| now_us.saturating_sub(last).min(OUT_US)) as f32;
        self.last_us = Some(now_us);
        self.through = if held {
            (self.through + step / IN_US as f32).min(1.0)
        } else {
            (self.through - step / OUT_US as f32).max(0.0)
        };
        0.5 - 0.5 * (PI * self.through).cos()
    }
}

#[derive(Default)]
struct Found {
    /// The stir held, and until when the state that said it holds.
    held: Mutex<Option<(bool, u64)>>,
    stop: AtomicBool,
}

pub struct RingStir {
    found: Arc<Found>,
    lift: Lift,
}

impl RingStir {
    /// With the switch's `value` on, Expression's state on the bus under
    /// `root`, read by a thread of its own from now on; off, none.
    pub fn switched(root: &Path, value: Option<&str>) -> io::Result<Option<Self>> {
        if enabled(value) {
            Self::on(root).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let found = Arc::new(Found::default());
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-stir".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    // A bus that cannot be read is no stir.
                    let held = bus
                        .latest(ExpressionState::TOPIC)
                        .ok()
                        .flatten()
                        .and_then(|message| held(&message));
                    if let Ok(mut slot) = reading.held.lock() {
                        *slot = held;
                    }
                    thread::sleep(Duration::from_micros(LOOK_EVERY_US));
                }
            })?;
        Ok(Self {
            found,
            lift: Lift::default(),
        })
    }

    /// Call from the ring's loop with its clock: how far the face is lifted
    /// now, 0 to 1. It reads no bus and never waits. A state past its
    /// 500 ms is no stir, whatever the thread last found.
    pub fn tick(&mut self, now_us: u64) -> f32 {
        let held = self
            .found
            .held
            .lock()
            .ok()
            .and_then(|held| *held)
            .is_some_and(|(held, until)| held && now_us < until);
        self.lift.step(held, now_us)
    }
}

impl Drop for RingStir {
    /// The thread ends at its next look. It is not waited for: the ring's
    /// own end never waits on the bus.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: u64 = 25_000;

    /// The lift begins at the first frame it is held, is whole by [`IN_US`],
    /// eases back over [`OUT_US`] once let go, and no frame jumps.
    #[test]
    fn the_lift_eases_in_quickly_and_back_slowly() {
        let mut lift = Lift::default();
        let start = 1_000_000_000;
        assert_eq!(lift.step(false, start), 0.0);
        let shares: Vec<(u64, f32)> = (1..=200)
            .map(|frame| {
                let at = start + frame * FRAME;
                (at, lift.step(at < start + 3_000_000, at))
            })
            .collect();
        let share = |at: u64| shares.iter().find(|(when, _)| *when == at).unwrap().1;
        assert!(share(start + FRAME) > 0.0);
        assert_eq!(share(start + IN_US), 1.0);
        assert_eq!(share(start + 3_000_000 - FRAME), 1.0);
        let half_out = share(start + 3_000_000 + OUT_US / 2);
        assert!((0.4..0.6).contains(&half_out), "{half_out}");
        assert_eq!(share(start + 3_000_000 + OUT_US), 0.0);
        for pair in shares.windows(2) {
            assert!((pair[1].1 - pair[0].1).abs() < 0.16, "{pair:?}");
        }
    }

    fn state(behaviours: &[&str], sequence: u64) -> ExpressionState {
        ExpressionState {
            stamp: lamp_msgs::ClockStamp {
                clock_id: "boot".into(),
                at_us: lamp_ipc::monotonic_us(),
                sequence,
            },
            enabled: true,
            turn: None,
            request: None,
            behaviours: behaviours.iter().map(|name| (*name).to_owned()).collect(),
            missing: Vec::new(),
            warnings: Vec::new(),
            body: false,
            light: false,
            asleep: Some(true),
        }
    }

    /// Ticks at the ring's resting pace until `done` says so, within five
    /// seconds: how long it took, from `since`.
    fn until(
        stir: &mut RingStir,
        since: std::time::Instant,
        mut done: impl FnMut(f32) -> bool,
    ) -> Duration {
        loop {
            if done(stir.tick(lamp_ipc::monotonic_us())) {
                return since.elapsed();
            }
            assert!(since.elapsed() < Duration::from_secs(5), "never");
            thread::sleep(Duration::from_micros(FRAME));
        }
    }

    /// #1672 on a real bus: Expression's `stirring` begins the lift within
    /// half a second of being said, `stir_settling` beside it lets it back
    /// down, and the same names from anyone but Expression lift nothing.
    /// With the switch off nothing is read at all.
    #[test]
    fn expressions_stirring_lifts_the_ring_within_half_a_second_and_its_settling_lets_it_back() {
        let root = std::path::PathBuf::from(format!("/tmp/lrs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        assert!(RingStir::switched(&root, None).unwrap().is_none());
        assert!(RingStir::switched(&root, Some("off")).unwrap().is_none());
        let mut stir = RingStir::switched(&root, Some("on")).unwrap().unwrap();
        let mut said = bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "expression",
            )
            .unwrap();
        // Asleep and still: no lift, once the reader has found it, so its
        // thread's start is not counted in the stir's latency.
        said.publish_json(lamp_ipc::monotonic_us(), &state(&["asleep"], 1))
            .unwrap();
        let reading = std::time::Instant::now();
        while stir.found.held.lock().unwrap().is_none() {
            assert!(reading.elapsed() < Duration::from_secs(20), "never read");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(stir.tick(lamp_ipc::monotonic_us()), 0.0);
        let stirred = std::time::Instant::now();
        said.publish_json(lamp_ipc::monotonic_us(), &state(&["asleep", STIRRING], 2))
            .unwrap();
        let began = until(&mut stir, stirred, |share| share > 0.0);
        assert!(began < Duration::from_millis(500), "{began:?}");
        // Held while said again, as Expression says it every 200 ms.
        for sequence in 3..8 {
            said.publish_json(
                lamp_ipc::monotonic_us(),
                &state(&["asleep", STIRRING], sequence),
            )
            .unwrap();
            thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(stir.tick(lamp_ipc::monotonic_us()), 1.0);
        // No answer: settling, said again every 200 ms as Expression says
        // it while the head eases down, it eases back, not at once, and
        // well before the 5 s `until` allows; held on `stirring` alone it
        // would stay up.
        let settled = std::time::Instant::now();
        let mut sequence = 8;
        let mut said_at = settled - Duration::from_secs(1);
        let mut shares = Vec::new();
        let back = until(&mut stir, settled, |share| {
            if said_at.elapsed() >= Duration::from_millis(200) {
                said.publish_json(
                    lamp_ipc::monotonic_us(),
                    &state(&["asleep", STIRRING, SETTLING], sequence),
                )
                .unwrap();
                sequence += 1;
                said_at = std::time::Instant::now();
            }
            shares.push(share);
            share == 0.0
        });
        assert!(back > Duration::from_millis(1_500), "{back:?}");
        assert!(shares.windows(2).all(|pair| pair[1] <= pair[0] + 1e-6));
        // Not Expression's: nothing.
        let mut other = bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "replay",
            )
            .unwrap();
        other
            .publish_json(lamp_ipc::monotonic_us(), &state(&["asleep", STIRRING], 1))
            .unwrap();
        for _ in 0..20 {
            assert_eq!(stir.tick(lamp_ipc::monotonic_us()), 0.0);
            thread::sleep(Duration::from_millis(20));
        }
        drop((said, other));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_on_turns_it_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!enabled(value), "{value:?}");
        }
    }
}
