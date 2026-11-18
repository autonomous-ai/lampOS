//! Whether a line was said to the lamp, by the face and eyes toward it and
//! by what is said (`LAMP_SAID_TO_ME=on`, #7442; the design on #7442). One
//! predicate, [`judge`], read by the second route's worker
//! (`second_provider::worker::Heeding::silence_turned`) before a line is
//! judged: what it holds leaves the line no face sign and no conversation
//! sign, so every use of those signs in `attention` (the `TURNED` admission,
//! `Attention::face_shows`, `name_or_follow_up`'s busy room, the name's
//! second sign) reads the same answer. `LAMP_TURNED_FACE_SPEAKS` holds a
//! subset of what it holds.
//!
//! A line is let out when:
//! 1. **named:** the lamp's name is heard or written in it;
//! 2. **in a live conversation** (its window): unless it is 0 to
//!    [`SHORT_WORDS`] words, no face seen speaking it and a face in view
//!    read silent (no face, no eyes or a mouth never read is unknown, and
//!    unknown blocks nothing: the first builder's HOLD on #7457);
//! 3. **said to the lamp:** a face turned to it, facing (pitch inside
//!    [`PITCH_DEG`] and not looking away, when the eyes say so), seen
//!    speaking the line, and at least [`SHORT_WORDS`] + 1 words, or a
//!    question (a `?` in it; no phrase list decides what a line means).
//!
//! Never held while the lamp waits for a yes or no of its own asking (a
//! held rename, forget or setup save, a read-back's send, a skill's yes:
//! the engine's `a_yes_held`, told for the judging by [`awaiting_yes`]): a
//! reply to the lamp's own question is said to it, however short ("Yes.").
//!
//! Anything else loses those signs and is journaled `held: <reason>`
//! ([`Held`]). A lip smack heard as one or two words while the owner faced
//! the lamp was answered as "someone is turned to the lamp" (lamp-74ff,
//! 17 Nov).

/// Where the second route's worker reads the switch: `on` only.
pub const ENVIRONMENT: &str = "LAMP_SAID_TO_ME";

/// A line of this many words or fewer is a short line.
pub const SHORT_WORDS: usize = 2;

/// The head's pitch, in degrees, down positive as `lamp_msgs::Face::pitch`
/// (radians) has it, inside which a face is facing the lamp: a head down to a
/// phone or a desk is not. The eyes' own window, copied (the voice does not
/// depend on the eyes): `lamp-eyes` `facing::MAX_PITCH_UP_DEG` (20) up and
/// `facing::MAX_PITCH_DOWN_DEG` (25) down. A guess, set on the row (#7442).
pub const PITCH_DEG: std::ops::RangeInclusive<f32> = -20.0..=25.0;

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

thread_local! {
    static YES_AWAITED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `judge` run inside `f` is told whether the lamp waits for a yes or no
/// (`awaited`): the engine judges a line on its own thread, with no await
/// between, so the rule reads it as [`yes_awaited`].
pub fn awaiting_yes<R>(awaited: bool, f: impl FnOnce() -> R) -> R {
    let before = YES_AWAITED.with(|cell| cell.replace(awaited));
    let result = f();
    YES_AWAITED.with(|cell| cell.set(before));
    result
}

/// Whether the line being judged is said while the lamp waits for a yes or
/// no ([`awaiting_yes`]).
pub fn yes_awaited() -> bool {
    YES_AWAITED.with(std::cell::Cell::get)
}

/// Why a line is not said to the lamp, as the journal says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// Its face's pitch is outside [`PITCH_DEG`].
    NotFacingPitch,
    /// The eyes say it is looking away.
    LookingAway,
    /// No face turned to the lamp was seen speaking it.
    NotSeenSpeaking,
    /// A face seen speaking it, but [`SHORT_WORDS`] words or fewer, and no
    /// `?`.
    Short,
}

impl Held {
    /// The journal's `held: <reason>`.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotFacingPitch => "held: not facing (pitch)",
            Self::LookingAway => "held: looking away",
            Self::NotSeenSpeaking => "held: no face seen speaking it",
            Self::Short => "held: a short line",
        }
    }
}

/// What the eyes and ears give [`judge`] for one line.
#[derive(Clone, Copy, Debug, Default)]
pub struct Line<'a> {
    /// Its words as heard.
    pub heard: &'a str,
    /// The lamp's name heard in its sound.
    pub name_heard: bool,
    /// Inside a live conversation's window.
    pub going_on: bool,
    /// A face turned to the lamp through it (the eyes' `facing`, yaw).
    pub turned: bool,
    /// That face's pitch, in radians, down positive (`lamp_msgs::Face::pitch`), when the eyes give one.
    pub pitch: Option<f32>,
    /// Whether that face looks at the lamp, when the eyes give it.
    pub looking: Option<bool>,
    /// A face seen speaking it: some face read `yes` in a look of it
    /// (`speaking_faces::SpeakingFaces::seen_speaking_between`).
    pub seen_speaking: bool,
    /// A face in view read silent through it: some look of it had a face
    /// whose mouth read `no`, and no face read `yes`
    /// (`speaking_faces::SpeakingFaces::read_silent_between`). No face, no
    /// eyes or a mouth never read is not: unknown blocks nothing.
    pub read_silent: bool,
}

