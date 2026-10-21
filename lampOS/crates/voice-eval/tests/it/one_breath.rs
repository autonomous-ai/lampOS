//! The lamp breathes as one (#1670, `docs/creature.md` "one creature"): the
//! ring's resting breath (`lamp-voice`) and the head's (`lamp-expression`'s
//! idle layer) are two processes, held to one breath here, in a crate that
//! links both. With Mood's breath they take the same phase; without it both
//! breathe the plain 5.2 s on the boot clock, as they always did.
use lamp_expression::{Breath, RING_BREATH_US};
use lamp_msgs::{BodyOffsets, MOOD_BREATH_US};
use lamp_voice::ring_face::resting_breath;

/// The head's breath at its plain period, the ring's.
fn head() -> Breath {
    Breath {
        period_us: RING_BREATH_US,
        inhale: BodyOffsets::ZERO,
    }
}

#[test]
fn the_ring_and_the_head_take_the_same_breath_with_mood_and_without() {
    assert_eq!(RING_BREATH_US, MOOD_BREATH_US);
    let head = head();
    // Over an hour's span, across the hour's turn, a moment every 37 ms.
    for step in 0..120_000u64 {
        let now_us = 3_590_000_000 + step * 37_000;
        // Without a mood: the boot clock, both.
        let (ring, body) = (resting_breath(None, now_us), head.share_at(now_us, None));
        assert!(
            (f64::from(ring) - body).abs() < 1e-3,
            "{now_us}: {ring} {body}"
        );
        // With Mood's breath, at any phase and pace: the same, whatever the
        // clock says.
        let phase = (step as f64 * 0.013).fract();
        let ring = resting_breath(Some((phase, 0.7)), now_us);
        let body = head.share_at(now_us, Some(phase));
        assert!(
            (f64::from(ring) - body).abs() < 1e-5,
            "{phase}: {ring} {body}"
        );
    }
}
