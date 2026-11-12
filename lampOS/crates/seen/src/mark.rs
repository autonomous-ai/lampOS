//! The ring's mark before any frame (phase 0's second line, #6543): whenever
//! a frame would go out, this part asks the ring for its mark
//! (`light/mark`, `lamp_msgs::RingMark`, asker `lamp_seen`, `camera`, a
//! `blink`), and a frame may pass only once the ring said it drew it
//! (`light/mark_drawn`, `lamp_msgs::RingMarkDrawn`), as lamp-control's
//! camera does (docs/control.md, "The camera").
//!
//! [`Marked`] wraps the frames' [`Sink`] at its boundary, so it holds for
//! every sink, phase 1's [`crate::frames::Discard`] too:
//!
//! - **Before each request** ([`Sink::ready`]) the mark is asked for, and
//!   no frame is asked of the camera until the ring said it drew it after
//!   that ask. Not drawn within [`WAIT_US`]: still no frame, and the mark is
//!   asked again. It fails closed: no ring, no word from it, its ceiling at
//!   0 or another boot's clock all mean no frame.
//! - **Each frame** ([`Sink::frame`]) passes only if it was captured while
//!   the blink was surely lit: at or after the ring's first word that it
//!   drew the mark since that ask, and no later than the ask plus
//!   [`BLINK_US`] (the Reviewer-lead's P2 on #6615). The ring starts the
//!   blink when it takes the ask, which is after the ask and before its
//!   first draw, and holds it [`BLINK_US`], so the dot is lit through all
//!   of that window. The capture time is the frame's
//!   `dequeue_completed_at_us`; it, the ask and the ring's word are all on
//!   the boot's `CLOCK_MONOTONIC` (the ring's word only for this boot's
//!   clock id). The ring's word must also be within [`SHOWN_WITHIN_US`] of
//!   now. Any other frame is dropped (and wiped by the frames), and a
//!   blink that ends with no frame inside it is asked again. So that the
//!   camera's newest frame is not one from before the dot lit, the request
//!   takes no frame older than the time since the first draw
//!   ([`Sink::max_age_us`]), and none is asked until [`FRESH_US`] after
//!   it. Each frame passed takes the next its own ask: one mark for each
//!   frame.
//! - **Sending stops** ([`Sink::stopped`]: the gate closes, the link is
//!   lost, the grant changes, the process stops): the mark is let go, no
//!   longer asked for, and its ask forgotten, so a word from the ring that
//!   came before the next ask counts for nothing. The ring keeps a mark at
//!   most until its claim lapses: a blink is the dot for 0.6 s, the claim
//!   [`HOLD_US`] ahead at most, and the ring takes no earlier end, so
//!   letting go is not asking again.
use crate::frames::Sink;
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_camera::frames::FrameMeta;
use lamp_msgs::{Claim, MarkKind, Priority, RingMark, RingMarkDrawn};
use std::{io, time::Duration};

/// A mark's claim holds this far ahead (`RingMark::MOST_US` at most); the
/// ring shows a blink for 0.6 s of it.
pub const HOLD_US: u64 = 1_000_000;
/// The ring's blink: the dot for this long from when it takes the ask, as
/// the voice's `ring_mark::BLINK_US` (`crates/voice/src/ring_mark.rs`).
pub const BLINK_US: u64 = 600_000;
/// No frame is asked for until this long after the ring first drew the
/// mark: about one frame at the camera's 25 fps, so that a request that
/// takes no frame older than the time since the draw can be met.
pub const FRESH_US: u64 = 40_000;
/// A frame passes only while the ring said it drew the mark this recently,
/// as lamp-control's `camera::MARK_SHOWN_WITHIN`.
pub const SHOWN_WITHIN_US: u64 = 1_000_000;
/// An ask the ring has not said it drew within this: asked again, and
/// still no frame.
pub const WAIT_US: u64 = 1_000_000;

/// The ring, as this part asks it for the mark.
pub trait Ring {
    /// Ask for the mark at `now_us`: a camera blink, its claim
    /// [`HOLD_US`] ahead.
    fn ask(&mut self, now_us: u64) -> io::Result<()>;
    /// When the ring first said it drew this part's mark at or after the
    /// last ask, on the boot's monotonic clock; none if not yet. A word
    /// timed before the last ask may be kept or not: [`Marked`] takes none.
    fn drawn_at(&mut self) -> Option<u64>;
}

/// Saturating counts, for `diagnostics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Marks asked for.
    pub asked: u64,
    /// Asks the ring never said it drew within [`WAIT_US`].
    pub not_drawn: u64,
    /// Frames that came with no mark drawn for them, or captured outside
    /// the blink: dropped.
    pub refused: u64,
    /// Blinks drawn that ended with no frame inside them: asked again.
    pub missed: u64,
    /// Frames passed to the sink inside.
    pub passed: u64,
}

/// A sink that takes no frame unless the ring drew the mark for it.
pub struct Marked<R, S> {
    ring: R,
    inner: S,
    /// The ask a frame waits on, if any.
    asked_at_us: Option<u64>,
    counts: Counts,
}

