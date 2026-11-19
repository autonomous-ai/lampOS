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
//!    unknown blocks nothing: the first builder's HOLD on #7457), and
//!    unless the speaker's head is down (pitch outside [`PITCH_DEG`]) or
//!    looking away over the line, no question of the lamp's waiting
//!    ([`Line::lamp_asked`]): a person head down talking at a phone just
//!    after the lamp answered is not going on with it (#7721, lamp-74ff,
//!    18 Nov). A pose unread lets it out ([`Said::PoseUnread`]);
//! 3. **said to the lamp:** a face turned to it, facing (pitch inside
//!    [`PITCH_DEG`] and not looking away over the line's looks, when the
//!    eyes say so), seen speaking the line in at least [`FACING_LOOKS`]
//!    looks while facing, and at least [`SHORT_WORDS`] + 1 words, or a
//!    question (a `?` in it; no phrase list decides what a line means);
//!    never after a second voice in the room ([`Line::second_voice`]),
//!    whatever the facing: across a meeting table, or on a call with a
//!    screen, someone talks to the room or the call facing the lamp (#7891).
//!
//! Never held while the lamp waits for a yes or no of its own asking (a
//! held rename, forget or setup save, a read-back's send, a skill's yes:
//! the engine's `a_yes_held`, told for the judging by [`awaiting_yes`]): a
//! reply to the lamp's own question is said to it, however short ("Yes.").
//! Nor, in a live conversation, a line said when the lamp's own newest
//! words asked something or a read-back waits for its answer
//! ([`Line::lamp_asked`], as the rule reads it): the answer window is the
//! conversation going on, whatever the line's length (#7698: on lamp-06c1 on
//! 18 Nov a bare "Yes." to the lamp's read-back was held, many times). Out
//! of the window, a line of [`SHORT_WORDS`] words or fewer that is a plain
//! yes or no (`services::plain_yes`, `services::plain_no`) while the lamp
//! still waits on its question ([`Line::question_waits`]: asked no more than
//! [`QUESTION_WAITS`] ago, or a read-back or a skill's yes within its own
//! wait) is the same answer (the first builder's ruling on #7698: request 99
//! there was one word, held out of the window; "while the lamp is still
//! waiting for the answer", #7570 19:46). A question older than that waits
//! no more: the thread drops exchanges by count, not age, so its
//! `lamp_asked` alone would answer a "No." hours after an "Anything else?"
//! (the Reviewer-lead's HOLD on #7705). Any other line out of the window is
//! judged as before.
//!
//! Anything else loses those signs and is journaled `held: <reason>`
//! ([`Held`]). A lip smack heard as one or two words while the owner faced
//! the lamp was answered as "someone is turned to the lamp" (lamp-74ff,
//! 17 Nov).

/// Where the second route's worker reads the switch: `on` only.
pub const ENVIRONMENT: &str = "LAMP_SAID_TO_ME";

/// A line of this many words or fewer is a short line.
pub const SHORT_WORDS: usize = 2;

/// The fewest looks of a line, out of a conversation's window, that read
/// the face turned to the lamp speaking while facing it for the line to be
/// said to the lamp on its face alone (`said to the lamp, facing and seen
/// speaking`). One frame of a moving mouth is not: on lamp-74ff on 19 Nov
/// meeting talk with no name and no window was let through on one look
/// (#7891, request 151, the first builder at 11:20).
pub const FACING_LOOKS: usize = 2;

/// How long, from the start of the lamp's words that asked, its question
/// still waits for a bare yes or no out of the window: as long as the lamp
/// waits for the answer to its own read-back (`crate::services::PENDING_FOR`)
/// and to a made skill's say-back (`crate::skill_write_yes::YES_WITHIN`).
pub const QUESTION_WAITS: std::time::Duration = crate::services::PENDING_FOR;

/// The head's pitch, in degrees, down positive as `lamp_msgs::Face::pitch`
/// (radians) has it, inside which a face is facing the lamp: a head down to a
/// phone or a desk is not. The eyes' own window, copied (the voice does not
/// depend on the eyes): `lamp-eyes` `facing::MAX_PITCH_UP_DEG` (20) up and
/// `facing::MAX_PITCH_DOWN_DEG` (25) down. A guess, set on the row (#7442).
pub const PITCH_DEG: std::ops::RangeInclusive<f32> = -20.0..=25.0;

