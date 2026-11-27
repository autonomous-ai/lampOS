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
//!    18 Nov). A pose unread lets it out ([`Said::PoseUnread`]), but with
//!    [`Line::looks_needed`] (`LAMP_NAME_SIGN_TIED`, #8092) a line with no
//!    look is held, but for one answer to the lamp's waiting question or
//!    one line that cut in over the lamp's own answer ([`Line::cut_in`],
//!    #8205), sharing that one use;
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
//! With `LAMP_ASKED_LOOKS=on` ([`ASKED_LOOKS`], #8395) the lamp's question
//! lets a line in the window skip the pitch and look checks only when the
//! line is the first after the lamp's words and begins within
//! `follows_on::WITHIN_US` (20 s) of their end
//! ([`Line::first_after_asked`]); any later line, or one answering an older
//! question, is judged by its pose as a line with no question waiting
//! (lamp-74ff, 23 Nov 16:09-16:21: in a meeting, lines looking away were let
//! in on a question asked 423 s before).
//!
//! Anything else loses those signs and is journaled `held: <reason>`
//! ([`Held`]). A lip smack heard as one or two words while the owner faced
//! the lamp was answered as "someone is turned to the lamp" (lamp-74ff,
//! 17 Nov).

/// Where the second route's worker reads the switch: `on` only.
pub const ENVIRONMENT: &str = "LAMP_SAID_TO_ME";

/// Where the second route's worker reads whether only a hearing of the
/// name that counts for the line's own sound
/// ([`crate::attention::hearing_counts_for_line`]) makes it named
/// ([`Line::name_heard`]), not any hearing drained with it, however old:
/// `on` only (#8384).
pub const NAME_IN_LINE_ENVIRONMENT: &str = "LAMP_SAID_TO_ME_NAME_IN_LINE";

/// Where the second route's worker reads `LAMP_ASKED_LOOKS` (#8395): `on`
/// only.
pub const ASKED_LOOKS: &str = "LAMP_ASKED_LOOKS";

/// Whether `LAMP_ASKED_LOOKS` is on in this process's environment.
pub fn asked_looks_on() -> bool {
    std::env::var(ASKED_LOOKS).ok().as_deref() == Some("on")
}

/// Where the second route's worker reads `LAMP_FOLLOW_UPS`, the product
/// owner's rules of 25 Nov 2026 (#7570, 18:16 and 18:30): `on` only. A
/// line in a live conversation's window is said to the lamp, looking or
/// not, unless another voice than the conversation's said it
/// ([`Line::follow_ups`], [`Said::FollowUp`]): no line is held on a camera
/// sign alone (its look, its mouth, its pose), as the camera can't yet tell
/// them reliably (lamp-74ff, 25 Nov: the mouth unread in 1,085 of 1,125
/// frames, "looking away" in 40 of 40 while the owner looked at the lamp).
pub const FOLLOW_UPS: &str = "LAMP_FOLLOW_UPS";

/// Whether `LAMP_FOLLOW_UPS` is on in this process's environment.
pub fn follow_ups_on() -> bool {
    std::env::var(FOLLOW_UPS).ok().as_deref() == Some("on")
}

/// Where the second route's worker reads `LAMP_FOLLOW_UPS_AFTER_THE_ANSWER`:
/// `on` only. With [`FOLLOW_UPS`], a line begun over the lamp's own answer
/// takes no follow-up pass ([`Line::follow_ups`] is off for it), and it is
/// judged as it was before `LAMP_FOLLOW_UPS`. The product owner's rule 2 of
/// 25 Nov 2026 is for follow-ups after the lamp's answer. A line said over
/// the answer gets no voice print (the worker prints only lines begun with
/// the lamp quiet), so the window could never tell another voice there. On
/// lamp-74ff on 26 Nov, from 01:55 to 02:05, October's two-person battery
/// let 41 lines of two people's talk over the lamp's answers out as
/// follow-ups, and 40 were answered. Their talk was left alone 11 times in
/// 29, against 74.7% in October.
pub const FOLLOW_UPS_AFTER_THE_ANSWER: &str = "LAMP_FOLLOW_UPS_AFTER_THE_ANSWER";

