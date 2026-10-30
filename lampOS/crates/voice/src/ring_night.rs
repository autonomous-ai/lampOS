//! The night's sleep gets the asleep face (#3748): a dozing creature looks
//! the same whichever way it fell asleep (CLAUDE.md rule 8). A hand's sleep
//! mutes the voice, and its wait paints the asleep face
//! ([`crate::ring_asleep::SleepFace`]); Expression's own sleep at night, by
//! itself or by the `night-sleep` skill, mutes nothing, so the ring worker
//! keeps the lamp and here, at rest, eases its face into the same faint warm
//! glow ([`lamp_ring::asleep::asleep_pixels`]) and back out on waking.
//!
//! Only with `LAMP_NIGHT_ASLEEP_FACE=on`, handed to the ring worker alone.
//! It shapes the resting frame only: the ring stays the one writer, a cue
//! paints over it as over any resting frame, and the person's light, on,
//! wins. As with Mood ([`crate::ring_mood`]), the bus is read by a thread
//! of its own, never by the ring's loop.
use crate::ring_asleep::{AFTER, FADE_IN, breath_at};
use lamp_bus::{Bus, Message};
use lamp_msgs::ExpressionState;
use lamp_ring::{PIXEL_COUNT, Rgb, asleep::asleep_pixels};
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

/// The switch: the night's sleep shows the asleep face only with `on`.
pub const ENVIRONMENT: &str = "LAMP_NIGHT_ASLEEP_FACE";

/// Whether the switch is on in this process's environment.
pub fn enabled() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Expression says its state every 200 ms and a state lasts 500 ms.
const LOOK_EVERY: Duration = Duration::from_millis(100);
/// How long after it was said a state still counts.
const LASTS_US: u64 = 500_000;

/// Whether `message` is Expression, enabled, saying now that it is asleep
/// (or falling asleep). Anything else, old, from another source or not
/// valid, is awake.
pub fn asleep_in(message: &Message, now_us: u64) -> bool {
    if message.expired
        || message.header.source != "expression"
        || message.header.at_us > now_us
        || now_us - message.header.at_us > LASTS_US
    {
        return false;
    }
    message
        .parse::<ExpressionState>()
        .is_ok_and(|state| state.check().is_ok() && state.enabled && state.asleep == Some(true))
}

/// How much of the asleep face shows, 0 to 1: rising over [`FADE_IN`] once
/// asleep for [`AFTER`], as the head folds, and falling as fast on waking.
#[derive(Debug, Default)]
pub struct Fade {
    asleep_since: Option<u64>,
    at_us: Option<u64>,
    share: f32,
}

impl Fade {
    pub fn step(&mut self, asleep: bool, now_us: u64) -> f32 {
        let elapsed = self.at_us.map_or(0, |at| now_us.saturating_sub(at));
        self.at_us = Some(now_us);
        self.asleep_since = asleep.then(|| self.asleep_since.unwrap_or(now_us));
        let change = elapsed as f32 / FADE_IN.as_micros() as f32;
        let rising = self
            .asleep_since
            .is_some_and(|since| now_us - since >= AFTER.as_micros() as u64);
        self.share = if rising {
            self.share + change
        } else {
            self.share - change
        }
        .clamp(0.0, 1.0);
        self.share
    }
}

/// The resting `face` with `share` of the asleep face over it, breathing
/// its 6 s breath on the boot clock.
pub fn over(face: &[Rgb; PIXEL_COUNT], share: f32, now_us: u64) -> [Rgb; PIXEL_COUNT] {
    let asleep = asleep_pixels(breath_at(Duration::from_micros(now_us)));
    let mix = |from: u8, to: u8| {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * share).round() as u8
    };
    std::array::from_fn(|index| {
        let (from, to) = (face[index], asleep[index]);
        Rgb::new(
            mix(from.red, to.red),
            mix(from.green, to.green),
            mix(from.blue, to.blue),
        )
    })
}

#[derive(Default)]
struct Found {
    state: Mutex<Option<Message>>,
    stop: AtomicBool,
}

pub struct RingNight {
    found: Arc<Found>,
    fade: Fade,
}

