//! Posture's rolling window (#6543 phase 3): the samples of the last
//! [`WINDOW_US`], each reduced at once to whether it was bad and which
//! regions scored high, by V1's rules (`pose.py` `_aggregate`, `hal/config.py`
//! `POSE_*`):
//!
//! - a sample is **bad** when its RULA `risk_level` is 2 or more, or any
//!   region's subscore on either side is [`HIGH_SUBSCORE`] or more;
//! - the **region** is the one most often high among the bad samples, ties
//!   to the first of neck, trunk, upper arm, lower arm (V1's order; wrist is
//!   left out, the server fixes it at 1);
//! - under [`MIN_SAMPLES`] samples the window says nothing at all: no
//!   value, so a `bad_share` of 0 is said only over at least three samples
//!   (V1 said a share of 0 there; `humans/posture` holds back instead).
//!
//! Nothing of the frame or the answer is kept: a sample is its time, one
//! flag and four bits, and the window lives in memory only for one sitting
//! (reset when they leave, a stranger comes, or the opt-in goes off).
use crate::wire::Ergo;
use lamp_msgs::{HumanPosture, PostureRegion};
use std::collections::VecDeque;

/// V1's window, `POSE_WINDOW_DURATION_S`: the last ten minutes.
pub const WINDOW_US: u64 = 600_000_000;
/// V1's `POSE_WINDOW_MIN_SAMPLES`.
pub const MIN_SAMPLES: usize = 3;
/// V1's `POSE_REGION_HIGH_SUBSCORE`.
pub const HIGH_SUBSCORE: i64 = 4;
/// V1's bad `risk_level` (RULA score 3 or more).
pub const BAD_RISK_LEVEL: i64 = 2;

/// The regions said, in V1's order (its `_REGIONS` without `wrist`).
const REGIONS: [(&str, PostureRegion); 4] = [
    ("neck", PostureRegion::Neck),
    ("trunk", PostureRegion::Trunk),
    ("upper_arm", PostureRegion::UpperArm),
    ("lower_arm", PostureRegion::LowerArm),
];
/// V1 counts `wrist` toward a bad sample too (the server fixes it at 1, so
/// it never does in practice).
const WRIST: &str = "wrist";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    at_us: u64,
    bad: bool,
    /// Which of [`REGIONS`] scored high, a bit each.
    high: u8,
}

/// The window of one sitting.
#[derive(Clone, Debug, Default)]
pub struct Window {
    started_us: Option<u64>,
    samples: VecDeque<Sample>,
}

/// A side's subscore for `region`, as V1's `int(body.get(region, 0) or 0)`:
/// a number truncated, anything else 0.
fn subscore(side: Option<&serde_json::Value>, region: &str) -> i64 {
    side.and_then(|side| side.get("body_scores"))
        .and_then(|scores| scores.get(region))
        .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
        .unwrap_or(0)
}

/// What one answer's `ergo` comes to: whether it is bad, and which regions
/// scored high.
pub fn judge(ergo: &Ergo) -> (bool, u8) {
    let sides = [ergo.left.as_ref(), ergo.right.as_ref()];
    let high_on = |region: &str| {
        sides
            .iter()
            .any(|side| subscore(*side, region) >= HIGH_SUBSCORE)
    };
    let mut high = 0u8;
    for (bit, (name, _)) in REGIONS.iter().enumerate() {
        if high_on(name) {
            high |= 1 << bit;
        }
    }
    let bad = ergo.risk_level >= BAD_RISK_LEVEL || high != 0 || high_on(WRIST);
    (bad, high)
}

impl Window {
    /// Whether a sitting is being watched: begun and not reset.
    pub fn begun(&self) -> bool {
        self.started_us.is_some()
    }

    /// The sitting begins at `now_us`, if it has not.
    pub fn begin(&mut self, now_us: u64) {
        self.started_us.get_or_insert(now_us);
    }

    /// Forget the sitting: every sample and its start.
    pub fn reset(&mut self) {
        self.samples.clear();
        self.started_us = None;
    }

