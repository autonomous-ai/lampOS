//! Who asks the lamp's body or light for something, and for how long (#940,
//! `docs/skills.md`). Every request to a part that owns an actuator carries
//! its source, a priority and a lease that lapses unless it is asked again.
//! A request gives way to any of higher priority: a person (their voice,
//! their hand), then safety, the conversation, skills, and idle life last.
//! Lasting settings, such as the person's own light on `light/command`,
//! carry no lease and are what is shown once every request has lapsed.
use crate::{Invalid, Light};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// The order requests give way in, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Idle,
    Skill,
    Conversation,
    Safety,
    Person,
}

/// Who asks, how much it matters, and until when, on the asker's clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// A part's name, or a skill's as `skill:<name>`.
    pub source: String,
    pub priority: Priority,
    /// The monotonic clock `until_us` is on: the host's boot, as parts on
    /// one lamp share it.
    pub clock_id: String,
    /// The request holds until this, unless asked again before.
    pub until_us: u64,
}

impl Claim {
    /// The longest a single request may hold: a skill renews what it keeps.
    pub const MOST_US: u64 = 30_000_000;

    /// Shape only; whether it still holds is [`Claim::holds_at`].
    pub fn check(&self) -> Result<(), Invalid> {
        let named = |text: &str| {
            !text.is_empty() && text.len() <= 64 && !text.chars().any(char::is_control)
        };
        if !named(&self.source) || !named(&self.clock_id) {
            return Err(Invalid(
                "a claim names its source and clock, each 1 to 64 printable characters".into(),
            ));
        }
        Ok(())
    }

    /// Whether it holds at `now_us` on `clock_id`, and has not asked for
    /// longer than [`Claim::MOST_US`] from now: a request from another boot,
    /// lapsed, or out of reach does not hold.
    pub fn holds_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.clock_id == clock_id
            && now_us < self.until_us
            && self.until_us - now_us <= Self::MOST_US
    }
}

/// A light asked for that lapses (`light/request`): a skill's glow, or
/// anything else that must not leave the person's own light changed. The
/// ring shows the highest-priority request that holds, the newest claim of
/// equal priority (a renewal keeps its place), and the person's lasting
/// light once none does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightRequest {
    pub claim: Claim,
    pub light: Light,
    /// How the light moves while it is shown, if it does (#940 item 3): a
    /// ring without patterns shows the light steady.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<LightPattern>,
    /// How long the ring takes to come into this light from what it shows,
    /// in ms, if slowly (#1677): a light going out when nobody is there
    /// fades like something falling asleep, never snapping off. Within
    /// [`LightRequest::FADE_MS`]; none, the ring's own quick ease. A ring
    /// without fades eases as quickly as ever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_ms: Option<u32>,
}

/// The ways a skill's light may move (#940 item 3), V1's effects judged by
/// CLAUDE.md's rule 8: a lamp breathes, flickers like a candle, or sends one
/// soft pulse around for "something came". It never blinks, flashes or
/// cycles colours on its own: those read as a machine. The one exception is
/// a light show a person asked for (#1679): its colours change, each held
/// 2 s or more, under a minute in all, and it stops once someone speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    /// The whole ring eases between `low` and the light's level on a sine.
    Breathe,
    /// Each pixel wanders gently a little under the level, at most a fifth
    /// as V1's candle did, its neighbours close; the colour never shifts.
    /// Only at level 3 and up, where the ring keeps a colour's channels.
    Candle,
    /// At the light's level, one soft brighter arc goes slowly once around
    /// the ring when the light is first drawn, then the steady light. Only
    /// at level 2 and up, where the arc can be seen.
    Pulse,
}

/// A light's movement: its kind, its time (a breath's, a candle's wander,
/// the pulse's way around), and for a breath the level it goes down to,
/// under the light's own, which is the top. A candle and a pulse do not
/// use `low`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightPattern {
    pub kind: PatternKind,
    pub period_ms: u32,
    pub low: u8,
}

