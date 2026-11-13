//! What a burst's labels say, and this sitting's timers (#6543 phase 2).
//!
//! - [`read`] turns one burst's answers into what was seen, as coarse kinds
//!   (drinking, eating, a break: V1's `drink`, `eat` and `break` groups),
//!   never the raw label. A false "drank" silences sip-of-water for 90 min,
//!   so a kind counts only when one answer is sure of it (at least
//!   [`SURE`]), or when at least [`RUN`] of the burst's last [`LAST`]
//!   answers have it at all (the server sends only what clears V1's 0.3).
//! - [`Sitting`] keeps the sitting's timers, published as
//!   `humans/doing` (`lamp_msgs::HumanDoing`): every timer always present,
//!   counting from the desk arrival when nothing has been seen, so a
//!   skill's `[[while]] above` runs on the clock as it did without this
//!   part. It begins again on `room/stood_up`, and on the gate closing for
//!   nobody or a stranger: no history is kept.
use crate::wire::{ActionAnswer, group};
use lamp_msgs::{Doing, DoingSeen, HumanDoing};

/// One answer this sure of a kind is enough.
pub const SURE: f64 = 0.5;
/// Otherwise, this many of the last [`LAST`] answers must have it.
pub const RUN: usize = 2;
pub const LAST: usize = 4;

/// The kinds the skills read, with V1's groups.
const KINDS: [(Doing, &str); 3] = [
    (Doing::Drinking, "drink"),
    (Doing::Eating, "eat"),
    (Doing::Break, "break"),
];

/// What one burst saw: each kind with its best confidence, the surest
/// first.
pub fn read(answers: &[ActionAnswer]) -> Vec<DoingSeen> {
    let best = |answer: &ActionAnswer, wanted: &str| {
        answer
            .detected_classes
            .iter()
            .filter(|class| group(&class.class_name) == Some(wanted))
            .map(|class| class.conf)
            .filter(|conf| conf.is_finite())
            .fold(None, |best: Option<f64>, conf| {
                Some(best.map_or(conf, |b| b.max(conf)))
            })
    };
    let tail = &answers[answers.len().saturating_sub(LAST)..];
    let mut seen: Vec<DoingSeen> = KINDS
        .iter()
        .filter_map(|(kind, wanted)| {
            let most = answers
                .iter()
                .filter_map(|answer| best(answer, wanted))
                .fold(None, |most: Option<f64>, conf| {
                    Some(most.map_or(conf, |m| m.max(conf)))
                })?;
            let run = tail
                .iter()
                .filter(|answer| best(answer, wanted).is_some())
                .count();
            (most >= SURE || run >= RUN).then(|| DoingSeen {
                what: *kind,
                confidence: most.clamp(0.0, 1.0) as f32,
            })
        })
        .collect();
    seen.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    seen
}

/// This sitting's timers, on the caller's monotonic microseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sitting {
    arrived_us: u64,
    drank_us: Option<u64>,
    ate_us: Option<u64>,
    break_us: Option<u64>,
    last: Doing,
    confidence: f32,
    /// The kinds the last burst that answered saw, so a kind seen in two
    /// bursts in a row is one sighting.
    last_burst: [bool; 3],
}

fn index(kind: Doing) -> Option<usize> {
    KINDS.iter().position(|(k, _)| *k == kind)
}

fn seconds(from_us: u64, now_us: u64) -> u32 {
    u32::try_from(now_us.saturating_sub(from_us) / 1_000_000).unwrap_or(u32::MAX)
}

impl Sitting {
    /// A sitting that began at `arrived_us`.
    pub fn new(arrived_us: u64) -> Self {
        Self {
            arrived_us,
            drank_us: None,
            ate_us: None,
            break_us: None,
            last: Doing::Nothing,
            confidence: 0.0,
            last_burst: [false; 3],
        }
    }

    /// Take one burst's sightings at `now_us`; returns those newly seen
    /// (not seen in the burst before), for `humans/doing_seen`.
    pub fn saw(&mut self, seen: &[DoingSeen], now_us: u64) -> Vec<DoingSeen> {
        let mut this_burst = [false; 3];
        let mut newly = Vec::new();
        for sighting in seen {
            let Some(at) = index(sighting.what) else {
                continue;
            };
            this_burst[at] = true;
            *match sighting.what {
                Doing::Drinking => &mut self.drank_us,
                Doing::Eating => &mut self.ate_us,
                _ => &mut self.break_us,
            } = Some(now_us);
            if !self.last_burst[at] {
                newly.push(*sighting);
            }
        }
        // The surest of this burst is the last thing seen.
        if let Some(surest) = seen.iter().find(|s| index(s.what).is_some()) {
            self.last = surest.what;
            self.confidence = surest.confidence.clamp(0.0, 1.0);
        }
        self.last_burst = this_burst;
        newly
    }

