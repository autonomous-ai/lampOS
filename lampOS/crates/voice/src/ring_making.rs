//! The ring's quiet thinking hint at rest (#7418, the lead's call): a make
//! whose turn a microphone stall cut still runs, and the body thinks on
//! with no turn (`coordinator::MakeHeld`). A thinking body over a resting
//! ring reads as a fault, so the voice says so on `voice/making`, and the
//! ring worker's resting face shows the thinking face at its resting
//! ceiling while it holds.
//!
//! It only shapes the resting frame. It grants no output permit and changes
//! no ring ownership: the ring stays the one writer, any turn's frame
//! paints over it as over any resting frame, and the person's light, on,
//! wins. Published only with the make's one run path
//! (`LAMP_MAKE_AGENT_ONE_PATH=on`); with no fresh message the face rests as
//! before. As with Mood ([`crate::ring_mood`]), the bus is read by a thread
//! of its own, never by the ring's loop.
use crate::{ring_face::Mood, ring_light::RingLight};
use lamp_bus::{Bus, Declaration, Kind, Message};
use serde::{Deserialize, Serialize};
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

/// The topic: whether a make cut from its turn is thought on now.
pub const TOPIC: &str = "voice/making";
/// Its type, the voice's own.
pub const TYPE: &str = "lamp_voice/Making";
const VERSION: u32 = 1;
/// Said every second while it holds, and once as it ends.
pub const EVERY: Duration = Duration::from_secs(1);
/// A message counts this long after it was said: a voice gone says no more,
/// and the hint ends by itself.
pub const LASTS: Duration = Duration::from_millis(2_500);
/// Read the bus this often.
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// What the voice says on [`TOPIC`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Making {
    pub making: bool,
}

/// The newest replaces the last; it lasts [`LASTS`].
pub fn declaration() -> Declaration {
    Declaration::new(TYPE, VERSION, Kind::Latest)
        .every(EVERY)
        .lasting(LASTS)
}

/// Whether `message` is the voice saying, within [`LASTS`] of `now_us`,
/// that a make cut from its turn is thought on. Anything else, old, from
/// the future, from another source or not valid, is no hint.
pub fn making_in(message: &Message, now_us: u64) -> bool {
    if message.expired
        || message.header.source != crate::bus_out::SOURCE
        || message.header.at_us > now_us
        || now_us - message.header.at_us > LASTS.as_micros() as u64
    {
        return false;
    }
    message.parse::<Making>().is_ok_and(|making| making.making)
}

/// Whether a skill or a computer holds the ring's light now: a light a
/// request holds, its pattern, its fade or a computer's own pixels. The
/// same test that keeps an update's look off them ([`crate::ring_update`]).
pub fn light_held(light: Option<&RingLight>) -> bool {
    light.is_some_and(|light| {
        light.held().is_some()
            || light.pixels().is_some()
            || light.fade().is_some()
            || light.pattern().is_some()
    })
}

/// The face at rest: the thinking face, under the resting ceiling, while
/// hinted and the ring shows neither the person's light nor one a skill or
/// a computer holds ([`light_held`]); else the resting face, as before.
pub fn resting_mood(hinted: bool, lit: bool, held: bool) -> Mood {
    if hinted && !lit && !held {
        Mood::Thinking
    } else {
        Mood::Resting
    }
}

#[derive(Default)]
struct Found {
    /// The last message said on [`TOPIC`], whole.
    said: Mutex<Option<Message>>,
    stop: AtomicBool,
}

/// The voice's hint, as the ring worker reads it.
pub struct RingMaking {
    found: Arc<Found>,
}

impl RingMaking {
    /// [`TOPIC`] on the bus under `root`, read by a thread of its own from
    /// now on.
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let found = Arc::new(Found::default());
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-making".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    // A bus that cannot be read is no hint.
                    let said = bus.latest(TOPIC).ok().flatten();
                    if let Ok(mut slot) = reading.said.lock() {
                        *slot = said;
                    }
                    thread::sleep(LOOK_EVERY);
                }
            })?;
        Ok(Self { found })
    }

    /// Whether the thinking hint holds at `now_us`, the ring's clock, as
    /// last read ([`making_in`]). It reads no bus and never waits.
    pub fn hinted(&self, now_us: u64) -> bool {
        self.found.said.lock().ok().is_some_and(|said| {
            said.as_ref()
                .is_some_and(|message| making_in(message, now_us))
        })
    }
}