impl RingNight {
    /// Expression's state on the bus under `root`, read by a thread of its
    /// own from now on.
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let found = Arc::new(Found::default());
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-night".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    // A bus that cannot be read is no sleep.
                    let state = bus.latest(ExpressionState::TOPIC).ok().flatten();
                    if let Ok(mut slot) = reading.state.lock() {
                        *slot = state;
                    }
                    thread::sleep(LOOK_EVERY);
                }
            })?;
        Ok(Self {
            found,
            fade: Fade::default(),
        })
    }

    /// Call from the ring's loop with its clock: how much of the asleep
    /// face shows now, where it may (`allowed`: not over the person's light
    /// on). It reads no bus and never waits.
    pub fn tick(&mut self, now_us: u64, allowed: bool) -> f32 {
        let asleep = allowed
            && self.found.state.lock().ok().is_some_and(|found| {
                found
                    .as_ref()
                    .is_some_and(|message| asleep_in(message, now_us))
            });
        self.fade.step(asleep, now_us)
    }
}

#[cfg(test)]
impl RingNight {
    /// As if Expression had slept long since and the face were all there.
    fn long_asleep(mut self) -> Self {
        self.fade = Fade {
            asleep_since: Some(0),
            at_us: Some(0),
            share: 1.0,
        };
        self
    }
}

impl Drop for RingNight {
    /// The thread ends at its next look; the ring's end never waits on it.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;
    use lamp_msgs::ClockStamp;
    use std::sync::atomic::AtomicU64;

    const TICK: u64 = 25_000;