/// Whether `LAMP_FOLLOW_UPS_AFTER_THE_ANSWER` is on in this process's
/// environment.
pub fn follow_ups_after_the_answer_on() -> bool {
    std::env::var(FOLLOW_UPS_AFTER_THE_ANSWER).ok().as_deref() == Some("on")
}

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
    // The judging thread sets and restores this value around awaiting_yes;
    // synchronous judge calls on that same thread read it.
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
    /// In the window with [`Line::looks_needed`]: no look, no face turned
    /// and the speaker's pose unread, and no first answer to the lamp's
    /// waiting question (#8092) nor first cut-in over its own answer
    /// (#8205).
    NoLook,
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
            Self::NoLook => "held: no look in a conversation",
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
    /// `LAMP_ASKED_LOOKS=on` (#8395): [`Self::lamp_asked`] lets the line
    /// skip the pose checks in the window only with
    /// [`Self::first_after_asked`].
    pub asked_looks: bool,
    /// The first line with words after the lamp's words, begun within
    /// `follows_on::WITHIN_US` of their end (the worker's
    /// `Heeding::first_after_asked`).
    pub first_after_asked: bool,
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
    /// `LAMP_NAME_SIGN_TIED` (#8092): in the window, a line with no look
    /// (no face turned to the lamp, the speaker's pose unread:
    /// [`Said::PoseUnread`] counts as no look) is held, but for one answer
    /// to the lamp's waiting question ([`Said::AnswerNoLook`]).
    pub looks_needed: bool,
    /// With [`Self::looks_needed`]: that one answer with no look was let
    /// out since the last line let out with a look or the name.
    pub look_free_answered: bool,
    /// It began over the lamp's own answer as the lamp was heard saying it,
    /// and so cut it: the coordinator admits such a line only by stopping
    /// the answer (`commit_admitted_input`'s `user_interrupted`; the
    /// journal's `N of its answer's M words were played ... before it was
    /// cut`). The second route's worker reads it from the line's Start
    /// (`Heeding::began_over`); a line begun over only the lamp's wait
    /// sound is not one (#8205).
    pub cut_in: bool,
    /// With [`FOLLOW_UPS_AFTER_THE_ANSWER`], a [`Self::cut_in`] line with no
    /// look takes neither [`Said::CutInNoLook`] nor [`Said::AnswerNoLook`]
    /// unless its words tie it to the conversation ([`Self::cut_in_tied`]):
    /// over the lamp's answer there is no print and here no face, so the
    /// words are what tell the asker from the room. On lamp-74ff on 26 Nov,
    /// with the switch's first head, 19 lines of two people's talk over the
    /// lamp's answers came through as that one look-free cut-in, each named
    /// call giving it back. With a blanket hold (its second head), the
    /// asker's own follow-ups and corrections over the answer were held too
    /// (Sixth's row on lamp-52e6: 0 of 3, 0 of 2).
    pub cut_in_held: bool,
    /// With [`Self::cut_in_held`]: the line's words tie it to the exchange
    /// under way (`untied_going_on::tied_read`), correct what was asked
    /// (`untied_going_on::corrects`), or are a short reply to the lamp's
    /// question.
    pub cut_in_tied: bool,
    /// `LAMP_FOLLOW_UPS=on` ([`FOLLOW_UPS`]): in the window, only
    /// [`Self::another_voice`] holds the line.
    pub follow_ups: bool,
    /// Another voice in the conversation: this line's voice print under
    /// `same_voice::DIFFERENT_BELOW` against the conversation's, or a line of
    /// it that showed another person (the worker's `another_voice`). A print
    /// not come yet is no other voice.
    pub another_voice: bool,
}