impl LightPattern {
    /// A breath under 5 s reads as anxious on a desk: allowed from 4 s, and
    /// 6 s when a skill names none (the first builder on #940).
    pub const BREATHE_MS: std::ops::RangeInclusive<u32> = 4_000..=12_000;
    pub const BREATHE_DEFAULT_MS: u32 = 6_000;
    /// A candle's wander: 1 s at the quickest (the reviewer on #1494: at
    /// 0.5 s the swing came twice a second).
    pub const CANDLE_MS: std::ops::RangeInclusive<u32> = 1_000..=4_000;
    pub const CANDLE_DEFAULT_MS: u32 = 1_500;
    /// A pulse's way around: 3 s at the quickest, as designed on #940, far
    /// slower than the thinking face's point (1.15 s a turn).
    pub const PULSE_MS: std::ops::RangeInclusive<u32> = 3_000..=6_000;
    pub const PULSE_DEFAULT_MS: u32 = 4_000;
    /// The lowest level a candle may flicker at: under it, a fifth of a
    /// level is less than a step of the ring's channels at the product's
    /// ceiling, and a pixel's colour changes as it rounds.
    pub const CANDLE_LEAST_LEVEL: u8 = 3;
    /// The lowest level a pulse is asked at: at level 1 its arc is under a
    /// step at the product's ceiling, and nobody would see it (the reviewer
    /// on #1494).
    pub const PULSE_LEAST_LEVEL: u8 = 2;
    /// One pulse a minute from the same asker at most: "something came"
    /// twice in a row is a nag, not a creature (the first builder on #940).
    pub const PULSE_GAP_US: u64 = 60_000_000;

    /// Its time when a skill names none.
    pub fn default_ms(kind: PatternKind) -> u32 {
        match kind {
            PatternKind::Breathe => Self::BREATHE_DEFAULT_MS,
            PatternKind::Candle => Self::CANDLE_DEFAULT_MS,
            PatternKind::Pulse => Self::PULSE_DEFAULT_MS,
        }
    }

    /// A pattern for a light at `level`: its time in its kind's bounds; a
    /// breath's `low` from 1 to under the level; a candle at level 3 or up.
    pub fn check(&self, level: u8) -> Result<(), Invalid> {
        let bounds = match self.kind {
            PatternKind::Breathe => Self::BREATHE_MS,
            PatternKind::Candle => Self::CANDLE_MS,
            PatternKind::Pulse => Self::PULSE_MS,
        };
        if !bounds.contains(&self.period_ms) {
            return Err(Invalid(format!(
                "a {:?} pattern takes {} to {} ms",
                self.kind,
                bounds.start(),
                bounds.end()
            )));
        }
        match self.kind {
            PatternKind::Breathe if self.low < 1 || self.low >= level => Err(Invalid(
                "a breath's low is from 1 to under the light's level".into(),
            )),
            PatternKind::Candle if level < Self::CANDLE_LEAST_LEVEL => Err(Invalid(format!(
                "a candle flickers at level {} and up",
                Self::CANDLE_LEAST_LEVEL
            ))),
            PatternKind::Pulse if level < Self::PULSE_LEAST_LEVEL => Err(Invalid(format!(
                "a pulse is seen at level {} and up",
                Self::PULSE_LEAST_LEVEL
            ))),
            _ if !(1..=5).contains(&self.low) => {
                Err(Invalid("a pattern's low is a level, 1 to 5".into()))
            }
            _ => Ok(()),
        }
    }
}

impl LightRequest {
    pub const TYPE: &str = "lamp_msgs/LightRequest";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "light/request";
    /// A fade from a second, longer than any ease of the face, to a minute:
    /// longer is not seen as a fade, only as a light that is not answering.
    pub const FADE_MS: std::ops::RangeInclusive<u32> = 1_000..=60_000;

    /// Each request is a message of its own: several askers share the topic.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.claim.check()?;
        self.light.check()?;
        if let Some(fade) = self.fade_ms
            && !Self::FADE_MS.contains(&fade)
        {
            return Err(Invalid(format!(
                "a fade of {fade} ms: from {} to {} ms",
                Self::FADE_MS.start(),
                Self::FADE_MS.end()
            )));
        }
        match &self.pattern {
            Some(_) if !self.light.on => Err(Invalid("a light that is off has no pattern".into())),
            Some(pattern) => pattern.check(self.light.level),
            None => Ok(()),
        }
    }
}