    fn state(asleep: Option<bool>, enabled: bool) -> ExpressionState {
        ExpressionState {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: 1,
                sequence: 1,
            },
            enabled,
            turn: None,
            request: None,
            behaviours: vec!["asleep".into()],
            missing: vec![],
            warnings: vec![],
            body: false,
            light: false,
            asleep,
        }
    }

    /// The last message `source` said on a real bus, `value` at `at_us`.
    fn said(source: &str, at_us: u64, value: &ExpressionState) -> Message {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ring-night-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bus = Bus::new(&root).unwrap();
        let mut out = bus
            .publisher(ExpressionState::TOPIC, Kind::Latest, source)
            .unwrap();
        out.publish_json(at_us, value).unwrap();
        let message = bus.latest(ExpressionState::TOPIC).unwrap().unwrap();
        let _ = std::fs::remove_dir_all(root);
        message
    }

    /// Only Expression, enabled, saying asleep within its half second is
    /// asleep: awake, shadow, no idle layer, another source, a state from
    /// the future or one past its life are not.
    #[test]
    fn only_a_fresh_enabled_expression_saying_asleep_is_asleep() {
        let at = 10_000_000;
        assert!(asleep_in(
            &said("expression", at, &state(Some(true), true)),
            at + LASTS_US
        ));
        for (message, now, label) in [
            (
                said("expression", at, &state(Some(false), true)),
                at,
                "awake",
            ),
            (
                said("expression", at, &state(None, true)),
                at,
                "no idle layer",
            ),
            (
                said("expression", at, &state(Some(true), false)),
                at,
                "shadow",
            ),
            (
                said("skills", at, &state(Some(true), true)),
                at,
                "another source",
            ),
            (
                said("expression", at, &state(Some(true), true)),
                at - 1,
                "future",
            ),
            (
                said("expression", at, &state(Some(true), true)),
                at + LASTS_US + 1,
                "old",
            ),
        ] {
            assert!(!asleep_in(&message, now), "{label}");
        }
    }

    /// Asleep, nothing shows for the first 1.5 s, then it fades in over
    /// 1.5 s; awake, it fades out as fast, from wherever it was.
    #[test]
    fn it_fades_in_after_a_moment_asleep_and_out_on_waking() {
        let mut fade = Fade::default();
        let mut now = 0;
        let mut run = |fade: &mut Fade, asleep: bool, for_us: u64| {
            let end = now + for_us;
            let mut share = 0.0;
            while now < end {
                now += TICK;
                share = fade.step(asleep, now);
            }
            share
        };
        assert_eq!(run(&mut fade, true, AFTER.as_micros() as u64 - TICK), 0.0);
        let half = run(&mut fade, true, FADE_IN.as_micros() as u64 / 2);
        assert!((0.45..=0.55).contains(&half), "{half}");
        assert_eq!(run(&mut fade, true, FADE_IN.as_micros() as u64), 1.0);
        let waking = run(&mut fade, false, FADE_IN.as_micros() as u64 / 2);
        assert!((0.45..=0.55).contains(&waking), "{waking}");
        assert_eq!(run(&mut fade, false, FADE_IN.as_micros() as u64), 0.0);
        // A moment asleep that ends before the face shows shows nothing.
        assert_eq!(run(&mut fade, true, AFTER.as_micros() as u64 / 2), 0.0);
        assert_eq!(run(&mut fade, false, TICK), 0.0);
        assert_eq!(
            run(&mut fade, true, AFTER.as_micros() as u64 - 2 * TICK),
            0.0
        );
    }

    struct Link {
        control: Option<crate::wire::Control>,
        calls: usize,
    }
    impl crate::ring_worker::RingLink for Link {
        fn receive_control(&mut self) -> io::Result<Option<crate::wire::Control>> {
            self.calls += 1;
            Ok(self
                .control
                .take()
                .or_else(|| (self.calls > 8).then_some(crate::wire::Control::Stop)))
        }
        fn receive_frame(&mut self) -> io::Result<Option<crate::ring_wire::RingFrame>> {
            Ok(None)
        }
        fn publish(&mut self, _: crate::wire::WorkerEvent) -> io::Result<()> {
            Ok(())
        }
    }
    struct Sink(Arc<Mutex<Vec<lamp_ring::EncodedFrame>>>);
    impl lamp_ring::FrameSink for Sink {
        fn write_frame(&mut self, frame: &lamp_ring::EncodedFrame) -> io::Result<()> {
            self.0.lock().unwrap().push(frame.clone());
            Ok(())
        }
    }

    /// The resting frames the actual ring worker writes, its night sense
    /// long asleep, with Expression saying `asleep` (or nothing) now and
    /// the person's light `lit` or off.
    fn resting_frames(asleep: Option<bool>, lit: bool) -> Vec<lamp_ring::EncodedFrame> {
        use lamp_turn_owner::{AdmissionState, CaptureState, Controller, MonoTime, Permission};
        let now = || MonoTime::from_micros(lamp_ipc::monotonic_us());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rnw-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bus = Bus::new(&root).unwrap();
        if let Some(asleep) = asleep {
            let mut out = bus
                .publisher(ExpressionState::TOPIC, Kind::Latest, "expression")
                .unwrap();
            out.publish_json(lamp_ipc::monotonic_us(), &state(Some(asleep), true))
                .unwrap();
        }
        let mut out = bus
            .publisher(lamp_msgs::Light::TOPICS[0], Kind::Latest, "actions")
            .unwrap();
        out.publish_json(
            lamp_ipc::monotonic_us(),
            &lamp_msgs::Light::new(lit, "warm", 2).unwrap(),
        )
        .unwrap();
        let light = crate::ring_light::RingLight::on(&root).unwrap();
        let boot = crate::process::new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now().as_micros() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let link = Link {
            control: Some(crate::wire::Control::Authority {
                snapshot: controller.snapshot(now()).unwrap(),
            }),
            calls: 0,
        };
        let night = RingNight::on(&root).unwrap().long_asleep();
        // The reading thread's first look.
        while night.found.state.lock().unwrap().is_none() && asleep.is_some() {
            thread::sleep(Duration::from_millis(1));
        }
        let frames = Arc::new(Mutex::new(Vec::new()));
        crate::ring_worker::run_with_sink_senses(
            link,
            boot,
            Sink(frames.clone()),
            Some(40),
            crate::ring_worker::Senses {
                night: Some(night),
                light: Some(light),
                ..Default::default()
            },
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
        frames.lock().unwrap().clone()
    }

    /// Through the actual worker loop and its guarded sink: asleep, the
    /// resting frame is the asleep face itself; awake or unsaid, or over the
    /// person's light on, it never is.
    #[test]
    fn the_worker_rests_on_the_asleep_face_only_while_expression_sleeps() {
        let pictures: Vec<_> = (0..=1000)
            .map(|step| {
                lamp_ring::encode(
                    &asleep_pixels(step as f32 / 1000.0),
                    lamp_ring::ChannelCeiling::new(40).unwrap(),
                )
            })
            .collect();
        let shows = |asleep, lit| {
            resting_frames(asleep, lit)
                .iter()
                .any(|frame| pictures.contains(frame))
        };
        assert!(shows(Some(true), false));
        assert!(!shows(Some(false), false));
        assert!(!shows(None, false));
        assert!(!shows(Some(true), true));
    }

    /// None of it is the resting face as it was; all of it is the asleep
    /// face exactly, the same picture a hand's sleep shows.
    #[test]
    fn all_of_it_is_the_asleep_face_and_none_is_the_face_as_it_was() {
        let face = [Rgb::new(40, 30, 20); PIXEL_COUNT];
        let now = 7_000_000;
        assert_eq!(over(&face, 0.0, now), face);
        assert_eq!(
            over(&face, 1.0, now),
            asleep_pixels(breath_at(Duration::from_micros(now)))
        );
        let half = over(&face, 0.5, now);
        assert_eq!(half[0], Rgb::new(20, 15, 10));
    }
}
