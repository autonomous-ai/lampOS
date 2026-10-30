//! Whether this is a moment to switch the voice: the rule every restart of
//! the voice follows (docs/brain-build.md, "Restarting the voice"). Nobody
//! has been heard and the lamp has said nothing for 45 seconds, and the
//! voice says it is resting.
//!
//! It is judged from what the voice says on the bus: `voice/heard` and
//! `voice/said`, a message with each piece of text heard or said, and
//! `voice/state`, resting, listening, thinking or speaking, said when it
//! changes and once a second. "Listening" is the voice hearing someone, so
//! anything but resting is someone in a turn. A voice that says nothing on
//! the bus cannot be judged quiet: what cannot be seen is not quiet.
//!
//! A muted voice says no state, so its mute is read too (`voice/mute`, said
//! once a second, #3770): muted, asleep or not, is quiet at once, since its
//! microphones are closed and no turn can be cut. Under the same 3 s as the
//! state, and never while a fresh state says a turn.
//!
//! Pure: it is told what was heard and when, and asked at a time.
use lamp_msgs::{Voice, VoiceMute};
use std::time::Duration;

/// How long nobody has been heard or answered before the voice is switched.
pub const QUIET: Duration = Duration::from_secs(45);
/// How long the voice has been resting with nothing heard or said before
/// it is put back after a failed switch: not in a turn, never mid-sentence.
pub const BETWEEN_TURNS: Duration = Duration::from_secs(5);
/// The voice says its state once a second. A state older than this is not
/// known any more.
pub const STATE_LASTS: Duration = Duration::from_secs(3);

/// Something the voice said on the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// A piece of text heard (`voice/heard`) or said (`voice/said`).
    Speech,
    /// Its state (`voice/state`).
    State(Voice),
    /// Its mute (`voice/mute`).
    Mute(VoiceMute),
}

#[derive(Clone, Debug)]
pub struct Quiet {
    /// When watching began: quiet for 45 s needs 45 s of watching.
    since_ms: u64,
    /// The last moment someone was in a turn.
    noise_ms: Option<u64>,
    /// The last state said, and when.
    state: Option<(Voice, u64)>,
    /// The last mute said, and when.
    mute: Option<(VoiceMute, u64)>,
}

impl Quiet {
    pub fn new(now_ms: u64) -> Self {
        Self {
            since_ms: now_ms,
            noise_ms: None,
            state: None,
            mute: None,
        }
    }

    pub fn hear(&mut self, heard: Heard, at_ms: u64) {
        match heard {
            Heard::Speech => self.noise(at_ms),
            Heard::State(state) => {
                if state != Voice::Resting {
                    self.noise(at_ms);
                }
                self.state = Some((state, at_ms));
            }
            Heard::Mute(mute) => self.mute = Some((mute, at_ms)),
        }
    }

    fn noise(&mut self, at_ms: u64) {
        self.noise_ms = Some(self.noise_ms.map_or(at_ms, |last| last.max(at_ms)));
    }

    /// Whether the voice has said its state within the last three seconds:
    /// whether it can be seen at all.
    pub fn seen(&self, now_ms: u64) -> bool {
        self.state.is_some_and(|(_, said_ms)| {
            now_ms.saturating_sub(said_ms) <= STATE_LASTS.as_millis() as u64
        })
    }

    /// Ok when the voice has said it is resting within the last three
    /// seconds, and for at least `long` nobody has been heard, nothing has
    /// been said and the voice has been in no other state. Ok at once when
    /// the voice has said within three seconds that it is muted (asleep or
    /// for privacy), unless a fresh state says it is in a turn. Otherwise
    /// why not, for a person.
    pub fn for_at_least(&self, long: Duration, now_ms: u64) -> Result<(), String> {
        let long_ms = long.as_millis() as u64;
        let fresh = |said_ms: u64| now_ms.saturating_sub(said_ms) <= STATE_LASTS.as_millis() as u64;
        let in_turn = self
            .state
            .is_some_and(|(state, said_ms)| fresh(said_ms) && state != Voice::Resting);
        if !in_turn
            && self
                .mute
                .is_some_and(|(mute, said_ms)| mute.muted && fresh(said_ms))
        {
            return Ok(());
        }
        let Some((state, said_ms)) = self.state else {
            return Err(
                "the voice says nothing on the bus (voice/state): quiet cannot be judged".into(),
            );
        };
        if now_ms.saturating_sub(said_ms) > STATE_LASTS.as_millis() as u64 {
            return Err(format!(
                "the voice last said its state {} s ago: quiet cannot be judged",
                now_ms.saturating_sub(said_ms) / 1000
            ));
        }
        if state != Voice::Resting {
            return Err(format!("the voice is {}", name(state)));
        }
        if let Some(noise_ms) = self.noise_ms {
            let ago = now_ms.saturating_sub(noise_ms);
            if ago < long_ms {
                return Err(format!(
                    "someone was heard or answered {} s ago, and {} s are needed",
                    ago / 1000,
                    long_ms / 1000
                ));
            }
        }
        let watched = now_ms.saturating_sub(self.since_ms);
        if watched < long_ms {
            return Err(format!(
                "watched for {} s of the {} s needed",
                watched / 1000,
                long_ms / 1000
            ));
        }
        Ok(())
    }
}