/// A light asked for on `light/request` that the ring has shown at rest long
/// enough to have eased in (`light/reached`): said then, and again after
/// each renewal while it is shown, so its asker can wait for it (a skill's
/// look over a page it lit, #1146). Nothing is said inside a turn, whose
/// face has colours of its own, nor for a light the ring never showed that
/// long: the privacy switch closed, the person's light over it, or a request
/// that lapsed first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightReached {
    /// The request's `claim.source`.
    pub source: String,
    pub light: Light,
}

impl LightReached {
    pub const TYPE: &str = "lamp_msgs/LightReached";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "light/reached";

    /// Each is a message of its own: one asker's must not hide another's.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }
}

/// A light asked for on `light/request` that the ring holds but does not
/// show, because another request or the person's own light is over it
/// (`light/outranked`): said when the ring takes it so, when a request
/// shown loses its place, when what is over it changes, and again after
/// each renewal while it stays hidden, so its asker is told it lost (#940).
/// It keeps its claim, and is shown by itself once what is over it lapses
/// or is given back. Nothing is said for what hides every request alike
/// (the privacy switch, a turn's face, a self-test): that is not a loss.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightOutranked {
    /// The hidden request's `claim.source`.
    pub source: String,
    /// The `claim.source` of the request shown over it, or
    /// [`LightOutranked::PERSON`] for the person's own light.
    pub by: String,
}

impl LightOutranked {
    pub const TYPE: &str = "lamp_msgs/LightOutranked";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "light/outranked";
    /// `by` for the person's own light (`light/command`), over every
    /// request below a person's that began before they asked.
    pub const PERSON: &str = "person";

    /// Each is a message of its own: one asker's must not hide another's.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }
}

/// The requests to actuators beside the first topics (#940).
pub fn arbitration_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (LightRequest::TOPIC, LightRequest::declaration()),
        (LightReached::TOPIC, LightReached::declaration()),
        (LightOutranked::TOPIC, LightOutranked::declaration()),
        (RingMark::TOPIC, RingMark::declaration()),
    ]
}

/// What a computer is taking through the lamp, for [`RingMark`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    Camera,
    Microphone,
}

/// The ring's mark while a computer sees or hears through the lamp
/// (`light/mark`, lamp-control's camera and microphone, docs/control.md
/// "The camera"): one small steady dot over whatever the ring shows, a
/// turn's face included, so a person can always tell. The asker renews it
/// while a stream runs; it lapses at its claim's `until_us`, at most
/// [`RingMark::MOST_US`] ahead, so a stalled asker can't leave it lit. A
/// snapshot asks for a `blink`: the dot for a moment, once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingMark {
    pub claim: Claim,
    pub kind: MarkKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blink: bool,
}

impl RingMark {
    pub const TYPE: &str = "lamp_msgs/RingMark";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "light/mark";
    /// The furthest ahead a mark may be asked to hold.
    pub const MOST_US: u64 = 3_000_000;

    /// Each is a message of its own: one asker's must not hide another's.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    /// Shape only: the claim's, and no further ahead than [`Self::MOST_US`]
    /// at `now_us`.
    pub fn check(&self, now_us: u64) -> Result<(), Invalid> {
        self.claim.check()?;
        if self.claim.until_us > now_us.saturating_add(Self::MOST_US) {
            return Err(Invalid("a mark holds 3 s ahead at most".to_owned()));
        }
        Ok(())
    }
}

/// The user a part runs as, by its product unit's `User=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunsAs {
    Root,
    Lampos,
}

