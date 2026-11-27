//! Posture's rolling window (#6543 phase 3): the last [`MIN_SAMPLES`]
//! samples, a minute of posture checks (the owner's call on #8377, 27 Nov:
//! "we should tell user when they slump for like 1m (two posture check
//! right?)"), each reduced at once to whether it was bad and which regions
//! scored high, by V1's rules (`pose.py` `_aggregate`, `hal/config.py`
//! `POSE_*`):
//!
//! - a sample is **bad** when its RULA `risk_level` is 2 or more, or any
//!   region's subscore on either side is high: [`NECK_HIGH_SUBSCORE`] (3,
//!   bent forward past 20°) or more for the neck, [`HIGH_SUBSCORE`] or more
//!   for any other (the owner, 27 Nov on #8377: a head bent to a phone is
//!   the neck's, never "no one region");
//! - the **region** is the one most often high among the bad samples, ties
//!   to the first of neck, trunk, upper arm, lower arm (V1's order; wrist is
//!   left out, the server fixes it at 1);
//! - a **partial** answer (`ergo.partial`, the hips hidden) is judged by the
//!   neck alone: the server fixes trunk and legs at 1 and caps `risk_level`
//!   low, and it measures the upper arm against a trunk it made up from the
//!   shoulder line, so neither `risk_level` nor any other region makes it
//!   bad, and its only region is neck. An answer with no `partial` field
//!   (an older server) is full, judged as before;
//! - each sample stands for the 30 s of its check ([`super::EVERY_US`]), so
//!   two samples are a full window: `window_s` is the oldest sample's age
//!   and those 30 s, at most [`WINDOW_US`]; a sample older than
//!   [`KEPT_US`] (a check or two missed, the server busy or no body found)
//!   no longer counts;
//! - under [`MIN_SAMPLES`] samples the window says nothing at all: no
//!   value, so a `bad_share` of 0 is said only over two samples (V1 said a
//!   share of 0 there; `humans/posture` holds back instead).
//!
//! Nothing of the frame or the answer is kept: a sample is its time, one
//! flag and four bits, and the window lives in memory only for one sitting
//! (reset when they stand up or stay away past [`crate::keep::KEEP_US`], a
//! forget, the privacy or sleep, or the opt-in goes off; whose face is in
//! view is never asked).
use crate::wire::Ergo;
use lamp_msgs::{HumanPosture, PostureRegion};
use std::collections::VecDeque;

/// The window: a minute (the owner, #8377; V1's production
/// `POSE_WINDOW_DURATION_S` was an hour, V2 had ten minutes).
pub const WINDOW_US: u64 = HumanPosture::SAID_WINDOW_S as u64 * 1_000_000;
/// Two checks, the window's samples: the newest two are kept, and fewer
/// say nothing (V1's `POSE_WINDOW_MIN_SAMPLES` was 3).
pub const MIN_SAMPLES: usize = 2;
/// The oldest a sample still counts: two of the slowest spacing between
/// checks (a hot board's one a minute, [`crate::heat`]), so one missed
/// check still pairs with the one before.
pub const KEPT_US: u64 = 120_000_000;
/// V1's `POSE_REGION_HIGH_SUBSCORE`: trunk, upper arm, lower arm, wrist.
pub const HIGH_SUBSCORE: i64 = 4;
/// The neck's: RULA's 3, bent forward more than 20°. Neck flexion alone
/// never reaches 4 (that takes extension or a twist), so at V1's 4 a head
/// bent to a phone read as no one region (06c1, 27 Nov 16:24: neck 25–37°,
/// every neck subscore 3; the owner's call on #8377).
pub const NECK_HIGH_SUBSCORE: i64 = 3;
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
/// Neck's bit in [`judge`]'s `high`: the first of [`REGIONS`].
const NECK_BIT: u8 = 1;

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

/// The subscore from which `region` counts as high.
fn high_subscore(region: &str) -> i64 {
    if region == "neck" {
        NECK_HIGH_SUBSCORE
    } else {
        HIGH_SUBSCORE
    }
}

/// What one answer's `ergo` comes to: whether it is bad, and which regions
/// scored high.
pub fn judge(ergo: &Ergo) -> (bool, u8) {
    let sides = [ergo.left.as_ref(), ergo.right.as_ref()];
    let high_on = |region: &str| {
        sides
            .iter()
            .any(|side| subscore(*side, region) >= high_subscore(region))
    };
    let mut high = 0u8;
    for (bit, (name, _)) in REGIONS.iter().enumerate() {
        if high_on(name) {
            high |= 1 << bit;
        }
    }
    if ergo.partial {
        let neck = high & NECK_BIT;
        return (neck != 0, neck);
    }
    let bad = ergo.risk_level >= BAD_RISK_LEVEL || high != 0 || high_on(WRIST);
    (bad, high)
}

