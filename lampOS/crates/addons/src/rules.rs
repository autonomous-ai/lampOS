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
    /// Since when the voice has been resting, on the judge's clock, while it
    /// still is: a say waits [`SAY_QUIET_US`] after anything spoken.
    pub resting_since_us: Option<u64>,
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
    /// An act the judge does not carry out yet: `note`, `move`.
    NotYet,
    /// A say within [`SAY_QUIET_US`] of the lamp speaking or being spoken to.
    TooSoon,
    /// The add-ons' says this hour, all together ([`SAYS_AN_HOUR`]).
    SaysThisHour,
    /// Another add-on's say is being put to the policy.
    Asking,
    /// The policy, asked about a say, said to wait.
    PolicyWait,
    /// The policy's answer acts on nothing: it is shadow (`--act addon` off).
    PolicyShadow,
    /// The policy did not answer within [`ASK_WAIT_US`].
    PolicySilent,
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
            Self::NotYet => "the lamp does not act on this yet (#2530)",
            Self::TooSoon => "the lamp spoke or was spoken to in the last 15 s",
            Self::SaysThisHour => "add-ons have said enough this hour",
            Self::Asking => "another add-on's say is being asked about",
            Self::PolicyWait => "the policy said to wait",
            Self::PolicyShadow => "the policy's answer is shadow, and acts on nothing",
            Self::PolicySilent => "the policy did not answer",
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
    /// A say past every rule: put to the policy ([`lamp_msgs::AddonAsk`]),
    /// kept or dropped by its answer ([`Judge::answered`]).
    Ask,
}

/// A say waits this long after the voice last spoke or was spoken to, as
/// progress news does (`docs/policy.md`).
pub const SAY_QUIET_US: u64 = 15_000_000;
/// All add-ons together: this many says kept an hour at most, as
/// `lamp-skills` lets this many through (`crates/skills/src/addon.rs`).
pub const SAYS_AN_HOUR: usize = 4;
/// How long the policy has to answer a say: past it the say is dropped.
pub const ASK_WAIT_US: u64 = 3_000_000;

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
    /// Every add-on's says kept within the hour.
    says: VecDeque<u64>,
    /// The say put to the policy, one at a time.
    asking: Option<Asking>,
    /// A shorter quiet before a say than [`SAY_QUIET_US`], for tests only.
    say_quiet_us: Option<u64>,
}

/// A say put to the policy, and its two deadlines on the judge's clock:
/// the proposal's own lease (its `holds_ms` from when it was made, its age
/// on receipt counted) and the policy's [`ASK_WAIT_US`]. An answer at or
/// past the earlier keeps nothing (the reviewer on #2666).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Asking {
    proposal: u64,
    expires_us: u64,
    ask_until_us: u64,
}

impl Asking {
    /// Why it can no longer be kept at `now_us`, if so: its lease first.
    fn past(self, now_us: u64) -> Option<Drop> {
        if now_us >= self.expires_us {
            Some(Drop::Late)
        } else if now_us >= self.ask_until_us {
            Some(Drop::PolicySilent)
        } else {
            None
        }
    }
}

