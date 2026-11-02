//! A line cut at a thinking pause, joined when the person goes on
//! (`LAMP_ENDPOINT_PAUSE_JOIN=on`, #4449).
//!
//! At a thinking pause Smart Turn scores 0.92 to 0.99, so the 400 ms commit
//! ends a line the person has not finished ("I need a gift for my sister."
//! scores 0.98), and its first half was answered before they went on. The
//! desk-kit-2 pause lines came whole 3 to 6 times in 15 on the lamps.
//!
//! With the switch, on the second route only, a line ended by a confident
//! verdict (`model_complete`) has its answer kept back until [`GRACE`] after
//! its speech ended. Its chat and speech go on meanwhile; only the answer's
//! first sound waits, never the lamp's own cue or light. Speech set off in
//! the grace is heard as any sound while an answer waits, so the body's
//! motors and a cough are let go as before, but it is decided by
//! [`decide_by`], while the answer is still kept back. Taken, it cuts the
//! first line short before any of its answer was heard, so the second
//! route's own join (`LAMP_JOIN_CUT_WORDS`) asks the two as one line when
//! one person said both, and the first answer is dropped and leaves the
//! thread. Not joined (another person), the first line's words are still
//! answered, first, in the second's answer ([`EARLIER_LINE`]). With nothing
//! in the grace the answer plays as before, no sooner than the grace's end.
//! Off, nothing here runs.

use crate::activity::EndpointDecision;
use std::time::Duration;

/// Where the voice reads it: `on` exactly; off unless set.
pub const ENVIRONMENT: &str = "LAMP_ENDPOINT_PAUSE_JOIN";

/// How long after a confidently ended line's speech ended its answer is
/// kept back, and speech set off in it may go on from it: a thinking pause
/// of up to about 1.5 s (#4449).
pub const GRACE: Duration = Duration::from_millis(1_500);

const GRACE_US: u64 = GRACE.as_micros() as u64;

/// How much sooner than the grace's end speech must be set off to go on
/// from the line: the coordinator decides it at once, and its Start reaches
/// the second worker while the answer is still kept back.
pub const DECIDE_REACH_US: u64 = 100_000;

/// The basis of an end a confident verdict made at the commit.
pub const CONFIDENT: &str = "model_complete";

/// Whether `value` turns it on: `on` exactly.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch as the voice reads it: only with the second route's own join
/// (`LAMP_JOIN_CUT_WORDS=on`), which asks the two lines as one and alone
/// knows whether one person said both. Without it a line cut short in the
/// grace would be dropped, not joined.
pub fn from_environment() -> bool {
    let set = |name: &str| std::env::var(name).ok();
    enabled(set(ENVIRONMENT).as_deref())
        && enabled(set(crate::second_provider::JOIN_CUT_WORDS_ENVIRONMENT).as_deref())
}

/// When the line's speech ended, by the capture clock, when `decision`
/// ended it on a confident verdict: the block that ended it (`ended_us`)
/// less the quiet the decision waited. `None` with the switch off, without
/// a decision, or for any other end.
pub fn paused_end(on: bool, decision: Option<&EndpointDecision>, ended_us: u64) -> Option<u64> {
    let decision = decision.filter(|_| on)?;
    (decision.basis == CONFIDENT)
        .then(|| ended_us.saturating_sub(decision.quiet_ms().saturating_mul(1_000)))
}

/// How long from `now_us` the answer of a line whose speech ended at
/// `speech_end_us` is still kept back: the rest of the grace, none past it.
pub fn kept_for(speech_end_us: u64, now_us: u64) -> Duration {
    Duration::from_micros(
        speech_end_us
            .saturating_add(GRACE_US)
            .saturating_sub(now_us),
    )
}

/// By when a sound set off in the grace after a line whose speech ended at
/// `speech_end_us` is decided, taken or let go: the grace's end, less
/// [`DECIDE_REACH_US`], so its Start reaches the second worker while the
/// line's answer is still kept back.
pub fn decide_by(speech_end_us: u64) -> u64 {
    speech_end_us.saturating_add(GRACE_US - DECIDE_REACH_US)
}

/// Whether speech set off at `trigger_us` goes on from a line whose speech
/// ended at `speech_end_us`: set off by [`decide_by`].
pub fn within(speech_end_us: u64, trigger_us: u64) -> bool {
    trigger_us <= decide_by(speech_end_us)
}

