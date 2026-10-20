//! A line's words asked for at a pause (`LAMP_SECOND_EARLY_FINALIZE=on`,
//! the first builder on #1722, 20 Oct 07:20 and 07:36). On lamp-4ace the
//! speech-to-text service never ends a line on its own: the lamp's quiet
//! tail, its noise floor and the canceller's residue, isn't silence to it.
//! So at every line's end the second route sent `Finalize` and waited
//! 270–300 ms for words it could have had already. With this, once the
//! line has gone quiet for [`QUIET_BLOCKS`], the coordinator tells the
//! second route (`ProviderInput::Paused`), which asks for the words so far
//! and keeps the stream open: speech that goes on comes as new finals, and
//! at the end the words are usually there.
//!
//! The coordinator's own count of the quiet is the one used
//! (`TurnDetector::pause_blocks`); no second detector. Bounded: once a
//! pause, and only after [`SPEECH_BLOCKS`] of speech since the last ask, so
//! a halting speaker doesn't cut the service's segments every word. The
//! Live worker takes nothing from it.

pub const ENVIRONMENT: &str = "LAMP_SECOND_EARLY_FINALIZE";

/// 150 ms of quiet, in the detector's 10 ms blocks.
pub(super) const QUIET_BLOCKS: u16 = 15;
/// 300 ms of speech since the last ask.
pub(super) const SPEECH_BLOCKS: u16 = 30;

pub(super) fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The speech heard in the current request since its last ask.
#[derive(Debug, Default)]
pub(super) struct EarlyFinalize {
    turn: Option<u64>,
    speech: u16,
}

impl EarlyFinalize {
    /// One block of `turn` heard, `pause_blocks` into the pause it's in (0
    /// while someone speaks): whether its words are to be asked for now.
    pub(super) fn heard(&mut self, turn: u64, pause_blocks: u16) -> bool {
        if self.turn != Some(turn) {
            self.turn = Some(turn);
            self.speech = 0;
        }
        if pause_blocks == 0 {
            self.speech = self.speech.saturating_add(1);
            return false;
        }
        if pause_blocks == QUIET_BLOCKS && self.speech >= SPEECH_BLOCKS {
            self.speech = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks of speech (`true`) and quiet as runs; how many asks they make.
    fn asks(early: &mut EarlyFinalize, turn: u64, runs: &[(bool, u16)]) -> usize {
        let mut asked = 0;
        for &(speech, blocks) in runs {
            for pause in 1..=blocks {
                asked += usize::from(early.heard(turn, if speech { 0 } else { pause }));
            }
        }
        asked
    }

    /// One ask a pause, at 150 ms of it, and never again in the same pause.
    #[test]
    fn a_pause_after_speech_asks_once_at_150_ms() {
        let mut early = EarlyFinalize::default();
        let mut asked_at = Vec::new();
        for _ in 0..40 {
            early.heard(1, 0);
        }
        for pause in 1..=60 {
            if early.heard(1, pause) {
                asked_at.push(pause);
            }
        }
        assert_eq!(asked_at, [QUIET_BLOCKS]);
    }

    /// The first builder at 07:36: three short pauses in one line, with
    /// little said between two of them, ask at most twice.
    #[test]
    fn three_short_pauses_in_one_line_ask_at_most_twice() {
        let mut early = EarlyFinalize::default();
        let line = [
            (true, 40),
            (false, 20),
            (true, 10),
            (false, 20),
            (true, 40),
            (false, 20),
        ];
        assert_eq!(asks(&mut early, 1, &line), 2);
    }

    /// Too little said before a pause asks nothing; a new request starts
    /// its count afresh.
    #[test]
    fn a_short_word_asks_nothing_and_each_request_counts_its_own() {
        let mut early = EarlyFinalize::default();
        assert_eq!(asks(&mut early, 1, &[(true, 20), (false, 30)]), 0);
        assert_eq!(asks(&mut early, 2, &[(true, 15), (false, 30)]), 0);
        assert_eq!(asks(&mut early, 3, &[(true, 30), (false, 30)]), 1);
    }
}
