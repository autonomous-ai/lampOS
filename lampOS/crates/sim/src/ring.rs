//! The ring's stand-in: the light the lamp draws, from the claims on
//! `light/request`, and the camera's and microphone's marks it must show
//! (`light/mark`) and say it showed (`light/mark_drawn`). The 32 pixels are
//! kept for a viewer. No SPI, no timing of a real WS2812 chain.
use lamp_bus::{Bus, Kind, Publisher, Subscriber};
use lamp_msgs::{
    Light, LightPattern, LightRequest, MarkKind, PatternKind, RingMark, RingMarkDrawn,
};
use serde_json::{Value, json};
use std::{io, time::Duration};

/// Pixels on the ring.
pub const PIXELS: usize = 32;
/// The ring's own name on the bus.
const SOURCE: &str = "ring";

/// What the lamp shows when nothing asks: off.
fn rest() -> Light {
    Light {
        on: false,
        colour: "warm".to_owned(),
        level: 1,
    }
}

/// The claim that holds the ring now.
struct Held {
    request: LightRequest,
    /// When the light was first drawn, on the monotonic clock: a pattern's
    /// cycles count from here, and a renewal of the same light keeps it
    /// (`lamp-voice`'s ring does the same, ring_light.rs: "a renewal keeps that").
    since_us: u64,
}

pub struct Ring {
    requests: Subscriber,
    marks: Subscriber,
    drawn: Publisher,
    state: Publisher,
    mark_drawn: Publisher,
    held: Option<Held>,
    shown: Option<Light>,
    last_said_us: u64,
    last_mark_said: std::collections::HashMap<String, u64>,
    /// Each held mark's asker and when its claim lapses.
    marks_held: std::collections::HashMap<String, u64>,
    pub pixels: [[u8; 3]; PIXELS],
    /// The pattern the shown light moves in, if any.
    pattern: Option<LightPattern>,
    /// When a camera mark and a microphone mark were last asked, on the monotonic clock.
    pub camera_marked_us: u64,
    pub microphone_marked_us: u64,
    pub speaker_marked_us: u64,
}

impl Ring {
    pub fn on(bus: &Bus) -> io::Result<Ring> {
        Ok(Ring {
            requests: bus.subscribe_typed(LightRequest::TOPIC, SOURCE, LightRequest::TYPE)?,
            marks: bus.subscribe_typed(RingMark::TOPIC, SOURCE, RingMark::TYPE)?,
            drawn: bus.publisher("light/drawn", Kind::Latest, SOURCE)?,
            state: bus.publisher(Light::TOPICS[1], Kind::Latest, SOURCE)?,
            mark_drawn: bus.publisher_declared(
                RingMarkDrawn::TOPIC,
                &RingMarkDrawn::declaration(),
                RingMarkDrawn::SOURCE,
            )?,
            held: None,
            shown: None,
            last_said_us: 0,
            last_mark_said: Default::default(),
            marks_held: Default::default(),
            pixels: [[0; 3]; PIXELS],
            pattern: None,
            camera_marked_us: 0,
            microphone_marked_us: 0,
            speaker_marked_us: 0,
        })
    }