impl<R: Ring, S: Sink> Marked<R, S> {
    pub fn new(ring: R, inner: S) -> Self {
        Self {
            ring,
            inner,
            asked_at_us: None,
            counts: Counts::default(),
        }
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// The ring, for tests.
    #[cfg(test)]
    pub(crate) fn ring_mut(&mut self) -> &mut R {
        &mut self.ring
    }

    /// Whether a mark is asked for and not let go.
    pub fn holds_mark(&self) -> bool {
        self.asked_at_us.is_some()
    }

    /// The ask waited on and when the ring first said it drew the mark at
    /// or after it, within [`SHOWN_WITHIN_US`] of `now_us`.
    fn drawn(&mut self, now_us: u64) -> Option<(u64, u64)> {
        let asked = self.asked_at_us?;
        self.ring
            .drawn_at()
            .filter(|drawn| *drawn >= asked && now_us.saturating_sub(*drawn) <= SHOWN_WITHIN_US)
            .map(|drawn| (asked, drawn))
    }
}

/// Whether a frame captured at `captured_us` was taken while the blink
/// asked at `asked_us`, first drawn at `drawn_us`, was surely lit.
fn inside_blink(asked_us: u64, drawn_us: u64, captured_us: u64) -> bool {
    drawn_us <= captured_us && captured_us <= asked_us.saturating_add(BLINK_US)
}

impl<R: Ring, S: Sink> Sink for Marked<R, S> {
    fn ready(&mut self, now_us: u64) -> bool {
        if let Some(asked) = self.asked_at_us {
            match self.drawn(now_us) {
                // A frame captured now would be inside the blink.
                Some((_, drawn)) if now_us <= asked.saturating_add(BLINK_US) => {
                    return now_us >= drawn.saturating_add(FRESH_US) && self.inner.ready(now_us);
                }
                Some(_) => self.counts.missed = self.counts.missed.saturating_add(1),
                None if now_us.saturating_sub(asked) < WAIT_US => return false,
                None => self.counts.not_drawn = self.counts.not_drawn.saturating_add(1),
            }
        }
        self.asked_at_us = match self.ring.ask(now_us) {
            Ok(()) => {
                self.counts.asked = self.counts.asked.saturating_add(1);
                Some(now_us)
            }
            Err(_) => None,
        };
        false
    }

    /// No older than the time since the ring first drew the mark: a frame
    /// the camera copies at or after `now_us` was then captured after the
    /// dot lit.
    fn max_age_us(&mut self, now_us: u64) -> u32 {
        let since_drawn = self
            .drawn(now_us)
            .map_or(0, |(_, drawn)| now_us.saturating_sub(drawn));
        let since_drawn = u32::try_from(since_drawn).unwrap_or(u32::MAX);
        self.inner.max_age_us(now_us).min(since_drawn)
    }

    fn frame(&mut self, now_us: u64, meta: &FrameMeta, jpeg: &[u8]) {
        let inside = self
            .drawn(now_us)
            .is_some_and(|(asked, drawn)| inside_blink(asked, drawn, meta.dequeue_completed_at_us));
        if !inside {
            self.counts.refused = self.counts.refused.saturating_add(1);
            return;
        }
        // The next frame takes its own mark.
        self.asked_at_us = None;
        self.counts.passed = self.counts.passed.saturating_add(1);
        self.inner.frame(now_us, meta, jpeg);
    }

    fn stopped(&mut self) {
        self.asked_at_us = None;
        self.inner.stopped();
    }
}

/// The ring on the bus: `light/mark` asked, `light/mark_drawn` read, only
/// the ring's own (`RingMarkDrawn::SOURCE`) for this part's mark
/// ([`crate::BUS_NAME`]) on this boot's clock.
pub struct BusRing {
    asks: Publisher,
    said: Subscriber,
    clock_id: String,
    /// The last ask's time, and the ring's first word at or after it.
    asked_at: Option<u64>,
    drawn_at: Option<u64>,
}

/// Messages read from `light/mark_drawn` in one look at most.
const DRAIN: usize = 64;

impl BusRing {
    /// On `bus`, on the boot's monotonic clock `clock_id`
    /// (`/proc/sys/kernel/random/boot_id`).
    pub fn on(bus: &Bus, clock_id: String) -> io::Result<Self> {
        Ok(Self {
            asks: bus.publisher_declared(
                RingMark::TOPIC,
                &RingMark::declaration(),
                crate::BUS_NAME,
            )?,
            said: bus.subscribe_typed(
                RingMarkDrawn::TOPIC,
                crate::BUS_NAME,
                RingMarkDrawn::TYPE,
            )?,
            clock_id,
            asked_at: None,
            drawn_at: None,
        })
    }
}

/// The mark this part asks for at `now_us` on `clock_id`.
pub fn mark(clock_id: &str, now_us: u64) -> RingMark {
    RingMark {
        claim: Claim {
            source: crate::BUS_NAME.to_owned(),
            priority: Priority::Skill,
            clock_id: clock_id.to_owned(),
            until_us: now_us.saturating_add(HOLD_US),
        },
        kind: MarkKind::Camera,
        blink: true,
    }
}

impl Ring for BusRing {
    fn ask(&mut self, now_us: u64) -> io::Result<()> {
        let mark = mark(&self.clock_id, now_us);
        mark.check(now_us)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.0))?;
        self.asks.publish_json(now_us, &mark)?;
        self.asked_at = Some(now_us);
        self.drawn_at = None;
        Ok(())
    }

    fn drawn_at(&mut self) -> Option<u64> {
        for _ in 0..DRAIN {
            match self.said.recv(Duration::ZERO) {
                Ok(Some(message)) => {
                    if message.header.source == RingMarkDrawn::SOURCE
                        && let Ok(drawn) = message.parse::<RingMarkDrawn>()
                        && drawn.source == crate::BUS_NAME
                        && drawn.clock_id == self.clock_id
                        && self.asked_at.is_some_and(|asked| drawn.at_us >= asked)
                    {
                        // The first word since the ask: when the dot lit.
                        self.drawn_at =
                            Some(self.drawn_at.map_or(drawn.at_us, |at| at.min(drawn.at_us)));
                    }
                }
                Ok(None) | Err(_) => break,
            }
        }
        let _ = self.said.notices();
        self.drawn_at
    }
}

#[cfg(test)]
pub(crate) mod tests;
