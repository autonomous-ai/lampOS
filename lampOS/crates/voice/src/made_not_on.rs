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
//! - **A reply that says it is on is not said** ([`Guard`]): a sentence
//!   whose subject is the skill ("the skill", "your skill", its spoken name,
//!   or "it" on a line that is only a yes) and that says it is on, turned on,
//!   ready, set up, active, running, installed or made, or "I've turned it
//!   on" and the like, with no "not", "n't", "never", condition or question
//!   in it ([`claims_on`]). That sentence and the rest of its round are held;
//!   at the round's end, a round that calls `make_skill` with an
//!   `approve_id` lets them go unsaid (the next round speaks from that call's
//!   answer), and any other says the fixed line [`NOT_ON_YET`] (within the
//!   wait) or [`NOT_ON_LATE`] (past it), once a turn. While the guard holds,
//!   a round's first words wait for their sentence's end, so a claim is
//!   never half said.
//!
//! Once the skill's yes is taken (its wait gone), nothing is held: "It's on
//! now." after a yes that ran is said. A skill that writes is on already
//! and its wait is for its sends, so it is never one of these. Off, or with
//! no such skill, the turn is byte for byte as before.
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

/// A turn's guard of a made skill not on ([`Guard::for_turn`]).
#[derive(Debug)]
pub struct Guard {
    request: u64,
    /// The skill's name, as it is waited for.
    skill: String,
    /// Its spoken name, as [`claims_on`] reads it.
    spoken: String,
    /// The line is only a yes: "it" is the skill.
    it_counts: bool,
    /// A claim was held in this round: it and the rest of the round wait for
    /// the round's end.
    held: bool,
    /// The fixed line was said in this turn.
    said: bool,
}

/// What a piece of a reply does under a [`Guard`].
#[derive(Debug, PartialEq, Eq)]
pub enum Judged {
    Say,
    /// Held, never said: a claim, or what follows one in its round.
    Held,
}

impl Guard {
    /// The guard of the heard line `request` (`heard`; `line`, its words)
    /// at `now`: with the switch `on`, while a made skill with code of its
    /// own is not on and its yes was asked before it.
    pub fn for_turn(on: bool, heard: bool, request: u64, line: &str, now: Instant) -> Option<Self> {
        if !on || !heard {
            return None;
        }
        let (skill, _) = crate::skill_write_yes::code_not_on(request, now, TOLD_FOR)?;
        Some(Self {
            request,
            spoken: crate::made_skill::spoken_name(&skill).to_lowercase(),
            skill,
            it_counts: crate::remember::heard::only_a_yes(line),
            held: false,
            said: false,
        })
    }

    /// Whether the skill is still not on at `now`: its wait still stands.
    fn still_off(&self, now: Instant) -> bool {
        crate::skill_write_yes::code_not_on(self.request, now, TOLD_FOR)
            .is_some_and(|(skill, _)| skill == self.skill)
    }

    /// What becomes of `piece`, a sentence of the reply, at `now`.
    pub fn judge(&mut self, piece: &str, now: Instant) -> Judged {
        if self.held {
            return Judged::Held;
        }
        if !self.still_off(now) || !claims_on(piece, &self.spoken, self.it_counts) {
            return Judged::Say;
        }
        eprintln!(
            "lamp-voice second: request {}: its reply says the made skill {} is on, and it isn't: not said (LAMP_MADE_NOT_ON_TOLD)",
            self.request, self.skill
        );
        // Said once a turn: a later claim is only held.
        self.held = true;
        Judged::Held
    }

    /// A round begins: nothing of an earlier round is held any more.
    pub fn round_begun(&mut self) {
        self.held = false;
    }

