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
//! within `second_provider::CARRY_AFTER_END` of the turn's end, before any of
//! its answer was heard, goes on from it. The answer wait (#1123, with its
//! words) and the body's motors don't let it go, and its Start says it goes
//! on from the turn given up, so the second route asks the two as one line.
//! That route holds the answer meanwhile (`Engine::waiting_for_a_name`). The
//! keeper's pause, the busy-room hold and asker-first still judge it: they
//! keep the room's next voice, a colleague's line begun in those 1.5 s, from
//! being taken as the name (the reviewer's P3 on #2355). A sound let go
//! before the words came is not brought back.

/// How long after a turn's end a sound may begin and still go on from it.
const GOES_ON_WITHIN_US: u64 = crate::second_provider::CARRY_AFTER_END.as_micros() as u64;

pub(super) fn from_environment() -> bool {
    enabled(
        std::env::var(crate::second_provider::OPEN_NAME_ENVIRONMENT)
            .ok()
            .as_deref(),
    )
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
    /// unheard, is opened; and a sound begun within 1.5 s of its end goes on
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
        assert!(goes_on(open, waiting, 2_500_000), "at 1.5 s");
        assert!(!goes_on(open, waiting, 2_500_001), "past 1.5 s");
        assert!(!goes_on(None, waiting, 1_009_000));
        assert!(!goes_on(open, None, 1_009_000));
        assert!(!goes_on(open, Some((8, true)), 1_009_000), "another turn");
        let heard = Some((7, false));
        assert!(!goes_on(open, heard, 1_009_000), "its answer heard");
        assert_eq!(opened(true, "Hey Lamp, my name is", heard, ended), None);
    }
}
