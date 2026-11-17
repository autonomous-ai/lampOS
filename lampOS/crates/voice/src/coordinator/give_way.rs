//! The owner's Update now (#7517): lamp-update gives the voice a moment
//! before its switch restarts it (`update/status`, `round.asked == "now"`
//! with `round.waiting == "give_way"`, at most 10 s). With someone near (a
//! turn under way, or speech in the last [`NEAR_US`]), the lamp's own reply
//! is ended and it says [`crate::language::UPDATING_NOW`] once, in the
//! owner's language, through the model's route as written; a line heard
//! meanwhile is answered with the same words and no chat. Nobody near, or
//! the lamp asleep or muted (the conversation's cycle has ended then):
//! nothing is said, and lamp-update switches as soon as it is quiet.
//!
//! No switch of its own: lamp-update says this only with `LAMP_UPDATE_NOW`
//! on, and lamp-cloud asks for it only with its own.
use lamp_msgs::UpdateStatus;

/// How often `update/status` is read: well within lamp-update's 3 s of
/// quiet, so the line begins before it switches.
const READ_EVERY_US: u64 = 250_000;

/// Speech this recent counts as someone near.
pub(super) const NEAR_US: u64 = 45_000_000;

/// The round giving way to the voice, by when it began, as `status` says it:
/// fresh, switching, asked now and giving way. None otherwise.
pub(super) fn giving_way(status: &UpdateStatus) -> Option<u64> {
    let round = status.round.as_ref()?;
    (status.now == "switching" && round.asked_now() && round.giving_way())
        .then_some(round.started_at_unix_ms)
}

/// Whether someone is near at `clock`: a turn under way, or speech heard or
/// said within [`NEAR_US`] (`last_speech_us`, `said_at_us`).
pub(super) fn near(turn_under_way: bool, last_speech_us: Option<u64>, clock: u64) -> bool {
    turn_under_way || last_speech_us.is_some_and(|at| clock.saturating_sub(at) < NEAR_US)
}

#[derive(Default)]
pub(super) struct GiveWay {
    bus: Option<lamp_bus::Bus>,
    read_at_us: Option<u64>,
    /// The round giving way now, by when it began.
    round: Option<u64>,
    /// The round whose line was said (or ended a reply for), so it is said once.
    said_round: Option<u64>,
}

impl GiveWay {
    pub(super) fn from_environment() -> Self {
        Self {
            bus: lamp_bus::Bus::from_environment().ok(),
            ..Self::default()
        }
    }

    /// Reads `update/status`, at most every [`READ_EVERY_US`].
    pub(super) fn read(&mut self, clock: u64) {
        if self
            .read_at_us
            .is_some_and(|at| clock.saturating_sub(at) < READ_EVERY_US)
        {
            return;
        }
        self.read_at_us = Some(clock);
        let status = self
            .bus
            .as_ref()
            .and_then(|bus| bus.latest(UpdateStatus::TOPIC).ok().flatten())
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<UpdateStatus>().ok());
        self.take(status.as_ref());
    }

    /// Takes what was read.
    pub(super) fn take(&mut self, status: Option<&UpdateStatus>) {
        let round = status.and_then(giving_way);
        if round.is_some() && round != self.round {
            eprintln!("lamp-voice: the owner's Update now: giving way before the switch");
        }
        self.round = round;
    }

    /// The line a heard line is answered with while the round gives way:
    /// told to the worker as the first update's hold is.
    pub(super) fn active_line(&self) -> Option<String> {
        self.round.and_then(|_| Self::line())
    }

    /// The line, in the owner's language, as the walk's own lines are said.
    pub(super) fn line() -> Option<String> {
        crate::onboarding::said_as_is(crate::language::updating_now(crate::language::at_start()))
    }

    /// Whether the line is owed now: a round giving way, someone near, and
    /// not said for this round yet.
    pub(super) fn owed(&self, near: bool) -> bool {
        near && self.round.is_some() && self.round != self.said_round
    }

    /// The line was said (or a reply ended for it) in this round.
    pub(super) fn said(&mut self) {
        self.said_round = self.round;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::UpdateRound;

    fn switching(asked: Option<&str>, waiting: Option<&str>, began: u64) -> UpdateStatus {
        UpdateStatus {
            running: None,
            now: "switching".into(),
            last: None,
            harness: None,
            round: Some(UpdateRound {
                started_at_unix_ms: began,
                step_started_at_unix_ms: began,
                waiting: waiting.map(str::to_owned),
                asked: asked.map(str::to_owned),
                ..Default::default()
            }),
            engines: None,
            refused: None,
        }
    }

    #[test]
    fn only_an_update_now_round_giving_way_is_given_way_to() {
        let now = Some(UpdateRound::ASKED_NOW);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        assert_eq!(giving_way(&switching(now, give, 7)), Some(7));
        // A plain round waiting for quiet, an Update now still waiting or
        // already switching, and any other phase: nothing.
        assert_eq!(giving_way(&switching(None, give, 7)), None);
        assert_eq!(
            giving_way(&switching(now, Some(UpdateRound::WAITING_QUIET), 7)),
            None
        );
        assert_eq!(giving_way(&switching(now, None, 7)), None);
        let mut fetching = switching(now, give, 7);
        fetching.now = "fetching".into();
        assert_eq!(giving_way(&fetching), None);
    }

    #[test]
    fn the_line_is_owed_once_a_round_and_only_to_someone_near() {
        let mut way = GiveWay::default();
        let now = Some(UpdateRound::ASKED_NOW);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        assert!(!way.owed(true));
        assert_eq!(way.active_line(), None);
        way.take(Some(&switching(now, give, 7)));
        // Nobody near: not owed, but a line heard is still answered with it.
        assert!(!way.owed(false));
        assert!(way.active_line().is_some());
        assert!(way.owed(true));
        way.said();
        assert!(!way.owed(true), "said once a round");
        // The give-way over: nothing more, and nothing told.
        way.take(None);
        assert!(!way.owed(true));
        assert_eq!(way.active_line(), None);
        // A later round's Update now is its own.
        way.take(Some(&switching(now, give, 9)));
        assert!(way.owed(true));
    }

    #[test]
    fn near_is_a_turn_or_speech_within_45_seconds() {
        let clock = 100_000_000;
        assert!(near(true, None, clock));
        assert!(!near(false, None, clock));
        assert!(near(false, Some(clock - 44_000_000), clock));
        assert!(!near(false, Some(clock - 45_000_000), clock));
    }

    #[test]
    fn the_line_is_the_owners_language_said_as_written() {
        let line = GiveWay::line().expect("a line the route says as it is");
        assert_eq!(
            crate::onboarding::own_words(&line),
            Some(crate::language::updating_now(crate::language::at_start()))
        );
        for language in [
            None,
            Some(crate::language::Language::Vietnamese),
            Some(crate::language::Language::Japanese),
            Some(crate::language::Language::Chinese),
            Some(crate::language::Language::Korean),
        ] {
            let words = crate::language::updating_now(language);
            assert!(
                crate::onboarding::said_as_is(words).is_some(),
                "{language:?}: {words}"
            );
        }
    }
}