    /// How many samples are in the window now.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    fn prune(&mut self, now_us: u64) {
        while self
            .samples
            .front()
            .is_some_and(|sample| now_us.saturating_sub(sample.at_us) > WINDOW_US)
        {
            self.samples.pop_front();
        }
    }

    /// One answer's score at `at_us`. The window begins if it had not.
    pub fn add(&mut self, at_us: u64, ergo: &Ergo) {
        self.begin(at_us);
        let (bad, high) = judge(ergo);
        self.samples.push_back(Sample { at_us, bad, high });
        self.prune(at_us);
    }

    /// The window at `now_us`, as `humans/posture` says it; none before the
    /// sitting begins or while it holds fewer than [`MIN_SAMPLES`] samples.
    pub fn summary(&mut self, now_us: u64) -> Option<HumanPosture> {
        let started = self.started_us?;
        self.prune(now_us);
        let window_s = (now_us.saturating_sub(started).min(WINDOW_US) / 1_000_000) as u32;
        let window_s = window_s.min(HumanPosture::LONGEST_WINDOW_S);
        if self.samples.len() < MIN_SAMPLES {
            return None;
        }
        let bad: Vec<&Sample> = self.samples.iter().filter(|sample| sample.bad).collect();
        let bad_share = (bad.len() as f32 / self.samples.len() as f32).clamp(0.0, 1.0);
        let mut counts = [0usize; REGIONS.len()];
        for sample in &bad {
            for (bit, count) in counts.iter_mut().enumerate() {
                if sample.high & (1 << bit) != 0 {
                    *count += 1;
                }
            }
        }
        // The most often high; the first of them on a tie, as Python's `max`.
        let mut region = PostureRegion::Nothing;
        let mut most = 0;
        for (index, count) in counts.iter().enumerate() {
            if *count > most {
                most = *count;
                region = REGIONS[index].1;
            }
        }
        Some(HumanPosture {
            window_s,
            bad_share,
            region,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const S: u64 = 1_000_000;

    fn ergo(risk_level: i64, left: serde_json::Value, right: serde_json::Value) -> Ergo {
        Ergo {
            score: 0,
            risk_level,
            left: Some(left),
            right: Some(right),
        }
    }

    fn good() -> Ergo {
        ergo(1, json!({"body_scores": {"neck": 1}}), json!({}))
    }

    fn neck() -> Ergo {
        ergo(1, json!({"body_scores": {"neck": 4}}), json!({}))
    }

    #[test]
    fn a_sample_is_bad_as_v1_judged_it() {
        assert_eq!(judge(&good()), (false, 0));
        // risk 2 alone is bad, with no region.
        assert_eq!(judge(&ergo(2, json!({}), json!({}))), (true, 0));
        // A high subscore on either side; a float truncated as int() does.
        assert_eq!(judge(&neck()), (true, 0b0001));
        assert_eq!(
            judge(&ergo(
                1,
                json!({}),
                json!({"body_scores": {"lower_arm": 4.7, "trunk": 3.9}})
            )),
            (true, 0b1000)
        );
        // Wrist counts toward bad, never as the region.
        assert_eq!(
            judge(&ergo(1, json!({"body_scores": {"wrist": 5}}), json!({}))),
            (true, 0)
        );
        // Anything not a number is 0, as V1 reads it.
        assert_eq!(
            judge(&ergo(1, json!({"body_scores": {"neck": "x"}}), json!(null))),
            (false, 0)
        );
        let none = Ergo::default();
        assert_eq!(judge(&none), (false, 0));
    }

    #[test]
    fn nothing_is_said_before_the_sitting_nor_under_three_samples() {
        let mut window = Window::default();
        assert_eq!(window.summary(5 * S), None);
        window.begin(10 * S);
        assert_eq!(window.summary(10 * S), None, "no sample: nothing");
        // Two bad samples are not yet a share: V1's three at least, and
        // no fresh 0 said for them either.
        window.add(20 * S, &neck());
        assert_eq!(window.summary(20 * S), None);
        window.add(50 * S, &neck());
        assert_eq!(window.summary(55 * S), None);
        window.add(80 * S, &good());
        let three = window.summary(80 * S).unwrap();
        assert_eq!(three.window_s, 70);
        assert!((three.bad_share - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(three.region, PostureRegion::Neck);
        three.check().unwrap();
    }

    #[test]
    fn the_share_and_region_over_the_last_ten_minutes() {
        let mut window = Window::default();
        window.begin(0);
        // 20 samples 30 s apart: the first 10 slumped (trunk), then sitting well.
        let trunk = ergo(1, json!({"body_scores": {"trunk": 5}}), json!({}));
        let well = good();
        for index in 0..20u64 {
            let at = index * 30 * S;
            window.add(at, if index < 10 { &trunk } else { &well });
        }
        let at = 19 * 30 * S;
        let all = window.summary(at).unwrap();
        assert_eq!(all.window_s, 570);
        assert_eq!(window.len(), 20);
        assert!((all.bad_share - 0.5).abs() < 1e-6);
        assert_eq!(all.region, PostureRegion::Trunk);
        // Five minutes on: the slumped half has aged out of the ten minutes.
        let later = at + 301 * S;
        let well = window.summary(later).unwrap();
        assert_eq!(well.window_s, 600, "held at the window's length");
        assert_eq!(window.len(), 10);
        assert_eq!(well.bad_share, 0.0);
        assert_eq!(well.region, PostureRegion::Nothing);
    }

    #[test]
    fn a_tie_goes_to_the_first_region_in_v1s_order() {
        let mut window = Window::default();
        let upper = ergo(1, json!({"body_scores": {"upper_arm": 4}}), json!({}));
        window.add(0, &upper);
        window.add(S, &neck());
        window.add(2 * S, &good());
        assert_eq!(window.summary(2 * S).unwrap().region, PostureRegion::Neck);
        window.add(3 * S, &upper);
        assert_eq!(
            window.summary(3 * S).unwrap().region,
            PostureRegion::UpperArm
        );
    }

    #[test]
    fn a_reset_forgets_the_sitting() {
        let mut window = Window::default();
        for index in 0..5 {
            window.add(index * S, &neck());
        }
        assert!(window.begun());
        window.reset();
        assert!(!window.begun() && window.is_empty());
        assert_eq!(window.summary(10 * S), None);
    }
}