    /// One look: take what came, draw, say it. Never waits long.
    pub fn step(&mut self, clock_id: &str) -> io::Result<()> {
        let now = lamp_ipc::monotonic_us();
        while let Some(message) = self.requests.recv(Duration::from_millis(2))? {
            let Ok(request) = message.parse::<LightRequest>() else {
                continue;
            };
            // The higher claim holds; an equal or higher one replaces it.
            let beats = self.held.as_ref().is_none_or(|held| {
                held.request.claim.until_us <= now
                    || request.claim.priority >= held.request.claim.priority
            });
            if beats && request.claim.clock_id == clock_id {
                // The same asker's same light and pattern again is a renewal.
                let since_us = match &self.held {
                    Some(held)
                        if held.request.claim.until_us > now
                            && held.request.claim.source == request.claim.source
                            && held.request.light == request.light
                            && held.request.pattern == request.pattern
                            && held.request.pixels == request.pixels =>
                    {
                        held.since_us
                    }
                    _ => now,
                };
                self.held = Some(Held { request, since_us });
            }
        }
        if self
            .held
            .as_ref()
            .is_some_and(|h| h.request.claim.until_us <= now)
        {
            self.held = None;
        }
        let light = self
            .held
            .as_ref()
            .map(|h| h.request.light.clone())
            .unwrap_or_else(rest);
        // The pattern moves a light the asker did not draw pixel by pixel
        // (its `pixels` are "at rest, without a pattern": arbitration.rs).
        self.pattern = self
            .held
            .as_ref()
            .and_then(|h| h.request.pixels.is_none().then_some(h.request.pattern))
            .flatten()
            .filter(|_| light.on);
        self.pixels = match (self.held.as_ref(), self.pattern) {
            (Some(held), Some(pattern)) => {
                moving(&light, pattern, now.saturating_sub(held.since_us))
            }
            (Some(Held { request, .. }), None) if request.pixels.is_some() => {
                let mut all = [[0u8; 3]; PIXELS];
                for (slot, pixel) in all
                    .iter_mut()
                    .zip(request.pixels.clone().unwrap_or_default())
                {
                    *slot = pixel;
                }
                all
            }
            _ => [colour_of(&light); PIXELS],
        };
        let changed = self.shown.as_ref() != Some(&light);
        if changed || now.saturating_sub(self.last_said_us) >= 1_000_000 {
            self.last_said_us = now;
            self.shown = Some(light.clone());
            let said = json!({"on": light.on, "colour": light.colour, "level": light.level});
            self.drawn.publish_json(now, &said)?;
            self.state.publish_json(now, &said)?;
        }
        // The marks: each held mark stays drawn until its claim lapses, and
        // the ring says so every 250 ms, as long as it draws it.
        while let Some(message) = self.marks.recv(Duration::ZERO)? {
            let Ok(mark) = message.parse::<RingMark>() else {
                continue;
            };
            match mark.kind {
                MarkKind::Camera => self.camera_marked_us = now,
                MarkKind::Microphone => self.microphone_marked_us = now,
                MarkKind::Speaker => self.speaker_marked_us = now,
            }
            self.marks_held
                .insert(mark.claim.source.clone(), mark.claim.until_us);
        }
        self.marks_held.retain(|_, until| *until > now);
        let due: Vec<String> = self
            .marks_held
            .keys()
            .filter(|source| {
                now.saturating_sub(self.last_mark_said.get(*source).copied().unwrap_or(0))
                    >= RingMarkDrawn::EVERY_US
            })
            .cloned()
            .collect();
        for source in due {
            self.last_mark_said.insert(source.clone(), now);
            self.mark_drawn.publish_json(
                now,
                &RingMarkDrawn {
                    source,
                    clock_id: clock_id.to_owned(),
                    at_us: now,
                },
            )?;
        }
        Ok(())
    }

    /// The ring as a viewer reads it: the pixels as drawn at this moment,
    /// and the pattern's name while a light moves in one (absent otherwise,
    /// so a steady light's view is what it always was).
    pub fn view(&self) -> Value {
        let shown = self.shown.clone().unwrap_or_else(rest);
        let mut view = json!({
            "on": shown.on, "colour": shown.colour, "level": shown.level,
            "pixels": self.pixels.iter().map(|p| format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2])).collect::<Vec<_>>(),
        });
        if let (Some(pattern), Some(object)) = (self.pattern, view.as_object_mut()) {
            object.insert("pattern".to_owned(), json!(kind_name(pattern.kind)));
        }
        view
    }
}

