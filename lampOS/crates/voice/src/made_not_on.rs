//! A made skill with code of its own that is not on is never said to be on
//! (`LAMP_MADE_NOT_ON_TOLD`, #7270; off unless set, in no unit).
//!
//! On lamp-9adf (16 Nov, build 13383) a voice make the lamp's agent wrote
//! took about four minutes, and its say-back ended "It waits for you to
//! agree before it runs any code." The person's "Yeah." came more than 200 s
//! after those words, past the wait's [`crate::skill_write_yes::YES_WITHIN`],
//! so the model was shown no skill waiting, made no call (`calls=none`), and
//! said "The skill is on the lamp. Next time you sit at your desk, the lamp
//! will read your newest email …". Nothing was on.
//!
//! With the switch, on a heard line after such a say-back while the skill
//! is still not on (its wait neither answered, dropped nor taken by its yes,
//! within [`TOLD_FOR`] of its start):
//!
//! - **Past its wait, the model is told** ([`note`]), in the "For you to
//!   know" form, that the skill is not on and no yes puts it on now, so they
//!   can ask for it to be made again. Within the wait it is already told the
//!   skill waits, with its `approve_id` (`skill_write_yes::waiting_note`).
//! - **The reply is judged by the model, never by a word list** ([`Guard`],
//!   the owner's rule of 13 Nov: the model judges what a line means). Each
//!   round's words are held to the round's end. A round that calls
//!   `make_skill` with an `approve_id` lets them go unsaid (the next round
//!   speaks from that call's answer). Any other round's words are asked of
//!   the same chat model the route uses, with no function declared, in one
//!   short call bounded by [`JUDGE_WITHIN`] ([`judge_messages`]): does this
//!   reply tell the person the skill is on now, or that it will run? In any
//!   language. Yes: the fixed line [`NOT_ON_YET`] (within the wait) or
//!   [`NOT_ON_LATE`] (past it) is said in its place. No: the words are said
//!   as they came. No answer in time, or one that is neither: the words are
//!   said, then the fixed line, which is never false, since the skill really
//!   is not on. The fixed line is said at most once a turn. While the guard
//!   holds, a round's first words wait for their sentence's end. The check
//!   adds its call's time (journalled in ms) to these turns alone.
//!
//! Once the skill's yes is taken (its wait gone), nothing is held: "It's on
//! now." after a yes that ran is said. A skill that writes is on already
//! and its wait is for its sends, so it is never one of these. Off, or with
//! no such skill, the turn is byte for byte as before.
use crate::second_provider::net::Message;
use std::time::{Duration, Instant};

/// The switch: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_MADE_NOT_ON_TOLD";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// How long after its wait began a made skill with code that is not on is
/// told and guarded: a person who answers its say-back minutes late, as on
/// lamp-9adf, is still answering it.
pub const TOLD_FOR: Duration = Duration::from_secs(15 * 60);

/// How many heard turns past a skill's wait are guarded: those that may
/// answer its say-back late. After them the note alone carries it.
pub const LATE_TURNS_GUARDED: u8 = 2;

/// The longest the model's yes or no on a held reply is waited for.
pub const JUDGE_WITHIN: Duration = Duration::from_millis(1_500);

/// Said in place of a reply that says the skill is on, while its yes may
/// still be taken.
pub const NOT_ON_YET: &str =
    "It isn't on yet: say yes to me while you're at the lamp, and I'll turn it on.";

/// Said in place of a reply that says the skill is on, once its yes came too
/// late to be taken.
pub const NOT_ON_LATE: &str =
    "It isn't on yet: your yes didn't come in time, so ask me to make it again.";

/// The note a heard line `request` is told at `now` while a made skill with
/// code of its own is not on ([`crate::skill_write_yes::code_not_on`]):
/// none with the switch off (`on`) or no such skill.
pub fn note(on: bool, request: u64, now: Instant) -> Option<String> {
    if !on {
        return None;
    }
    let (skill, late) = crate::skill_write_yes::code_not_on(request, now, TOLD_FOR)?;
    // Within its wait, `skill_write_yes::waiting_note` tells the model the
    // skill waits, with its `approve_id`.
    if !late {
        return None;
    }
    eprintln!(
        "lamp-voice second: request {request}: the model is told the made skill {skill} is not on, its yes too late (LAMP_MADE_NOT_ON_TOLD)"
    );
    Some(format!(
        "(For you to know, not to answer: the skill {} that the lamp made is NOT on. It waited \
         for the person's yes, and none came in time, so no yes can turn it on now. Never say it \
         is on, turned on, set up or ready. If they agree to it or ask about it, say it isn't on \
         yet, and that they can ask you to make it again.)",
        crate::made_skill::spoken_name(&skill)
    ))
}

