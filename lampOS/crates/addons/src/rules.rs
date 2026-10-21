//! The rules, first for every act (#2530 part 1, `docs/addons.md`, "The
//! judge"). A proposal is dropped when the lamp should not change anything
//! now, whoever suggests it: its privacy closed, paused, asleep, setting up,
//! someone talking to it or it speaking, a busy room, or, for a light, the
//! person's own light shown. A reading the judge cannot trust (none yet, or
//! too old) counts as the cautious answer: the lamp stays as it is, except
//! where nothing said is the lamp's ordinary state (a pause never said; no
//! sleep mute on a lamp without the head's controls), as its parts mean it. Then each add-on is
//! held to a rate of its own, so the lamp stays a living thing that does
//! little, not a machine running a partner's loop (rule 8).
use lamp_msgs::AddonProposal;
use std::collections::{HashMap, VecDeque};

/// One reading off the bus and when it was taken, on the judge's monotonic
/// clock in µs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading<T> {
    pub value: T,
    pub at_us: u64,
}

impl<T: Copy> Reading<T> {
    pub fn new(value: T, at_us: u64) -> Self {
        Self { value, at_us }
    }

    /// The value, if taken within `fresh_us` of `now_us`.
    fn fresh(self, now_us: u64, fresh_us: u64) -> Option<T> {
        (now_us.saturating_sub(self.at_us) <= fresh_us).then_some(self.value)
    }
}

/// How fresh each reading must be. Each is a little over its topic's own
/// lifespan or the voice's own rule for it, so one late message does not
/// close the judge, and a part that stopped does.
pub const PRIVACY_FRESH_US: u64 = 1_000_000;
pub const VOICE_FRESH_US: u64 = 3_000_000;
pub const ROOM_FRESH_US: u64 = 3_000_000;
pub const ASLEEP_FRESH_US: u64 = 3_000_000;

/// What the lamp is doing, as the bus last said it. `None`: never read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Situation {
    /// `voice/privacy`: true while the privacy switch closes the microphones.
    pub privacy_closed: Option<Reading<bool>>,
    /// `lamp/pause`: kept until it changes, so it has no age. Never said is
    /// not paused: only `lamp-keeper pause` says it, and with nothing said
    /// everything runs as before (`docs/keeper.md`).
    pub paused: Option<bool>,
    /// `voice/mute` asleep, or `expression/rest_state` resting.
    pub asleep: Option<Reading<bool>>,
    /// `onboarding/pending` holds an invitation: setup is running a step,
    /// and owns the light and the voice until it ends.
    pub onboarding: bool,
    /// `voice/state`: true only while the voice is `resting`.
    pub voice_resting: Option<Reading<bool>>,
    /// `room/state`: true only while it says `quiet`.
    pub room_quiet: Option<Reading<bool>>,
    /// The light shown is the person's own: the newest `light/command` this
    /// boot came from a person (the actions part or the app) and
    /// `light/state` shows it. False while the ring shows the lamp's own
    /// idle light, as before anyone set one.
    pub persons_light: bool,
    /// The person's light rests: the lamp has slept since they set it, or
    /// 12 h have passed. It stays their base for good, but an add-on may
    /// lay a brief glow over it ([`crate::carry::BRIEF_GLOW_US`]).
    pub persons_light_rests: bool,
}

/// Why a proposal is dropped, as the journal and the decision say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    PrivacyClosed,
    Paused,
    Asleep,
    Onboarding,
    NotResting,
    BusyRoom,
    PersonsLight,
    /// A mood that would make the lamp colder: warmth goes up only (#2530).
    Colder,
    /// An act part 1 does not carry out: `say`, `note`, `move` (#2530 part 2).
    NotYet,
    /// Its add-on's own rate ([`EVERY_US`], [`AN_HOUR`]).
    Rate,
    /// Its lease had passed by the time it was judged.
    Late,
}