/// The pitch, in degrees, the eyes give for marks no head could make
/// (`lamp-eyes` `facing::LOST_DEG`, copied): not a measure of the head, so
/// the speaker's pose in a conversation reads it as unread, not down
/// (#7721: on lamp-74ff the mouth went unread in 1085 of 1125 looks).
pub const LOST_DEG: f32 = 90.0;

/// Whether `pitch` (radians) is the eyes' [`LOST_DEG`] sentinel.
pub fn lost(pitch: f32) -> bool {
    (pitch.to_degrees() - LOST_DEG).abs() < 0.01
}

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
    /// In a conversation's window, the speaker's pitch over the line is
    /// outside [`PITCH_DEG`] (#7721).
    NotFacingInConversation,
    /// In a conversation's window, the speaker looked away over the line
    /// (#7721).
    AwayInConversation,
    /// No face turned to the lamp was seen speaking it.
    NotSeenSpeaking,
    /// A face seen speaking it, but [`SHORT_WORDS`] words or fewer, and no
    /// `?`.
    Short,
    /// Out of the window, the turned face read speaking while facing in
    /// fewer than [`FACING_LOOKS`] looks (#7891).
    FewLooks,
    /// Out of the window, a second voice in the room lately
    /// ([`Line::second_voice`], #7891).
    SecondVoice,
}

impl Held {
    /// The journal's `held: <reason>`.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotFacingPitch => "held: not facing (pitch)",
            Self::LookingAway => "held: looking away",
            Self::NotFacingInConversation => "held: not facing in a conversation (pitch)",
            Self::AwayInConversation => "held: looking away in a conversation",
            Self::NotSeenSpeaking => "held: no face seen speaking it",
            Self::Short => "held: a short line",
            Self::FewLooks => "held: seen speaking facing in fewer than 2 looks",
            Self::SecondVoice => "held: a second voice in the room",
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
    /// The lamp's own question waits for its answer: its newest words
    /// asked something, or a read-back waits for its answer (the rule's
    /// `Attention::lamp_asked`, as the worker tells it).
    pub lamp_asked: bool,
    /// The lamp still waits on its question, as the out-of-window answer
    /// reads it: the thread's question asked no more than
    /// [`QUESTION_WAITS`] ago (`Thread::lamp_asked_within`), or a read-back
    /// (`services::answer_awaited`) or a skill's yes
    /// (`skill_write_yes::answer_awaited`) within its own wait. Unlike
    /// [`Self::lamp_asked`], never a question gone stale.
    pub question_waits: bool,
    /// A face turned to the lamp through it (the eyes' `facing`, yaw).
    pub turned: bool,
    /// The pitch, in radians, down positive (`lamp_msgs::Face::pitch`),
    /// when the eyes give one, over the line: in a conversation's window
    /// the speaker's (`speaking_faces::SpeakingFaces::speaker_pose_between`),
    /// else the turned speaking face's (`turned_pose_between`).
    pub pitch: Option<f32>,
    /// Whether that face looks at the lamp, when the eyes give it, read as
    /// [`Self::pitch`].
    pub looking: Option<bool>,
    /// A face seen speaking it: some face read `yes` in a look of it
    /// (`speaking_faces::SpeakingFaces::seen_speaking_between`).
    pub seen_speaking: bool,
    /// A face in view read silent through it: some look of it had a face
    /// whose mouth read `no`, and no face read `yes`
    /// (`speaking_faces::SpeakingFaces::read_silent_between`). No face, no
    /// eyes or a mouth never read is not: unknown blocks nothing.
    pub read_silent: bool,
    /// How many looks of it read the face turned to the lamp speaking while
    /// facing (`speaking_faces::SpeakingFaces::turned_speaking_facing_looks`).
    pub facing_looks: usize,
    /// A second voice in the room since the lamp last spoke and within
    /// `room_told::BUSY_ENDS_AFTER_US`, as the worker knows one
    /// (`Heeding::second_voice_at`): an earlier line a face in view was read
    /// silent through, none read speaking (a voice with no mouth: a call's
    /// far side, a video, someone off camera), or seen spoken by two faces.
    /// Faces and looks only: no voice print (#7891).
    pub second_voice: bool,
}

