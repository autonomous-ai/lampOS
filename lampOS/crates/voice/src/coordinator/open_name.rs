//! A line that ends before its name, and the name said next
//! (`LAMP_SECOND_OPEN_NAME=on`, #2135, the first builder on #1722 at 02:16).
//! On lamp-4ace "Hey Lamp, my name is" and 400 ms of quiet ended a line
//! (`model_complete`, 0.933). The name began 9 ms after its end, was judged
//! as a sound while an answer was awaited, and was let go 1.13 s after the
//! end; the lamp answered "my name is" alone, and the person said it all
//! again 14 s later.
//!
//! With the switch, once the second route's final words for the turn in
//! hand end before a name (`known_faces::opens_a_name`), a sound that began
//! within `second_provider::OPEN_NAME_WITHIN` (2.5 s) of the turn's end, before any of
//! its answer was heard, goes on from it. The answer wait (#1123, with its
//! words) and the body's motors don't let it go, and its Start says it goes
//! on from the turn given up, so the second route asks the two as one line.
//! That route holds the answer meanwhile (`Engine::waiting_for_a_name`). The
//! keeper's pause, the busy-room hold and asker-first still judge it: they
//! keep the room's next voice, a colleague's line begun in those 2.5 s, from
//! being taken as the name (the reviewer's P3 on #2355). A sound let go
//! before the words came is not brought back.

/// How long after a turn's end a sound may begin and still go on from it.
const GOES_ON_WITHIN_US: u64 = crate::second_provider::OPEN_NAME_WITHIN.as_micros() as u64;

pub(super) fn from_environment() -> bool {
    enabled(
        std::env::var(crate::second_provider::OPEN_NAME_ENVIRONMENT)
            .ok()
            .as_deref(),
    )
}

/// Where the coordinator reads whether a line that is only the lamp's name
/// lets the line said next take the turn at once (`on`; off unless set; only
/// with `LAMP_SECOND_NAME_CARRIES=on` on the second route).
///
/// Why: the second route holds a bare "Lamp." for the line after it, as
/// long as `second_provider::NAME_CARRIES_FOR` (2.5 s). But the line after
/// it began while that answer was awaited, so the answer wait (#1123) judged
/// it as a sound: one that ended or went quiet before it had gone on was let
/// go as the room's, and the name was answered as a call. On lamp-4ace on
/// 24 October "Lamp, {2 s} tell me a joke." went so in 2 of 10 pause lines
/// with `LAMP_WAIT_KEEPS_ANSWER=on` ("Yes?", "Mm? I'm here."), and 1 of 10
/// with `words`, whose words were not back before it was let go.
pub const CALL_TAKES_NEXT_ENVIRONMENT: &str = "LAMP_CALL_TAKES_NEXT_LINE";

pub(super) fn call_takes_next_from_environment() -> bool {
    enabled(std::env::var(CALL_TAKES_NEXT_ENVIRONMENT).ok().as_deref())
        && enabled(
            std::env::var(crate::second_provider::NAME_CARRIES_ENVIRONMENT)
                .ok()
                .as_deref(),
        )
}

/// The turn, and when its speech ended, whose final `words` are only the
/// lamp's name (`crate::attention::only_the_name`), as [`opened`] finds a
/// line that ends before a name: a sound begun within 2.5 s of its end takes
/// the turn as [`goes_on`] says, decided at once. `None` with the switch off.
pub(super) fn called(
    on: bool,
    words: &str,
    in_hand: Option<(u64, bool)>,
    ended: Option<(u64, u64)>,
) -> Option<(u64, u64)> {
    let (turn, _) = in_hand.filter(|(_, unheard)| on && *unheard)?;
    let ended = ended.filter(|(ended, _)| *ended == turn)?;
    crate::attention::only_the_name(words).then_some(ended)
}

fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The turn, and when its speech ended, whose final `words` end before a
/// name: the turn in hand (`in_hand`: the turn, and whether none of its
/// answer has been heard, `answer_unheard`), whose end (`ended`, the turn
/// and when) is the last one sent. `None` with the switch off.
pub(super) fn opened(
    on: bool,
    words: &str,
    in_hand: Option<(u64, bool)>,
    ended: Option<(u64, u64)>,
) -> Option<(u64, u64)> {
    let (turn, _) = in_hand.filter(|(_, unheard)| on && *unheard)?;
    let ended = ended.filter(|(ended, _)| *ended == turn)?;
    crate::known_faces::opens_a_name(words).then_some(ended)
}

