//! A partner's add-on suggests what the lamp might do (`addon/proposal`,
//! `docs/addons.md`): a skill of kind `addon` proposes, `lamp-skills` writes
//! the proposal on its behalf and stamps who it is, and a judge keeps or
//! drops it. A proposal is never an order: nothing here reaches a speaker,
//! a light or a motor by itself.
use crate::{ClockStamp, Invalid, Light, LightPattern};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// What an add-on may propose.
pub const ADDON_ACTS: [&str; 5] = ["say", "light", "move", "mood", "note"];

/// Why it proposes it: a fixed list, never free text, so the reason a
/// partner gives says nothing the lamp did not already know.
pub const ADDON_WHYS: [&str; 9] = [
    "music",
    "laughter",
    "evening",
    "morning",
    "arrived",
    "left",
    "quiet",
    "long_sitting",
    "mood",
];

/// The topics an add-on may read: summaries only, never what a person
/// said or typed, a face, a picture, a sound or the lamp's memory
/// (`docs/addons.md`, "What an add-on may read").
pub const ADDON_TOPICS: [&str; 6] = [
    "voice/state",
    "room/presence",
    "mood/state",
    "light/state",
    "ears/classes",
    "clock/local",
];

/// A proposal holds this long at most, as a claim's lease does.
pub const ADDON_HOLDS_MS: std::ops::RangeInclusive<u32> = 1_000..=30_000;
/// A mood nudge moves energy or warmth by this much at most...
pub const ADDON_MOOD_AT_MOST: f32 = 0.1;
/// ...for this long at most.
pub const ADDON_MOOD_FOR_MS: std::ops::RangeInclusive<u32> = 1_000..=600_000;
/// A note, as the add-on's folder holds it.
pub const ADDON_NOTE_BYTES: usize = 300;

/// A light an add-on proposes: the same shape a skill's light takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddonLight {
    /// One of [`Light::COLOURS`].
    pub colour: String,
    /// 1 to [`Light::BRIGHTEST`].
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<LightPattern>,
}

/// A small nudge of the lamp's inner state (`mood/state`), for a while.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddonMood {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warmth: Option<f32>,
    pub for_ms: u32,
}

