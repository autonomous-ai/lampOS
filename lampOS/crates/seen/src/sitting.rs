//! What a burst's labels say, and this sitting's timers (#6543 phase 2).
//!
//! - [`read`] turns one burst's answers into what was seen, as coarse kinds
//!   (drinking, eating, a break: V1's `drink`, `eat` and `break` groups),
//!   never the raw label. A false "drank" silences sip-of-water for 30 min,
//!   so a kind counts only when one answer is sure of it (at least
//!   [`SURE`]), or when at least [`RUN`] of the burst's last [`LAST`]
//!   answers have it at all (the server sends only what clears V1's 0.3).
//! - With [`BREAK_IS_STANDING`] on, a break is a stand-up, not a stretch
//!   (#6543 step 2): a stretch is a sign they need one, not that they took
//!   it. A stretch then neither restarts `break_s_ago` nor becomes `last`
//!   or a `humans/doing_seen`, and a sitting begun after `room/stood_up`
//!   counts its break from that return ([`Sitting::break_is_standing`]).
//! - [`Sitting`] keeps the sitting's timers, published as
//!   `humans/doing` (`lamp_msgs::HumanDoing`): every timer always present,
//!   counting from the desk arrival when nothing has been seen, so a
//!   skill's `[[while]] above` runs on the clock as it did without this
//!   part. It begins again on `room/stood_up` or a forget, when nobody
//!   has been at the desk for more than
//!   [`crate::keep::KEEP_US`] (120 s; kept unsaid until then,
//!   [`crate::keep`]), when the gate opens again on another passing id
//!   than the one it was kept for, and when a held sitting ends on
//!   another face ([`crate::hold::End::OtherFace`]): no history is kept.
//!   It begins from the arrival, but never from before the chair last
//!   emptied (120 s closed, or a held sitting ended on another face;
//!   [`begins_us`], [`crate::keep::Seat`]), so the next person's timers
//!   never count the first one's time. After a stand-up, a forget or a
//!   new id without the hold it begins from the arrival, as before.
//! - With [`ENVIRONMENT`] on ([`more`]), the same bursts also time, on
//!   `humans/doing_more` (`lamp_msgs::HumanDoingMore`), a topic of its own
//!   that a skill reads only by naming it, tiredness (V1's `tired`, a yawn, and a stretch of an arm or a leg,
//!   which still counts as a break too), desk work (V1's `sedentary`, as a
//!   coarse [`DeskWork`]) and celebrating (V1's `celebrate`), by the same
//!   sureness rule. They never set `last` or `humans/doing_seen`, and V1's
//!   `emotional` (laughing, crying, singing) is never said. The same topic
//!   says how many drinks and how much eating were newly seen this sitting,
//!   the sightings `humans/doing_seen` says: a count only of what was seen.
use crate::wire::{ActionAnswer, group};
use lamp_msgs::{DeskWork, Doing, DoingSeen, HumanDoing, HumanDoingMore};

/// The switch for tiredness, desk work and celebrating on
/// `humans/doing_more`: `on` exactly; anything else and that topic is
/// neither declared nor said, and `humans/doing` is as before.
pub const ENVIRONMENT: &str = "LAMP_SEEN_MORE_DOINGS";

/// The switch for a break that is a stand-up, not a stretch: `on` exactly
/// ([`crate::switched_on`]); anything else and a stretch is a break, as
/// before.
pub const BREAK_IS_STANDING: &str = "LAMP_SEEN_BREAK_IS_STANDING";

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

/// The best confidence of one answer's labels that `wanted` takes.
fn best(answer: &ActionAnswer, wanted: &impl Fn(&str) -> bool) -> Option<f64> {
    answer
        .detected_classes
        .iter()
        .filter(|class| wanted(&class.class_name))
        .map(|class| class.conf)
        .filter(|conf| conf.is_finite())
        .fold(None, |best: Option<f64>, conf| {
            Some(best.map_or(conf, |b| b.max(conf)))
        })
}

