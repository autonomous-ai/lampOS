use super::*;
use std::collections::VecDeque;

/// The ring, as told: each ask kept, and the word it says it drew.
#[derive(Default)]
pub(crate) struct FakeRing {
    pub asks: Vec<u64>,
    pub drawn: Option<u64>,
    pub broken: bool,
}

impl FakeRing {
    /// The ring draws the last ask's mark at `at_us`.
    pub fn draws(&mut self, at_us: u64) {
        self.drawn = Some(at_us);
    }
}

impl Ring for FakeRing {
    fn ask(&mut self, now_us: u64) -> io::Result<()> {
        if self.broken {
            return Err(io::Error::other("no bus"));
        }
        self.asks.push(now_us);
        Ok(())
    }
    fn drawn_at(&mut self) -> Option<u64> {
        self.drawn
    }
}

/// A sink that counts what it was lent, and whether it was told to stop.
#[derive(Default)]
pub(crate) struct Inner {
    pub frames: VecDeque<u64>,
    pub stopped: u64,
}

impl Sink for Inner {
    fn frame(&mut self, now_us: u64, _: &FrameMeta, _: &[u8]) {
        self.frames.push_back(now_us);
    }
    fn stopped(&mut self) {
        self.stopped += 1;
    }
}

const MS: u64 = 1_000;

#[test]
fn not_ready_until_the_ring_says_it_drew_the_mark() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    assert!(!marked.ready(0), "asked, not drawn");
    assert_eq!(marked.ring.asks, [0]);
    assert!(marked.holds_mark());
    for now in [100 * MS, 300 * MS, 500 * MS] {
        assert!(!marked.ready(now), "no word from the ring: no frame");
    }
    assert_eq!(marked.ring.asks.len(), 1, "one ask while it waits");
    marked.ring.draws(500 * MS);
    assert!(
        !marked.ready(500 * MS),
        "a frame's time after the draw first"
    );
    assert!(marked.ready(500 * MS + FRESH_US));
}

/// A frame the camera captured at `at_us`.
fn captured(at_us: u64) -> FrameMeta {
    let mut meta = crate::frames::tests::meta(crate::frames::tests::grant(1));
    meta.dequeue_completed_at_us = at_us;
    meta
}

const S: u64 = 1_000_000;

#[test]
fn a_frame_captured_before_the_draw_is_dropped() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    marked.ready(S);
    marked.ring.draws(S + 20 * MS);
    // The camera's frame from 0.4 s before the dot lit, come after it.
    marked.frame(S + 50 * MS, &captured(S + 20 * MS - 400 * MS), b"jpeg");
    assert!(marked.inner.frames.is_empty());
    assert_eq!(marked.counts().refused, 1);
    assert!(
        marked.holds_mark(),
        "the ask still waits for a frame inside it"
    );
    // One captured inside the blink passes.
    marked.frame(S + 150 * MS, &captured(S + 100 * MS), b"jpeg");
    assert_eq!(marked.inner.frames.len(), 1);
}

#[test]
fn a_frame_captured_after_the_blink_ended_is_dropped() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    marked.ready(S);
    marked.ring.draws(S);
    // 0.7 s after the draw: the 0.6 s blink is off, though the ring's word
    // is still within a second.
    marked.frame(S + 750 * MS, &captured(S + 700 * MS), b"jpeg");
    assert!(marked.inner.frames.is_empty());
    assert_eq!(marked.counts().refused, 1);
    // The blink's last moment is inside it.
    marked.frame(S + 650 * MS, &captured(S + BLINK_US), b"jpeg");
    assert_eq!(marked.inner.frames.len(), 1);
}

#[test]
fn a_late_draw_does_not_stretch_the_blink() {
    // The ring took the ask by its first draw, so the dot is surely lit
    // only until the ask plus the blink, whenever it was first drawn.
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    marked.ready(S);
    marked.ring.draws(S + 300 * MS);
    marked.frame(S + 750 * MS, &captured(S + 700 * MS), b"jpeg");
    assert!(marked.inner.frames.is_empty());
    marked.frame(S + 600 * MS, &captured(S + 500 * MS), b"jpeg");
    assert_eq!(marked.inner.frames.len(), 1);
}