/// The most each part's caller may ask at, by its systemd unit (phase 0 of
/// the lamp programming protocol, #3504): a claim above it is held to it.
pub const MOST_BY_UNIT: [(&str, RunsAs, Priority); 7] = [
    ("lamp-voice.service", RunsAs::Root, Priority::Person),
    // The app's light preview asks at conversation (`cloud/src/lamp.rs`).
    ("lamp-cloud.service", RunsAs::Lampos, Priority::Conversation),
    // The voice's light and volume tools go through it.
    (
        "lamp-actions.service",
        RunsAs::Lampos,
        Priority::Conversation,
    ),
    ("lamp-skills.service", RunsAs::Lampos, Priority::Skill),
    // The programming door (#3715), not `lamp-controls`, the touch pads.
    ("lamp-control.service", RunsAs::Lampos, Priority::Skill),
    ("lamp-addons.service", RunsAs::Lampos, Priority::Idle),
    ("lamp-expression.service", RunsAs::Lampos, Priority::Idle),
];

/// The most a caller may ask at, as the kernel named it (`lamp_ipc::caller`):
/// its unit's in [`MOST_BY_UNIT`], only while it runs as that unit's user
/// (`lampos`: that user's uid, if known); `None`, no bound, for root's own
/// helpers, which keep what they ask; and idle for anyone else, a known
/// unit with another user's uid among them.
pub fn most_for(unit: Option<&str>, uid: u32, lampos: Option<u32>) -> Option<Priority> {
    let named = MOST_BY_UNIT.iter().find(|(name, _, _)| Some(*name) == unit);
    match named {
        Some((_, RunsAs::Root, most)) if uid == 0 => Some(*most),
        Some((_, RunsAs::Lampos, most)) if Some(uid) == lampos => Some(*most),
        Some(_) => Some(Priority::Idle),
        None if uid == 0 => None,
        None => Some(Priority::Idle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_unit_asks_at_most_at_its_own_and_only_as_its_own_user() {
        use Priority::*;
        let lampos = Some(990);
        let most = |unit, uid| most_for(unit, uid, lampos);
        assert_eq!(most(Some("lamp-voice.service"), 0), Some(Person));
        assert_eq!(most(Some("lamp-cloud.service"), 990), Some(Conversation));
        assert_eq!(most(Some("lamp-actions.service"), 990), Some(Conversation));
        assert_eq!(most(Some("lamp-skills.service"), 990), Some(Skill));
        assert_eq!(most(Some("lamp-control.service"), 990), Some(Skill));
        assert_eq!(most(Some("lamp-addons.service"), 990), Some(Idle));
        assert_eq!(most(Some("lamp-expression.service"), 990), Some(Idle));
        // Root's own helpers keep what they ask.
        assert_eq!(most(Some("lamp-waking.service"), 0), None);
        assert_eq!(most(None, 0), None);
        // A known name with another unit's user is nobody's.
        assert_eq!(most(Some("lamp-voice.service"), 990), Some(Idle));
        assert_eq!(most(Some("lamp-skills.service"), 1000), Some(Idle));
        assert_eq!(most(Some("lamp-skills.service"), 0), Some(Idle));
        assert_eq!(most_for(Some("lamp-skills.service"), 990, None), Some(Idle));
        // Anyone else asks at idle.
        assert_eq!(most(None, 1000), Some(Idle));
        assert_eq!(most(Some("lamp-eyes.service"), 990), Some(Idle));
    }

    fn claim(priority: Priority, until_us: u64) -> Claim {
        Claim {
            source: "skill:welcome-back".into(),
            priority,
            clock_id: "boot-one".into(),
            until_us,
        }
    }

    #[test]
    fn a_person_comes_before_safety_the_conversation_skills_and_idle_life() {
        use Priority::*;
        let mut order = [Skill, Person, Idle, Safety, Conversation];
        order.sort();
        assert_eq!(order, [Idle, Skill, Conversation, Safety, Person]);
        assert_eq!(
            serde_json::to_string(&Conversation).unwrap(),
            "\"conversation\""
        );
    }

    #[test]
    fn a_claim_holds_on_its_own_clock_until_its_time_and_never_too_far_ahead() {
        let held = claim(Priority::Skill, 10_000_000);
        assert!(held.check().is_ok());
        assert!(held.holds_at("boot-one", 9_999_999));
        assert!(!held.holds_at("boot-one", 10_000_000));
        assert!(!held.holds_at("boot-two", 5_000_000));
        // Asked for longer than 30 s from now: out of reach, not held.
        let far = claim(Priority::Skill, 40_000_001);
        assert!(!far.holds_at("boot-one", 10_000_000));
        assert!(far.holds_at("boot-one", 10_000_001));
        for wrong in ["", "x\u{7}", &"y".repeat(65)] {
            let mut bad = claim(Priority::Idle, 1);
            bad.source = wrong.into();
            assert!(bad.check().is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn a_light_request_is_a_claim_and_a_light_there_is() {
        let request = LightRequest {
            claim: claim(Priority::Skill, 10_000_000),
            light: Light::new(true, "warm", 3).unwrap(),
            pattern: None,
            fade_ms: None,
        };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(
            text,
            "{\"claim\":{\"source\":\"skill:welcome-back\",\"priority\":\"skill\",\
             \"clock_id\":\"boot-one\",\"until_us\":10000000},\
             \"light\":{\"on\":true,\"colour\":\"warm\",\"level\":3}}"
        );
        assert_eq!(
            serde_json::from_str::<LightRequest>(&text).unwrap(),
            request
        );
        assert!(request.check().is_ok());
        let mut dark = request.clone();
        dark.light.level = 9;
        assert!(dark.check().is_err());
        let declared = crate::declaration_of(LightRequest::TOPIC).unwrap();
        assert_eq!(
            (declared.type_name.as_str(), declared.kind),
            (LightRequest::TYPE, Kind::Event)
        );
    }

    /// #940 item 3: a pattern rides on the request, inside its kind's
    /// bounds and under the light's level; without one the message is as
    /// before.
    #[test]
    fn a_light_request_may_carry_a_pattern_within_its_bounds() {
        let breath = LightPattern {
            kind: PatternKind::Breathe,
            period_ms: LightPattern::BREATHE_DEFAULT_MS,
            low: 1,
        };
        let request = LightRequest {
            claim: claim(Priority::Skill, 10_000_000),
            light: Light::new(true, "warm", 3).unwrap(),
            pattern: Some(breath),
            fade_ms: None,
        };
        let text = serde_json::to_string(&request).unwrap();
        assert!(
            text.ends_with(",\"pattern\":{\"kind\":\"breathe\",\"period_ms\":6000,\"low\":1}}"),
            "{text}"
        );
        assert_eq!(
            serde_json::from_str::<LightRequest>(&text).unwrap(),
            request
        );
        assert!(request.check().is_ok());
        let wrong = |pattern: LightPattern, on: bool| {
            let mut request = request.clone();
            request.pattern = Some(pattern);
            request.light.on = on;
            request.check().is_err()
        };
        for (kind, ms) in [
            (PatternKind::Breathe, 3_999),
            (PatternKind::Breathe, 12_001),
            (PatternKind::Candle, 999),
            (PatternKind::Pulse, 2_999),
            (PatternKind::Pulse, 6_001),
        ] {
            let pattern = LightPattern {
                kind,
                period_ms: ms,
                low: 1,
            };
            assert!(wrong(pattern, true), "{kind:?} {ms}");
        }
        assert!(wrong(LightPattern { low: 0, ..breath }, true));
        assert!(
            wrong(LightPattern { low: 3, ..breath }, true),
            "low under the level"
        );
        assert!(wrong(breath, false), "a light off has no pattern");
        for kind in [
            PatternKind::Breathe,
            PatternKind::Candle,
            PatternKind::Pulse,
        ] {
            let pattern = LightPattern {
                kind,
                period_ms: LightPattern::default_ms(kind),
                low: 2,
            };
            assert!(pattern.check(3).is_ok(), "{kind:?}'s default");
        }
        // A candle only from level 3; a pulse at any level, its low unused.
        let candle = LightPattern {
            kind: PatternKind::Candle,
            period_ms: LightPattern::CANDLE_DEFAULT_MS,
            low: 1,
        };
        assert!(candle.check(2).is_err());
        assert!(candle.check(3).is_ok());
        let pulse = LightPattern {
            kind: PatternKind::Pulse,
            period_ms: LightPattern::PULSE_DEFAULT_MS,
            low: 1,
        };
        assert!(pulse.check(1).is_err(), "unseen at level 1");
        assert!(pulse.check(2).is_ok());
    }

    /// #1677: a light going out asks the ring to fade into it, from a
    /// second to a minute; a request without one is written as before.
    #[test]
    fn a_light_request_may_ask_to_fade_in_from_a_second_to_a_minute() {
        let out = LightRequest {
            claim: claim(Priority::Skill, 10_000_000),
            light: Light::new(false, "warm", 3).unwrap(),
            pattern: None,
            fade_ms: Some(30_000),
        };
        let text = serde_json::to_string(&out).unwrap();
        assert!(text.ends_with(",\"fade_ms\":30000}"), "{text}");
        assert_eq!(serde_json::from_str::<LightRequest>(&text).unwrap(), out);
        assert!(out.check().is_ok());
        for (fade, holds) in [(999, false), (1_000, true), (60_000, true), (60_001, false)] {
            let asked = LightRequest {
                fade_ms: Some(fade),
                ..out.clone()
            };
            assert_eq!(asked.check().is_ok(), holds, "{fade} ms");
        }
        // An older asker's request reads as one with no fade.
        let older = text.replace(",\"fade_ms\":30000", "");
        let read: LightRequest = serde_json::from_str(&older).unwrap();
        assert_eq!(read.fade_ms, None);
        assert!(!serde_json::to_string(&read).unwrap().contains("fade"));
    }

    #[test]
    fn a_light_reached_names_its_asker_and_the_light() {
        let reached = LightReached {
            source: "skill:desk-tutor".into(),
            light: Light::new(true, "white", 5).unwrap(),
        };
        let text = serde_json::to_string(&reached).unwrap();
        assert_eq!(
            text,
            "{\"source\":\"skill:desk-tutor\",\
             \"light\":{\"on\":true,\"colour\":\"white\",\"level\":5}}"
        );
        assert_eq!(
            serde_json::from_str::<LightReached>(&text).unwrap(),
            reached
        );
        let declared = crate::declaration_of(LightReached::TOPIC).unwrap();
        assert_eq!(
            (declared.type_name.as_str(), declared.kind),
            (LightReached::TYPE, Kind::Event)
        );
    }

    #[test]
    fn a_light_outranked_names_its_asker_and_what_is_over_it() {
        let outranked = LightOutranked {
            source: "skill:stretch-break".into(),
            by: "skill:desk-tutor".into(),
        };
        let text = serde_json::to_string(&outranked).unwrap();
        assert_eq!(
            text,
            "{\"source\":\"skill:stretch-break\",\"by\":\"skill:desk-tutor\"}"
        );
        assert_eq!(
            serde_json::from_str::<LightOutranked>(&text).unwrap(),
            outranked
        );
        let declared = crate::declaration_of(LightOutranked::TOPIC).unwrap();
        assert_eq!(
            (declared.type_name.as_str(), declared.kind),
            (LightOutranked::TYPE, Kind::Event)
        );
    }

    /// docs/control.schema.json's colours and patterns are the lamp's own.
    #[test]
    fn the_control_schemas_colours_and_patterns_are_the_lamps() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/control.schema.json")).unwrap();
        let names = |def: &str| -> Vec<String> {
            schema["$defs"][def]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(names("colour"), crate::Light::COLOURS);
        // A new kind does not build here until it is listed below and in the schema.
        let _listed = |kind: PatternKind| match kind {
            PatternKind::Breathe | PatternKind::Candle | PatternKind::Pulse => (),
        };
        let patterns: Vec<String> = [
            PatternKind::Breathe,
            PatternKind::Candle,
            PatternKind::Pulse,
        ]
        .iter()
        .map(|kind| {
            serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
        assert_eq!(names("pattern"), patterns);
    }
}