/// An add-on's proposal, as `lamp-skills` writes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddonProposal {
    /// This proposal's number, which the judge's decision names:
    /// `lamp-skills`' monotonic clock in microseconds when it was made, so
    /// it grows and is never 0.
    pub proposal: u64,
    /// The add-on's skill name, as the folder it was loaded from is named:
    /// stamped by `lamp-skills`, never taken from the add-on's own words.
    pub addon: String,
    /// The add-on's version, as loaded.
    pub version: String,
    /// One of [`ADDON_ACTS`].
    pub act: String,
    /// One of [`ADDON_WHYS`].
    pub why: String,
    /// How sure the add-on says it is, 0 to 1: the judge may weigh it,
    /// never be bound by it.
    pub sure: f32,
    /// How long the proposal stays worth doing ([`ADDON_HOLDS_MS`]).
    pub holds_ms: u32,
    /// For `light`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<AddonLight>,
    /// For `move`: one of the body's gestures, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gesture: Option<String>,
    /// For `mood`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<AddonMood>,
    /// For `say` and `note`: what the add-on suggests the lamp might
    /// mention, written in its folder. It reaches the model quoted and
    /// untrusted, never as an instruction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl AddonProposal {
    pub const TYPE: &str = "lamp_msgs/AddonProposal";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "addon/proposal";

    /// Something proposed once: no value is kept, none is promised.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        let invalid = |why: &str| Err(Invalid(why.into()));
        if self.proposal == 0 {
            return invalid("a proposal's number is positive");
        }
        if !addon_name(&self.addon) {
            return invalid("an add-on's name is 1 to 64 lowercase letters, digits and hyphens");
        }
        if !version(&self.version) {
            return invalid("an add-on's version is MAJOR.MINOR.PATCH");
        }
        if !ADDON_WHYS.contains(&self.why.as_str()) {
            return invalid("why is one of the add-on reasons");
        }
        if !(0.0..=1.0).contains(&self.sure) {
            return invalid("sure is 0 to 1");
        }
        if !ADDON_HOLDS_MS.contains(&self.holds_ms) {
            return invalid("a proposal holds 1 s to 30 s");
        }
        // Each act carries its own part and no other.
        let parts = (
            self.light.is_some(),
            self.gesture.is_some(),
            self.mood.is_some(),
            self.note.is_some(),
        );
        let wanted = match self.act.as_str() {
            "light" => (true, false, false, false),
            "move" => (false, true, false, false),
            "mood" => (false, false, true, false),
            "say" | "note" => (false, false, false, true),
            _ => return invalid("an act is say, light, move, mood or note"),
        };
        if parts != wanted {
            return invalid("a proposal carries the part its act names, and no other");
        }
        if let Some(light) = &self.light {
            if !Light::COLOURS.contains(&light.colour.as_str()) {
                return invalid("a light's colour is one of the light's colours");
            }
            if !(1..=Light::BRIGHTEST).contains(&light.level) {
                return invalid("a light's level is 1 to 5");
            }
            if let Some(pattern) = &light.pattern {
                pattern.check(light.level)?;
            }
        }
        if let Some(gesture) = &self.gesture
            && (gesture.is_empty()
                || gesture.len() > 32
                || !gesture
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'))
        {
            return invalid("a gesture is named in lowercase letters and _");
        }
        if let Some(mood) = &self.mood {
            let small = |nudge: Option<f32>| {
                nudge.is_none_or(|nudge| nudge.is_finite() && nudge.abs() <= ADDON_MOOD_AT_MOST)
            };
            if mood.energy.is_none() && mood.warmth.is_none() {
                return invalid("a mood nudge moves energy or warmth");
            }
            if !small(mood.energy) || !small(mood.warmth) {
                return invalid("a mood nudge is at most 0.1 either way");
            }
            if !ADDON_MOOD_FOR_MS.contains(&mood.for_ms) {
                return invalid("a mood nudge lasts 1 s to 10 min");
            }
        }
        if let Some(note) = &self.note
            && (note.trim().is_empty()
                || note.len() > ADDON_NOTE_BYTES
                || note.chars().any(char::is_control))
        {
            return invalid("a note is 1 to 300 bytes on one line");
        }
        Ok(())
    }
}

/// A skill's name, as `lamp-skills` names folders.
fn addon_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// What the add-ons' judge said of one proposal (`addon/decision`, #2530):
/// kept or dropped, by which rule, and why in words for the journal and a
/// person reading the bus. The add-on and its act are named; no words of the
/// proposal's note are said again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddonDecision {
    /// The proposal's number ([`AddonProposal::proposal`]).
    pub proposal: u64,
    pub addon: String,
    pub act: String,
    /// `kept` or `dropped`.
    pub choice: String,
    /// What decided: `rule` in part 1; `jev` for a say in part 2.
    pub by: String,
    /// Why, in at most 200 bytes.
    pub why: String,
}

impl AddonDecision {
    pub const TYPE: &str = "lamp_msgs/AddonDecision";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "addon/decision";
    pub const CHOICES: [&str; 2] = ["kept", "dropped"];
    pub const BY: [&str; 2] = ["rule", "jev"];

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        let invalid = |why: &str| Err(Invalid(why.into()));
        if self.proposal == 0 {
            return invalid("a decision names its proposal's number");
        }
        if !addon_name(&self.addon) {
            return invalid("an add-on's name is 1 to 64 lowercase letters, digits and hyphens");
        }
        if !ADDON_ACTS.contains(&self.act.as_str()) {
            return invalid("an act is say, light, move, mood or note");
        }
        if !Self::CHOICES.contains(&self.choice.as_str()) {
            return invalid("a choice is kept or dropped");
        }
        if !Self::BY.contains(&self.by.as_str()) {
            return invalid("a decision is by rule or jev");
        }
        if self.why.is_empty() || self.why.len() > 200 || self.why.chars().any(char::is_control) {
            return invalid("why is 1 to 200 bytes on one line");
        }
        Ok(())
    }
}

