//! The ring's mark while a computer sees or hears through the lamp
//! (`light/mark`, `lamp_msgs::RingMark`; docs/control.md, "The camera").
//!
//! lamp-control asks for it while a computer streams the camera (later the
//! microphone), renewing it each second at most 3 s ahead, and once as a
//! blink for each snapshot. The ring draws one small steady dot, the top
//! pixel ([`MARK_PIXEL`]), dim white, over every frame it draws: a turn's
//! face, a resting frame, a skill's light. It is the only thing on the ring
//! that is never the lamp's own mood, so it never breathes or moves. It goes
//! when every claim has lapsed, so an asker that stops can't leave it lit.
//! Never over the shutdown's warning, and nothing at all on a dark ring.
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::RingMark;
use lamp_ring::{PIXEL_COUNT, Rgb};
use std::{io, path::Path, time::Duration};

/// The pixel the mark takes: the ring's first, at the top.
pub const MARK_PIXEL: usize = 0;
/// The mark: dim, white, steady; the ring's ceiling still applies.
pub const MARK: Rgb = Rgb::new(24, 24, 24);
/// A snapshot's blink: the dot for this long.
pub const BLINK_US: u64 = 600_000;
/// Read the bus at most this often.
const LOOK_EVERY_US: u64 = 50_000;
/// Marks taken in one look at most.
const MOST_AT_ONCE: usize = 16;
const SOURCE: &str = "voice_ring_mark";

/// The marks asked for, on the bus.
pub struct Marks {
    asked: Subscriber,
    clock_id: String,
    /// Each asker's mark, until when (on `clock_id`).
    held: Vec<(String, u64)>,
    looked_at_us: Option<u64>,
}

impl Marks {
    /// Reads `light/mark` on the bus under `root`, whose claims are on the
    /// monotonic clock `clock_id`.
    pub fn on(root: &Path, clock_id: &str) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let asked = bus.subscribe_typed(RingMark::TOPIC, SOURCE, RingMark::TYPE)?;
        Ok(Self {
            asked,
            clock_id: clock_id.to_owned(),
            held: Vec::new(),
            looked_at_us: None,
        })
    }

    /// Whether the mark is drawn at `now_us`.
    pub fn shown(&mut self, now_us: u64) -> bool {
        if self
            .looked_at_us
            .is_none_or(|at| now_us.saturating_sub(at) >= LOOK_EVERY_US)
        {
            self.looked_at_us = Some(now_us);
            for _ in 0..MOST_AT_ONCE {
                let Ok(Some(message)) = self.asked.recv(Duration::ZERO) else {
                    break;
                };
                if let Ok(mark) = message.parse::<RingMark>() {
                    self.take(&mark, now_us);
                }
            }
        }
        self.held.retain(|(_, until)| now_us < *until);
        !self.held.is_empty()
    }

    /// One mark asked for: held until its claim lapses, or for a blink's
    /// [`BLINK_US`]. A mark for another boot's clock, or too far ahead, is
    /// none.
    pub fn take(&mut self, mark: &RingMark, now_us: u64) {
        if mark.check(now_us).is_err() || !mark.claim.holds_at(&self.clock_id, now_us) {
            return;
        }
        let until = if mark.blink {
            now_us.saturating_add(BLINK_US).min(mark.claim.until_us)
        } else {
            mark.claim.until_us
        };
        let source = &mark.claim.source;
        match self.held.iter_mut().find(|(asker, _)| asker == source) {
            Some(held) => held.1 = held.1.max(until),
            None => self.held.push((source.clone(), until)),
        }
    }
}

/// `pixels` with the mark on them.
pub fn marked(mut pixels: [Rgb; PIXEL_COUNT]) -> [Rgb; PIXEL_COUNT] {
    pixels[MARK_PIXEL] = MARK;
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{Claim, MarkKind, Priority};

    fn mark(source: &str, until_us: u64, blink: bool) -> RingMark {
        RingMark {
            claim: Claim {
                source: source.to_owned(),
                priority: Priority::Skill,
                clock_id: "boot".to_owned(),
                until_us,
            },
            kind: MarkKind::Camera,
            blink,
        }
    }

    fn marks() -> Marks {
        let root = std::env::temp_dir().join(format!(
            "ring-mark-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Marks::on(&root, "boot").unwrap()
    }

    const S: u64 = 1_000_000;

    #[test]
    fn a_stream_holds_the_mark_while_renewed_and_it_goes_when_it_lapses() {
        let mut marks = marks();
        marks.take(&mark("control:c_1", 12 * S, false), 10 * S);
        assert!(marks.shown(11 * S));
        marks.take(&mark("control:c_1", 14 * S, false), 12 * S - 1);
        assert!(marks.shown(13 * S));
        assert!(!marks.shown(14 * S), "lapsed with no renewal");
    }

    #[test]
    fn a_snapshot_blinks_once() {
        let mut marks = marks();
        marks.take(&mark("control:c_1", 13 * S, true), 10 * S);
        assert!(marks.shown(10 * S + BLINK_US - 1));
        assert!(!marks.shown(10 * S + BLINK_US));
    }

    #[test]
    fn a_mark_too_far_ahead_or_on_another_clock_is_none() {
        let mut marks = marks();
        marks.take(
            &mark("control:c_1", 10 * S + RingMark::MOST_US + 1, false),
            10 * S,
        );
        assert!(!marks.shown(10 * S));
        let mut other = mark("control:c_1", 12 * S, false);
        other.claim.clock_id = "another boot".to_owned();
        marks.take(&other, 10 * S);
        assert!(!marks.shown(10 * S));
    }

    #[test]
    fn the_mark_takes_the_top_pixel_whatever_the_face() {
        let face = [Rgb::new(90, 40, 0); PIXEL_COUNT];
        let drawn = marked(face);
        assert_eq!(drawn[MARK_PIXEL], MARK);
        assert_eq!(drawn[1..], face[1..]);
    }
}