impl Judge {
    /// A judge whose says wait `quiet_us` (at most [`SAY_QUIET_US`]) after
    /// the voice last spoke or was spoken to: for tests only, the product
    /// keeps 15 s.
    pub fn with_say_quiet(quiet_us: u64) -> Self {
        Self {
            say_quiet_us: Some(quiet_us.min(SAY_QUIET_US)),
            ..Self::default()
        }
    }

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
            None if proposal.act == "say" => {
                self.asking = Some(Asking {
                    proposal: proposal.proposal,
                    expires_us: (now_us.saturating_sub(age_us))
                        .saturating_add(u64::from(proposal.holds_ms) * 1_000),
                    ask_until_us: now_us + ASK_WAIT_US,
                });
                Verdict::Ask
            }
            None => {
                self.kept
                    .entry(proposal.addon.clone())
                    .or_default()
                    .push_back(now_us);
                Verdict::Kept
            }
        }
    }

    /// The policy's answer on the say it was asked about, come at `now_us`:
    /// `say_now` (not shadow) keeps it if neither deadline has passed and
    /// the rules still hold, and counts it; anything else drops it. `None`
    /// for an answer about another.
    pub fn answered(
        &mut self,
        proposal: &AddonProposal,
        say_now: bool,
        shadow: bool,
        s: &Situation,
        now_us: u64,
    ) -> Option<Verdict> {
        let asking = self
            .asking
            .filter(|asking| asking.proposal == proposal.proposal)?;
        self.asking = None;
        let verdict = if let Some(drop) = asking.past(now_us) {
            Verdict::Dropped(drop)
        } else if shadow {
            Verdict::Dropped(Drop::PolicyShadow)
        } else if !say_now {
            Verdict::Dropped(Drop::PolicyWait)
        } else if let Some(drop) = holds(proposal, s, now_us, self.quiet_us()) {
            Verdict::Dropped(drop)
        } else {
            self.kept
                .entry(proposal.addon.clone())
                .or_default()
                .push_back(now_us);
            self.says.push_back(now_us);
            Verdict::Kept
        };
        Some(verdict)
    }

    /// The say asked about, once the earlier of its deadlines has passed
    /// unanswered: its proposal number, given up, and why.
    pub fn lapsed(&mut self, now_us: u64) -> Option<(u64, Drop)> {
        let asking = self.asking?;
        let drop = asking.past(now_us)?;
        self.asking = None;
        Some((asking.proposal, drop))
    }

    fn quiet_us(&self) -> u64 {
        self.say_quiet_us.unwrap_or(SAY_QUIET_US)
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
        if let Some(drop) = holds(proposal, s, now_us, self.quiet_us()) {
            return Some(drop);
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
        if proposal.act == "say" {
            self.says.retain(|at| now_us.saturating_sub(*at) < HOUR_US);
            if self.says.len() >= SAYS_AN_HOUR {
                return Some(Drop::SaysThisHour);
            }
            if self.asking.is_some() {
                return Some(Drop::Asking);
            }
        }
        None
    }
}