/// Every one of [`REGIONS`] in `high` ([`judge`]'s bits), as
/// `humans/posture` writes them, joined by commas; empty when none is
/// high: for the journal.
pub fn high_names(high: u8) -> String {
    REGIONS
        .iter()
        .enumerate()
        .filter(|(bit, _)| high & (1 << bit) != 0)
        .map(|(_, (name, _))| *name)
        .collect::<Vec<_>>()
        .join(",")
}

/// `region` as `humans/posture` writes it: for the journal.
pub fn region_word(region: PostureRegion) -> &'static str {
    REGIONS
        .iter()
        .find(|(_, r)| *r == region)
        .map_or("general", |(name, _)| name)
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
        while self.samples.len() > MIN_SAMPLES
            || self
                .samples
                .front()
                .is_some_and(|sample| now_us.saturating_sub(sample.at_us) > KEPT_US)
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
        self.started_us?;
        self.prune(now_us);
        if self.samples.len() < MIN_SAMPLES {
            return None;
        }
        // Each sample stands for its check's 30 s: two 30 s apart are a
        // minute, the window full.
        let oldest = self.samples.front()?.at_us;
        let covered = now_us
            .saturating_sub(oldest)
            .saturating_add(super::EVERY_US);
        let window_s = (covered.min(WINDOW_US) / 1_000_000) as u32;
        let window_s = window_s.min(HumanPosture::LONGEST_WINDOW_S);
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
        let mut region = PostureRegion::General;
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
            partial: false,
        }
    }

    fn partial(left: serde_json::Value) -> Ergo {
        Ergo {
            partial: true,
            ..ergo(2, left, json!({}))
        }
    }

    fn good() -> Ergo {
        ergo(1, json!({"body_scores": {"neck": 1}}), json!({}))
    }

    fn neck() -> Ergo {
        ergo(1, json!({"body_scores": {"neck": 4}}), json!({}))
    }

    /// The owner's call (#8377, 27 Nov): a minute of bad posture is two
    /// checks, so two bad samples 30 s apart are a full window, all bad.
    #[test]
    fn two_bad_samples_30_s_apart_are_a_full_window() {
        let mut window = Window::default();
        window.add(0, &neck());
        assert_eq!(window.summary(0), None, "one check is not a minute");
        window.add(30 * S, &neck());
        let full = window.summary(30 * S).unwrap();
        assert_eq!(full.window_s, 60, "full: the skill's window_s above 59");
        assert_eq!(full.bad_share, 1.0);
        assert_eq!(full.region, PostureRegion::Neck);
        full.check().unwrap();
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
    fn a_partial_sample_is_judged_by_the_neck_alone() {
        // Neck 4 is bad, on the neck, with the capped risk level aside.
        assert_eq!(
            judge(&partial(
                json!({"body_scores": {"neck": 4, "upper_arm": 2}})
            )),
            (true, 0b0001)
        );
        // Neck 3 is bad too, on the neck (the owner, #8377); neck 2 with
        // risk 2 is not: the risk level does not count.
        assert_eq!(
            judge(&partial(json!({"body_scores": {"neck": 3}}))),
            (true, 0b0001)
        );
        assert_eq!(
            judge(&partial(json!({"body_scores": {"neck": 2}}))),
            (false, 0)
        );
        // Trunk, the arms or wrist high never make it bad.
        assert_eq!(
            judge(&partial(json!({"body_scores": {
                "trunk": 4, "upper_arm": 4, "lower_arm": 4, "wrist": 4
            }}))),
            (false, 0)
        );
        // Neck high with trunk high: bad, and the region is neck only.
        assert_eq!(
            judge(&partial(json!({"body_scores": {"neck": 5, "trunk": 4}}))),
            (true, 0b0001)
        );
        // The same full answer: the neck at 3 and the trunk at 4 both high.
        let full = ergo(
            2,
            json!({"body_scores": {"neck": 3, "trunk": 4}}),
            json!({}),
        );
        assert_eq!(judge(&full), (true, 0b0011));
    }

    /// 06c1, 27 Nov 16:24 (the owner's call on #8377): looking down at a
    /// phone, the neck bent 25–37°, the back straight, every neck subscore
    /// 3 and the RULA risk 2. The neck's, not "no one region": a neck of 3
    /// is high; any other region still needs 4.
    #[test]
    fn a_head_bent_to_a_phone_is_the_necks() {
        let phone = ergo(
            2,
            json!({"body_scores": {"neck": 3, "trunk": 1, "upper_arm": 3, "lower_arm": 3}}),
            json!({"body_scores": {"neck": 3, "trunk": 1, "upper_arm": 3, "lower_arm": 3}}),
        );
        assert_eq!(judge(&phone), (true, 0b0001));
        let mut window = Window::default();
        window.add(0, &phone);
        window.add(30 * S, &phone);
        let said = window.summary(30 * S).unwrap();
        assert_eq!((said.bad_share, said.region), (1.0, PostureRegion::Neck));
        // Neck 2, risk 2: bad overall, no one region high.
        let mild = ergo(
            2,
            json!({"body_scores": {"neck": 2, "trunk": 3}}),
            json!({}),
        );
        assert_eq!(judge(&mild), (true, 0));
    }

    #[test]
    fn a_partial_window_says_neck_or_nothing() {
        let mut window = Window::default();
        let slumped = partial(json!({"body_scores": {"neck": 4, "upper_arm": 4}}));
        let arms = partial(json!({"body_scores": {"upper_arm": 4, "trunk": 4}}));
        window.add(0, &arms);
        window.add(30 * S, &arms);
        let none = window.summary(30 * S).unwrap();
        assert_eq!(none.bad_share, 0.0);
        assert_eq!(none.region, PostureRegion::General);
        window.add(60 * S, &slumped);
        let neck = window.summary(60 * S).unwrap();
        assert_eq!(neck.bad_share, 0.5);
        assert_eq!(neck.region, PostureRegion::Neck);
    }

    #[test]
    fn nothing_is_said_before_the_sitting_nor_under_two_samples() {
        let mut window = Window::default();
        assert_eq!(window.summary(5 * S), None);
        window.begin(10 * S);
        assert_eq!(window.summary(10 * S), None, "no sample: nothing");
        // One bad sample is not yet a share, and no fresh 0 said for it.
        window.add(20 * S, &neck());
        assert_eq!(window.summary(20 * S), None);
        assert_eq!(window.summary(45 * S), None);
        window.add(50 * S, &good());
        let two = window.summary(50 * S).unwrap();
        assert_eq!(two.window_s, 60);
        assert_eq!(two.bad_share, 0.5);
        assert_eq!(two.region, PostureRegion::Neck);
        two.check().unwrap();
    }

    #[test]
    fn the_share_and_region_over_the_last_two_checks() {
        let mut window = Window::default();
        window.begin(0);
        // Checks 30 s apart: slumped (trunk) to 120 s, then sitting well.
        let trunk = ergo(1, json!({"body_scores": {"trunk": 5}}), json!({}));
        let well = good();
        let mut said = Vec::new();
        for index in 0..8u64 {
            let at = index * 30 * S;
            window.add(at, if index < 5 { &trunk } else { &well });
            said.push(
                window
                    .summary(at)
                    .map(|p| (p.window_s, p.bad_share, p.region)),
            );
            assert!(window.len() <= MIN_SAMPLES, "two kept at most");
        }
        let trunk_bad = Some((60, 1.0, PostureRegion::Trunk));
        assert_eq!(
            said,
            [
                None,
                trunk_bad,
                trunk_bad,
                trunk_bad,
                trunk_bad,
                Some((60, 0.5, PostureRegion::Trunk)),
                Some((60, 0.0, PostureRegion::General)),
                Some((60, 0.0, PostureRegion::General)),
            ]
        );
        // Between checks the window stays full; a check missed still pairs
        // with the one before, two missed do not.
        let last = 7 * 30 * S;
        assert_eq!(window.summary(last + 29 * S).unwrap().window_s, 60);
        assert_eq!(window.summary(last + 60 * S).unwrap().window_s, 60);
        assert_eq!(
            window.summary(last + 91 * S),
            None,
            "the older past KEPT_US"
        );
    }

    /// A hot board's one check a minute (`crate::heat`): two checks a
    /// minute apart are a full window too.
    #[test]
    fn a_check_a_minute_apart_still_fills_the_window() {
        let mut window = Window::default();
        window.add(0, &neck());
        window.add(60 * S, &neck());
        let full = window.summary(60 * S).unwrap();
        assert_eq!((full.window_s, full.bad_share), (60, 1.0));
        window.add(120 * S, &good());
        assert_eq!(window.summary(120 * S).unwrap().bad_share, 0.5);
    }

    #[test]
    fn a_tie_goes_to_the_first_region_in_v1s_order() {
        let mut window = Window::default();
        let upper = ergo(1, json!({"body_scores": {"upper_arm": 4}}), json!({}));
        window.add(0, &upper);
        window.add(30 * S, &neck());
        assert_eq!(window.summary(30 * S).unwrap().region, PostureRegion::Neck);
        window.add(60 * S, &upper);
        assert_eq!(
            window.summary(60 * S).unwrap().region,
            PostureRegion::Neck,
            "neck and upper arm once each: neck first"
        );
        window.add(90 * S, &upper);
        assert_eq!(
            window.summary(90 * S).unwrap().region,
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