/// An add-on's say, past the judge's rules, put to the policy (#2530 part
/// 2, `addon/ask`): written only by the add-ons' judge (`lamp-addons`), and
/// answered by a [`crate::Decision`] with `about` `addon` naming the
/// `proposal`. It carries no words: the note stays with the judge, and the
/// decision model is shown only why and how sure, as a skill's ask is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddonAsk {
    /// The proposal this asks about ([`AddonProposal::proposal`]).
    pub proposal: u64,
    /// The add-on, as `lamp-skills` named it.
    pub addon: String,
    /// One of [`ADDON_WHYS`].
    pub why: String,
    /// How sure the add-on said it was, 0 to 1.
    pub sure: f32,
}

impl AddonAsk {
    pub const TYPE: &str = "lamp_msgs/AddonAsk";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "addon/ask";
    /// The `about` of the decision that answers it.
    pub const ABOUT: &str = "addon";

    /// Each is a message of its own, answered once.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        let invalid = |why: &str| Err(Invalid(why.into()));
        if self.proposal == 0 {
            return invalid("an add-on's ask names its proposal");
        }
        if self.addon.is_empty()
            || self.addon.len() > 64
            || self.addon.chars().any(char::is_control)
        {
            return invalid("an add-on's name is 1 to 64 bytes on one line");
        }
        if !ADDON_WHYS.contains(&self.why.as_str()) {
            return invalid("an add-on's why is one of ADDON_WHYS");
        }
        if !(0.0..=1.0).contains(&self.sure) {
            return invalid("sure is from 0 to 1");
        }
        Ok(())
    }
}

/// A kept add-on's nudge of Mood (`mood/nudge`, #2530): written only by the
/// add-ons' judge (`lamp-addons`), read only by Mood. It moves energy both
/// ways and warmth up only, each by [`ADDON_MOOD_AT_MOST`] at most,
/// inside Mood's own bounds, until `until_us`, fading over its last minute.
/// The newest replaces the last. Manner only, as the mood itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoodNudge {
    pub stamp: ClockStamp,
    /// The add-on whose proposal was kept, as `lamp-skills` named it.
    pub addon: String,
    pub energy: f32,
    pub warmth: f32,
    /// On the stamp's clock, at most [`ADDON_MOOD_FOR_MS`] after it.
    pub until_us: u64,
}

impl MoodNudge {
    pub const TYPE: &str = "lamp_msgs/MoodNudge";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "mood/nudge";

    /// Each is a message of its own; Mood keeps the newest.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        let most = ADDON_MOOD_AT_MOST;
        if self.stamp.sequence == 0
            || self.addon.is_empty()
            || self.addon.len() > 64
            || self.addon.chars().any(char::is_control)
        {
            return Err(Invalid(
                "a mood nudge needs a positive sequence and a bounded add-on name".into(),
            ));
        }
        if !(-most..=most).contains(&self.energy) || !(0.0..=most).contains(&self.warmth) {
            return Err(Invalid(format!(
                "a mood nudge moves energy within ±{most} and warmth from 0 to {most}"
            )));
        }
        let longest = u64::from(*ADDON_MOOD_FOR_MS.end()) * 1_000;
        if self.until_us <= self.stamp.at_us || self.until_us - self.stamp.at_us > longest {
            return Err(Invalid(
                "a mood nudge holds after its stamp, for ten minutes at most".into(),
            ));
        }
        Ok(())
    }
}