/// Whether a sound first read at `first_read_us` goes on from the turn
/// `open` names: still the turn in hand, none of its answer heard, and the
/// sound begun within [`GOES_ON_WITHIN_US`] of its end.
pub(super) fn goes_on(
    open: Option<(u64, u64)>,
    in_hand: Option<(u64, bool)>,
    first_read_us: u64,
) -> bool {
    open.is_some_and(|(turn, ended_us)| {
        in_hand == Some((turn, true)) && first_read_us <= ended_us.saturating_add(GOES_ON_WITHIN_US)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only `on` turns it on.
    #[test]
    fn it_is_on_only_with_on() {
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!enabled(value), "{value:?}");
        }
        assert!(enabled(Some("on")));
    }

    /// #2135: the turn in hand whose words end before its name, its answer
    /// unheard, is opened; and a sound begun within 2.5 s of its end goes on
    /// from it, while one later, for another turn, or once the answer is
    /// heard does not. Nothing with the switch off, words that give the
    /// name, or an end not this turn's.
    #[test]
    fn a_sound_just_after_a_line_that_ends_before_its_name_goes_on_from_it() {
        let waiting = Some((7, true));
        let ended = Some((7, 1_000_000));
        let open = opened(true, "Hey Lamp, my name is", waiting, ended);
        assert_eq!(open, Some((7, 1_000_000)));
        assert_eq!(opened(false, "Hey Lamp, my name is", waiting, ended), None);
        assert_eq!(
            opened(true, "Hey Lamp, my name is Jim.", waiting, ended),
            None
        );
        assert_eq!(
            opened(true, "Hey Lamp, my name is", waiting, Some((6, 1_000_000))),
            None
        );
        assert_eq!(opened(true, "Hey Lamp, my name is", None, ended), None);
        assert_eq!(opened(true, "Hey Lamp, my name is", waiting, None), None);
        assert!(goes_on(open, waiting, 1_009_000), "9 ms after");
        assert!(
            goes_on(open, waiting, 990_000),
            "its pre-roll before the end"
        );
        assert!(
            goes_on(open, waiting, 3_050_000),
            "2.05 s after, as at 05:53"
        );
        assert!(goes_on(open, waiting, 3_500_000), "at 2.5 s");
        assert!(!goes_on(open, waiting, 3_500_001), "past 2.5 s");
        assert!(!goes_on(None, waiting, 1_009_000));
        assert!(!goes_on(open, None, 1_009_000));
        assert!(!goes_on(open, Some((8, true)), 1_009_000), "another turn");
        let heard = Some((7, false));
        assert!(!goes_on(open, heard, 1_009_000), "its answer heard");
        assert_eq!(opened(true, "Hey Lamp, my name is", heard, ended), None);
    }

    /// `LAMP_CALL_TAKES_NEXT_LINE`: the turn in hand whose words are only the
    /// lamp's name, its answer unheard, is called; a sound begun within 2.5 s
    /// of its end takes the turn. Nothing with the switch off, with more words
    /// than the name, for an end not this turn's, or once its answer is heard.
    #[test]
    fn a_sound_just_after_the_bare_name_takes_the_turn() {
        let waiting = Some((7, true));
        let ended = Some((7, 1_000_000));
        for alone in ["Lamp.", "Lamp", "Hey Lamp,", "Lamb."] {
            assert_eq!(
                called(true, alone, waiting, ended),
                Some((7, 1_000_000)),
                "{alone}"
            );
        }
        assert_eq!(called(false, "Lamp.", waiting, ended), None);
        assert_eq!(called(true, "Lamp, tell me a joke.", waiting, ended), None);
        assert_eq!(called(true, "Hey Lamp, my name is", waiting, ended), None);
        assert_eq!(called(true, "Lamp.", waiting, Some((6, 1_000_000))), None);
        assert_eq!(called(true, "Lamp.", Some((7, false)), ended), None);
        assert_eq!(called(true, "Lamp.", None, ended), None);
        let call = called(true, "Lamp.", waiting, ended);
        assert!(goes_on(call, waiting, 1_300_000), "the line 300 ms after");
        assert!(goes_on(call, waiting, 3_500_000), "at 2.5 s");
        assert!(!goes_on(call, waiting, 3_500_001), "past 2.5 s");
        assert!(
            !goes_on(call, Some((7, false)), 1_300_000),
            "its answer heard"
        );
    }
}
