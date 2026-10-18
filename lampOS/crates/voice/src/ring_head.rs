//! A hand kept on the lamp's head, for the ring's countdown (#711,
//! `docs/creature.md` "Touch"): from one second the eye dims slowly, and at
//! three the lamp goes to sleep ([`crate::ring_face::Face::hand`]). The hand's
//! time is Controls' own (`controls/touch_state`, the head's `held_us`, from
//! `--touch` Controls), read from the bus by the ring's own worker. It only
//! shapes the face; it never decides whether the ring may show anything.
//!
//! A tap on the head is not read here. Its listening face at rest follows
//! the voice's "hm?" ([`crate::ring_face::Face::hum_frame`]), so a tap the
//! voice does not answer gets no face (First on #705, 17 Oct 07:26).
//!
//! Switched on with `LAMP_HEAD_TOUCH=on`, which the voice passes to its ring
//! worker as `--head-touch`. Off unless set.
//!
//! The bus is read by a thread of its own, never by the ring's loop, so a
//! slow bus never stands between the loop and its Stop, Authority and
//! expiry (#849 review). The loop only reads what the thread last found.
use lamp_bus::{Bus, Message};
use lamp_msgs::{TOUCH_STATE_TTL_US, TouchSource, TouchState};
use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// The ring worker's flag for it.
pub const FLAG: &str = "--head-touch";

/// Controls says the head's state when it changes, a held hand's time at
/// most every 20 ms, and every 100 ms otherwise (#1469), inside the 300 ms
/// it lasts; this looks every 50.
const LOOK_EVERY_US: u64 = 50_000;

#[derive(Default)]
struct Found {
    /// The hand's time, and until when the state that said it holds, on
    /// the ring's monotonic clock: a stalled intake lets it expire.
    held: Mutex<Option<(u64, u64)>>,
    stop: AtomicBool,
}

pub struct RingHead {
    found: Arc<Found>,
}

impl RingHead {
    /// The head's state on the bus under `root`, read by a thread of its
    /// own from now on.
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let found = Arc::new(Found::default());
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-head".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    // A bus that cannot be read is no hand.
                    let held = bus
                        .latest(TouchState::TOPIC)
                        .ok()
                        .flatten()
                        .and_then(|message| held(&message));
                    if let Ok(mut slot) = reading.held.lock() {
                        *slot = held;
                    }
                    thread::sleep(Duration::from_micros(LOOK_EVERY_US));
                }
            })?;
        Ok(Self { found })
    }

    /// Call from the ring's loop with its clock: how long a hand has been
    /// on the head, as Controls counts it, or `None`. It reads no bus and
    /// never waits. A hand whose state has outlived Controls' 300 ms is no
    /// hand, whatever the head's thread last found (#849 review).
    pub fn tick(&mut self, now_us: u64) -> Option<u64> {
        let held = *self.found.held.lock().ok()?;
        held.filter(|(_, until)| now_us < *until)
            .map(|(held, _)| held)
    }
}

impl Drop for RingHead {
    /// The thread ends at its next look. It is not waited for: the ring's
    /// own end never waits on the bus.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

/// The head's hand time in a fresh, valid, enabled state from Controls, and
/// until when that state holds (its stamp and [`TOUCH_STATE_TTL_US`], on the
/// same monotonic clock as the ring's). None for a hand a push from the body
/// spoiled: Controls gives it no time.
pub fn held(message: &Message) -> Option<(u64, u64)> {
    if message.expired || message.header.source != "controls" {
        return None;
    }
    let state = message.parse::<TouchState>().ok()?;
    (state.check().is_ok() && state.enabled).then_some(())?;
    let held = state
        .sources
        .iter()
        .find(|source| source.source == TouchSource::Head)?
        .held_us?;
    Some((held, state.stamp.at_us.saturating_add(TOUCH_STATE_TTL_US)))
}

/// The ring worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    if crate::hand_controls::head_touch_from_environment() {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--head-touch` off the ring worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `read` gives once the head's thread has found it, within a
    /// second.
    fn soon<T: PartialEq + std::fmt::Debug>(wanted: T, mut read: impl FnMut() -> T) {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let found = read();
            if found == wanted {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{found:?}, not {wanted:?}"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn the_heads_hand_time_is_read_from_controls_state_and_nothing_else() {
        let root = std::path::PathBuf::from(format!("/tmp/lrh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let mut head = RingHead::on(&root).unwrap();
        assert_eq!(head.tick(lamp_ipc::monotonic_us()), None);
        let (state, _) =
            crate::hand_controls::test_head_touch("boot", lamp_ipc::monotonic_us() - 60_000);
        let held_us = state.sources[0].held_us;
        assert!(held_us.is_some());
        let mut said = bus
            .publisher_declared(TouchState::TOPIC, &TouchState::declaration(), "controls")
            .unwrap();
        said.publish_json(1, &state).unwrap();
        // Looked at every 50 ms by the head's thread: the hand's time.
        soon(held_us, || head.tick(lamp_ipc::monotonic_us()));
        // Controls' state holds 300 ms from its stamp: past that the hand is
        // gone, though the head's thread has nothing newer (#849 review).
        let stamped = state.stamp.at_us;
        assert_eq!(head.tick(stamped + TOUCH_STATE_TTL_US - 1), held_us);
        assert_eq!(head.tick(stamped + TOUCH_STATE_TTL_US), None);
        // Not from anyone but Controls.
        let mut other = bus
            .publisher_declared(TouchState::TOPIC, &TouchState::declaration(), "replay")
            .unwrap();
        other.publish_json(2, &state).unwrap();
        soon(None, || head.tick(lamp_ipc::monotonic_us()));
        drop((said, other));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_flag_comes_first_or_not_at_all() {
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        let none = ["x".to_owned()];
        assert_eq!(take(&none), (false, &none[..]));
    }
}