    /// The timers at `now_us`: each from its last sighting, else from the
    /// arrival.
    pub fn doing(&self, now_us: u64) -> HumanDoing {
        let since = |seen: Option<u64>| seconds(seen.unwrap_or(self.arrived_us), now_us);
        HumanDoing {
            drank_s_ago: since(self.drank_us),
            ate_s_ago: since(self.ate_us),
            break_s_ago: since(self.break_us),
            last: self.last,
            confidence: self.confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Detected;

    const S: u64 = 1_000_000;

    fn answer(classes: &[(&str, f64)]) -> ActionAnswer {
        ActionAnswer {
            detected_classes: classes
                .iter()
                .map(|(name, conf)| Detected {
                    class_name: (*name).into(),
                    conf: *conf,
                })
                .collect(),
        }
    }

    #[test]
    fn a_kind_counts_when_sure_once_or_in_a_run() {
        // One sure answer.
        let seen = read(&[answer(&[]), answer(&[("drinking beer", 0.62)])]);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].what, Doing::Drinking);
        assert!((seen[0].confidence - 0.62).abs() < 1e-6);
        // One unsure answer: nothing.
        assert!(read(&[answer(&[("dining", 0.35)]), answer(&[])]).is_empty());
        // Unsure, but in two of the last four.
        let seen = read(&[
            answer(&[]),
            answer(&[("eating cake", 0.31)]),
            answer(&[]),
            answer(&[("dining", 0.4), ("using computer", 0.9)]),
        ]);
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].what, Doing::Eating);
        // Two unsure, but not both in the last four.
        let mut answers = vec![answer(&[("stretching arm", 0.33)])];
        answers.extend((0..3).map(|_| answer(&[])));
        answers.push(answer(&[("stretching leg", 0.33)]));
        assert!(read(&answers).is_empty());
        // Labels outside drink, eat and break never count; the surest first.
        let seen = read(&[answer(&[
            ("yawning", 0.99),
            ("stretching arm", 0.55),
            ("drinking", 0.8),
        ])]);
        let kinds: Vec<_> = seen.iter().map(|s| s.what).collect();
        assert_eq!(kinds, [Doing::Drinking, Doing::Break]);
    }

    #[test]
    fn eating_chips_is_eating() {
        let seen = read(&[answer(&[("eating chips", 0.58)])]);
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].what, Doing::Eating);
    }

    #[test]
    fn the_timers_count_from_the_arrival_until_something_is_seen() {
        let sitting = Sitting::new(100 * S);
        let doing = sitting.doing(1_000 * S);
        assert_eq!(
            (doing.drank_s_ago, doing.ate_s_ago, doing.break_s_ago),
            (900, 900, 900)
        );
        assert_eq!(doing.last, Doing::Nothing);
        assert_eq!(doing.confidence, 0.0);
        doing.check().unwrap();
        // Before the arrival (a clock read early): 0, never wrapped.
        assert_eq!(sitting.doing(0).drank_s_ago, 0);
    }

    #[test]
    fn a_sighting_restarts_its_timer_and_is_new_once() {
        let mut sitting = Sitting::new(0);
        let drink = DoingSeen {
            what: Doing::Drinking,
            confidence: 0.7,
        };
        let newly = sitting.saw(&[drink], 600 * S);
        assert_eq!(newly, [drink]);
        let doing = sitting.doing(660 * S);
        assert_eq!(doing.drank_s_ago, 60);
        assert_eq!(doing.ate_s_ago, 660, "eating not seen: from the arrival");
        assert_eq!(doing.last, Doing::Drinking);
        assert!((doing.confidence - 0.7).abs() < 1e-6);
        doing.check().unwrap();
        // Seen in the next burst too: the timer restarts, not a new sighting.
        assert!(sitting.saw(&[drink], 660 * S).is_empty());
        assert_eq!(sitting.doing(660 * S).drank_s_ago, 0);
        // A burst without it, then it again: new again.
        assert!(sitting.saw(&[], 720 * S).is_empty());
        assert_eq!(sitting.doing(720 * S).last, Doing::Drinking, "kept");
        assert_eq!(sitting.saw(&[drink], 780 * S), [drink]);
    }
}
