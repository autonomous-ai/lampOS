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
//! is answered with it as above. At night "near" is only a turn under way or
//! speech since the lamp first saw the round, so a "goodnight" said before it
//! gets no unasked line.
//!
//! Once such a round has switched (`last.outcome == "switched"` for the round
//! this voice saw as Auto update's), by day the lamp says
//! [`crate::language::UPDATE_DONE`] once, in the owner's language; at night
//! nothing.
//!
//! Auto update's unasked lines (the one before by day and the done line)
//! are said only with [`SAY_SWITCH`] `on`: off by default (the product
//! owner, 27 Nov: the team ships builds often, and a lamp telling each one
//! bothers its owner). Off, an Auto update round says nothing unasked, day or
//! night; someone who speaks during it is still told it is updating, and the
//! ring still shows it (`LAMP_RING_UPDATE`).
//!
//! No switch of its own for Update now: lamp-update says this only with `LAMP_UPDATE_NOW`
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

/// Auto update's unasked lines, `on` exactly; off unless set (#8769).
pub const SAY_SWITCH: &str = "LAMP_UPDATE_AUTO_SAY";

/// A done line not said by then is dropped: it is about the update just
/// over, never one long ago.
const DONE_WITHIN_US: u64 = 300_000_000;

/// Whether `last`, the end `status` says, brought its build in: `switched`,
/// or `self-switched` with the parts running it (lamp-update's own build
/// switched with them). A climb, also `self-switched`, took only the updater
/// the build carries, so the build does not run yet and nothing is done.
fn build_in(status: &UpdateStatus, last: &lamp_msgs::UpdateResult) -> bool {
    match last.outcome.as_str() {
        "switched" => true,
        "self-switched" => {
            last.sequence.is_some()
                && status.running.as_ref().map(|running| running.sequence) == last.sequence
        }
        _ => false,
    }
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
    /// When (this voice's clock) the round giving way was first seen.
    seen_at_us: u64,
    /// An Auto update round this voice saw under way, by when it began, not
    /// yet ended.
    auto_round: Option<u64>,
    /// Auto update's unasked lines are said ([`SAY_SWITCH`]).
    tell: bool,
    /// That round's build came in by day, when (this voice's clock): the
    /// done line is owed.
    done: Option<u64>,
}