/// The colour a light shows on every pixel, at its level (1 to 5).
fn colour_of(light: &Light) -> [u8; 3] {
    if !light.on {
        return [0; 3];
    }
    drawn(light, f32::from(light.level.clamp(1, 5)) / 5.0)
}

/// A pattern's name as the pattern's own wire spelling.
fn kind_name(kind: PatternKind) -> &'static str {
    match kind {
        PatternKind::Breathe => "breathe",
        PatternKind::Candle => "candle",
        PatternKind::Pulse => "pulse",
    }
}

/// The light's colour at `share` of the full level (level 5 is 1.0), under
/// the real ring's cap (level/5 of 120 a channel).
fn drawn(light: &Light, share: f32) -> [u8; 3] {
    let tint = Light::TINTS
        .iter()
        .find(|(name, _)| *name == light.colour)
        .map(|(_, t)| *t)
        .unwrap_or([1.0, 1.0, 1.0]);
    // The cap first, then the tint: the order the steady light has always had.
    let scale = share * 120.0;
    [0, 1, 2].map(|k| (tint[k] * scale).round() as u8)
}

/// How far under its level a candle dips at most (ring_face.rs `CANDLE_DIP`).
const CANDLE_DIP: f32 = 0.2;
/// How much brighter than the level a pulse's arc is at its middle
/// (ring_face.rs `PULSE_RISE`).
const PULSE_RISE: f32 = 1.35;