/// The surest confidence of the burst's labels that `wanted` takes, when
/// they count: one answer at least [`SURE`], or [`RUN`] of the last
/// [`LAST`] answers with one at all.
fn counted(answers: &[ActionAnswer], wanted: impl Fn(&str) -> bool) -> Option<f32> {
    let most = answers
        .iter()
        .filter_map(|answer| best(answer, &wanted))
        .fold(None, |most: Option<f64>, conf| {
            Some(most.map_or(conf, |m| m.max(conf)))
        })?;
    let tail = &answers[answers.len().saturating_sub(LAST)..];
    let run = tail
        .iter()
        .filter(|answer| best(answer, &wanted).is_some())
        .count();
    (most >= SURE || run >= RUN).then(|| most.clamp(0.0, 1.0) as f32)
}

/// What one burst saw: each kind with its best confidence, the surest
/// first.
pub fn read(answers: &[ActionAnswer]) -> Vec<DoingSeen> {
    let mut seen: Vec<DoingSeen> = KINDS
        .iter()
        .filter_map(|(kind, wanted)| {
            counted(answers, |label| group(label) == Some(*wanted)).map(|confidence| DoingSeen {
                what: *kind,
                confidence,
            })
        })
        .collect();
    seen.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    seen
}

/// Whether the switch's value turns [`ENVIRONMENT`] on: `on` exactly.
pub fn more(value: Option<&str>) -> bool {
    crate::switched_on(value)
}

/// What one burst saw beyond the three kinds, for [`ENVIRONMENT`]: never
/// `emotional`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct More {
    /// A yawn or a stretch counted.
    pub tired: bool,
    /// Desk work counted, as the kind of its surest label.
    pub desk_work: Option<DeskWork>,
    /// Celebrating counted.
    pub celebrate: bool,
}

impl More {
    pub fn any(&self) -> bool {
        self.tired || self.desk_work.is_some() || self.celebrate
    }
}

/// A desk-work label's coarse kind; `None` outside V1's `sedentary`.
pub fn desk_work(label: &str) -> Option<DeskWork> {
    Some(match label {
        "using computer" | "playing controller" => DeskWork::Computer,
        "texting" => DeskWork::Phone,
        "reading" | "reading book" | "reading newspaper" => DeskWork::Reading,
        "writing" | "drawing" => DeskWork::Writing,
        _ => return None,
    })
}

/// What one burst saw of tiredness, desk work and celebrating, each by the
/// same rule as [`read`]'s kinds. Desk work counts as one group, and its
/// kind is that of the surest desk-work label in the burst.
pub fn read_more(answers: &[ActionAnswer]) -> More {
    let tired = counted(answers, |label| {
        matches!(group(label), Some("tired") | Some("break"))
    })
    .is_some();
    let celebrate = counted(answers, |label| group(label) == Some("celebrate")).is_some();
    let desk = counted(answers, |label| group(label) == Some("sedentary")).and_then(|_| {
        answers
            .iter()
            .flat_map(|answer| &answer.detected_classes)
            .filter(|class| class.conf.is_finite())
            .filter_map(|class| desk_work(&class.class_name).map(|kind| (kind, class.conf)))
            .fold(
                None,
                |surest: Option<(DeskWork, f64)>, (kind, conf)| match surest {
                    Some((_, most)) if most >= conf => surest,
                    _ => Some((kind, conf)),
                },
            )
            .map(|(kind, _)| kind)
    });
    More {
        tired,
        desk_work: desk,
        celebrate,
    }
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
    /// [`BREAK_IS_STANDING`] on: a stretch is not taken as a break.
    break_is_standing: bool,
    /// How many times a drink and eating were newly seen this sitting:
    /// each sighting said on `humans/doing_seen`, said with [`ENVIRONMENT`].
    drank_seen: u32,
    ate_seen: u32,
    /// [`ENVIRONMENT`] on: the timers below are said.
    more: bool,
    tired_us: Option<u64>,
    tired_seen: u32,
    /// Whether the last burst that answered saw tiredness.
    tired_last_burst: bool,
    desk_work_us: Option<u64>,
    desk_work: DeskWork,
    celebrate_us: Option<u64>,
}

fn index(kind: Doing) -> Option<usize> {
    KINDS.iter().position(|(k, _)| *k == kind)
}

fn seconds(from_us: u64, now_us: u64) -> u32 {
    u32::try_from(now_us.saturating_sub(from_us) / 1_000_000).unwrap_or(u32::MAX)
}