impl Drop for RingMaking {
    /// The thread ends at its next look; the ring never waits on the bus.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// The last message `source` said on a real bus, `value` at `at_us`.
    fn said(source: &str, at_us: u64, value: Making) -> Message {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ring-making-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bus = Bus::new(&root).unwrap();
        let mut out = bus
            .publisher_declared(TOPIC, &declaration(), source)
            .unwrap();
        out.publish_json(at_us, &value).unwrap();
        let message = bus.latest(TOPIC).unwrap().unwrap();
        let _ = std::fs::remove_dir_all(root);
        message
    }

    /// Only the voice saying a make is thought on, within its 2.5 s, is a
    /// hint; the face it gives is the thinking face only over a dark ring
    /// showing no computer's pixels.
    #[test]
    fn only_the_voice_saying_a_cut_make_runs_hints_the_resting_face() {
        let at = 10_000_000;
        let lasts = LASTS.as_micros() as u64;
        let on = Making { making: true };
        assert!(making_in(&said("voice", at, on), at));
        assert!(making_in(&said("voice", at, on), at + lasts));
        for (message, now, label) in [
            (said("voice", at, Making { making: false }), at, "ended"),
            (said("voice", at, on), at + lasts + 1, "old"),
            (said("voice", at, on), at - 1, "from the future"),
            (said("skills", at, on), at, "another source"),
        ] {
            assert!(!making_in(&message, now), "{label}");
        }
        assert_eq!(resting_mood(true, false, false), Mood::Thinking);
        assert_eq!(resting_mood(false, false, false), Mood::Resting);
        assert_eq!(resting_mood(true, true, false), Mood::Resting);
        assert_eq!(resting_mood(true, false, true), Mood::Resting);
    }

    /// A skill's light the ring holds wins over the hint (Heron's P3 on
    /// #8432): a light-off it fades into, as at a sleep, keeps the resting
    /// face; given back, the hint thinks again. With no requests at all,
    /// nothing is held.
    #[test]
    fn a_skills_held_light_wins_over_a_makes_thinking_hint() {
        use lamp_msgs::{Claim, Light, LightRequest, Priority};
        assert!(!light_held(None));
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ring-making-held-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (mut ring, _) = RingLight::on(&root).unwrap();
        ring.take_requests(&root, "boot-test").unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let asked = |until_us| LightRequest {
            claim: Claim {
                source: "skill:light-out".into(),
                priority: Priority::Skill,
                clock_id: "boot-test".into(),
                until_us,
            },
            light: Light {
                on: false,
                colour: "warm".into(),
                level: 4,
            },
            pattern: None,
            fade_ms: Some(30_000),
            pixels: None,
        };
        let mut at = 0u64;
        let mut looked = |ring: &mut RingLight| {
            for _ in 0..5 {
                at += 50_000;
                ring.tick(at);
                thread::sleep(Duration::from_millis(2));
            }
        };
        ring.tick(0);
        assert!(!light_held(Some(&ring)), "the person's light");
        glows.publish_json(1, &asked(20_000_000)).unwrap();
        looked(&mut ring);
        assert!(light_held(Some(&ring)));
        assert_eq!(
            resting_mood(true, false, light_held(Some(&ring))),
            Mood::Resting
        );
        glows.publish_json(2, &asked(0)).unwrap();
        looked(&mut ring);
        assert!(!light_held(Some(&ring)), "given back");
        assert_eq!(
            resting_mood(true, false, light_held(Some(&ring))),
            Mood::Thinking
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// The voice says it only with the make's one run path: while it holds,
    /// then once as it ends; with the switch off, never.
    #[test]
    fn the_voice_says_a_cut_make_is_thought_on_only_on_the_one_path() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ring-making-out-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bus = Bus::new(&root).unwrap();
        let mut off = crate::bus_out::BusOut::on(&bus)
            .unwrap()
            .with_making(&bus, false)
            .unwrap();
        off.making(true, 1_000_000);
        assert!(bus.latest(TOPIC).unwrap().is_none(), "nothing said off");
        let mut on = crate::bus_out::BusOut::on(&bus)
            .unwrap()
            .with_making(&bus, true)
            .unwrap();
        on.making(true, 1_000_000);
        let said = bus.latest(TOPIC).unwrap().unwrap();
        assert!(making_in(&said, 1_000_000));
        on.making(false, 1_100_000);
        let said = bus.latest(TOPIC).unwrap().unwrap();
        assert!(!making_in(&said, 1_100_000));
        assert_eq!(said.parse::<Making>().unwrap(), Making { making: false });
        let _ = std::fs::remove_dir_all(root);
    }
}
