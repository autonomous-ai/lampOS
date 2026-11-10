//! The ring's stand-in: the light the lamp draws, from the claims on
//! `light/request`, and the camera's and microphone's marks it must show
//! (`light/mark`) and say it showed (`light/mark_drawn`). The 32 pixels are
//! kept for a viewer. No SPI, no timing of a real WS2812 chain.
use lamp_bus::{Bus, Kind, Publisher, Subscriber};
use lamp_msgs::{Light, LightRequest, MarkKind, RingMark, RingMarkDrawn};
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
    /// When a camera mark and a microphone mark were last asked, on the monotonic clock.
    pub camera_marked_us: u64,
    pub microphone_marked_us: u64,
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
            camera_marked_us: 0,
            microphone_marked_us: 0,
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
                self.held = Some(Held { request });
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
        self.pixels = match self.held.as_ref().and_then(|h| h.request.pixels.clone()) {
            Some(each) => {
                let mut all = [[0u8; 3]; PIXELS];
                for (slot, pixel) in all.iter_mut().zip(each) {
                    *slot = pixel;
                }
                all
            }
            None => [colour_of(&light); PIXELS],
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

    /// The ring as a viewer reads it.
    pub fn view(&self) -> Value {
        let shown = self.shown.clone().unwrap_or_else(rest);
        json!({
            "on": shown.on, "colour": shown.colour, "level": shown.level,
            "pixels": self.pixels.iter().map(|p| format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2])).collect::<Vec<_>>(),
        })
    }
}

/// The colour a light shows on every pixel, at its level (1 to 5).
fn colour_of(light: &Light) -> [u8; 3] {
    if !light.on {
        return [0; 3];
    }
    let tint = Light::TINTS
        .iter()
        .find(|(name, _)| *name == light.colour)
        .map(|(_, t)| *t)
        .unwrap_or([1.0, 1.0, 1.0]);
    let scale = f32::from(light.level.clamp(1, 5)) / 5.0 * 120.0;
    [0, 1, 2].map(|k| (tint[k] * scale).round() as u8)
}