/// What [`judge`] says of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// Named, or not judged here: its signs stand.
    Stands,
    /// In a live conversation and let out by it.
    InConversation,
    /// Said to the lamp: facing, seen speaking, long enough or a question.
    ToTheLamp,
    /// Not said to the lamp: the face and the conversation are no sign.
    Held(Held),
}

/// The rule, on one line (see the module).
pub fn judge(line: Line<'_>) -> Said {
    let words = line.heard.split_whitespace().count();
    let named = line.name_heard
        || crate::attention::called(line.heard)
        || crate::attention::called_by_name_on(line.heard, true);
    if named || yes_awaited() {
        return Said::Stands;
    }
    if line.going_on {
        return if words <= SHORT_WORDS && !line.seen_speaking && line.read_silent {
            Said::Held(Held::NotSeenSpeaking)
        } else {
            Said::InConversation
        };
    }
    if !line.turned {
        return Said::Stands;
    }
    if line
        .pitch
        .is_some_and(|pitch| !PITCH_DEG.contains(&pitch.to_degrees()))
    {
        Said::Held(Held::NotFacingPitch)
    } else if line.looking == Some(false) {
        Said::Held(Held::LookingAway)
    } else if !line.seen_speaking {
        Said::Held(Held::NotSeenSpeaking)
    } else if words <= SHORT_WORDS && !line.heard.contains('?') {
        Said::Held(Held::Short)
    } else {
        Said::ToTheLamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facing(heard: &str, seen_speaking: bool) -> Line<'_> {
        Line {
            heard,
            turned: true,
            seen_speaking,
            ..Line::default()
        }
    }

    /// #7442's rules, one by one.
    #[test]
    fn a_line_is_said_to_the_lamp_by_name_conversation_or_face_and_words() {
        // Named: answered, head down or not, seen speaking or not.
        let named = Line {
            pitch: Some(50f32.to_radians()),
            ..facing("Lamp, hi.", false)
        };
        assert_eq!(judge(named), Said::Stands);
        // A lip smack facing the lamp, no face seen speaking: held.
        assert_eq!(
            judge(facing("Mm.", false)),
            Said::Held(Held::NotSeenSpeaking)
        );
        // The same seen speaking: still a short line, no `?`.
        assert_eq!(judge(facing("Mm.", true)), Said::Held(Held::Short));
        // A short question seen speaking: answered.
        assert_eq!(judge(facing("Weather today?", true)), Said::ToTheLamp);
        assert_eq!(judge(facing("What time is it", true)), Said::ToTheLamp);
        // Three words, not seen speaking: held.
        assert_eq!(
            judge(facing("What time is it", false)),
            Said::Held(Held::NotSeenSpeaking)
        );
        // Head down to a phone, or looking away: held; unknown blocks nothing.
        let down = Line {
            pitch: Some(40f32.to_radians()),
            ..facing("Can you call Ada back?", true)
        };
        assert_eq!(judge(down), Said::Held(Held::NotFacingPitch));
        let away = Line {
            looking: Some(false),
            ..facing("Can you call Ada back?", true)
        };
        assert_eq!(judge(away), Said::Held(Held::LookingAway));
        let level = Line {
            pitch: Some(5f32.to_radians()),
            looking: Some(true),
            ..facing("Can you call Ada back?", true)
        };
        assert_eq!(judge(level), Said::ToTheLamp);
        // In a live conversation: answered, but a short line needs a face
        // seen speaking it.
        // A face in view, read speaking or read silent.
        let going_on = |heard, seen_speaking| Line {
            heard,
            going_on: true,
            seen_speaking,
            read_silent: !seen_speaking,
            ..Line::default()
        };
        // No face read at all: unknown, so a short reply stands.
        let unknown = Line {
            heard: "Pasta.",
            going_on: true,
            ..Line::default()
        };
        assert_eq!(judge(unknown), Said::InConversation);
        assert_eq!(
            judge(going_on("And tomorrow in Hanoi?", false)),
            Said::InConversation
        );
        assert_eq!(judge(going_on("Thanks.", true)), Said::InConversation);
        assert_eq!(
            judge(going_on("Uh huh.", false)),
            Said::Held(Held::NotSeenSpeaking)
        );
        // Nobody turned, not going on: not this rule's.
        assert_eq!(judge(Line::default()), Said::Stands);
        // A yes the lamp waits for stands, in a conversation or out of one.
        assert_eq!(
            awaiting_yes(true, || judge(going_on("Yes.", false))),
            Said::Stands
        );
        assert_eq!(
            awaiting_yes(true, || judge(facing("Yes.", true))),
            Said::Stands
        );
        assert_eq!(
            judge(going_on("Yes.", false)),
            Said::Held(Held::NotSeenSpeaking)
        );
    }
}