impl Drop {
    pub fn why(self) -> &'static str {
        match self {
            Self::PrivacyClosed => "the privacy switch is closed, or not known",
            Self::Paused => "the lamp is paused",
            Self::Asleep => "the lamp is asleep, or that is not known",
            Self::Onboarding => "setup is running a step, and owns the light and the voice",
            Self::NotResting => {
                "someone is talking to the lamp or it is speaking, or that is not known"
            }
            Self::BusyRoom => "the room is busy, or that is not known",
            Self::PersonsLight => "the light shown is the person's own, which always wins",
            Self::Colder => "an add-on may warm the lamp, never cool it",
            Self::NotYet => "the lamp does not act on this yet (#2530 part 2)",
            Self::Rate => "its add-on has had enough kept lately",
            Self::Late => "it no longer holds",
        }
    }
}

/// What the judge says of one proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Kept,
    Dropped(Drop),
}

/// One kept act an add-on every this long at most: a light, then nothing new
/// from it for two minutes.
pub const EVERY_US: u64 = 120_000_000;
/// And this many kept an hour at most, an add-on.
pub const AN_HOUR: usize = 10;
const HOUR_US: u64 = 3_600_000_000;

/// The judge's own memory: what each add-on had kept lately.
#[derive(Debug, Default)]
pub struct Judge {
    kept: HashMap<String, VecDeque<u64>>,
}

impl Judge {
    /// Keeps or drops `proposal`, made `age_us` ago, at `now_us`. A kept one
    /// is counted against its add-on's rate; a dropped one is not.
    pub fn judge(
        &mut self,
        proposal: &AddonProposal,
        age_us: u64,
        situation: &Situation,
        now_us: u64,
    ) -> Verdict {
        match self.rule(proposal, age_us, situation, now_us) {
            Some(drop) => Verdict::Dropped(drop),
            None => {
                self.kept
                    .entry(proposal.addon.clone())
                    .or_default()
                    .push_back(now_us);
                Verdict::Kept
            }
        }
    }