/// Whether a line ended on `basis` whose speech ended at `speech_end_us`
/// goes on in the speech set off at `trigger_us`: the replay's rule, and
/// the coordinator's with its turn checks ([`goes_on`]).
pub fn joins(basis: &str, speech_end_us: u64, trigger_us: u64) -> bool {
    basis == CONFIDENT && within(speech_end_us, trigger_us)
}

/// Whether a sound set off at `trigger_us` goes on from the turn `paused`
/// names (the turn, and when its speech ended): still the turn in hand,
/// none of its answer heard (`in_hand`: the turn, and whether its answer is
/// unheard), and set off [`within`] the grace.
pub fn goes_on(paused: Option<(u64, u64)>, in_hand: Option<(u64, bool)>, trigger_us: u64) -> bool {
    paused.is_some_and(|(turn, speech_end_us)| {
        in_hand == Some((turn, true)) && within(speech_end_us, trigger_us)
    })
}

/// Told beside a line that cut short, in their pause's grace, words it was
/// not joined to (another person's, or not shown one person's): those words
/// were a whole question whose answer was kept back for the grace and never
/// heard, so the model answers them first, then the line (Sixth's P2 on
/// #4916: "What time is it?" was lost to another person speaking 1 s after
/// it). The asker comes first, as without the switch.
pub const EARLIER_LINE: &str = "(For you to know: just before this, someone asked you something you have not answered yet, in these words: \"{words}\". Answer that first, briefly, then what is said now.)";

/// [`EARLIER_LINE`] with `words`.
pub fn earlier_line(words: &str) -> String {
    EARLIER_LINE.replace("{words}", words.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(basis: &'static str, silence_ms: u64) -> EndpointDecision {
        EndpointDecision {
            basis,
            silence_ms,
            probability: Some(0.98),
            fallback: None,
            verdict: None,
            call_wait_ms: None,
            short_asks: 0,
            speech_ms: None,
            rechecked_from: None,
            unfinished_wait_ms: None,
        }
    }

    /// Only `on` turns it on.
    #[test]
    fn it_is_on_only_with_on() {
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!enabled(value), "{value:?}");
        }
        assert!(enabled(Some("on")));
    }

    /// A confident end at the 400 ms commit dates its speech's end 400 ms
    /// before the block that ended it; any other end, or the switch off,
    /// is no paused end.
    #[test]
    fn only_a_confident_end_is_a_paused_end() {
        let complete = decision("model_complete", 400);
        assert_eq!(
            paused_end(true, Some(&complete), 5_000_000),
            Some(4_600_000)
        );
        assert_eq!(paused_end(false, Some(&complete), 5_000_000), None);
        assert_eq!(paused_end(true, None, 5_000_000), None);
        for other in [
            "model_uncertain",
            "model_incomplete",
            "silence_fallback",
            "silence",
        ] {
            assert_eq!(
                paused_end(true, Some(&decision(other, 600)), 5_000_000),
                None,
                "{other}"
            );
        }
    }

    /// Speech set off up to 1.4 s after the line's speech ended goes on from
    /// it; later, for another turn, or once its answer is heard, it is a new
    /// turn. The answer is kept back for the rest of the 1.5 s.
    #[test]
    fn speech_in_the_grace_goes_on_and_after_it_is_a_new_turn() {
        let paused = Some((7, 1_000_000));
        let waiting = Some((7, true));
        assert!(goes_on(paused, waiting, 1_400_000), "0.4 s after");
        assert!(goes_on(paused, waiting, 2_400_000), "at 1.4 s");
        assert!(!goes_on(paused, waiting, 2_400_001), "past 1.4 s");
        assert!(!goes_on(paused, waiting, 2_600_000), "past the grace");
        assert!(!goes_on(None, waiting, 1_400_000));
        assert!(!goes_on(paused, Some((8, true)), 1_400_000), "another turn");
        assert!(!goes_on(paused, Some((7, false)), 1_400_000), "heard");
        assert!(!goes_on(paused, None, 1_400_000));
        assert_eq!(kept_for(1_000_000, 1_400_000), Duration::from_millis(1_100));
        assert_eq!(kept_for(1_000_000, 2_500_000), Duration::ZERO);
        assert_eq!(kept_for(1_000_000, 9_000_000), Duration::ZERO);
        assert!(joins("model_complete", 1_000_000, 2_000_000));
        assert!(!joins("model_uncertain", 1_000_000, 2_000_000));
        assert!(!joins("model_complete", 1_000_000, 2_500_000));
    }
}