impl Line<'_> {
    /// This line with the eyes' signs from `from` to `to`, as the second
    /// route's worker (`Heeding::not_said_to_me`) and the replay of saved
    /// lamp journals (`crate::replay`, #7772) both read them: out of a
    /// conversation's window the turned speaking face's pitch and gaze over
    /// the line (#7891), and how many looks read it speaking facing; in it
    /// the speaker's over the line, turned or not (#7721), the lone face's
    /// pose counted only when no `another_voice` may be saying the line (the
    /// Reviewer-lead's HOLD on #7726); a face seen speaking it (in a
    /// conversation any face, else the turned one) and a face in view read
    /// silent. With it, for the journal: the number of looks that gave the
    /// pose, and whether the lone face's pose was skipped for another voice.
    pub fn with_eyes(
        self,
        eyes: &crate::speaking_faces::SpeakingFaces,
        from: std::time::Instant,
        to: std::time::Instant,
        another_voice: bool,
    ) -> (Self, usize, bool) {
        let (pitch, looking, looks, skipped) = if self.going_on {
            let (pitch, looking, looks) = eyes.speaker_pose_between(from, to, !another_voice);
            let skipped = another_voice
                && eyes.speaker_pose_between(from, to, true) != (pitch, looking, looks);
            (pitch, looking, looks, skipped)
        } else {
            let (pitch, looking, looks) = eyes.turned_pose_between(from, to);
            (pitch, looking, looks, false)
        };
        let line = Self {
            pitch,
            looking,
            seen_speaking: eyes.seen_speaking_between(from, to, self.turned && !self.going_on),
            read_silent: eyes.read_silent_between(from, to),
            facing_looks: eyes.turned_speaking_facing_looks(from, to),
            ..self
        };
        (line, looks, skipped)
    }
}

/// What [`judge`] says of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// Named, or not judged here: its signs stand.
    Stands,
    /// In a live conversation and let out by it.
    InConversation,
    /// In a live conversation and let out by it, the speaker's pose unread
    /// (no pitch, no gaze): journaled so a row can count it (#6902: a head
    /// down may give no pitch; #7721).
    PoseUnread,
    /// Said to the lamp: facing, seen speaking, long enough or a question.
    ToTheLamp,
    /// Not said to the lamp: the face and the conversation are no sign.
    Held(Held),
}

impl Said {
    /// How the journal says it was let through or held.
    pub fn verdict(self) -> &'static str {
        match self {
            Self::Stands => "stands (named, a yes awaited, or not judged here)",
            Self::InConversation => "said to the lamp, in a conversation",
            Self::PoseUnread => "said to the lamp, in a conversation, the speaker's pose unread",
            Self::ToTheLamp => "said to the lamp, facing and seen speaking",
            Self::Held(held) => held.reason(),
        }
    }
}

/// The pose as the journal says it: `pitch <degrees>|unread, looking
/// true|false|unread, <n> looks`.
pub fn pose(pitch: Option<f32>, looking: Option<bool>, looks: usize) -> String {
    let pitch = pitch.map_or("unread".to_owned(), |pitch| {
        format!("{:.0}", pitch.to_degrees())
    });
    let looking = looking.map_or("unread".to_owned(), |looking| looking.to_string());
    format!("pitch {pitch}, looking {looking}, {looks} looks")
}

/// The rule, on one line (see the module).
pub fn judge(line: Line<'_>) -> Said {
    let words = line.heard.split_whitespace().count();
    if named(line) || yes_awaited() {
        return Said::Stands;
    }
    if line.going_on {
        return if line.lamp_asked {
            Said::InConversation
        } else if words <= SHORT_WORDS && !line.seen_speaking && line.read_silent {
            Said::Held(Held::NotSeenSpeaking)
        } else if outside_pitch(line.pitch) {
            Said::Held(Held::NotFacingInConversation)
        } else if line.looking == Some(false) {
            Said::Held(Held::AwayInConversation)
        } else if line.pitch.is_none() && line.looking.is_none() {
            Said::PoseUnread
        } else {
            Said::InConversation
        };
    }
    if !line.turned {
        return Said::Stands;
    }
    // Out of the window, a bare yes or no to the lamp's own question still
    // answers it while the lamp waits on it (the first builder's ruling on
    // #7698: lamp-06c1, 18 Nov, request 99, a one-word "Yes." after the
    // rename's read-back was held), never a stale one (#7705's HOLD).
    if line.question_waits && bare_yes_or_no(line.heard) {
        return Said::InConversation;
    }
    // After a second voice in the room, facing is no sign at all (#7891).
    if line.second_voice {
        return Said::Held(Held::SecondVoice);
    }
    if outside_pitch(line.pitch) {
        Said::Held(Held::NotFacingPitch)
    } else if line.looking == Some(false) {
        Said::Held(Held::LookingAway)
    } else if !line.seen_speaking {
        Said::Held(Held::NotSeenSpeaking)
    } else if words <= SHORT_WORDS && !line.heard.contains('?') {
        Said::Held(Held::Short)
    } else if line.facing_looks < FACING_LOOKS {
        Said::Held(Held::FewLooks)
    } else {
        Said::ToTheLamp
    }
}