    fn rule(
        &mut self,
        proposal: &AddonProposal,
        age_us: u64,
        s: &Situation,
        now_us: u64,
    ) -> Option<Drop> {
        if age_us >= u64::from(proposal.holds_ms) * 1_000 {
            return Some(Drop::Late);
        }
        if !matches!(proposal.act.as_str(), "light" | "mood") {
            return Some(Drop::NotYet);
        }
        if proposal
            .mood
            .as_ref()
            .and_then(|mood| mood.warmth)
            .is_some_and(|warmth| warmth < 0.0)
        {
            return Some(Drop::Colder);
        }
        let closed = s
            .privacy_closed
            .and_then(|r| r.fresh(now_us, PRIVACY_FRESH_US))
            .unwrap_or(true);
        if closed {
            return Some(Drop::PrivacyClosed);
        }
        if s.paused.unwrap_or(false) {
            return Some(Drop::Paused);
        }
        if s.asleep
            .and_then(|r| r.fresh(now_us, ASLEEP_FRESH_US))
            .unwrap_or(true)
        {
            return Some(Drop::Asleep);
        }
        if s.onboarding {
            return Some(Drop::Onboarding);
        }
        if !s
            .voice_resting
            .and_then(|r| r.fresh(now_us, VOICE_FRESH_US))
            .unwrap_or(false)
        {
            return Some(Drop::NotResting);
        }
        if !s
            .room_quiet
            .and_then(|r| r.fresh(now_us, ROOM_FRESH_US))
            .unwrap_or(false)
        {
            return Some(Drop::BusyRoom);
        }
        if proposal.act == "light" && s.persons_light && !s.persons_light_rests {
            return Some(Drop::PersonsLight);
        }
        let kept = self.kept.entry(proposal.addon.clone()).or_default();
        kept.retain(|at| now_us.saturating_sub(*at) < HOUR_US);
        if kept
            .back()
            .is_some_and(|last| now_us.saturating_sub(*last) < EVERY_US)
            || kept.len() >= AN_HOUR
        {
            return Some(Drop::Rate);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{AddonLight, AddonMood};

    const NOW: u64 = 10_000_000_000;

    fn light(addon: &str) -> AddonProposal {
        AddonProposal {
            proposal: 1,
            addon: addon.into(),
            version: "1.0.0".into(),
            act: "light".into(),
            why: "evening".into(),
            sure: 0.6,
            holds_ms: 30_000,
            light: Some(AddonLight {
                colour: "warm".into(),
                level: 2,
                pattern: None,
            }),
            gesture: None,
            mood: None,
            note: None,
        }
    }

    fn mood(addon: &str) -> AddonProposal {
        AddonProposal {
            act: "mood".into(),
            why: "music".into(),
            light: None,
            mood: Some(AddonMood {
                energy: Some(0.1),
                warmth: None,
                for_ms: 600_000,
            }),
            ..light(addon)
        }
    }

    /// A quiet lamp at rest, everything freshly read.
    fn calm() -> Situation {
        calm_at(NOW)
    }

    /// The same, read just before `at`.
    fn calm_at(at: u64) -> Situation {
        let now = |value| Some(Reading::new(value, at - 100_000));
        Situation {
            privacy_closed: now(false),
            paused: Some(false),
            asleep: now(false),
            onboarding: false,
            voice_resting: now(true),
            room_quiet: now(true),
            persons_light: false,
            persons_light_rests: false,
        }
    }

    #[test]
    fn a_calm_lamp_keeps_a_light_and_a_mood() {
        let mut judge = Judge::default();
        assert_eq!(judge.judge(&light("glow"), 0, &calm(), NOW), Verdict::Kept);
        assert_eq!(judge.judge(&mood("music"), 0, &calm(), NOW), Verdict::Kept);
    }

    #[test]
    fn each_never_drops_the_proposal_and_says_why() {
        let cases: [(&str, Situation, Drop); 7] = [
            (
                "privacy closed",
                Situation {
                    privacy_closed: Some(Reading::new(true, NOW)),
                    ..calm()
                },
                Drop::PrivacyClosed,
            ),
            (
                "paused",
                Situation {
                    paused: Some(true),
                    ..calm()
                },
                Drop::Paused,
            ),
            (
                "asleep",
                Situation {
                    asleep: Some(Reading::new(true, NOW)),
                    ..calm()
                },
                Drop::Asleep,
            ),
            (
                "setting up",
                Situation {
                    onboarding: true,
                    ..calm()
                },
                Drop::Onboarding,
            ),
            (
                "someone talking",
                Situation {
                    voice_resting: Some(Reading::new(false, NOW)),
                    ..calm()
                },
                Drop::NotResting,
            ),
            (
                "busy room",
                Situation {
                    room_quiet: Some(Reading::new(false, NOW)),
                    ..calm()
                },
                Drop::BusyRoom,
            ),
            (
                "the person's light",
                Situation {
                    persons_light: true,
                    ..calm()
                },
                Drop::PersonsLight,
            ),
        ];
        for (what, situation, drop) in cases {
            let mut judge = Judge::default();
            assert_eq!(
                judge.judge(&light("glow"), 0, &situation, NOW),
                Verdict::Dropped(drop),
                "{what}"
            );
            assert!(!drop.why().is_empty());
        }
    }

    /// The first builder on #2530 (20:1x): a person's light that rests
    /// lets an add-on's light through, which the ring is asked for briefly.
    #[test]
    fn a_resting_persons_light_lets_a_light_through() {
        let rests = Situation {
            persons_light: true,
            persons_light_rests: true,
            ..calm()
        };
        assert_eq!(
            Judge::default().judge(&light("glow"), 0, &rests, NOW),
            Verdict::Kept
        );
    }

    #[test]
    fn the_persons_light_stops_a_light_but_not_a_mood() {
        let mut judge = Judge::default();
        let theirs = Situation {
            persons_light: true,
            ..calm()
        };
        assert_eq!(
            judge.judge(&light("glow"), 0, &theirs, NOW),
            Verdict::Dropped(Drop::PersonsLight)
        );
        assert_eq!(judge.judge(&mood("music"), 0, &theirs, NOW), Verdict::Kept);
    }

    /// What is not known, or no longer fresh, is taken the cautious way.
    #[test]
    fn a_reading_never_had_or_gone_stale_keeps_the_lamp_as_it_is() {
        let stale = NOW - 5_000_000;
        let cases: [(Situation, Drop); 7] = [
            (
                Situation {
                    privacy_closed: None,
                    ..calm()
                },
                Drop::PrivacyClosed,
            ),
            (
                Situation {
                    privacy_closed: Some(Reading::new(false, stale)),
                    ..calm()
                },
                Drop::PrivacyClosed,
            ),
            (
                Situation {
                    asleep: None,
                    ..calm()
                },
                Drop::Asleep,
            ),
            (
                Situation {
                    voice_resting: None,
                    ..calm()
                },
                Drop::NotResting,
            ),
            (
                Situation {
                    voice_resting: Some(Reading::new(true, stale)),
                    ..calm()
                },
                Drop::NotResting,
            ),
            (
                Situation {
                    room_quiet: None,
                    ..calm()
                },
                Drop::BusyRoom,
            ),
            (
                Situation {
                    room_quiet: Some(Reading::new(true, stale)),
                    ..calm()
                },
                Drop::BusyRoom,
            ),
        ];
        for (situation, drop) in cases {
            assert_eq!(
                Judge::default().judge(&light("glow"), 0, &situation, NOW),
                Verdict::Dropped(drop),
                "{situation:?}"
            );
        }
    }

    /// A pause never said is not one: on a fresh boot nobody has said it.
    #[test]
    fn a_pause_never_said_is_not_a_pause() {
        let situation = Situation {
            paused: None,
            ..calm()
        };
        assert_eq!(
            Judge::default().judge(&light("glow"), 0, &situation, NOW),
            Verdict::Kept
        );
    }

    #[test]
    fn a_mood_that_would_cool_the_lamp_is_dropped() {
        let colder = AddonProposal {
            mood: Some(AddonMood {
                energy: None,
                warmth: Some(-0.05),
                for_ms: 600_000,
            }),
            ..mood("music")
        };
        assert_eq!(
            Judge::default().judge(&colder, 0, &calm(), NOW),
            Verdict::Dropped(Drop::Colder)
        );
    }

    #[test]
    fn a_say_a_note_and_a_move_wait_for_part_two() {
        for act in ["say", "note", "move"] {
            let proposal = AddonProposal {
                act: act.into(),
                ..light("glow")
            };
            assert_eq!(
                Judge::default().judge(&proposal, 0, &calm(), NOW),
                Verdict::Dropped(Drop::NotYet),
                "{act}"
            );
        }
    }

    #[test]
    fn a_proposal_judged_after_its_lease_is_late() {
        let mut judge = Judge::default();
        assert_eq!(
            judge.judge(&light("glow"), 30_000_000, &calm(), NOW),
            Verdict::Dropped(Drop::Late)
        );
        assert_eq!(
            judge.judge(&light("glow"), 29_999_999, &calm(), NOW),
            Verdict::Kept
        );
    }

    #[test]
    fn each_add_on_keeps_one_every_two_minutes_and_ten_an_hour() {
        let mut judge = Judge::default();
        assert_eq!(judge.judge(&light("glow"), 0, &calm(), NOW), Verdict::Kept);
        // A mood from the same add-on too soon after: its rate is the add-on's.
        assert_eq!(
            judge.judge(&mood("glow"), 0, &calm(), NOW + 1),
            Verdict::Dropped(Drop::Rate)
        );
        // Another add-on is not held by it.
        assert_eq!(
            judge.judge(&mood("music"), 0, &calm(), NOW + 1),
            Verdict::Kept
        );
        // A drop is not counted: a busy room does not use up its chance.
        let busy = Situation {
            room_quiet: Some(Reading::new(false, NOW + EVERY_US)),
            ..calm_at(NOW + EVERY_US)
        };
        assert_eq!(
            judge.judge(&light("glow"), 0, &busy, NOW + EVERY_US),
            Verdict::Dropped(Drop::BusyRoom)
        );
        let mut at = NOW;
        for _ in 1..AN_HOUR {
            at += EVERY_US;
            assert_eq!(
                judge.judge(&light("glow"), 0, &calm_at(at), at),
                Verdict::Kept
            );
        }
        at += EVERY_US;
        assert_eq!(
            judge.judge(&light("glow"), 0, &calm_at(at), at),
            Verdict::Dropped(Drop::Rate),
            "ten kept within the hour"
        );
        // An hour after the first, its place is free again.
        let later = NOW + HOUR_US;
        assert_eq!(
            judge.judge(&light("glow"), 0, &calm_at(later), later),
            Verdict::Kept
        );
    }
}