/// When a sitting begun now counts from: `room/activity`'s arrival
/// (`arrived_us`, a gap under two minutes forgiven, whoever's face), but
/// never before the chair last emptied (`emptied_us`,
/// [`crate::keep::chair_emptied`]), so someone sitting down after another
/// person left never counts that person's time (the first builder's rule
/// on #6543).
pub fn begins_us(arrived_us: u64, emptied_us: Option<u64>) -> u64 {
    emptied_us.map_or(arrived_us, |emptied| arrived_us.max(emptied))
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
            break_is_standing: false,
            drank_seen: 0,
            ate_seen: 0,
            more: false,
            tired_us: None,
            tired_seen: 0,
            tired_last_burst: false,
            desk_work_us: None,
            desk_work: DeskWork::Nothing,
            celebrate_us: None,
        }
    }

    /// The same, with [`ENVIRONMENT`] on when `more`.
    pub fn with_more(arrived_us: u64, more: bool) -> Self {
        Self {
            more,
            ..Self::new(arrived_us)
        }
    }

    /// Take one burst's tiredness, desk work and celebrating at `now_us`.
    /// Returns whether it saw any, with [`ENVIRONMENT`] on; off, it keeps
    /// nothing and returns false. `last` is never changed by it.
    pub fn saw_more(&mut self, more: &More, now_us: u64) -> bool {
        if !self.more {
            return false;
        }
        if more.tired {
            self.tired_us = Some(now_us);
            if !self.tired_last_burst {
                self.tired_seen = self.tired_seen.saturating_add(1);
            }
        }
        self.tired_last_burst = more.tired;
        if let Some(kind) = more.desk_work {
            self.desk_work_us = Some(now_us);
            self.desk_work = kind;
        }
        if more.celebrate {
            self.celebrate_us = Some(now_us);
        }
        more.any()
    }

    /// With [`BREAK_IS_STANDING`] on, for a sitting begun at `now_us`: a
    /// stretch is no longer a break ([`Sitting::takes`]), and if
    /// `room/stood_up` came at `stood_up_us`, at or after the arrival this
    /// sitting counts from, the break counts from `now_us`, their return.
    /// The arrival can be from before that stand-up, as `room/activity`'s
    /// `sitting_s` goes on through a gap under two minutes; the other
    /// timers keep counting from it. A stand-up before the arrival (a gap
    /// longer than that, or none said) leaves the break counting from the
    /// arrival, as every timer does: there the sitting itself is new.
    pub fn break_is_standing(mut self, now_us: u64, stood_up_us: Option<u64>) -> Self {
        self.break_is_standing = true;
        if stood_up_us.is_some_and(|at| at >= self.arrived_us) {
            self.break_us = Some(now_us.max(self.arrived_us));
        }
        self
    }

    /// Whether a sighting of `kind` is taken: every kind, except a break
    /// (a stretch) with [`BREAK_IS_STANDING`] on.
    pub fn takes(&self, kind: Doing) -> bool {
        !(self.break_is_standing && kind == Doing::Break)
    }

    /// Take one burst's sightings at `now_us`; returns those newly seen
    /// (not seen in the burst before), for `humans/doing_seen`.
    pub fn saw(&mut self, seen: &[DoingSeen], now_us: u64) -> Vec<DoingSeen> {
        let mut this_burst = [false; 3];
        let mut newly = Vec::new();
        for sighting in seen {
            let Some(at) = index(sighting.what).filter(|_| self.takes(sighting.what)) else {
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
                match sighting.what {
                    Doing::Drinking => self.drank_seen = self.drank_seen.saturating_add(1),
                    Doing::Eating => self.ate_seen = self.ate_seen.saturating_add(1),
                    _ => {}
                }
            }
        }
        // The surest of this burst is the last thing seen.
        if let Some(surest) = seen
            .iter()
            .find(|s| index(s.what).is_some() && self.takes(s.what))
        {
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

    /// With [`ENVIRONMENT`] on, the other timers at `now_us`, each from its
    /// last sighting, else from the arrival, for `humans/doing_more`; off,
    /// none.
    pub fn doing_more(&self, now_us: u64) -> Option<HumanDoingMore> {
        let since = |seen: Option<u64>| seconds(seen.unwrap_or(self.arrived_us), now_us);
        self.more.then(|| HumanDoingMore {
            tired_s_ago: since(self.tired_us),
            tired_seen: self.tired_seen,
            drank_seen: self.drank_seen,
            ate_seen: self.ate_seen,
            desk_work_s_ago: since(self.desk_work_us),
            desk_work: self.desk_work,
            celebrate_s_ago: since(self.celebrate_us),
        })
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

    const STRETCH: DoingSeen = DoingSeen {
        what: Doing::Break,
        confidence: 0.8,
    };

    #[test]
    fn switch_off_a_stretch_is_a_break() {
        let mut sitting = Sitting::new(0);
        assert_eq!(sitting.saw(&[STRETCH], 600 * S), [STRETCH]);
        let doing = sitting.doing(660 * S);
        assert_eq!(doing.break_s_ago, 60);
        assert_eq!(doing.last, Doing::Break);
        assert!(sitting.takes(Doing::Break));
        // Off, `break_is_standing` is never called: `new` alone is the old
        // sitting, field for field.
        assert_eq!(
            Sitting::new(5 * S),
            Sitting {
                arrived_us: 5 * S,
                drank_us: None,
                ate_us: None,
                break_us: None,
                last: Doing::Nothing,
                confidence: 0.0,
                last_burst: [false; 3],
                break_is_standing: false,
                drank_seen: 0,
                ate_seen: 0,
                more: false,
                tired_us: None,
                tired_seen: 0,
                tired_last_burst: false,
                desk_work_us: None,
                desk_work: DeskWork::Nothing,
                celebrate_us: None,
            }
        );
    }

    #[test]
    fn switch_on_a_stretch_is_not_a_break() {
        let mut sitting = Sitting::new(0).break_is_standing(0, None);
        assert!(!sitting.takes(Doing::Break));
        assert!(sitting.takes(Doing::Drinking));
        // A stretch alone: not new, not last, the break still from the
        // arrival.
        assert!(sitting.saw(&[STRETCH], 600 * S).is_empty());
        let doing = sitting.doing(660 * S);
        assert_eq!(doing.break_s_ago, 660);
        assert_eq!(doing.last, Doing::Nothing);
        assert_eq!(doing.confidence, 0.0);
        doing.check().unwrap();
        // A stretch surer than a drink: the drink is new and last.
        let drink = DoingSeen {
            what: Doing::Drinking,
            confidence: 0.6,
        };
        let surest_first = [
            DoingSeen {
                confidence: 0.9,
                ..STRETCH
            },
            drink,
        ];
        assert_eq!(sitting.saw(&surest_first, 720 * S), [drink]);
        let doing = sitting.doing(780 * S);
        assert_eq!((doing.drank_s_ago, doing.break_s_ago), (60, 780));
        assert_eq!(doing.last, Doing::Drinking);
        assert!((doing.confidence - 0.6).abs() < 1e-6);
    }

    #[test]
    fn a_sitting_begun_after_the_chair_emptied_counts_from_then() {
        // lamp-06c1, run 8 (#7506): the chair emptied at 2_100 s, and the
        // next sitting counted `break_s_ago` from `room/activity`'s arrival
        // 2_100 s before, the first one's time.
        let begins = begins_us(0, Some(2_100 * S));
        assert_eq!(begins, 2_100 * S);
        let doing = Sitting::new(begins).doing(2_160 * S);
        assert_eq!(
            (doing.break_s_ago, doing.drank_s_ago, doing.ate_s_ago),
            (60, 60, 60)
        );
        // Never emptied, or before the arrival: from the arrival.
        assert_eq!(begins_us(500 * S, None), 500 * S);
        assert_eq!(begins_us(500 * S, Some(100 * S)), 500 * S);
    }

    #[test]
    fn switch_on_a_break_counts_from_the_return_after_a_stand_up() {
        // Sat from 0; stood up at 2_070 s, back at 2_160 s, within the
        // two-minute gap, so the arrival is still 0 (`room/activity`).
        let sitting = Sitting::new(0).break_is_standing(2_160 * S, Some(2_072 * S));
        let doing = sitting.doing(3_000 * S);
        assert_eq!(doing.break_s_ago, 840, "from the return");
        assert_eq!(
            (doing.drank_s_ago, doing.ate_s_ago),
            (3_000, 3_000),
            "the others from the arrival"
        );
        assert_eq!(doing.last, Doing::Nothing);
        // Back after a longer gap: the sitting is new, every timer from it.
        let sitting = Sitting::new(2_400 * S).break_is_standing(2_400 * S, Some(2_072 * S));
        assert_eq!(sitting.doing(3_000 * S).break_s_ago, 600);
        // No stand-up said: from the arrival.
        let sitting = Sitting::new(0).break_is_standing(2_160 * S, None);
        assert_eq!(sitting.doing(3_000 * S).break_s_ago, 3_000);
        // A stretch after the return changes nothing.
        let mut sitting = Sitting::new(0).break_is_standing(2_160 * S, Some(2_072 * S));
        assert!(sitting.saw(&[STRETCH], 2_900 * S).is_empty());
        assert_eq!(sitting.doing(3_000 * S).break_s_ago, 840);
        // The same stand-up, the switch off: the sitting begun again counts
        // from its arrival, the break included.
        assert_eq!(Sitting::new(0).doing(3_000 * S).break_s_ago, 3_000);
    }

    #[test]
    fn drinks_and_eating_are_counted_as_the_sightings_said() {
        let drink = DoingSeen {
            what: Doing::Drinking,
            confidence: 0.7,
        };
        let eat = DoingSeen {
            what: Doing::Eating,
            confidence: 0.6,
        };
        let stretch = DoingSeen {
            what: Doing::Break,
            confidence: 0.6,
        };
        let mut sitting = Sitting::with_more(0, true);
        let mut said = 0;
        // A drink in two bursts in a row is one; again after a burst
        // without it; eating and a stretch are not drinks.
        for (at, seen) in [
            (60, &[drink][..]),
            (120, &[drink, eat][..]),
            (180, &[][..]),
            (240, &[drink, stretch][..]),
            (300, &[eat][..]),
            (360, &[drink][..]),
        ] {
            said += sitting
                .saw(seen, at * S)
                .iter()
                .filter(|s| s.what == Doing::Drinking)
                .count();
        }
        let more = sitting.doing_more(400 * S).unwrap();
        assert_eq!((more.drank_seen, more.ate_seen), (3, 2));
        assert_eq!(
            more.drank_seen as usize, said,
            "each one said on doing_seen"
        );
        // Nothing seen, no count; a new sitting begins it again.
        assert_eq!(
            Sitting::with_more(0, true)
                .doing_more(S)
                .unwrap()
                .drank_seen,
            0
        );
        // Off, the count is never said.
        let mut off = Sitting::new(0);
        off.saw(&[drink], 60 * S);
        assert_eq!(off.doing_more(120 * S), None);
    }

    #[test]
    fn more_doings_map_v1s_groups_and_never_emotion() {
        // A yawn is tiredness; a stretch is tiredness and still a break.
        let yawn = [answer(&[("yawning", 0.7)])];
        assert_eq!(
            read_more(&yawn),
            More {
                tired: true,
                ..More::default()
            }
        );
        assert!(read(&yawn).is_empty(), "a yawn is not a break");
        let stretch = [answer(&[("stretching arm", 0.6)])];
        assert!(read_more(&stretch).tired);
        assert_eq!(read(&stretch)[0].what, Doing::Break);
        assert!(read_more(&[answer(&[("stretching leg", 0.55)])]).tired);
        // Desk work, as a coarse kind: never the label.
        for (label, kind) in [
            ("using computer", DeskWork::Computer),
            ("playing controller", DeskWork::Computer),
            ("texting", DeskWork::Phone),
            ("reading", DeskWork::Reading),
            ("reading book", DeskWork::Reading),
            ("reading newspaper", DeskWork::Reading),
            ("writing", DeskWork::Writing),
            ("drawing", DeskWork::Writing),
        ] {
            let more = read_more(&[answer(&[(label, 0.8)])]);
            assert_eq!(more.desk_work, Some(kind), "{label}");
            assert!(!more.tired && !more.celebrate, "{label}");
            assert!(read(&[answer(&[(label, 0.8)])]).is_empty(), "{label}");
        }
        // The surest desk-work label names the kind.
        let both = read_more(&[answer(&[("texting", 0.6), ("using computer", 0.9)])]);
        assert_eq!(both.desk_work, Some(DeskWork::Computer));
        for label in ["celebrating", "clapping", "applauding"] {
            assert!(read_more(&[answer(&[(label, 0.6)])]).celebrate, "{label}");
        }
        // Emotion is never said.
        for label in ["laughing", "crying", "singing"] {
            assert_eq!(
                read_more(&[answer(&[(label, 0.99)])]),
                More::default(),
                "{label}"
            );
            assert!(read(&[answer(&[(label, 0.99)])]).is_empty(), "{label}");
        }
        // The same sureness: one unsure answer is nothing, two of the last
        // four count.
        assert_eq!(read_more(&[answer(&[("texting", 0.35)])]), More::default());
        let run = read_more(&[
            answer(&[("yawning", 0.31)]),
            answer(&[]),
            answer(&[("yawning", 0.32), ("reading", 0.33)]),
            answer(&[("reading", 0.34)]),
        ]);
        assert!(run.tired);
        assert_eq!(run.desk_work, Some(DeskWork::Reading));
    }

    #[test]
    fn more_doings_are_timed_only_with_the_switch() {
        let desk = More {
            desk_work: Some(DeskWork::Computer),
            ..More::default()
        };
        // Off: nothing kept, nothing said, `humans/doing` as before.
        let mut off = Sitting::new(0);
        assert!(!off.saw_more(&desk, 60 * S));
        assert_eq!(off.doing_more(120 * S), None);
        assert_eq!(off, Sitting::new(0));
        assert!(!more(None) && !more(Some("ON")) && !more(Some("1")));
        assert!(more(Some("on")));
        // On: every timer from the arrival until seen, and `humans/doing`
        // the same message, byte for byte.
        let mut sitting = Sitting::with_more(100 * S, true);
        assert_eq!(
            serde_json::to_string(&sitting.doing(400 * S)).unwrap(),
            serde_json::to_string(&Sitting::new(100 * S).doing(400 * S)).unwrap()
        );
        assert_eq!(
            sitting.doing_more(400 * S),
            Some(HumanDoingMore {
                tired_s_ago: 300,
                tired_seen: 0,
                drank_seen: 0,
                ate_seen: 0,
                desk_work_s_ago: 300,
                desk_work: DeskWork::Nothing,
                celebrate_s_ago: 300,
            })
        );
        // Desk work: its timer and kind, and never `last` or a sighting.
        let drink = DoingSeen {
            what: Doing::Drinking,
            confidence: 0.7,
        };
        sitting.saw(&[drink], 400 * S);
        assert!(sitting.saw_more(&desk, 460 * S));
        assert!(sitting.saw(&[], 460 * S).is_empty());
        let doing = sitting.doing(500 * S);
        assert_eq!(doing.last, Doing::Drinking, "desk work never sets last");
        let more_now = sitting.doing_more(500 * S).unwrap();
        assert_eq!(more_now.desk_work_s_ago, 40);
        assert_eq!(more_now.desk_work, DeskWork::Computer);
        assert_eq!((more_now.drank_seen, more_now.ate_seen), (1, 0));
        // A stretch: a break (on `humans/doing`) and tiredness, counted once
        // across bursts in a row, again after a burst without.
        let stretch = DoingSeen {
            what: Doing::Break,
            confidence: 0.6,
        };
        let tired = More {
            tired: true,
            ..More::default()
        };
        let stretches = [stretch];
        let none = More::default();
        for (at, tired_now) in [(520, true), (580, true), (640, false), (700, true)] {
            let (seen, more_seen) = if tired_now {
                (&stretches[..], &tired)
            } else {
                (&[][..], &none)
            };
            sitting.saw(seen, at * S);
            sitting.saw_more(more_seen, at * S);
        }
        let doing = sitting.doing(710 * S);
        assert_eq!(doing.break_s_ago, 10);
        let more_now = sitting.doing_more(710 * S).unwrap();
        assert_eq!((more_now.tired_s_ago, more_now.tired_seen), (10, 2));
        assert_eq!(more_now.desk_work, DeskWork::Computer, "kept");
        // Celebrating.
        sitting.saw_more(
            &More {
                celebrate: true,
                ..More::default()
            },
            720 * S,
        );
        assert_eq!(sitting.doing_more(725 * S).unwrap().celebrate_s_ago, 5);
        // A stand-up begins the sitting again (`main`: a new `Sitting`):
        // nothing carried.
        let again = Sitting::with_more(800 * S, true);
        assert_eq!(
            again.doing_more(860 * S),
            Some(HumanDoingMore {
                tired_s_ago: 60,
                tired_seen: 0,
                drank_seen: 0,
                ate_seen: 0,
                desk_work_s_ago: 60,
                desk_work: DeskWork::Nothing,
                celebrate_s_ago: 60,
            })
        );
    }
}
