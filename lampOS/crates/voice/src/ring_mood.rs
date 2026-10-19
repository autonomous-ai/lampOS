//! The lamp's one breath, from its inner state, for the eye at rest (#1670,
//! `docs/creature.md` "Its inner state": "mood shows only as pace and
//! warmth"). Mood (`lamp-mood`, `mood/state`) keeps the breath, its phase and
//! its pace: a little quicker with energy, slower as the lamp dozes, the
//! slowest asleep, and the plain 5.2 s when calm and awake. The ring's resting
//! breath and Expression's head (`alive.rs`) both take their phase from it
//! ([`lamp_msgs::MoodState::breath_in`]), so the lamp breathes as one; with
//! no fresh mood both breathe the plain breath on the boot clock, as before.
//! Listening, thinking, speaking and muted are fixed and never take it.
//!
//! It only shapes the face; it never decides whether the ring may show
//! anything. There is no switch of its own: Mood is off unless
//! `LAMP_MOOD=on`, and without it nothing here changes a frame. As with the
//! head's hand ([`crate::ring_head`]), the bus is read by a thread of its
//! own, never by the ring's loop.
use lamp_bus::{Bus, Message};
use lamp_msgs::{MOOD_STATE_TTL_US, MoodState};
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

/// Mood says its state every second and on each meaningful change, and a
/// state lasts 2.5 s; this looks four times a second.
const LOOK_EVERY_US: u64 = 250_000;

/// How long after its stamp a state still says the breath.
pub const LASTS_US: u64 = MOOD_STATE_TTL_US;

#[derive(Default)]
struct Found {
    /// The last state Mood said, whole: the ring moves its breath on to its
    /// own now.
    state: Mutex<Option<Message>>,
    stop: AtomicBool,
}

pub struct RingMood {
    found: Arc<Found>,
}

impl RingMood {
    /// Mood's state on the bus under `root`, read by a thread of its own
    /// from now on.
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let found = Arc::new(Found::default());
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-mood".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    // A bus that cannot be read is no mood.
                    let state = bus.latest(MoodState::TOPIC).ok().flatten();
                    if let Ok(mut slot) = reading.state.lock() {
                        *slot = state;
                    }
                    thread::sleep(Duration::from_micros(LOOK_EVERY_US));
                }
            })?;
        Ok(Self { found })
    }

    /// Call from the ring's loop with its clock: the breath's phase and pace
    /// now, or `None` for the plain breath. It reads no bus and never waits.
    /// A state before its stamp, past its 2.5 s, from anyone but Mood or not
    /// valid is no mood, whatever the thread last found.
    pub fn tick(&mut self, now_us: u64) -> Option<(f64, f32)> {
        let found = self.found.state.lock().ok()?;
        found
            .as_ref()
            .and_then(|message| MoodState::breath_in(message, now_us))
    }
}

impl Drop for RingMood {
    /// The thread ends at its next look. It is not waited for: the ring's
    /// own end never waits on the bus.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{
        ClockStamp, Familiarity, MOOD_BREATH_US, MOOD_PACE_QUICKEST, MOOD_PACE_SLOWEST, MoodBreath,
        mood_pace,
    };

    fn stamped(phase: f32, pace: f32, at_us: u64, sequence: u64) -> MoodState {
        MoodState {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us,
                sequence,
            },
            session_id: "s".into(),
            awake: 0.7,
            energy: 0.3,
            warmth: 0.6,
            asleep: false,
            attention: None,
            familiarity: Familiarity::Unknown,
            familiar: None,
            breath: Some(MoodBreath { phase, pace }),
        }
    }

    #[test]
    fn calm_and_awake_is_the_plain_pace_and_every_pace_stays_small() {
        // Day and evening, calm: exactly the breath there was before.
        assert_eq!(mood_pace(0.70, 0.30, false), 1.0);
        assert_eq!(mood_pace(0.40, 0.30, false), 1.0);
        // A bang's energy: quicker, never past the quickest.
        assert_eq!(mood_pace(0.85, 0.70, false), MOOD_PACE_QUICKEST);
        assert!((mood_pace(0.85, 0.50, false) - 1.125).abs() < 1e-6);
        // Dozing: slower as awake falls; asleep the slowest.
        assert!((mood_pace(0.20, 0.30, false) - 0.85).abs() < 1e-6);
        assert_eq!(mood_pace(0.0, 0.30, false), MOOD_PACE_SLOWEST);
        assert_eq!(mood_pace(0.0, 0.30, true), MOOD_PACE_SLOWEST);
        for awake in 0..=20 {
            for energy in 0..=20 {
                let pace = mood_pace(awake as f32 / 20.0, energy as f32 / 20.0, false);
                assert!((MOOD_PACE_SLOWEST..=MOOD_PACE_QUICKEST).contains(&pace));
            }
        }
    }

    /// What `read` gives once the thread has found it, within two seconds.
    fn soon<T: PartialEq + std::fmt::Debug>(wanted: T, mut read: impl FnMut() -> T) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let found = read();
            if found == wanted {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{found:?}, not {wanted:?}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_breath_is_read_from_moods_fresh_valid_state_and_moved_on_to_now() {
        let root = std::path::PathBuf::from(format!("/tmp/lrm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let mut mood = RingMood::on(&root).unwrap();
        // Nothing said yet: the plain breath.
        assert_eq!(mood.tick(lamp_ipc::monotonic_us()), None);
        let mut said = bus
            .publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "mood")
            .unwrap();
        let at = lamp_ipc::monotonic_us();
        said.publish_json(1, &stamped(0.25, MOOD_PACE_QUICKEST, at, 1))
            .unwrap();
        soon(true, || mood.tick(at).is_some());
        // At its stamp, the phase it said; a second on, moved on at its pace;
        // before its stamp or past its 2.5 s, no mood.
        assert_eq!(mood.tick(at), Some((0.25, MOOD_PACE_QUICKEST)));
        let (later, _) = mood.tick(at + 1_000_000).unwrap();
        let moved = 1_000_000.0 / MOOD_BREATH_US as f64 * f64::from(MOOD_PACE_QUICKEST);
        assert!((later - (0.25 + moved)).abs() < 1e-6, "{later}");
        assert_eq!(mood.tick(at - 1), None);
        assert!(mood.tick(at + LASTS_US - 1).is_some());
        assert_eq!(mood.tick(at + LASTS_US), None);
        // A breath out of its bounds is no mood.
        let at = lamp_ipc::monotonic_us();
        said.publish_json(2, &stamped(0.25, 3.0, at, 2)).unwrap();
        soon(None, || mood.tick(at));
        // Not from anyone but Mood.
        let mut other = bus
            .publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "replay")
            .unwrap();
        let at = lamp_ipc::monotonic_us();
        other.publish_json(3, &stamped(0.25, 1.0, at, 3)).unwrap();
        soon(None, || mood.tick(at));
        drop((said, other));
        std::fs::remove_dir_all(root).unwrap();
    }
}
