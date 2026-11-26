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
//! The owner's Auto update (#8769): a round it switches gives way the same
//! (`round.asked == "auto"`). By day the line is said once even with nobody
//! near, so the room hears the lamp is updating; in the lamp's quiet hours
//! (`lamp_local_time::quiet_hours`, the one definition lamp-control and
//! lamp-skills keep) only to someone near, as Update now's, and a line heard
//! is answered with it as above.
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
/// fresh, switching, asked now (or by Auto update, #8769) and giving way.
/// None otherwise.
pub(super) fn giving_way(status: &UpdateStatus) -> Option<u64> {
    let round = status.round.as_ref()?;
    (status.now == "switching" && (round.asked_now() || round.asked_auto()) && round.giving_way())
        .then_some(round.started_at_unix_ms)
}

/// Whether the round giving way is Auto update's (#8769).
fn auto(status: &UpdateStatus) -> bool {
    status
        .round
        .as_ref()
        .is_some_and(lamp_msgs::UpdateRound::asked_auto)
}

/// Whether it is the lamp's quiet hours now; a local time that can't be
/// read is not.
fn quiet_hours_now() -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    crate::local_time::system()
        .is_some_and(|zone| lamp_local_time::quiet_hours(zone.minute_of_day(now_ms)))
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
    /// The round giving way is Auto update's (#8769).
    auto: bool,
    /// The lamp's quiet hours, as read with the status.
    quiet_hours: bool,
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
        let quiet_hours = status.as_ref().is_some_and(auto) && quiet_hours_now();
        self.take(status.as_ref(), quiet_hours);
    }

    /// Takes what was read.
    pub(super) fn take(&mut self, status: Option<&UpdateStatus>, quiet_hours: bool) {
        let round = status.and_then(giving_way);
        self.auto = round.is_some() && status.is_some_and(auto);
        self.quiet_hours = quiet_hours;
        if round.is_some() && round != self.round {
            eprintln!(
                "lamp-voice: {}: giving way before the switch",
                if self.auto {
                    "Auto update"
                } else {
                    "the owner's Update now"
                }
            );
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
    /// Auto update's round by day (#8769) is owed with nobody near too.
    pub(super) fn owed(&self, near: bool) -> bool {
        (near || (self.auto && !self.quiet_hours))
            && self.round.is_some()
            && self.round != self.said_round
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
    fn auto_updates_round_gives_way_and_by_day_is_told_with_nobody_near() {
        let auto = Some(UpdateRound::ASKED_AUTO);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        assert_eq!(giving_way(&switching(auto, give, 9)), Some(9));
        let mut way = GiveWay::default();
        way.take(Some(&switching(auto, give, 9)), false);
        assert!(way.owed(false), "by day, said with nobody near");
        assert!(way.active_line().is_some());
        way.said();
        assert!(!way.owed(true), "said once a round");
    }

    #[test]
    fn auto_updates_round_in_the_quiet_hours_is_said_only_to_someone_near() {
        let auto = Some(UpdateRound::ASKED_AUTO);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        let mut way = GiveWay::default();
        way.take(Some(&switching(auto, give, 9)), true);
        assert!(!way.owed(false), "at night, nothing to an empty room");
        assert!(way.owed(true), "someone talking is told");
        assert!(
            way.active_line().is_some(),
            "a line heard is answered with it"
        );
    }

    #[test]
    fn the_line_is_owed_once_a_round_and_only_to_someone_near() {
        let mut way = GiveWay::default();
        let now = Some(UpdateRound::ASKED_NOW);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        assert!(!way.owed(true));
        assert_eq!(way.active_line(), None);
        way.take(Some(&switching(now, give, 7)), false);
        // Nobody near: not owed, but a line heard is still answered with it.
        assert!(!way.owed(false));
        assert!(way.active_line().is_some());
        assert!(way.owed(true));
        way.said();
        assert!(!way.owed(true), "said once a round");
        // The give-way over: nothing more, and nothing told.
        way.take(None, false);
        assert!(!way.owed(true));
        assert_eq!(way.active_line(), None);
        // A later round's Update now is its own.
        way.take(Some(&switching(now, give, 9)), false);
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