/// The rules that hold by how the lamp is, whoever asks: checked when a
/// proposal comes, and again for a say when the policy's answer does.
fn holds(proposal: &AddonProposal, s: &Situation, now_us: u64, quiet_us: u64) -> Option<Drop> {
    if !matches!(proposal.act.as_str(), "light" | "mood" | "say") {
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
    if proposal.act == "say"
        && s.resting_since_us
            .is_none_or(|since| now_us.saturating_sub(since) < quiet_us)
    {
        return Some(Drop::TooSoon);
    }
    None
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
            resting_since_us: Some(at - 60_000_000),
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
    fn a_note_and_a_move_wait() {
        for act in ["note", "move"] {
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

    fn say(addon: &str, proposal: u64) -> AddonProposal {
        AddonProposal {
            proposal,
            act: "say".into(),
            why: "music".into(),
            light: None,
            note: Some("Music is playing.".into()),
            ..light(addon)
        }
    }

    /// A say past every rule is put to the policy; one at a time.
    #[test]
    fn a_say_is_asked_of_the_policy_one_at_a_time() {
        let mut judge = Judge::default();
        assert_eq!(judge.judge(&say("a", 1), 0, &calm(), NOW), Verdict::Ask);
        assert_eq!(
            judge.judge(&say("b", 2), 0, &calm(), NOW + 1),
            Verdict::Dropped(Drop::Asking)
        );
        // Not answered within its wait: given up, and the next may ask.
        assert_eq!(judge.lapsed(NOW + ASK_WAIT_US - 1), None);
        assert_eq!(
            judge.lapsed(NOW + ASK_WAIT_US),
            Some((1, Drop::PolicySilent))
        );
        let at = NOW + ASK_WAIT_US;
        assert_eq!(judge.judge(&say("b", 3), 0, &calm_at(at), at), Verdict::Ask);
    }

    /// A say waits 15 s after the lamp spoke or was spoken to.
    #[test]
    fn a_say_waits_fifteen_seconds_of_quiet() {
        let soon = Situation {
            resting_since_us: Some(NOW - SAY_QUIET_US + 1),
            ..calm()
        };
        assert_eq!(
            Judge::default().judge(&say("a", 1), 0, &soon, NOW),
            Verdict::Dropped(Drop::TooSoon)
        );
        let never = Situation {
            resting_since_us: None,
            ..calm()
        };
        assert_eq!(
            Judge::default().judge(&say("a", 1), 0, &never, NOW),
            Verdict::Dropped(Drop::TooSoon)
        );
        // A light does not wait for it.
        assert_eq!(
            Judge::default().judge(&light("glow"), 0, &soon, NOW),
            Verdict::Kept
        );
    }

    /// The policy's answer: only `say_now`, acting, with the rules still
    /// holding, keeps it; and only a kept one counts.
    #[test]
    fn the_policys_answer_keeps_or_drops_the_say() {
        let cases = [
            (true, false, calm(), Verdict::Kept),
            (false, false, calm(), Verdict::Dropped(Drop::PolicyWait)),
            (true, true, calm(), Verdict::Dropped(Drop::PolicyShadow)),
            (
                true,
                false,
                Situation {
                    voice_resting: Some(Reading::new(false, NOW)),
                    resting_since_us: None,
                    ..calm()
                },
                Verdict::Dropped(Drop::NotResting),
            ),
        ];
        for (say_now, shadow, s, expected) in cases {
            let mut judge = Judge::default();
            assert_eq!(judge.judge(&say("a", 1), 0, &calm(), NOW), Verdict::Ask);
            // An answer about another say is not this one's.
            assert_eq!(judge.answered(&say("a", 9), true, false, &s, NOW), None);
            assert_eq!(
                judge.answered(&say("a", 1), say_now, shadow, &s, NOW),
                Some(expected),
                "{say_now} {shadow}"
            );
            // Answered once: no second answer counts.
            assert_eq!(judge.answered(&say("a", 1), true, false, &s, NOW), None);
        }
    }

    /// The reviewer on #2666: an answer keeps a say only before both its
    /// deadlines, the proposal's own lease (its age on receipt counted) and
    /// the policy's wait; at either, or past it, it is dropped, uncounted.
    #[test]
    fn an_answer_keeps_a_say_only_before_both_deadlines() {
        let short = |holds_ms| AddonProposal {
            holds_ms,
            ..say("a", 1)
        };
        // The lease first: holds 1 s, received 200 ms old.
        for (at, expected) in [
            (NOW + 799_999, Verdict::Kept),
            (NOW + 800_000, Verdict::Dropped(Drop::Late)),
            (NOW + 1_500_000, Verdict::Dropped(Drop::Late)),
        ] {
            let mut judge = Judge::default();
            assert_eq!(
                judge.judge(&short(1_000), 200_000, &calm(), NOW),
                Verdict::Ask
            );
            assert_eq!(
                judge.answered(&short(1_000), true, false, &calm_at(at), at),
                Some(expected),
                "{at}"
            );
        }
        // The policy's wait first: the proposal holds 30 s.
        for (at, expected) in [
            (NOW + ASK_WAIT_US - 1, Verdict::Kept),
            (NOW + ASK_WAIT_US, Verdict::Dropped(Drop::PolicySilent)),
            (
                NOW + ASK_WAIT_US + 100_000,
                Verdict::Dropped(Drop::PolicySilent),
            ),
        ] {
            let mut judge = Judge::default();
            assert_eq!(judge.judge(&say("a", 1), 0, &calm(), NOW), Verdict::Ask);
            assert_eq!(
                judge.answered(&say("a", 1), true, false, &calm_at(at), at),
                Some(expected),
                "{at}"
            );
            // A dropped one never counts: the add-on may ask again at once.
            if expected != Verdict::Kept {
                assert_eq!(judge.judge(&say("a", 2), 0, &calm_at(at), at), Verdict::Ask);
            }
        }
        // Unanswered, it lapses at the earlier deadline, with why.
        let mut judge = Judge::default();
        assert_eq!(
            judge.judge(&short(1_000), 200_000, &calm(), NOW),
            Verdict::Ask
        );
        assert_eq!(judge.lapsed(NOW + 799_999), None);
        assert_eq!(judge.lapsed(NOW + 800_000), Some((1, Drop::Late)));
    }

    /// All add-ons together keep four says an hour at most.
    #[test]
    fn four_says_an_hour_across_every_add_on() {
        let mut judge = Judge::default();
        for n in 0..SAYS_AN_HOUR as u64 {
            let at = NOW + n;
            let addon = format!("addon-{n}");
            assert_eq!(
                judge.judge(&say(&addon, n + 1), 0, &calm_at(at), at),
                Verdict::Ask
            );
            assert_eq!(
                judge.answered(&say(&addon, n + 1), true, false, &calm_at(at), at),
                Some(Verdict::Kept)
            );
        }
        let at = NOW + 10;
        assert_eq!(
            judge.judge(&say("another", 99), 0, &calm_at(at), at),
            Verdict::Dropped(Drop::SaysThisHour)
        );
        let later = NOW + HOUR_US;
        assert_eq!(
            judge.judge(&say("another", 100), 0, &calm_at(later), later),
            Verdict::Ask
        );
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