/// For the journal: which input let a short line ([`SHORT_WORDS`] words or
/// fewer) in a conversation's window stand, the first in [`judge`]'s order;
/// `None` for a longer line, one out of the window, or one held (#7698).
pub fn short_stood_by(line: Line<'_>) -> Option<&'static str> {
    if !line.going_on
        || line.heard.split_whitespace().count() > SHORT_WORDS
        || matches!(judge(line), Said::Held(_))
    {
        return None;
    }
    Some(if named(line) {
        "named"
    } else if yes_awaited() {
        "yes awaited"
    } else if line.lamp_asked {
        "lamp_asked"
    } else if line.seen_speaking {
        "seen_speaking"
    } else {
        "read_silent false"
    })
}

/// A line of [`SHORT_WORDS`] words or fewer that is a plain yes or a plain
/// no and nothing else (`services::plain_yes`, `services::plain_no`: the
/// read-back's own deterministic test, no question).
fn bare_yes_or_no(heard: &str) -> bool {
    heard.split_whitespace().count() <= SHORT_WORDS
        && (crate::services::plain_yes(heard) || crate::services::plain_no(heard))
}

/// The lamp's name heard in the line's sound or written in its words.
fn named(line: Line<'_>) -> bool {
    line.name_heard
        || crate::attention::called(line.heard)
        || crate::attention::called_by_name_on(line.heard, true)
}

/// A pitch (radians) outside [`PITCH_DEG`]; unknown is not.
pub(crate) fn outside_pitch(pitch: Option<f32>) -> bool {
    pitch.is_some_and(|pitch| !PITCH_DEG.contains(&pitch.to_degrees()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seen speaking in [`FACING_LOOKS`] looks, or not at all.
    fn facing(heard: &str, seen_speaking: bool) -> Line<'_> {
        Line {
            heard,
            turned: true,
            seen_speaking,
            facing_looks: if seen_speaking { FACING_LOOKS } else { 0 },
            ..Line::default()
        }
    }

    /// #7891 (lamp-74ff, 19 Nov, request 151): out of the window, meeting
    /// talk seen speaking facing the lamp in one look is held; in two it is
    /// said to the lamp. After a second voice in the room it is held
    /// whatever the facing, but the name, the window and a bare yes to the
    /// lamp's pending question stand as before.
    #[test]
    fn facing_alone_needs_two_looks_and_no_second_voice() {
        let meeting = "They can do a and terminal. Hey. I terminate. Okay.";
        let once = Line {
            facing_looks: 1,
            ..facing(meeting, true)
        };
        assert_eq!(judge(once), Said::Held(Held::FewLooks));
        assert_eq!(judge(facing(meeting, true)), Said::ToTheLamp);
        let second = |line: Line<'static>| Line {
            second_voice: true,
            ..line
        };
        assert_eq!(
            judge(second(facing(meeting, true))),
            Said::Held(Held::SecondVoice)
        );
        assert_eq!(
            judge(second(facing("Hey Lamp, what is one plus one?", true))),
            Said::Stands
        );
        let window = Line {
            going_on: true,
            ..second(facing(meeting, true))
        };
        assert_eq!(judge(window), Said::PoseUnread);
        let answer = Line {
            question_waits: true,
            ..second(facing("Yes.", false))
        };
        assert_eq!(judge(answer), Said::InConversation);
        assert_eq!(
            awaiting_yes(true, || judge(second(facing("Yes.", false)))),
            Said::Stands
        );
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
        // No face read at all: unknown, so a short reply stands, its pose
        // unread (#7721).
        let unknown = Line {
            heard: "Pasta.",
            going_on: true,
            ..Line::default()
        };
        assert_eq!(judge(unknown), Said::PoseUnread);
        assert_eq!(
            judge(going_on("And tomorrow in Hanoi?", false)),
            Said::PoseUnread
        );
        assert_eq!(judge(going_on("Thanks.", true)), Said::PoseUnread);
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

    /// #7721: in a conversation's window, Jay says six words head down at
    /// his phone, no name, the lamp asked nothing: held, as with his eyes
    /// read away; a level face answered; a pose unread lets it out, said so.
    fn in_the_window(heard: &str, pitch_deg: Option<f32>, looking: Option<bool>) -> Line<'_> {
        Line {
            heard,
            going_on: true,
            seen_speaking: true,
            pitch: pitch_deg.map(f32::to_radians),
            looking,
            ..Line::default()
        }
    }

    #[test]
    fn in_the_window_a_line_said_head_down_to_a_phone_is_held() {
        let line = "Tell Ada I'm running late now";
        assert_eq!(line.split_whitespace().count(), 6);
        assert_eq!(
            judge(in_the_window(line, Some(40.0), None)),
            Said::Held(Held::NotFacingInConversation)
        );
    }

    #[test]
    fn in_the_window_a_line_said_looking_away_is_held() {
        let line = "Tell Ada I'm running late now";
        assert_eq!(
            judge(in_the_window(line, None, Some(false))),
            Said::Held(Held::AwayInConversation)
        );
        assert_eq!(
            judge(in_the_window(line, Some(5.0), Some(false))),
            Said::Held(Held::AwayInConversation)
        );
    }

    #[test]
    fn in_the_window_a_head_down_line_naming_the_lamp_is_answered() {
        let named = in_the_window("Lamp, what time is it?", Some(40.0), Some(false));
        assert_eq!(judge(named), Said::Stands);
    }

    #[test]
    fn in_the_window_a_head_down_yes_to_the_lamps_question_is_answered() {
        let answer = Line {
            lamp_asked: true,
            ..in_the_window("Yes.", Some(40.0), Some(false))
        };
        assert_eq!(judge(answer), Said::InConversation);
        // A longer reply to the lamp's open question, head down: the same.
        let reply = Line {
            lamp_asked: true,
            ..in_the_window("Pasta with garlic, please", Some(40.0), None)
        };
        assert_eq!(judge(reply), Said::InConversation);
        // The lamp's own yes awaited: the same, asked or not.
        let held_yes = in_the_window("Yes.", Some(40.0), Some(false));
        assert_eq!(awaiting_yes(true, || judge(held_yes)), Said::Stands);
    }

    #[test]
    fn in_the_window_a_head_down_line_with_pitch_unread_passes_said_unread() {
        let line = in_the_window("Tell Ada I'm running late now", None, None);
        assert_eq!(judge(line), Said::PoseUnread);
    }

    #[test]
    fn in_the_window_a_facing_line_is_answered() {
        let line = in_the_window("And what about tomorrow then?", Some(5.0), Some(true));
        assert_eq!(judge(line), Said::InConversation);
        // Near the edge of the eyes' window, 24 degrees down, still facing.
        let edge = in_the_window("And what about tomorrow then?", Some(24.0), None);
        assert_eq!(judge(edge), Said::InConversation);
    }

    /// #7721: lost marks (the eyes' 90 degrees) are no pitch, and every
    /// decision's journal line gives the pose and the looks used.
    #[test]
    fn lost_marks_are_unread_and_the_journal_says_the_pose() {
        assert!(lost(std::f32::consts::FRAC_PI_2));
        assert!(!lost(80f32.to_radians()));
        assert_eq!(
            pose(Some(40f32.to_radians()), Some(false), 3),
            "pitch 40, looking false, 3 looks"
        );
        assert_eq!(pose(None, None, 0), "pitch unread, looking unread, 0 looks");
        let unread = judge(in_the_window("Tell Ada I'm running late now", None, None));
        assert_eq!(
            unread.verdict(),
            "said to the lamp, in a conversation, the speaker's pose unread"
        );
        assert_eq!(
            Said::Held(Held::NotFacingInConversation).verdict(),
            "held: not facing in a conversation (pitch)"
        );
    }

    /// #7698: the lamp read back what it heard and asked; the person's bare
    /// "Yes." in the answer window, their face in view read silent, is the
    /// conversation going on.
    #[test]
    fn a_bare_yes_to_the_lamps_own_question_is_answered_in_its_window() {
        let answer = |heard| Line {
            heard,
            going_on: true,
            lamp_asked: true,
            read_silent: true,
            ..Line::default()
        };
        for heard in ["Yes.", "Yeah.", "No."] {
            assert_eq!(judge(answer(heard)), Said::InConversation, "{heard}");
        }
    }

    /// The first builder's ruling on #7698 (lamp-06c1, request 99): past the
    /// window, Jay's face turned to the lamp and not seen speaking, a bare
    /// yes or no to the lamp's pending question still answers it; anything
    /// else past the window is judged as before.
    #[test]
    fn a_bare_yes_or_no_to_the_lamps_pending_question_is_answered_out_of_the_window() {
        let late = |heard, lamp_asked| Line {
            heard,
            going_on: false,
            turned: true,
            lamp_asked,
            question_waits: lamp_asked,
            read_silent: true,
            ..Line::default()
        };
        for heard in ["Yes.", "Yeah.", "No.", "Yes please."] {
            assert_eq!(judge(late(heard, true)), Said::InConversation, "{heard}");
        }
        // No question pending: held as before.
        assert_eq!(
            judge(late("Yes.", false)),
            Said::Held(Held::NotSeenSpeaking)
        );
        // The Reviewer-lead's HOLD on #7705: the thread's question gone
        // stale (past `QUESTION_WAITS`), still `lamp_asked`: held.
        for heard in ["Yes.", "No."] {
            let stale = Line {
                question_waits: false,
                ..late(heard, true)
            };
            assert_eq!(
                judge(stale),
                Said::Held(Held::NotSeenSpeaking),
                "{heard} to a stale question"
            );
        }
        // A question pending, but not a bare yes or no: judged as before.
        assert_eq!(
            judge(late("Pasta.", true)),
            Said::Held(Held::NotSeenSpeaking)
        );
        assert_eq!(
            judge(late("Yes, and Ada too", true)),
            Said::Held(Held::NotSeenSpeaking)
        );
        assert_eq!(judge(late("Yes?", true)), Said::Held(Held::NotSeenSpeaking));
    }

    /// #7698's control: two people talking, Jay and Ada, the lamp asked
    /// nothing; Ada's bare "Yes." to Jay, her face read silent to the lamp,
    /// stays held, in a conversation's window or out of one.
    #[test]
    fn a_bare_yes_with_no_question_pending_between_two_people_stays_held() {
        let between = Line {
            heard: "Yes.",
            going_on: true,
            lamp_asked: false,
            read_silent: true,
            ..Line::default()
        };
        assert_eq!(judge(between), Said::Held(Held::NotSeenSpeaking));
        assert_eq!(judge(facing("Yes.", true)), Said::Held(Held::Short));
    }

    /// #7698: the journal says which input let a short line in the window
    /// stand; nothing for a held one, a longer one or one out of the window.
    #[test]
    fn a_short_line_standing_in_the_window_says_which_input_let_it() {
        let short = Line {
            heard: "Yes.",
            going_on: true,
            ..Line::default()
        };
        let named = Line {
            name_heard: true,
            read_silent: true,
            ..short
        };
        assert_eq!(short_stood_by(named), Some("named"));
        let asked = Line {
            lamp_asked: true,
            read_silent: true,
            ..short
        };
        assert_eq!(short_stood_by(asked), Some("lamp_asked"));
        let seen = Line {
            seen_speaking: true,
            read_silent: true,
            ..short
        };
        assert_eq!(short_stood_by(seen), Some("seen_speaking"));
        assert_eq!(short_stood_by(short), Some("read_silent false"));
        let silent = Line {
            read_silent: true,
            ..short
        };
        assert_eq!(
            awaiting_yes(true, || short_stood_by(silent)),
            Some("yes awaited")
        );
        // Held, longer, or out of the window: nothing.
        assert_eq!(short_stood_by(silent), None);
        let longer = Line {
            heard: "Turn the light on",
            ..short
        };
        assert_eq!(short_stood_by(longer), None);
        let late = Line {
            going_on: false,
            ..short
        };
        assert_eq!(short_stood_by(late), None);
    }
}