impl Line<'_> {
    /// The lamp's question lets this line in the window skip the pose
    /// checks: [`Self::lamp_asked`], and with [`Self::asked_looks`] only the
    /// first line soon after the lamp's words (#8395).
    pub fn asked_lets_in(self) -> bool {
        self.lamp_asked && (!self.asked_looks || self.first_after_asked)
    }

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
    /// With [`Line::looks_needed`], in the window with no look: the one
    /// answer to the lamp's waiting question, let out; it renews nothing
    /// (#8092).
    AnswerNoLook,
    /// With [`Line::looks_needed`], in the window with no look: the one
    /// line that cut in over the lamp's own answer ([`Line::cut_in`]), let
    /// out as [`Self::AnswerNoLook`] is, sharing its one use; it renews
    /// nothing (#8205: lamp-52e6's core row w3, "Hey Lamp, ..." said over
    /// the lamp's story, its name not heard).
    CutInNoLook,
    /// With `LAMP_FOLLOW_UPS`, in the window and not said by another voice
    /// than the conversation's: its pose, look and mouth are not read (the
    /// product owner's rules, 25 Nov 2026).
    FollowUp,
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
            Self::AnswerNoLook => {
                "an answer to the lamp's waiting question, no look: it renews nothing"
            }
            Self::CutInNoLook => "a cut-in over the lamp's own answer, no look: it renews nothing",
            Self::FollowUp => {
                "said to the lamp, in a conversation, looking or not: no camera sign holds it alone (LAMP_FOLLOW_UPS)"
            }
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
        // `LAMP_FOLLOW_UPS`, the product owner's rules (25 Nov 2026, #7570
        // 18:16 and 18:30): a follow-up in a conversation is said to the
        // lamp, looking or not, and no line is held on a camera sign alone;
        // only another voice than the conversation's holds it here. On
        // lamp-74ff that evening follow-ups were held "looking away" in 40
        // of 40 looks while the owner looked straight at the lamp, and "no
        // look" while the camera saw no face.
        if line.follow_ups && !line.another_voice {
            return Said::FollowUp;
        }
        // `LAMP_NAME_SIGN_TIED` (#8092): no look, no renewal; one answer
        // to the lamp's waiting question, or one line that cut in over the
        // lamp's own answer (#8205), only.
        if line.looks_needed && !line.turned && line.pitch.is_none() && line.looking.is_none() {
            // `LAMP_FOLLOW_UPS_AFTER_THE_ANSWER`: over the answer, only a
            // line its words tie to the conversation.
            let untied_over = line.cut_in_held && !line.cut_in_tied;
            return if line.look_free_answered {
                Said::Held(Held::NoLook)
            } else if line.lamp_asked && line.question_waits && !untied_over {
                Said::AnswerNoLook
            } else if line.cut_in && !untied_over {
                Said::CutInNoLook
            } else {
                Said::Held(Held::NoLook)
            };
        }
        return if line.asked_lets_in() {
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
    } else if judge(line) == Said::FollowUp {
        "follow_ups"
    } else if line.asked_lets_in() {
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

    /// `LAMP_FOLLOW_UPS`, the product owner's rules of 25 Nov 2026: in a
    /// conversation's window no camera sign holds a line alone. lamp-74ff
    /// that evening held the owner's follow-ups "looking away" (pitch -15,
    /// 40 looks, all looking false) while they looked straight at the lamp,
    /// "no look" while the camera saw no face, and "no face seen speaking"
    /// on a short line. With the switch each is said to the lamp; another
    /// voice than the conversation's still holds it, and out of the window
    /// nothing changes. Without the switch every hold stands as before.
    #[test]
    fn with_follow_ups_no_camera_sign_alone_holds_a_line_in_the_window() {
        let in_window = |heard| Line {
            heard,
            going_on: true,
            ..Line::default()
        };
        let looking_away = Line {
            pitch: Some((-15.0f32).to_radians()),
            looking: Some(false),
            ..in_window("And what is two plus two")
        };
        let head_down = Line {
            pitch: Some(40.0f32.to_radians()),
            looking: Some(true),
            ..in_window("And what about tomorrow")
        };
        let no_look = Line {
            looks_needed: true,
            ..in_window("And what is two plus two")
        };
        let short_silent = Line {
            read_silent: true,
            ..in_window("Thank you")
        };
        let second_voice = Line {
            second_voice: true,
            ..looking_away
        };
        // Without the switch: held, as before.
        assert_eq!(judge(looking_away), Said::Held(Held::AwayInConversation));
        assert_eq!(judge(head_down), Said::Held(Held::NotFacingInConversation));
        assert_eq!(judge(no_look), Said::Held(Held::NoLook));
        assert_eq!(judge(short_silent), Said::Held(Held::NotSeenSpeaking));
        // With it: said to the lamp, whatever the camera read, a second
        // voice by faces and looks alone too.
        let on = |line: Line<'static>| Line {
            follow_ups: true,
            ..line
        };
        for line in [looking_away, head_down, no_look, short_silent, second_voice] {
            assert_eq!(judge(on(line)), Said::FollowUp, "{line:?}");
        }
        // Another voice than the conversation's still holds it.
        assert_eq!(
            judge(Line {
                another_voice: true,
                ..on(looking_away)
            }),
            Said::Held(Held::AwayInConversation)
        );
        // Out of the window nothing changes: a turned face looking away,
        // not named, is held as before.
        let out = Line {
            going_on: false,
            turned: true,
            ..on(looking_away)
        };
        assert_eq!(judge(out), Said::Held(Held::LookingAway));
        // A short line let out so says which input let it stand.
        assert_eq!(short_stood_by(on(short_silent)), Some("follow_ups"));
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

    /// #8395 (lamp-74ff, 23 Nov 16:09-16:21): with `LAMP_ASKED_LOOKS` the
    /// lamp's question lets only the first line soon after its words skip
    /// the pose checks; a later line, or one long after the question
    /// (request 132: asked 423 s before), is judged by its look.
    #[test]
    fn with_asked_looks_only_the_first_line_after_the_question_skips_its_look() {
        let line = |heard, looking, first_after_asked, asked_looks| Line {
            lamp_asked: true,
            asked_looks,
            first_after_asked,
            ..in_the_window(heard, Some(-8.0), looking)
        };
        let talk = "Then we ship it on Monday";
        // (a) The first line, soon after: let in, looking away or not.
        assert_eq!(
            judge(line(talk, Some(false), true, true)),
            Said::InConversation
        );
        // (b)-(d) A later line, one begun past 20 s, or an old question.
        assert_eq!(
            judge(line(talk, Some(false), false, true)),
            Said::Held(Held::AwayInConversation)
        );
        // (e) Looking at the lamp: let in.
        assert_eq!(
            judge(line(talk, Some(true), false, true)),
            Said::InConversation
        );
        // (f) Naming the lamp: stands.
        assert_eq!(
            judge(line("Lamp, ship it on Monday", Some(false), false, true)),
            Said::Stands
        );
        // A short line standing by its look is not said to stand by the
        // question.
        assert_eq!(
            short_stood_by(line("Okay.", Some(true), false, true)),
            Some("seen_speaking")
        );
        // Off: the question lets any line in, as before.
        for first in [true, false] {
            assert_eq!(
                judge(line(talk, Some(false), first, false)),
                Said::InConversation
            );
        }
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

    /// #8092, the first builder's ruling 3 (12:45): lamp-74ff, build 14967,
    /// 20 Nov, a meeting: after a line taken as the name, about 10 lines of
    /// meeting talk with 0 looks were let out as the conversation going on
    /// ("pitch unread, looking unread, 0 looks"). With `LAMP_NAME_SIGN_TIED`,
    /// in the window a line with no look (no face turned, the pose unread)
    /// is held; the one exception is an answer to the lamp's waiting
    /// question, let out once ([`Said::AnswerNoLook`], which renews
    /// nothing). A look, a face turned, or the name let it out as before;
    /// off, as before.
    #[test]
    fn with_looks_needed_a_line_with_no_look_in_the_window_is_held() {
        let talk = Line {
            heard: "We should ship the slides by Friday.",
            going_on: true,
            looks_needed: true,
            ..Line::default()
        };
        assert_eq!(judge(talk), Said::Held(Held::NoLook));
        assert_eq!(
            judge(Line {
                looks_needed: false,
                ..talk
            }),
            Said::PoseUnread
        );
        let answer = Line {
            heard: "Yes, at three.",
            lamp_asked: true,
            question_waits: true,
            ..talk
        };
        assert_eq!(judge(answer), Said::AnswerNoLook);
        assert_eq!(
            judge(Line {
                look_free_answered: true,
                ..answer
            }),
            Said::Held(Held::NoLook)
        );
        // A stale question (`lamp_asked` alone, no clock) is no exception.
        assert_eq!(
            judge(Line {
                question_waits: false,
                ..answer
            }),
            Said::Held(Held::NoLook)
        );
        for looked in [
            Line {
                pitch: Some(0.1),
                ..talk
            },
            Line {
                looking: Some(true),
                ..talk
            },
            Line {
                turned: true,
                ..talk
            },
        ] {
            assert!(!matches!(judge(looked), Said::Held(_)), "{looked:?}");
        }
        assert_eq!(
            judge(Line {
                name_heard: true,
                ..talk
            }),
            Said::Stands
        );
        assert_eq!(
            judge(Line {
                heard: "Lamp, what time is it?",
                ..talk
            }),
            Said::Stands
        );
    }

    /// #8205 (lamp-52e6, core row w3, 21 Nov): in the window with no look,
    /// a line that cut in over the lamp's own answer is let out once, as
    /// the answer to its waiting question is, sharing that one use; the
    /// same line not over the lamp's answer is held as before, and with
    /// `looks_needed` off, or out of the window, nothing changes.
    #[test]
    fn with_looks_needed_one_cut_in_over_the_lamps_answer_is_let_out() {
        let cut_in = Line {
            heard: "wait, what was the dragon's name again?",
            going_on: true,
            looks_needed: true,
            cut_in: true,
            ..Line::default()
        };
        assert_eq!(judge(cut_in), Said::CutInNoLook);
        assert_eq!(
            judge(Line {
                cut_in: false,
                ..cut_in
            }),
            Said::Held(Held::NoLook)
        );
        assert_eq!(
            judge(Line {
                look_free_answered: true,
                ..cut_in
            }),
            Said::Held(Held::NoLook)
        );
        assert_eq!(
            judge(Line {
                looks_needed: false,
                ..cut_in
            }),
            Said::PoseUnread
        );
        assert_eq!(
            judge(Line {
                going_on: false,
                ..cut_in
            }),
            Said::Stands
        );
    }
}