#[test]
fn a_blink_that_ends_with_no_frame_is_asked_again() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    marked.ready(0);
    marked.ring.draws(10 * MS);
    assert!(marked.ready(BLINK_US), "a frame captured now is inside it");
    assert!(!marked.ready(BLINK_US + 1), "over: asked again");
    assert_eq!(marked.ring.asks, [0, BLINK_US + 1]);
    assert_eq!(marked.counts().missed, 1);
}

#[test]
fn an_ask_never_drawn_is_asked_again_and_still_no_frame() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    assert!(!marked.ready(0));
    assert!(!marked.ready(WAIT_US), "waited out: asked again");
    assert_eq!(marked.ring.asks, [0, WAIT_US]);
    assert_eq!(marked.counts().not_drawn, 1);
    // A word for the first ask only, before the second: not this ask's.
    marked.ring.draws(WAIT_US - 1);
    assert!(!marked.ready(WAIT_US + 1));
}

#[test]
fn no_ring_means_no_frame() {
    let mut marked = Marked::new(
        FakeRing {
            broken: true,
            ..FakeRing::default()
        },
        Inner::default(),
    );
    for now in [0, WAIT_US, 5 * WAIT_US] {
        assert!(!marked.ready(now));
    }
    assert!(!marked.holds_mark());
    assert_eq!(marked.counts().asked, 0);
}

#[test]
fn a_frame_with_no_mark_drawn_for_it_is_dropped() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    // Never asked.
    marked.frame(0, &captured(0), b"jpeg");
    // Asked, not drawn.
    marked.ready(MS);
    marked.frame(2 * MS, &captured(2 * MS), b"jpeg");
    // Drawn, then the word too long ago.
    marked.ring.draws(3 * MS);
    marked.frame(3 * MS + SHOWN_WITHIN_US + 1, &captured(4 * MS), b"jpeg");
    assert!(marked.inner.frames.is_empty());
    assert_eq!(marked.counts().refused, 3);
    // Drawn, fresh and captured inside the blink: it passes, and the next
    // frame takes its own mark.
    marked.frame(5 * MS, &captured(4 * MS), b"jpeg");
    assert_eq!(marked.inner.frames.len(), 1);
    assert!(!marked.holds_mark());
    marked.frame(6 * MS, &captured(5 * MS), b"jpeg");
    assert_eq!(marked.inner.frames.len(), 1, "one mark, one frame");
    assert!(!marked.ready(7 * MS), "asked anew");
    assert_eq!(marked.ring.asks.len(), 2);
}

#[test]
fn stopping_lets_the_mark_go_and_an_old_word_counts_for_nothing() {
    let mut marked = Marked::new(FakeRing::default(), Inner::default());
    marked.ready(0);
    marked.ring.draws(MS);
    assert!(marked.ready(MS + FRESH_US));
    marked.stopped();
    assert!(!marked.holds_mark(), "let go");
    assert_eq!(marked.inner.stopped, 1);
    // Sending again: the ring's word from before is not this ask's.
    assert!(!marked.ready(MS + FRESH_US + 1));
    assert_eq!(marked.ring.asks, [0, MS + FRESH_US + 1]);
    marked.stopped();
    marked.stopped();
    assert!(!marked.holds_mark(), "stopped again harmlessly");
}

#[test]
fn the_mark_asked_for_is_one_the_ring_takes() {
    let now = 50 * 1_000_000;
    let asked = mark("boot", now);
    assert_eq!(asked.claim.source, crate::BUS_NAME);
    assert_eq!(asked.kind, MarkKind::Camera);
    assert!(asked.blink);
    assert!(asked.check(now).is_ok());
    assert!(asked.claim.holds_at("boot", now));
    assert!(!asked.claim.holds_at("another boot", now));
    const { assert!(HOLD_US <= RingMark::MOST_US) };
    // With no boot to name, the ring would take none: not asked.
    assert!(mark("", now).check(now).is_err());
    let json = serde_json::to_value(&asked).unwrap();
    assert_eq!(json["kind"], "camera");
    assert_eq!(json["blink"], true);
}

