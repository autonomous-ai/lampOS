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
}

impl LightRequest {
    pub const TYPE: &str = "lamp_msgs/LightRequest";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "light/request";

    /// Each request is a message of its own: several askers share the topic.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.claim.check()?;
        self.light.check()
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