impl GiveWay {
    pub(super) fn from_environment() -> Self {
        Self {
            bus: lamp_bus::Bus::from_environment().ok(),
            tell: std::env::var(SAY_SWITCH).is_ok_and(|value| value == "on"),
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
        let quiet_hours =
            (status.as_ref().is_some_and(auto) || self.auto_round.is_some() || self.done.is_some())
                && quiet_hours_now();
        self.take(status.as_ref(), quiet_hours, clock);
    }

    /// Takes what was read.
    pub(super) fn take(&mut self, status: Option<&UpdateStatus>, quiet_hours: bool, clock: u64) {
        let round = status.and_then(giving_way);
        if let Some(status) = status {
            if let Some(began) = status
                .round
                .as_ref()
                .filter(|round| round.asked_auto())
                .map(|round| round.started_at_unix_ms)
            {
                self.auto_round = Some(began);
            } else if status.now == "idle"
                && let Some(began) = self.auto_round
            {
                // The round this voice saw has ended: its build in by day,
                // the done line is owed once; any other end, nothing.
                let done = status.last.as_ref().is_some_and(|last| {
                    build_in(status, last)
                        && last
                            .started_at_unix_ms
                            .map_or(last.at_unix_ms >= began, |at| at == began)
                });
                self.done = (done && self.tell && !quiet_hours).then_some(clock);
                self.auto_round = None;
            }
        }
        // A done line not said within DONE_WITHIN_US, or not by the quiet
        // hours, is dropped.
        if self
            .done
            .is_some_and(|at| quiet_hours || clock.saturating_sub(at) >= DONE_WITHIN_US)
        {
            self.done = None;
        }
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
        if round.is_some() && round != self.round {
            self.seen_at_us = clock;
        }
        self.round = round;
    }

    /// Whether someone is near at `clock` ([`near`]); at night, for Auto
    /// update's round, only a turn under way or speech since the round was
    /// first seen.
    pub(super) fn near(
        &self,
        turn_under_way: bool,
        last_speech_us: Option<u64>,
        clock: u64,
    ) -> bool {
        let night = self.auto && self.quiet_hours;
        near(
            turn_under_way,
            last_speech_us.filter(|at| !night || *at >= self.seen_at_us),
            clock,
        )
    }

    /// Whether the done line is owed: Auto update's round switched by day.
    pub(super) fn done_owed(&self) -> bool {
        self.done.is_some()
    }

    /// The line owed now: the done line, else the updating one.
    pub(super) fn note(&self) -> Option<String> {
        if self.done.is_some() {
            crate::onboarding::said_as_is(crate::language::update_done(crate::language::at_start()))
        } else {
            Self::line()
        }
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
        (near || (self.auto && self.tell && !self.quiet_hours))
            && self.round.is_some()
            && self.round != self.said_round
    }

    /// The line was said (or a reply ended for it) in this round.
    pub(super) fn said(&mut self) {
        if self.done.take().is_some() {
            return;
        }
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
        let mut way = told();
        way.take(Some(&switching(auto, give, 9)), false, 0);
        assert!(way.owed(false), "by day, said with nobody near");
        assert!(way.active_line().is_some());
        way.said();
        assert!(!way.owed(true), "said once a round");
    }

    #[test]
    fn auto_updates_round_in_the_quiet_hours_is_said_only_to_someone_near() {
        let auto = Some(UpdateRound::ASKED_AUTO);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        let mut way = told();
        way.take(Some(&switching(auto, give, 9)), true, 0);
        assert!(!way.owed(false), "at night, nothing to an empty room");
        assert!(way.owed(true), "someone talking is told");
        assert!(
            way.active_line().is_some(),
            "a line heard is answered with it"
        );
    }

    /// A give-way with Auto update's unasked lines on.
    fn told() -> GiveWay {
        GiveWay {
            tell: true,
            ..GiveWay::default()
        }
    }

    #[test]
    fn with_its_switch_off_auto_update_says_nothing_unasked() {
        let auto = Some(UpdateRound::ASKED_AUTO);
        let mut way = GiveWay::default();
        way.take(
            Some(&switching(auto, Some(UpdateRound::WAITING_GIVE_WAY), 9)),
            false,
            0,
        );
        assert!(!way.owed(false), "by day, nothing to an empty room");
        assert!(way.owed(true), "someone talking is still told");
        assert!(way.active_line().is_some(), "a line heard is answered");
        let mut watching = switching(auto, None, 9);
        watching.now = "watching".into();
        way.take(Some(&watching), false, 0);
        way.take(Some(&ended("switched", 9)), false, 0);
        assert!(!way.done_owed(), "no done line");
    }

    fn ended(outcome: &str, began: u64) -> UpdateStatus {
        UpdateStatus {
            now: "idle".into(),
            round: None,
            last: Some(lamp_msgs::UpdateResult {
                outcome: outcome.into(),
                sequence: Some(2),
                at_unix_ms: began + 60_000,
                reason: None,
                started_at_unix_ms: Some(began),
            }),
            ..switching(None, None, began)
        }
    }

    #[test]
    fn auto_updates_round_that_switched_by_day_is_told_done_once() {
        let auto = Some(UpdateRound::ASKED_AUTO);
        let mut way = told();
        // The new build's voice sees the round watching, then its end.
        let mut watching = switching(auto, None, 9);
        watching.now = "watching".into();
        way.take(Some(&watching), false, 0);
        assert!(!way.done_owed());
        way.take(Some(&ended("switched", 9)), false, 0);
        assert!(way.done_owed());
        let note = way.note().expect("the done line");
        assert_eq!(
            crate::onboarding::own_words(&note),
            Some(crate::language::update_done(crate::language::at_start()))
        );
        way.said();
        assert!(!way.done_owed(), "once");
        way.take(Some(&ended("switched", 9)), false, 0);
        assert!(!way.done_owed(), "the same end again is nothing");
        // Put back, or at night: nothing.
        way.take(Some(&watching), false, 0);
        way.take(Some(&ended("rolled-back", 9)), false, 0);
        assert!(!way.done_owed());
        way.take(Some(&watching), true, 0);
        way.take(Some(&ended("switched", 9)), true, 0);
        assert!(!way.done_owed(), "the quiet hours");
        // An Update now round, or an end this voice saw no round of: nothing.
        let mut now = switching(Some(UpdateRound::ASKED_NOW), None, 9);
        now.now = "watching".into();
        way.take(Some(&now), false, 0);
        way.take(Some(&ended("switched", 9)), false, 0);
        assert!(!way.done_owed());
    }

    fn running(mut status: UpdateStatus, sequence: u64) -> UpdateStatus {
        status.running = Some(lamp_msgs::UpdateBuild {
            sequence,
            commit: "c".into(),
        });
        status
    }

    #[test]
    fn a_self_switch_with_the_build_in_is_told_done_and_a_climb_is_not() {
        let mut watching = switching(Some(UpdateRound::ASKED_AUTO), None, 9);
        watching.now = "watching".into();
        // lamp-update's own build switched with the parts: the build runs.
        let mut way = told();
        way.take(Some(&watching), false, 0);
        way.take(Some(&running(ended("self-switched", 9), 2)), false, 0);
        assert!(way.done_owed());
        // A climb: only the updater was taken, the parts still run build 1.
        let mut way = told();
        way.take(Some(&watching), false, 0);
        way.take(Some(&running(ended("self-switched", 9), 1)), false, 0);
        assert!(!way.done_owed(), "a climb is not the update done");
        // The new updater's first word, from its records, has no start: an
        // end at or after the round's start counts.
        let mut way = told();
        way.take(Some(&watching), false, 0);
        let mut from_records = running(ended("self-switched", 9), 2);
        from_records.last.as_mut().unwrap().started_at_unix_ms = None;
        way.take(Some(&from_records), false, 0);
        assert!(way.done_owed());
    }

    #[test]
    fn a_done_line_not_said_is_dropped_after_five_minutes_or_at_night() {
        let mut watching = switching(Some(UpdateRound::ASKED_AUTO), None, 9);
        watching.now = "watching".into();
        let mut way = told();
        way.take(Some(&watching), false, 0);
        way.take(Some(&ended("switched", 9)), false, 1_000);
        way.take(Some(&ended("switched", 9)), false, 1_000 + 299_000_000);
        assert!(way.done_owed());
        way.take(Some(&ended("switched", 9)), false, 1_000 + 300_000_000);
        assert!(!way.done_owed(), "five minutes on");
        way.take(Some(&watching), false, 0);
        way.take(Some(&ended("switched", 9)), false, 0);
        assert!(way.done_owed());
        way.take(Some(&ended("switched", 9)), true, 10);
        assert!(!way.done_owed(), "the quiet hours came");
    }

    #[test]
    fn at_night_only_speech_since_the_round_counts_as_near() {
        let auto = Some(UpdateRound::ASKED_AUTO);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        let mut way = told();
        way.take(Some(&switching(auto, give, 9)), true, 100_000_000);
        assert!(
            !way.near(false, Some(70_000_000), 101_000_000),
            "goodnight 30 s before"
        );
        assert!(
            way.near(false, Some(100_500_000), 101_000_000),
            "spoken during it"
        );
        assert!(way.near(true, None, 101_000_000), "a turn under way");
        // By day, any speech within 45 s, as Update now's.
        way.take(Some(&switching(auto, give, 10)), false, 100_000_000);
        assert!(way.near(false, Some(70_000_000), 101_000_000));
    }

    #[test]
    fn the_line_is_owed_once_a_round_and_only_to_someone_near() {
        let mut way = told();
        let now = Some(UpdateRound::ASKED_NOW);
        let give = Some(UpdateRound::WAITING_GIVE_WAY);
        assert!(!way.owed(true));
        assert_eq!(way.active_line(), None);
        way.take(Some(&switching(now, give, 7)), false, 0);
        // Nobody near: not owed, but a line heard is still answered with it.
        assert!(!way.owed(false));
        assert!(way.active_line().is_some());
        assert!(way.owed(true));
        way.said();
        assert!(!way.owed(true), "said once a round");
        // The give-way over: nothing more, and nothing told.
        way.take(None, false, 0);
        assert!(!way.owed(true));
        assert_eq!(way.active_line(), None);
        // A later round's Update now is its own.
        way.take(Some(&switching(now, give, 9)), false, 0);
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
