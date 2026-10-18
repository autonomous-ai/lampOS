//! In a busy room, a line the voice would let out only because a
//! conversation with the lamp is going on waits for the Policy's decision
//! about it, and is played only on `answer` or on a `stay_quiet` said back
//! to the lamp (#1522), off unless `LAMP_GOING_ON_ASKS=on`. Only with the
//! attention rule and the bus.
//!
//! On lamp-4ace on 2026-10-18 at 14:44, on `3f4f3c5c3`, after Daniel's call
//! in the battery's scene 3, every line of the two people's talk was let
//! out as "a conversation with the lamp is going on", each 2.4 to 6.8 s
//! after the lamp's own reply to the line before, and across the 6 s gap to
//! the next scene's opening lines: the lamp answered "Okay, I will book the
//! car for eight." with the time, then spoke the two people's own words
//! back. The Policy had the room as busy throughout, and its own decisions
//! on those lines were `stay_quiet` 5, `look_up` 1 and a `stay_quiet` with
//! `follows_lamp` 0.66 1 (Samantha's "And what about tomorrow?"). Over the
//! day (17 Oct 17:00 to 18 Oct 08:10 UTC, both builds), of 48 lines let out
//! as going on, all in a busy room, it said `look_up` 20, `stay_quiet` 19
//! and a following `stay_quiet` 9: it would have held 39 (Fourth's
//! `going_on_fourth.py`, run by the first builder, #1522 at 15:14). The
//! first builder's decision there: such a line waits for the Policy.
//!
//! - **Which lines:** in a busy room, a line whose only reason to be played
//!   is the conversation going on: not called in its words, by its name
//!   heard (plainly or close), by a face turned to the lamp or by a line
//!   said to it from close by (`crate::attention`), and not a reply to the
//!   lamp's own question ("Yes, go ahead." to "Shall I start it?", Eighth on
//!   #1538). Nor someone else's line, a question about the lamp's own work,
//!   or a remark to the room with `LAMP_REMARKS`, which the rule plays or
//!   holds without the Policy: only a line whose verdict would come to the
//!   Policy's arm waits (the reviewer and Jarvis on #1538). Those are played
//!   or held as before, at once. In a room not known to be busy nothing
//!   changes.
//! - **What it waits for:** the Policy's attention decision on the line, by
//!   its number, made for real ([`crate::policy_holds::Verdicts`]). The
//!   decision is made as the line's words end, and Gemini's first output
//!   usually comes later, so it is often there already and nothing waits.
//!   Otherwise the line's first output and everything after it is kept
//!   until the decision comes, for at most [`crate::policy_holds::WAIT`]
//!   from that first output; meanwhile the worker takes nothing more from
//!   the service, as when its output is full, and still serves its control
//!   on every tick.
//! - **What it plays on** ([`Says`]): `answer`, or a `stay_quiet` whose
//!   `follows_lamp` is [`crate::policy_holds::FOLLOWS_FROM`] or more (said
//!   back to the lamp). Held on any other `stay_quiet`, and on `look_up`,
//!   which turns to whoever spoke without speaking, its call run all the
//!   same (the first builder's ruling on #1538: the Policy's choice
//!   decides; tonight's battery sets the bar). With no decision in time it is
//!   played, as without the switch, and the journal says so: a Policy that
//!   is down must not make the lamp deaf in a conversation.
//! - **Its cost:** none of the 48 lines got `answer`, so what it lets
//!   through, it lets through by `follows_lamp`. A follow-up said to the
//!   lamp that the Policy does not read as said back to it is held (17 of
//!   the 33 lines said to the lamp on 2026-10-16 scored 0.6 or more,
//!   `crate::policy_holds`). And a line that waits is that much later.
//!
//! The journal says each wait and its end ("its answer waits for the
//! Policy's decision", "the Policy's decision came after N ms" or "no
//! decision of the Policy's came in N ms"), and the verdict's reason. Nothing
//! of what was heard is read here.

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_GOING_ON_ASKS";
/// The provider worker's flag for it, after `--remarks`.
pub const FLAG: &str = "--going-on-asks";