/// On a bus of its own: the mark asked on `light/mark`, and only the
/// ring's word for this part's mark on this boot taken from
/// `light/mark_drawn`.
#[test]
fn on_the_bus() {
    // Short: a Unix socket's address holds 103 bytes on macOS.
    let root = std::path::PathBuf::from(format!("/tmp/lsm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap();
    let mut asks = bus
        .subscribe_typed(RingMark::TOPIC, "test", RingMark::TYPE)
        .unwrap();
    let mut ring = BusRing::on(&bus, "boot".into()).unwrap();
    ring.ask(10 * MS).unwrap();
    let asked = asks.recv(Duration::ZERO).unwrap().unwrap();
    assert_eq!(asked.header.source, crate::BUS_NAME);
    assert_eq!(asked.parse::<RingMark>().unwrap(), mark("boot", 10 * MS));
    assert!(BusRing::on(&bus, String::new()).unwrap().ask(MS).is_err());
    let drawn = |source: &str, clock_id: &str, at_us| RingMarkDrawn {
        source: source.into(),
        clock_id: clock_id.into(),
        at_us,
    };
    let mut the_ring = bus
        .publisher_declared(
            RingMarkDrawn::TOPIC,
            &RingMarkDrawn::declaration(),
            RingMarkDrawn::SOURCE,
        )
        .unwrap();
    let mut a_stray = bus
        .publisher_declared(RingMarkDrawn::TOPIC, &RingMarkDrawn::declaration(), "stray")
        .unwrap();
    assert_eq!(ring.drawn_at(), None);
    a_stray
        .publish_json(1, &drawn(crate::BUS_NAME, "boot", 20 * MS))
        .unwrap();
    the_ring
        .publish_json(1, &drawn("control:c_1", "boot", 21 * MS))
        .unwrap();
    the_ring
        .publish_json(1, &drawn(crate::BUS_NAME, "other", 22 * MS))
        .unwrap();
    assert_eq!(
        ring.drawn_at(),
        None,
        "none is the ring's word for this mark"
    );
    the_ring
        .publish_json(1, &drawn(crate::BUS_NAME, "boot", 15 * MS))
        .unwrap();
    assert_eq!(ring.drawn_at(), Some(15 * MS));
    // The first word since the ask is when the dot lit: a later one, or
    // one from before the ask, does not move it.
    the_ring
        .publish_json(1, &drawn(crate::BUS_NAME, "boot", 30 * MS))
        .unwrap();
    the_ring
        .publish_json(1, &drawn(crate::BUS_NAME, "boot", 5 * MS))
        .unwrap();
    assert_eq!(ring.drawn_at(), Some(15 * MS));
    // A new ask forgets it.
    ring.ask(40 * MS).unwrap();
    assert_eq!(ring.drawn_at(), None);
    the_ring
        .publish_json(1, &drawn(crate::BUS_NAME, "boot", 45 * MS))
        .unwrap();
    assert_eq!(ring.drawn_at(), Some(45 * MS));
    let _ = std::fs::remove_dir_all(root);
}

/// A sink that wants no frame now.
struct Resting;

impl Sink for Resting {
    fn ready(&mut self, _: u64) -> bool {
        false
    }
    fn frame(&mut self, _: u64, _: &FrameMeta, _: &[u8]) {}
}

#[test]
fn no_mark_is_asked_while_the_sink_wants_no_frame() {
    let mut marked = Marked::new(FakeRing::default(), Resting);
    for now in [0, WAIT_US, 10 * WAIT_US] {
        assert!(!marked.ready(now));
    }
    assert!(marked.ring_mut().asks.is_empty(), "the ring never blinks");
    assert_eq!(marked.counts().asked, 0);
}