/// The 32 pixels of a light moving in `pattern`, `age_us` after it was first
/// drawn. The shapes, periods and bounds are lamp-voice's own:
/// `crates/voice/src/ring_face.rs` `moving` (the sine of a breath from `low`
/// to the level, a candle's two waves of two and three lobes up to a fifth
/// under the level, a pulse's arc a sixth of the ring wide, up to 1.35 times
/// the level, once around over the period and then the steady light); the
/// period is the pattern's own, `LightPattern::check`'s bounds hold it
/// (breathe 4 to 12 s, candle 1 to 4 s, pulse 3 to 6 s). Level shares are
/// the sim's linear level/5 (the real ring's glow table is its own).
fn moving(light: &Light, pattern: LightPattern, age_us: u64) -> [[u8; 3]; PIXELS] {
    use std::f32::consts::{PI, TAU};
    let top = f32::from(light.level.clamp(1, 5)) / 5.0;
    let period_us = (u64::from(pattern.period_ms) * 1_000).max(1);
    let cycles = (age_us as f64 / period_us as f64) as f32;
    let mut all = [[0u8; 3]; PIXELS];
    for (pixel, slot) in all.iter_mut().enumerate() {
        let angle = TAU * pixel as f32 / PIXELS as f32;
        let share = match pattern.kind {
            PatternKind::Breathe => {
                let bottom = f32::from(pattern.low.clamp(1, 5)) / 5.0;
                bottom + (top - bottom) * (0.5 - 0.5 * (TAU * cycles).cos())
            }
            PatternKind::Candle => {
                let wander = 0.5
                    + 0.3 * (TAU * cycles + 2.0 * angle).sin()
                    + 0.2 * (TAU * cycles / 1.618 - 3.0 * angle + 1.1).sin();
                top * (1.0 - CANDLE_DIP * wander.clamp(0.0, 1.0))
            }
            PatternKind::Pulse if cycles < 1.0 => {
                let apart = (angle - TAU * cycles).rem_euclid(TAU);
                let apart = apart.min(TAU - apart);
                let width = TAU / 6.0;
                let near = if apart < width {
                    0.5 + 0.5 * (PI * apart / width).cos()
                } else {
                    0.0
                };
                top + ((top * PULSE_RISE).min(1.0) - top) * near
            }
            PatternKind::Pulse => top,
        };
        *slot = drawn(light, share);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{Claim, Priority};

    fn light(colour: &str, level: u8) -> Light {
        Light {
            on: true,
            colour: colour.to_owned(),
            level,
        }
    }

    fn pattern(kind: PatternKind, period_ms: u32, low: u8) -> LightPattern {
        LightPattern {
            kind,
            period_ms,
            low,
        }
    }

    /// The brightest channel of a pixel, as a share of the cap (level 5 is 120).
    fn bright(pixel: [u8; 3]) -> u8 {
        pixel.into_iter().max().unwrap_or(0)
    }

    /// Each pattern's pixels, over its period, in `steps` looks.
    fn over(light: &Light, p: LightPattern, steps: u64) -> Vec<[[u8; 3]; PIXELS]> {
        let period_us = u64::from(p.period_ms) * 1_000;
        (0..steps)
            .map(|k| moving(light, p, period_us * k / steps))
            .collect()
    }

    #[test]
    fn every_tint_at_every_level_is_what_the_old_expression_gave() {
        for (name, tint) in Light::TINTS {
            for level in 1..=5u8 {
                let scale = f32::from(level.clamp(1, 5)) / 5.0 * 120.0;
                let old = [0, 1, 2].map(|k| (tint[k] * scale).round() as u8);
                assert_eq!(colour_of(&light(name, level)), old, "{name} at {level}");
            }
        }
    }

    #[test]
    fn a_breath_goes_between_its_low_and_the_level_and_back_in_one_period() {
        let l = light("white", 4);
        let p = pattern(PatternKind::Breathe, 6_000, 1);
        let looks = over(&l, p, 60);
        let lows = bright(drawn(&l, 1.0 / 5.0));
        let tops = bright(colour_of(&l));
        let seen: Vec<u8> = looks.iter().map(|all| bright(all[0])).collect();
        assert_eq!(seen[0], lows, "a breath starts at its low");
        assert!(seen.iter().all(|b| (lows..=tops).contains(b)), "{seen:?}");
        assert!(seen[30] >= tops - 1, "half a period on it is at the level");
        // The whole ring breathes together.
        assert!(looks.iter().all(|all| all.iter().all(|px| *px == all[0])));
    }

    #[test]
    fn a_candle_wanders_under_the_level_and_not_alike_on_every_pixel() {
        let l = light("orange", 4);
        let p = pattern(PatternKind::Candle, 1_500, 1);
        let top = colour_of(&l);
        let looks = over(&l, p, 30);
        for all in &looks {
            for px in all {
                // A fifth under the level at most, and never over it.
                assert!(
                    (0..3)
                        .all(|k| px[k] <= top[k]
                            && f32::from(px[k]) >= f32::from(top[k]) * 0.8 - 1.0),
                    "{px:?} vs {top:?}"
                );
            }
        }
        assert_ne!(looks[0], looks[10], "it moves over its period");
        assert!(
            looks.iter().any(|all| all.iter().any(|px| *px != all[0])),
            "the pixels are not all the same at a moment"
        );
        // The colour never shifts: a pixel's channels keep the tint's ratio.
        let first = looks[3][5];
        assert!(first[0] >= first[1] && first[1] >= first[2], "{first:?}");
    }

    #[test]
    fn a_pulse_sends_one_arc_around_then_rests_at_the_level() {
        let l = light("blue", 3);
        let p = pattern(PatternKind::Pulse, 4_000, 1);
        let rest = colour_of(&l);
        let looks = over(&l, p, 40);
        let top_of = |all: &[[u8; 3]; PIXELS]| all.iter().map(|px| bright(*px)).max().unwrap_or(0);
        assert!(
            top_of(&looks[10]) > bright(rest),
            "the arc is brighter than the level"
        );
        assert!(top_of(&looks[10]) <= 120, "and under the real ring's cap");
        // The arc is somewhere else a quarter period later.
        let place = |all: &[[u8; 3]; PIXELS]| (0..PIXELS).max_by_key(|k| bright(all[*k])).unwrap();
        assert_ne!(place(&looks[5]), place(&looks[20]));
        // After a period: the steady light.
        assert_eq!(moving(&l, p, 4_000_000), [rest; PIXELS]);
        assert_eq!(moving(&l, p, 9_000_000), [rest; PIXELS]);
    }

    /// A ring on a bus of its own, asked by a test.
    fn asked(name: &str) -> (Ring, lamp_bus::Publisher, String, std::path::PathBuf) {
        let root = crate::test_root().join(format!("lsm-ring-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let ring = Ring::on(&bus).unwrap();
        let asker = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        (ring, asker, "test-boot".to_owned(), root)
    }

    fn request(
        light: Light,
        pattern: Option<LightPattern>,
        until_ms: u64,
        clock: &str,
    ) -> LightRequest {
        LightRequest {
            claim: Claim {
                source: "skill:test".to_owned(),
                priority: Priority::Skill,
                clock_id: clock.to_owned(),
                until_us: lamp_ipc::monotonic_us() + until_ms * 1_000,
            },
            light,
            pattern,
            fade_ms: None,
            pixels: None,
        }
    }

    #[test]
    fn a_light_without_a_pattern_is_the_steady_view_it_always_was() {
        let (mut ring, mut asker, clock, root) = asked("steady");
        let l = light("warm", 3);
        asker
            .publish_json(1, &request(l.clone(), None, 5_000, &clock))
            .unwrap();
        ring.step(&clock).unwrap();
        let expected = json!({
            "on": true, "colour": "warm", "level": 3,
            "pixels": vec![format!("#{:02x}{:02x}{:02x}", colour_of(&l)[0], colour_of(&l)[1], colour_of(&l)[2]); PIXELS],
        });
        // The old formula, written out: the tint at level/5 of 120 a channel.
        let tint = Light::TINTS.iter().find(|(n, _)| *n == "warm").unwrap().1;
        let old = [0, 1, 2].map(|k| (tint[k] * (3.0f32 / 5.0 * 120.0)).round() as u8);
        assert_eq!(colour_of(&l), old);
        // The same text, key for key and byte for byte (no `pattern` key).
        assert_eq!(ring.view().to_string(), expected.to_string());
        std::thread::sleep(Duration::from_millis(60));
        ring.step(&clock).unwrap();
        assert_eq!(ring.view().to_string(), expected.to_string());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_pattern_is_named_in_the_view_and_ends_with_a_new_plain_light() {
        let (mut ring, mut asker, clock, root) = asked("ends");
        let l = light("white", 4);
        let p = pattern(PatternKind::Breathe, 4_000, 1);
        asker
            .publish_json(1, &request(l.clone(), Some(p), 5_000, &clock))
            .unwrap();
        ring.step(&clock).unwrap();
        let first = ring.view();
        assert_eq!(first["pattern"], "breathe");
        assert_eq!(first["pixels"].as_array().unwrap().len(), PIXELS);
        std::thread::sleep(Duration::from_millis(300));
        ring.step(&clock).unwrap();
        assert_ne!(ring.view()["pixels"], first["pixels"], "it moves with time");
        // A new request without one ends it.
        asker
            .publish_json(2, &request(l.clone(), None, 5_000, &clock))
            .unwrap();
        ring.step(&clock).unwrap();
        assert!(ring.view().get("pattern").is_none());
        assert_eq!(ring.pixels, [colour_of(&l); PIXELS]);
        // And a pattern's light that lapses ends it with the light.
        asker
            .publish_json(3, &request(l, Some(p), 100, &clock))
            .unwrap();
        ring.step(&clock).unwrap();
        assert_eq!(ring.view()["pattern"], "breathe");
        std::thread::sleep(Duration::from_millis(200));
        ring.step(&clock).unwrap();
        assert!(ring.view().get("pattern").is_none());
        assert_eq!(bright(ring.pixels[0]), 0, "the light is off again");
        let _ = std::fs::remove_dir_all(root);
    }
}