/// The verdict's reasons.
pub const ANSWER: &str =
    "a conversation with the lamp is going on, and the Policy took the line as the lamp's";
pub const FOLLOWING: &str =
    "a conversation with the lamp is going on, and the Policy found the line said back to the lamp";
pub const NOT_THE_LAMPS: &str = "a busy room: a conversation with the lamp is going on, but the Policy judged the line not for the lamp";
/// A `look_up`: its words are held, and a call it makes is left to the
/// lamp's own gate for actions ([`crate::policy_holds`]), which takes a
/// `look_up` up (the first builder's ruling on #1538).
pub const LOOKED_UP: &str = "a busy room: a conversation with the lamp is going on, and the Policy chose to turn to whoever spoke without the lamp speaking";
pub const NO_DECISION: &str =
    "a conversation with the lamp is going on, and no decision of the Policy's came in time";
/// The Policy decided the line only in shadow (`lamp-policy` without `--act
/// attention`): nothing it says is acted on, so the line is let out as
/// without the switch, as soon as that decision comes rather than after
/// the wait (the reviewer on #1538).
pub const SHADOW_ONLY: &str =
    "a conversation with the lamp is going on, and the Policy decided it only in shadow";

/// What the Policy's decision lets the lamp say of a line let out only as a
/// conversation going on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Says {
    /// `answer`: the lamp's.
    Answer,
    /// `stay_quiet`, but said back to the lamp
    /// ([`crate::policy_holds::Verdict::StayQuietFollowing`]).
    Following,
    /// `look_up`: turn to whoever spoke, without speaking. Its words are
    /// held, and its call runs as the action gate says.
    LookUp,
    /// `stay_quiet`: not for the lamp to answer.
    Not,
    /// Decided only in shadow: nothing to act on ([`SHADOW_ONLY`]).
    Shadow,
}

/// The verdict for such a line from what the Policy said, or `None` if
/// nothing came in time.
pub fn verdict(says: Option<Says>) -> crate::attention::Verdict {
    use crate::attention::Verdict;
    match says {
        Some(Says::Answer) => Verdict::Play(ANSWER),
        Some(Says::Following) => Verdict::Play(FOLLOWING),
        Some(Says::LookUp) => Verdict::Hold(LOOKED_UP),
        Some(Says::Not) => Verdict::Hold(NOT_THE_LAMPS),
        Some(Says::Shadow) => Verdict::Play(SHADOW_ONLY),
        None => Verdict::Play(NO_DECISION),
    }
}

/// The worker's arguments for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::Verdict;

    /// The first builder's decision on #1522: played on `answer` and on a
    /// `stay_quiet` said back to the lamp, held otherwise, and played as
    /// without the switch when no decision came.
    #[test]
    fn a_line_going_on_is_played_on_answer_or_following_and_held_otherwise() {
        assert_eq!(verdict(Some(Says::Answer)), Verdict::Play(ANSWER));
        assert_eq!(verdict(Some(Says::Following)), Verdict::Play(FOLLOWING));
        assert_eq!(verdict(Some(Says::Not)), Verdict::Hold(NOT_THE_LAMPS));
        assert_eq!(verdict(Some(Says::LookUp)), Verdict::Hold(LOOKED_UP));
        assert_eq!(verdict(Some(Says::Shadow)), Verdict::Play(SHADOW_ONLY));
        assert_eq!(verdict(None), Verdict::Play(NO_DECISION));
        // Each reason still says the conversation was going on, for the
        // journal's readers that look for it.
        for why in [
            ANSWER,
            FOLLOWING,
            NOT_THE_LAMPS,
            LOOKED_UP,
            NO_DECISION,
            SHADOW_ONLY,
        ] {
            assert!(
                why.contains("a conversation with the lamp is going on"),
                "{why}"
            );
        }
    }

    #[test]
    fn the_switch_is_on_only_and_its_flag_is_taken_once() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for value in [None, Some("off"), Some("1"), Some("")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let flags = [FLAG.to_owned(), "--next".to_owned()];
        let (on, rest) = take(&flags);
        assert!(on);
        assert_eq!(rest, ["--next"]);
        assert!(!take(rest).0);
    }
}