    /// The round ended, `approves` whether it called `make_skill` with an
    /// `approve_id`: the fixed line to say in place of its held claim, at
    /// `now`, or none.
    pub fn round_ended(&mut self, approves: bool, now: Instant) -> Option<&'static str> {
        if !std::mem::replace(&mut self.held, false) {
            return None;
        }
        if approves {
            eprintln!(
                "lamp-voice second: request {}: its claim that {} is on came before its make_skill approve_id call ran: not said, no line (LAMP_MADE_NOT_ON_TOLD)",
                self.request, self.skill
            );
            return None;
        }
        if std::mem::replace(&mut self.said, true) {
            return None;
        }
        let late = crate::skill_write_yes::code_not_on(self.request, now, TOLD_FOR)
            .is_none_or(|(_, late)| late);
        let line = if late { NOT_ON_LATE } else { NOT_ON_YET };
        eprintln!(
            "lamp-voice second: request {}: said \"{line}\" in its place (LAMP_MADE_NOT_ON_TOLD)",
            self.request
        );
        Some(line)
    }
}

/// Whether a call of `name` with `args` is a made skill's yes: `make_skill`
/// with an `approve_id`.
pub fn approves(name: &str, args: &serde_json::Value) -> bool {
    name == crate::made_skill::MAKE && args[crate::skill_write_yes::APPROVE_ID].is_string()
}

/// What a sentence that says it is on may not carry: a negation, a
/// condition, or an ask for the yes.
const NOT_A_CLAIM: [&str; 14] = [
    "n't",
    " not ",
    " not,",
    " not.",
    "never",
    "no longer",
    " yet",
    " once ",
    " when ",
    " if ",
    " after ",
    " until ",
    "say yes",
    "agree",
];

/// Who a claim is about, besides the skill's own name: the skill.
const SUBJECTS: [&str; 7] = [
    "the skill",
    "your skill",
    "the new skill",
    "your new skill",
    "this skill",
    "that skill",
    "my skill",
];

/// What comes between the subject and its state.
const LINKS: [&str; 7] = [
    "'s ",
    " is ",
    "'s now ",
    " is now ",
    " has been ",
    " was ",
    " got ",
];

/// The states a claim says.
const STATES: [&str; 14] = [
    "on",
    "turned on",
    "switched on",
    "put on",
    "ready",
    "set up",
    "all set",
    "active",
    "live",
    "enabled",
    "running",
    "installed",
    "made",
    "up and running",
];

/// How a claim says the lamp did it, before its subject.
const DONE_BY_ME: [&str; 6] = [
    "i've ",
    "i have ",
    "i ",
    "i've just ",
    "i just ",
    "i've now ",
];

/// What the lamp says it did, around its subject: "turned <it> on" or
/// "turned on <it>".
const DID: [&str; 3] = ["turned", "put", "switched"];

/// What the lamp says it did to its subject, after it: "installed <it>".
const DID_TO: [&str; 4] = ["installed", "enabled", "activated", "set up"];

/// Whether `piece` has a sentence that says the made skill whose spoken
/// name is `spoken` (lower case) is on, by the module's rule. `it_counts`:
/// "it" is the skill too.
pub fn claims_on(piece: &str, spoken: &str, it_counts: bool) -> bool {
    let text: String = piece
        .chars()
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .flat_map(char::to_lowercase)
        .collect();
    let named = [spoken.to_owned(), format!("{spoken} skill")];
    let mut subjects: Vec<&str> = SUBJECTS.to_vec();
    subjects.extend(
        named
            .iter()
            .map(String::as_str)
            .filter(|name| !name.is_empty()),
    );
    if it_counts {
        subjects.push("it");
    }
    text.split_inclusive(['.', '!', '?', ';'])
        .filter(|sentence| !sentence.trim_end().ends_with('?'))
        .map(|sentence| format!(" {} ", sentence.trim_end_matches(['.', '!', ';'])))
        .filter(|sentence| !NOT_A_CLAIM.iter().any(|not| sentence.contains(not)))
        .any(|sentence| {
            subjects
                .iter()
                .any(|subject| said_on(&sentence, subject) || said_done(&sentence, subject))
        })
}