/// The add-ons' optional topics, apart from the first topics.
pub fn addon_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (AddonProposal::TOPIC, AddonProposal::declaration()),
        (AddonDecision::TOPIC, AddonDecision::declaration()),
        (MoodNudge::TOPIC, MoodNudge::declaration()),
        (AddonAsk::TOPIC, AddonAsk::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PatternKind;

    fn glow() -> AddonProposal {
        AddonProposal {
            proposal: 1_792_115_940_123_456,
            addon: "evening-glow".into(),
            version: "1.0.0".into(),
            act: "light".into(),
            why: "evening".into(),
            sure: 0.6,
            holds_ms: 30_000,
            light: Some(AddonLight {
                colour: "warm".into(),
                level: 2,
                pattern: Some(LightPattern {
                    kind: PatternKind::Breathe,
                    period_ms: 8_000,
                    low: 1,
                }),
            }),
            gesture: None,
            mood: None,
            note: None,
        }
    }

    fn lift() -> AddonProposal {
        AddonProposal {
            addon: "happy-with-music".into(),
            act: "mood".into(),
            why: "music".into(),
            light: None,
            mood: Some(AddonMood {
                energy: Some(0.1),
                warmth: None,
                for_ms: 600_000,
            }),
            ..glow()
        }
    }

    #[test]
    fn a_proposal_is_checked_and_its_line_is_as_written() {
        assert_eq!(glow().check(), Ok(()));
        assert_eq!(lift().check(), Ok(()));
        assert_eq!(
            serde_json::to_string(&lift()).unwrap(),
            r#"{"proposal":1792115940123456,"addon":"happy-with-music","version":"1.0.0","act":"mood","why":"music","sure":0.6,"holds_ms":30000,"mood":{"energy":0.1,"for_ms":600000}}"#
        );
        let line: AddonProposal =
            serde_json::from_str(&serde_json::to_string(&glow()).unwrap()).unwrap();
        assert_eq!(line, glow());
        let note = AddonProposal {
            act: "note".into(),
            light: None,
            note: Some("Music is playing.".into()),
            ..glow()
        };
        assert_eq!(note.check(), Ok(()));
        let nod = AddonProposal {
            act: "move".into(),
            light: None,
            gesture: Some("nod".into()),
            ..glow()
        };
        assert_eq!(nod.check(), Ok(()));
    }

    #[test]
    fn a_wrong_proposal_is_refused() {
        let wrong = [
            AddonProposal {
                proposal: 0,
                ..glow()
            },
            AddonProposal {
                addon: "Evening Glow".into(),
                ..glow()
            },
            AddonProposal {
                version: "1.0".into(),
                ..glow()
            },
            AddonProposal {
                why: "because".into(),
                ..glow()
            },
            AddonProposal {
                sure: 1.5,
                ..glow()
            },
            AddonProposal {
                sure: f32::NAN,
                ..glow()
            },
            AddonProposal {
                holds_ms: 999,
                ..glow()
            },
            AddonProposal {
                holds_ms: 30_001,
                ..glow()
            },
            AddonProposal {
                act: "dance".into(),
                ..glow()
            },
            // A light act without its light, or with a mood beside it.
            AddonProposal {
                light: None,
                ..glow()
            },
            AddonProposal {
                mood: lift().mood,
                ..glow()
            },
            AddonProposal {
                light: Some(AddonLight {
                    colour: "ultraviolet".into(),
                    level: 2,
                    pattern: None,
                }),
                ..glow()
            },
            AddonProposal {
                light: Some(AddonLight {
                    colour: "warm".into(),
                    level: 6,
                    pattern: None,
                }),
                ..glow()
            },
            // A breath of 2 s reads as anxious, as for any skill's light.
            AddonProposal {
                light: Some(AddonLight {
                    colour: "warm".into(),
                    level: 2,
                    pattern: Some(LightPattern {
                        kind: PatternKind::Breathe,
                        period_ms: 2_000,
                        low: 1,
                    }),
                }),
                ..glow()
            },
            AddonProposal {
                mood: Some(AddonMood {
                    energy: Some(0.5),
                    warmth: None,
                    for_ms: 1_000,
                }),
                ..lift()
            },
            AddonProposal {
                mood: Some(AddonMood {
                    energy: None,
                    warmth: None,
                    for_ms: 1_000,
                }),
                ..lift()
            },
            AddonProposal {
                mood: Some(AddonMood {
                    energy: Some(0.1),
                    warmth: None,
                    for_ms: 600_001,
                }),
                ..lift()
            },
            AddonProposal {
                act: "say".into(),
                light: None,
                note: Some("a\nb".into()),
                ..glow()
            },
            AddonProposal {
                act: "say".into(),
                light: None,
                note: Some("x".repeat(301)),
                ..glow()
            },
            AddonProposal {
                act: "move".into(),
                light: None,
                gesture: Some("Wave!".into()),
                ..glow()
            },
        ];
        for proposal in wrong {
            assert!(proposal.check().is_err(), "{proposal:?}");
        }
    }

    #[test]
    fn a_decision_is_checked_and_names_no_words() {
        let kept = AddonDecision {
            proposal: 7,
            addon: "evening-glow".into(),
            act: "light".into(),
            choice: "kept".into(),
            by: "rule".into(),
            why: "the lamp is calm".into(),
        };
        assert_eq!(kept.check(), Ok(()));
        assert_eq!(
            serde_json::to_string(&kept).unwrap(),
            r#"{"proposal":7,"addon":"evening-glow","act":"light","choice":"kept","by":"rule","why":"the lamp is calm"}"#
        );
        for wrong in [
            AddonDecision {
                proposal: 0,
                ..kept.clone()
            },
            AddonDecision {
                choice: "maybe".into(),
                ..kept.clone()
            },
            AddonDecision {
                by: "partner".into(),
                ..kept.clone()
            },
            AddonDecision {
                act: "dance".into(),
                ..kept.clone()
            },
            AddonDecision {
                why: String::new(),
                ..kept.clone()
            },
            AddonDecision {
                why: "a\nb".into(),
                ..kept.clone()
            },
        ] {
            assert!(wrong.check().is_err(), "{wrong:?}");
        }
        assert_eq!(
            crate::declaration_of(AddonDecision::TOPIC),
            Some(AddonDecision::declaration())
        );
    }

    #[test]
    fn the_topic_is_declared_as_an_event() {
        assert_eq!(
            crate::declaration_of(AddonProposal::TOPIC),
            Some(AddonProposal::declaration())
        );
    }

    #[test]
    fn a_mood_nudge_is_small_warmth_only_up_and_ten_minutes_at_most() {
        let nudge = MoodNudge {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: 1_000,
                sequence: 1,
            },
            addon: "music-mood".into(),
            energy: -0.1,
            warmth: 0.1,
            until_us: 1_000 + 600_000_000,
        };
        assert_eq!(nudge.check(), Ok(()));
        for wrong in [
            MoodNudge {
                energy: 0.11,
                ..nudge.clone()
            },
            MoodNudge {
                energy: f32::NAN,
                ..nudge.clone()
            },
            MoodNudge {
                warmth: -0.01,
                ..nudge.clone()
            },
            MoodNudge {
                until_us: 1_000,
                ..nudge.clone()
            },
            MoodNudge {
                until_us: 1_001 + 600_000_000,
                ..nudge.clone()
            },
            MoodNudge {
                addon: String::new(),
                ..nudge.clone()
            },
        ] {
            assert!(wrong.check().is_err(), "{wrong:?}");
        }
        assert_eq!(
            crate::declaration_of(MoodNudge::TOPIC),
            Some(MoodNudge::declaration())
        );
    }

    #[test]
    fn an_add_ons_ask_carries_no_words_and_is_checked() {
        let ask = AddonAsk {
            proposal: 7,
            addon: "music-mood".into(),
            why: "music".into(),
            sure: 0.7,
        };
        assert_eq!(ask.check(), Ok(()));
        let json = serde_json::to_value(&ask).unwrap();
        assert!(json.get("note").is_none());
        for wrong in [
            AddonAsk {
                proposal: 0,
                ..ask.clone()
            },
            AddonAsk {
                why: "because".into(),
                ..ask.clone()
            },
            AddonAsk {
                sure: 1.5,
                ..ask.clone()
            },
            AddonAsk {
                addon: String::new(),
                ..ask.clone()
            },
        ] {
            assert!(wrong.check().is_err(), "{wrong:?}");
        }
        assert_eq!(
            crate::declaration_of(AddonAsk::TOPIC),
            Some(AddonAsk::declaration())
        );
    }
}