fn name(state: Voice) -> &'static str {
    match state {
        Voice::Resting => "resting",
        Voice::Listening => "hearing someone",
        Voice::Thinking => "thinking of an answer",
        Voice::Speaking => "speaking",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1000;

    /// A voice that says it is resting once a second, from `from` to `to`.
    fn resting(quiet: &mut Quiet, from: u64, to: u64) {
        for at in (from..=to).step_by(S as usize) {
            quiet.hear(Heard::State(Voice::Resting), at);
        }
    }

    #[test]
    fn forty_five_seconds_of_nothing_after_the_last_word() {
        let mut quiet = Quiet::new(0);
        resting(&mut quiet, 0, 10 * S);
        quiet.hear(Heard::Speech, 10 * S);
        resting(&mut quiet, 10 * S, 55 * S);
        assert!(quiet.for_at_least(QUIET, 55 * S - 1).is_err());
        assert_eq!(quiet.for_at_least(QUIET, 55 * S), Ok(()));
    }

    #[test]
    fn hearing_thinking_or_speaking_is_a_turn() {
        for state in [Voice::Listening, Voice::Thinking, Voice::Speaking] {
            let mut quiet = Quiet::new(0);
            resting(&mut quiet, 0, 60 * S);
            quiet.hear(Heard::State(state), 60 * S);
            let why = quiet.for_at_least(QUIET, 60 * S).unwrap_err();
            assert!(why.starts_with("the voice is"), "{why}");
            // Back to resting: 45 s from the end of the turn.
            resting(&mut quiet, 61 * S, 104 * S);
            assert!(quiet.for_at_least(QUIET, 104 * S).is_err());
            resting(&mut quiet, 105 * S, 105 * S);
            assert_eq!(quiet.for_at_least(QUIET, 105 * S), Ok(()));
        }
    }

    #[test]
    fn a_voice_that_says_nothing_is_never_quiet() {
        let quiet = Quiet::new(0);
        assert!(quiet.for_at_least(QUIET, 600 * S).is_err());
        let mut quiet = Quiet::new(0);
        resting(&mut quiet, 0, 50 * S);
        assert_eq!(quiet.for_at_least(QUIET, 53 * S), Ok(()));
        let why = quiet.for_at_least(QUIET, 54 * S).unwrap_err();
        assert!(why.contains("cannot be judged"), "{why}");
    }

    #[test]
    fn quiet_needs_as_long_a_watch() {
        let mut quiet = Quiet::new(100 * S);
        resting(&mut quiet, 100 * S, 144 * S);
        assert!(quiet.for_at_least(QUIET, 144 * S).is_err());
        resting(&mut quiet, 145 * S, 145 * S);
        assert_eq!(quiet.for_at_least(QUIET, 145 * S), Ok(()));
        // Between turns asks only five seconds.
        let mut quiet = Quiet::new(0);
        resting(&mut quiet, 0, 20 * S);
        quiet.hear(Heard::Speech, 14 * S);
        assert!(quiet.for_at_least(BETWEEN_TURNS, 18 * S).is_err());
        assert_eq!(quiet.for_at_least(BETWEEN_TURNS, 19 * S), Ok(()));
    }

    fn muted(reason: Option<lamp_msgs::MuteReason>) -> Heard {
        Heard::Mute(VoiceMute {
            muted: true,
            reason,
            muted_by: None,
        })
    }

    #[test]
    fn a_voice_asleep_or_muted_for_privacy_is_quiet_at_once() {
        for reason in [Some(lamp_msgs::MuteReason::Sleep), None] {
            // Muted, it says no state: only its mute, once a second.
            let mut quiet = Quiet::new(0);
            quiet.hear(muted(reason), S);
            assert_eq!(quiet.for_at_least(QUIET, S), Ok(()));
            // Someone was heard just before the mute: no turn can be cut.
            let mut quiet = Quiet::new(0);
            quiet.hear(Heard::Speech, S);
            quiet.hear(muted(reason), 2 * S);
            assert_eq!(quiet.for_at_least(QUIET, 2 * S), Ok(()));
        }
    }

    #[test]
    fn a_stale_mute_or_one_in_a_turn_is_not_quiet() {
        let mut quiet = Quiet::new(0);
        quiet.hear(muted(Some(lamp_msgs::MuteReason::Sleep)), 10 * S);
        assert_eq!(quiet.for_at_least(QUIET, 13 * S), Ok(()));
        let why = quiet.for_at_least(QUIET, 14 * S).unwrap_err();
        assert!(why.contains("cannot be judged"), "{why}");
        // A fresh state that says a turn wins over the mute.
        let mut quiet = Quiet::new(0);
        quiet.hear(muted(None), 10 * S);
        quiet.hear(Heard::State(Voice::Speaking), 10 * S);
        let why = quiet.for_at_least(QUIET, 10 * S).unwrap_err();
        assert!(why.starts_with("the voice is"), "{why}");
        // Open again: judged by its state, as before.
        let mut quiet = Quiet::new(0);
        quiet.hear(
            Heard::Mute(VoiceMute {
                muted: false,
                reason: None,
                muted_by: None,
            }),
            10 * S,
        );
        let why = quiet.for_at_least(QUIET, 10 * S).unwrap_err();
        assert!(why.contains("cannot be judged"), "{why}");
    }
}