/// What the model is asked of a held `reply` about the skill `spoken` (its
/// spoken name): one yes or no, in any language the reply is in.
pub fn judge_messages(spoken: &str, reply: &str) -> Vec<Message> {
    vec![
        Message::system(format!(
            "You check one reply a voice assistant is about to say. The skill \"{spoken}\" is \
             NOT on: the person's yes to it never ran. The reply may be in any language. Does \
             this reply tell the person that the skill {spoken} is on now, or that it will run? \
             Answer only yes or no."
        )),
        Message::user(reply.to_owned()),
    ]
}

/// The model's `answer` to [`judge_messages`]: `Some(true)` for a yes,
/// `Some(false)` for a no, `None` for anything else.
pub fn verdict(answer: &str) -> Option<bool> {
    let first: String = answer
        .trim()
        .chars()
        .take_while(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect();
    match first.as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// A turn's guard of a made skill not on ([`Guard::for_turn`]).
#[derive(Debug)]
pub struct Guard {
    request: u64,
    /// The skill's name, as it is waited for.
    skill: String,
    /// The pieces of the reply held until the round's end: none when
    /// nothing is held.
    held: Option<Vec<String>>,
    /// The fixed line was said in this turn.
    said: bool,
}

/// What a piece of a reply does under a [`Guard`].
#[derive(Debug, PartialEq, Eq)]
pub enum Judged {
    Say,
    /// Held to the round's end, when it is judged.
    Held,
}

/// What becomes of a round's held words ([`Guard::round_ended`]).
#[derive(Debug, PartialEq, Eq)]
pub enum Round {
    /// Nothing held, or let go beside the yes's own call.
    Nothing,
    /// Held words with nothing to say in them: said as they are.
    Say(Vec<String>),
    /// Held words to judge.
    Judge(Vec<String>),
}

impl Guard {
    /// The guard of the heard line `request` (`heard`) at `now`: with the
    /// switch `on`, while a made skill with code of its own is not on and its
    /// wait ran out before it, for the first [`LATE_TURNS_GUARDED`] such
    /// turns only. A turn within the wait streams as before: a yes there is
    /// its `approve_id` call's (Heron's latency P2 on #7284).
    pub fn for_turn(on: bool, heard: bool, request: u64, now: Instant) -> Option<Self> {
        if !on || !heard {
            return None;
        }
        let skill =
            crate::skill_write_yes::late_guarded(request, now, TOLD_FOR, LATE_TURNS_GUARDED)?;
        Some(Self {
            request,
            skill,
            held: None,
            said: false,
        })
    }

    /// The skill's spoken name, as the model is asked of it.
    pub fn spoken(&self) -> String {
        crate::made_skill::spoken_name(&self.skill)
    }

    /// Whether the skill is still not on at `now`: its wait still stands.
    fn still_off(&self, now: Instant) -> bool {
        crate::skill_write_yes::code_not_on(self.request, now, TOLD_FOR)
            .is_some_and(|(skill, _)| skill == self.skill)
    }

    /// What becomes of `piece`, a sentence of the reply, at `now`: held
    /// while the skill is still not on, or once something of the round is.
    pub fn judge(&mut self, piece: &str, now: Instant) -> Judged {
        if self.held.is_none() && !self.still_off(now) {
            return Judged::Say;
        }
        self.held
            .get_or_insert_with(Vec::new)
            .push(piece.to_owned());
        Judged::Held
    }

    /// The round ended, `approves` whether it called `make_skill` with an
    /// `approve_id`: what becomes of its held words.
    pub fn round_ended(&mut self, approves: bool) -> Round {
        let Some(held) = self.held.take() else {
            return Round::Nothing;
        };
        if approves {
            eprintln!(
                "lamp-voice second: request {}: its words came before its make_skill approve_id call for {} ran: not said, no line (LAMP_MADE_NOT_ON_TOLD)",
                self.request, self.skill
            );
            return Round::Nothing;
        }
        if held
            .iter()
            .all(|piece| !piece.chars().any(char::is_alphanumeric))
        {
            return Round::Say(held);
        }
        Round::Judge(held)
    }

    /// The fixed line, at most once a turn, as it is at `now`: within the
    /// wait or past it.
    pub fn line_once(&mut self, now: Instant) -> Option<&'static str> {
        if std::mem::replace(&mut self.said, true) {
            return None;
        }
        let late = crate::skill_write_yes::code_not_on(self.request, now, TOLD_FOR)
            .is_none_or(|(_, late)| late);
        Some(if late { NOT_ON_LATE } else { NOT_ON_YET })
    }

    pub fn request(&self) -> u64 {
        self.request
    }
}

/// Whether a call of `name` with `args` is a made skill's yes: `make_skill`
/// with an `approve_id`.
pub fn approves(name: &str, args: &serde_json::Value) -> bool {
    name == crate::made_skill::MAKE && args[crate::skill_write_yes::APPROVE_ID].is_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model's answer is read by its first word alone.
    #[test]
    fn the_verdict_is_the_answers_first_word() {
        assert_eq!(verdict("Yes"), Some(true));
        assert_eq!(verdict(" yes."), Some(true));
        assert_eq!(verdict("No."), Some(false));
        assert_eq!(verdict("no"), Some(false));
        assert_eq!(verdict("Nope"), None);
        assert_eq!(verdict(""), None);
        assert_eq!(verdict("Maybe"), None);
    }

    /// The model is asked of the reply as it is, by the skill's name.
    #[test]
    fn the_judge_is_asked_of_the_reply_by_the_skills_name() {
        let asked = judge_messages("newest email subject", "Kỹ năng đã bật rồi.");
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].role, "system");
        assert_eq!(asked[1].role, "user");
        let system = asked[0].content.as_ref().and_then(|c| c.as_str()).unwrap();
        assert!(system.contains("\"newest email subject\" is NOT on"));
        assert!(system.contains("any language"));
        assert!(system.ends_with("Answer only yes or no."));
        assert_eq!(
            asked[1].content.as_ref().and_then(|c| c.as_str()),
            Some("Kỹ năng đã bật rồi.")
        );
    }

    /// Off, or with nothing waiting, no guard and no note; within the wait
    /// the round's words are held, beside the yes's call let go.
    #[test]
    fn off_or_with_nothing_waiting_there_is_no_guard() {
        crate::skill_write_yes::clear_for_test();
        let now = Instant::now();
        assert!(Guard::for_turn(true, true, 2, now).is_none());
        assert!(note(true, 2, now).is_none());
        crate::skill_write_yes::wait_for_yes(
            crate::skill_write_yes::Asks {
                skill: "newest-email-subject".into(),
                sha256: "ab".repeat(32),
                writes: Vec::new(),
                say_back: String::new(),
                grants: None,
                code: true,
                remove: false,
            },
            1,
            now,
        );
        assert!(Guard::for_turn(false, true, 2, now).is_none());
        assert!(Guard::for_turn(true, false, 2, now).is_none());
        assert!(
            Guard::for_turn(true, true, 1, now).is_none(),
            "its own request"
        );
        assert!(
            Guard::for_turn(true, true, 2, now).is_none(),
            "within its wait: streamed"
        );
        let late = now + Duration::from_secs(200);
        let mut guard = Guard::for_turn(true, true, 2, late).unwrap();
        assert_eq!(guard.judge("All set!", late), Judged::Held);
        assert_eq!(
            guard.round_ended(false),
            Round::Judge(vec!["All set!".into()])
        );
        assert_eq!(guard.judge("It's on now.", late), Judged::Held);
        assert_eq!(guard.round_ended(true), Round::Nothing);
        assert_eq!(guard.round_ended(false), Round::Nothing);
        assert_eq!(guard.line_once(late), Some(NOT_ON_LATE));
        assert_eq!(guard.line_once(late), None, "once a turn");
        assert!(
            Guard::for_turn(true, true, 3, late).is_some(),
            "the second late turn"
        );
        assert!(
            Guard::for_turn(true, true, 4, late).is_none(),
            "a third late turn"
        );
        assert!(note(false, 2, now + Duration::from_secs(200)).is_none());
        assert!(note(true, 2, now).is_none(), "within its wait");
        assert!(note(true, 2, now + Duration::from_secs(200)).is_some());
        assert!(note(true, 2, now + TOLD_FOR + Duration::from_secs(1)).is_none());
        crate::skill_write_yes::clear_for_test();
        assert_eq!(
            guard.judge("It's on now.", now),
            Judged::Say,
            "the wait gone"
        );
    }
}