/// Whether `sentence` (padded with a space each side) says `subject` is in
/// one of [`STATES`].
fn said_on(sentence: &str, subject: &str) -> bool {
    at_word(sentence, subject).any(|after| {
        LINKS.iter().any(|link| {
            after.strip_prefix(link).is_some_and(|rest| {
                STATES
                    .iter()
                    .any(|state| rest.strip_prefix(state).is_some_and(word_ends))
            })
        })
    })
}

/// Whether `sentence` says the lamp did one of [`DID`] or [`DID_TO`] to
/// `subject`.
fn said_done(sentence: &str, subject: &str) -> bool {
    let on_it = [format!(" {subject} on"), format!(" on {subject}")];
    let to_it = format!(" {subject}");
    DONE_BY_ME.iter().any(|me| {
        at_word(sentence, me).any(|rest| {
            DID.iter().any(|did| {
                rest.strip_prefix(did).is_some_and(|rest| {
                    on_it
                        .iter()
                        .any(|it| rest.strip_prefix(it.as_str()).is_some_and(word_ends))
                })
            }) || DID_TO.iter().any(|did| {
                rest.strip_prefix(did)
                    .and_then(|rest| rest.strip_prefix(to_it.as_str()))
                    .is_some_and(word_ends)
            })
        })
    })
}

/// What follows each place `word` begins a word in `text`.
fn at_word<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    text.match_indices(word).filter_map(move |(at, _)| {
        let before = text[..at].chars().next_back();
        let after = &text[at + word.len()..];
        (before.is_none_or(|c| !c.is_alphanumeric())).then_some(after)
    })
}

/// Whether `rest` begins past a word's end.
fn word_ends(rest: &str) -> bool {
    rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "newest email subject";

    /// The claims a reply makes that the skill is on, and the sentences
    /// that make none.
    #[test]
    fn a_claim_says_the_skill_itself_is_on() {
        for reply in [
            "The skill is on the lamp.",
            "Great, your skill is now on.",
            "Newest email subject is ready!",
            "Your newest email subject skill's all set.",
            "Okay. The new skill is up and running.",
            "I've turned it on.",
            "Done, I turned on the skill.",
            "I've installed it.",
        ] {
            assert!(claims_on(reply, NAME, true), "{reply}");
        }
        for reply in [
            NOT_ON_YET,
            NOT_ON_LATE,
            "It isn't on yet.",
            "The skill is not on.",
            "Is the skill on?",
            "It'll be on once you say yes.",
            "The skill is on when you agree.",
            "I made newest email subject: when someone has been at the desk for 5 seconds, it runs its own code.",
            " The lamp's model wrote its code, and that code may use your Gmail. It waits for you to agree before it runs any code.",
            "The light is on.",
            "Within the hour.",
        ] {
            assert!(!claims_on(reply, NAME, true), "{reply}");
        }
        // "It" is the skill only on a line that is only a yes.
        assert!(claims_on("It's on now!", NAME, true));
        assert!(!claims_on("It's on now!", NAME, false));
        assert!(claims_on("The skill is on.", NAME, false));
    }

    /// Off, or with nothing waiting, no guard and no note.
    #[test]
    fn off_or_with_nothing_waiting_there_is_no_guard() {
        crate::skill_write_yes::clear_for_test();
        let now = Instant::now();
        assert!(Guard::for_turn(true, true, 2, "Yes.", now).is_none());
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
        assert!(Guard::for_turn(false, true, 2, "Yes.", now).is_none());
        assert!(Guard::for_turn(true, false, 2, "Yes.", now).is_none());
        assert!(
            Guard::for_turn(true, true, 1, "Yes.", now).is_none(),
            "its own request"
        );
        assert!(Guard::for_turn(true, true, 2, "Yes.", now).is_some());
        assert!(note(false, 2, now + Duration::from_secs(200)).is_none());
        assert!(note(true, 2, now).is_none(), "within its wait");
        assert!(note(true, 2, now + Duration::from_secs(200)).is_some());
        assert!(note(true, 2, now + TOLD_FOR + Duration::from_secs(1)).is_none());
        crate::skill_write_yes::clear_for_test();
    }
}
