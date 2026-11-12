//! The app told when an update starts and when it ends (#6284, the
//! product's call: "MQTT when the update starts and when it ends"), so it can
//! show "Updating" while the lamp updates: two unasked `data` replies on
//! `fd_channel`, as `onboarding.step` is pushed.
//!
//! - [`STARTED`], `{"round", "from", "to", "at"}`: a round went past
//!   `checking` (`fetching` on, a build with changes). A round that only
//!   checks and finds the lamp current sends nothing.
//! - [`ENDED`], `{"round", "outcome", "build", "at"}`: that round ended, its
//!   outcome by name as `update/status` says it. Only for a round whose
//!   start was told.
//!
//! `round` is the round's own `started_at_unix_ms`, the app's key. Built
//! from `update/status` (`lamp_msgs::UpdateStatus`) with
//! [`crate::update_alerts`]' typing, so nothing that came in rides along:
//! builds are digits, hex digits and `+` ([`crate::update_alerts::Version`]),
//! the outcome one of [`crate::update_alerts::outcome_name`]'s fixed names,
//! the rest numbers. Not gated on the reports' consent: these are the app's
//! own screen, not the backend's alerts.
//!
//! `update/status` is looked at every [`EVERY`], connected or not, and what
//! is owed is kept in [`FILE`] in lamp-cloud's state folder: a round that
//! switches lamp-cloud restarts it mid-round, and the end must still go.
//! Each event is sent only while connected and kept until the broker's ack
//! (sent again on the next connection otherwise), as `onboarding.done` is;
//! once acked it is never sent again. A round whose end lamp-cloud never
//! saw (lamp-update stopped mid-round, and a later round began) ends as
//! `other`, so the app is not left "Updating". Off unless [`ENVIRONMENT`]
//! is `on`, with a state folder; `system.info` then says
//! `update_events_supported`.
use crate::update_alerts::{Version, outcome_name};
use lamp_msgs::{UpdateResult, UpdateStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The kind of a round's start.
pub const STARTED: &str = "update.started";

/// The kind of a round's end.
pub const ENDED: &str = "update.ended";

/// The switch, in lamp-cloud's unit: `on` exactly. Off in the product's
/// until a run on a test lamp.
pub const ENVIRONMENT: &str = "LAMP_APP_UPDATE_EVENTS";

/// The file, in `STATE_DIRECTORY`, of the rounds told and owed.
pub const FILE: &str = "update-events";

/// How often `update/status` is looked at.
pub const EVERY: Duration = Duration::from_secs(1);

/// The most rounds kept owed: more is a lamp offline through many rounds,
/// and the oldest are let go.
const MOST: usize = 4;

/// Whether the switch is on in the environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// [`FILE`]'s path while the switch is on, set once at start ([`switch`]).
static KEPT_IN: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Turns it on, kept in `file`, or off (`None`): lamp-cloud's start, with
/// the switch on and a state folder.
pub fn switch(file: Option<PathBuf>) {
    *KEPT_IN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = file;
}

/// [`switch`]'s setting: where the rounds are kept, while it is on.
pub fn kept_in() -> Option<PathBuf> {
    KEPT_IN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// lamp-update's kept `update/status`, if it is fresh and one it could
/// have said: a stale one (lamp-update stopped saying it) is not believed.
pub fn read(kept: Option<lamp_bus::Message>) -> Option<UpdateStatus> {
    let message = kept.filter(|message| !message.expired)?;
    let status = message.parse::<UpdateStatus>().ok()?;
    UpdateStatus::NOW
        .contains(&status.now.as_str())
        .then_some(status)
}

/// Which of a round's two events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Which {
    Started,
    Ended,
}

impl Which {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Started => STARTED,
            Self::Ended => ENDED,
        }
    }
}

/// A round's end, as told.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct End {
    outcome: String,
    build: Option<String>,
    at: u64,
}

/// A round with changes, told or owed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Round {
    round: u64,
    from: Option<String>,
    to: Option<String>,
    at: u64,
    #[serde(default)]
    started_acked: bool,
    #[serde(default)]
    end: Option<End>,
    #[serde(default)]
    ended_acked: bool,
}

impl Round {
    fn done(&self) -> bool {
        self.started_acked && self.ended_acked
    }
}

/// What [`FILE`] keeps: the newest round seen, and the rounds not yet
/// wholly acked, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Kept {
    newest: Option<u64>,
    rounds: Vec<Round>,
}

/// A build as text, only as [`Version`] types it.
fn build(build: Option<&lamp_msgs::UpdateBuild>) -> Option<String> {
    build.map(|build| Version::running(build).text().to_owned())
}

/// Whether `last` is round `round`'s end: its own start, or (read back
/// from lamp-update's records after it restarted, with no start) an end
/// after the round began.
fn ends(round: u64, last: &UpdateResult) -> bool {
    match last.started_at_unix_ms {
        Some(began) => began == round,
        None => last.at_unix_ms >= round,
    }
}

/// Whether a round after `round` is known, so its end was missed.
fn passed(round: u64, status: &UpdateStatus) -> bool {
    let later_round = status.now != "idle"
        && status
            .round
            .as_ref()
            .is_some_and(|now| now.started_at_unix_ms > round);
    let later_end = status
        .last
        .as_ref()
        .and_then(|last| last.started_at_unix_ms)
        .is_some_and(|began| began > round);
    later_round || later_end
}

/// The rounds told to the app, and those owed.
pub struct Events {
    file: PathBuf,
    kept: Kept,
    flying: Vec<(u64, Which)>,
    looked: Option<Instant>,
}

impl Events {
    pub fn new(file: PathBuf) -> Self {
        let kept = std::fs::read(&file)
            .ok()
            .and_then(|text| serde_json::from_slice(&text).ok())
            .unwrap_or_default();
        Self {
            file,
            kept,
            flying: Vec::new(),
            looked: None,
        }
    }

    /// Looks at `update/status`, `read` at most every [`EVERY`], at
    /// `now_ms` (Unix ms): a round's start or end newly seen is owed, and
    /// kept in [`FILE`]. A journal line if it cannot be kept.
    pub fn look(
        &mut self,
        now: Instant,
        now_ms: u64,
        read: impl FnOnce() -> Option<UpdateStatus>,
    ) -> Option<String> {
        if self.looked.is_some_and(|looked| now < looked + EVERY) {
            return None;
        }
        self.looked = Some(now);
        let status = read()?;
        self.observe(&status, now_ms).then(|| self.keep()).flatten()
    }

    /// What `status` is news of, at `now_ms`: whether anything changed.
    fn observe(&mut self, status: &UpdateStatus, now_ms: u64) -> bool {
        let before = self.kept.clone();
        let running = build(status.running.as_ref());
        for round in self
            .kept
            .rounds
            .iter_mut()
            .filter(|round| round.end.is_none())
        {
            if let Some(last) = status.last.as_ref().filter(|last| ends(round.round, last)) {
                round.end = Some(End {
                    outcome: outcome_name(&last.outcome).to_owned(),
                    build: running.clone(),
                    at: last.at_unix_ms,
                });
            } else if passed(round.round, status) {
                round.end = Some(End {
                    outcome: "other".to_owned(),
                    build: running.clone(),
                    at: now_ms,
                });
            }
        }
        // `checking` opens every round, most of which find nothing to do:
        // a round has changes once it names a newer build as its target, or
        // goes on to fetch.
        if status.brings_a_build()
            && let Some(now) = &status.round
            && self
                .kept
                .newest
                .is_none_or(|newest| now.started_at_unix_ms > newest)
        {
            self.kept.newest = Some(now.started_at_unix_ms);
            self.kept.rounds.push(Round {
                round: now.started_at_unix_ms,
                from: running,
                to: build(now.target.as_ref()),
                at: now_ms,
                started_acked: false,
                end: None,
                ended_acked: false,
            });
        }
        self.kept.rounds.retain(|round| !round.done());
        let over = self.kept.rounds.len().saturating_sub(MOST);
        self.kept.rounds.drain(..over);
        self.kept != before
    }

    /// The events to send now, in order, with their round and data: owed,
    /// not in flight, a round's end only after its start. Each is handed
    /// in turn; one the client does not take stops the rest until the next
    /// look.
    pub fn due(&self) -> Vec<(u64, Which, Value)> {
        let mut due = Vec::new();
        for round in &self.kept.rounds {
            let flying = |which| self.flying.contains(&(round.round, which));
            if !round.started_acked && !flying(Which::Started) {
                due.push((
                    round.round,
                    Which::Started,
                    json!({
                        "round": round.round,
                        "from": round.from,
                        "to": round.to,
                        "at": round.at,
                    }),
                ));
            }
            if let Some(end) = &round.end
                && !round.ended_acked
                && !flying(Which::Ended)
            {
                due.push((
                    round.round,
                    Which::Ended,
                    json!({
                        "round": round.round,
                        "outcome": end.outcome,
                        "build": end.build,
                        "at": end.at,
                    }),
                ));
            }
        }
        due
    }

    /// `which` of `round` was handed to the client. The journal's line.
    pub fn handed(&mut self, round: u64, which: Which) -> String {
        self.flying.push((round, which));
        format!("{}: handed to the client, round {round}", which.kind())
    }

    /// The broker acked `which` of `round`: kept in [`FILE`], never sent
    /// again. The journal's line.
    pub fn acked(&mut self, round: u64, which: Which) -> String {
        self.flying.retain(|flying| *flying != (round, which));
        if let Some(kept) = self.kept.rounds.iter_mut().find(|kept| kept.round == round) {
            match which {
                Which::Started => kept.started_acked = true,
                Which::Ended => kept.ended_acked = true,
            }
        }
        self.kept.rounds.retain(|round| !round.done());
        let kind = which.kind();
        match self.keep() {
            None => format!("{kind}: acked by the broker, round {round} kept"),
            Some(why) => format!("{kind}: acked by the broker, round {round}, {why}"),
        }
    }

    /// The connection went before the acks: sent again on the next one.
    pub fn lost(&mut self) {
        self.flying.clear();
    }

    /// Writes [`FILE`] (a temporary file renamed over it): why not, if it
    /// could not be.
    fn keep(&self) -> Option<String> {
        let text = serde_json::to_vec(&self.kept).unwrap_or_default();
        let temporary = self.file.with_extension("tmp");
        std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, &self.file))
            .err()
            .map(|error| {
                let _ = std::fs::remove_file(&temporary);
                format!(
                    "update events not kept ({}): may be said again after a restart",
                    error.kind()
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{UpdateBuild, UpdateRound};

    /// The round's start.
    const R: u64 = 1_792_296_000_000;

    fn built(sequence: u64) -> UpdateBuild {
        UpdateBuild {
            sequence,
            commit: "4a39f04".into(),
        }
    }

    /// A status: `now`, the round under way (begun at `R`, aiming at 813)
    /// unless idle, and the last end `(outcome, its start, at)`.
    fn status(now: &str, running: u64, last: Option<(&str, Option<u64>, u64)>) -> UpdateStatus {
        UpdateStatus {
            running: Some(built(running)),
            now: now.into(),
            last: last.map(|(outcome, started, at)| UpdateResult {
                outcome: outcome.into(),
                sequence: Some(813),
                at_unix_ms: at,
                reason: None,
                started_at_unix_ms: started,
            }),
            harness: None,
            engines: None,
            refused: None,
            round: (now != "idle").then(|| UpdateRound {
                started_at_unix_ms: R,
                step_started_at_unix_ms: R + 1_000,
                // A round names its target once it has read a newer build:
                // the helper's `checking` hasn't yet (an up-to-date round's
                // never does).
                target: (now != "checking").then(|| built(813)),
            }),
        }
    }

    /// The earlier round's end, said all through this one.
    const BEFORE: Option<(&str, Option<u64>, u64)> =
        Some(("up-to-date", Some(R - 900_000), R - 899_000));

    fn kinds(due: &[(u64, Which, Value)]) -> Vec<&'static str> {
        due.iter().map(|(_, which, _)| which.kind()).collect()
    }

    /// A round that only checks and finds the lamp current owes nothing,
    /// nor does an idle lamp.
    #[test]
    fn an_up_to_date_round_tells_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut events = Events::new(dir.path().join(FILE));
        let t = Instant::now();
        events.look(t, R, || Some(status("checking", 812, BEFORE)));
        events.look(t + EVERY, R + 2_000, || {
            Some(status(
                "idle",
                812,
                Some(("up-to-date", Some(R), R + 2_000)),
            ))
        });
        assert!(events.due().is_empty());
        assert!(!dir.path().join(FILE).exists());
    }

    /// A round still `checking` that has named a newer build as its target
    /// owes the start at once, before anything is fetched (#6284: the app is
    /// told as early as the lamp says its line).
    #[test]
    fn a_checking_round_with_a_newer_target_owes_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let mut events = Events::new(dir.path().join(FILE));
        let mut checking = status("checking", 812, BEFORE);
        if let Some(round) = checking.round.as_mut() {
            round.target = Some(built(813));
        }
        events.look(Instant::now(), R + 1_500, || Some(checking));
        let due = events.due();
        assert_eq!(kinds(&due), [STARTED]);
        assert_eq!(
            due[0].2,
            json!({"round": R, "from": "812+4a39f04", "to": "813+4a39f04", "at": R + 1_500})
        );
    }

    /// `fetching` owes the start once, with its builds; acked, it is kept
    /// and a restart owes it no more; the end, seen after the restart, is
    /// owed once, with the build now running, until it is acked.
    #[test]
    fn a_rounds_start_and_end_are_owed_once_through_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let mut events = Events::new(file.clone());
        let t = Instant::now();
        events.look(t, R + 1_500, || Some(status("fetching", 812, BEFORE)));
        let due = events.due();
        assert_eq!(kinds(&due), [STARTED]);
        assert_eq!(
            due[0].2,
            json!({"round": R, "from": "812+4a39f04", "to": "813+4a39f04", "at": R + 1_500})
        );
        // Looked at no more often than every second; said again, the same
        // round is not new.
        events.look(t + EVERY, R + 2_500, || {
            Some(status("switching", 812, BEFORE))
        });
        assert_eq!(kinds(&events.due()), [STARTED]);
        assert!(events.handed(R, Which::Started).contains("handed"));
        assert!(events.due().is_empty());
        assert!(events.acked(R, Which::Started).ends_with("kept"));
        // The switch restarts lamp-cloud mid-round: nothing owed again.
        let mut again = Events::new(file.clone());
        again.look(t, R + 9_000, || Some(status("watching", 813, BEFORE)));
        assert!(again.due().is_empty());
        // The round ends: its end is owed, once.
        let end = Some(("switched", Some(R), R + 60_000));
        again.look(t + EVERY, R + 61_000, || Some(status("idle", 813, end)));
        let due = again.due();
        assert_eq!(kinds(&due), [ENDED]);
        assert_eq!(
            due[0].2,
            json!({"round": R, "outcome": "switched", "build": "813+4a39f04", "at": R + 60_000})
        );
        again.handed(R, Which::Ended);
        // The connection goes before the ack: owed again.
        again.lost();
        assert_eq!(kinds(&again.due()), [ENDED]);
        again.handed(R, Which::Ended);
        again.acked(R, Which::Ended);
        assert!(again.due().is_empty());
        // Said again every minute, and after another restart: nothing.
        again.look(t + 2 * EVERY, R + 120_000, || {
            Some(status("idle", 813, end))
        });
        assert!(again.due().is_empty());
        let mut third = Events::new(file);
        third.look(t, R + 180_000, || Some(status("idle", 813, end)));
        assert!(third.due().is_empty());
    }

    /// A restart before the start's ack: the start and then the end are
    /// both owed, in that order; an end read back from lamp-update's
    /// records (no start) after the round began is this round's.
    #[test]
    fn a_start_not_acked_goes_before_its_end_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let mut events = Events::new(file.clone());
        let t = Instant::now();
        events.look(t, R + 1_500, || Some(status("fetching", 812, BEFORE)));
        events.handed(R, Which::Started);
        let mut again = Events::new(file);
        let end = Some(("self-switched", None, R + 30_000));
        again.look(t, R + 31_000, || Some(status("idle", 813, end)));
        let due = again.due();
        assert_eq!(kinds(&due), [STARTED, ENDED]);
        assert_eq!(due[1].2["outcome"], "self-switched");
    }

    /// A round whose end was never seen ends as `other` once a later
    /// round is known; an outcome this build does not know is `other`.
    #[test]
    fn a_missed_or_unknown_end_is_other() {
        let dir = tempfile::tempdir().unwrap();
        let mut events = Events::new(dir.path().join(FILE));
        let t = Instant::now();
        events.look(t, R + 1_500, || Some(status("fetching", 812, BEFORE)));
        let later = Some(("up-to-date", Some(R + 900_000), R + 901_000));
        events.look(t + EVERY, R + 902_000, || Some(status("idle", 812, later)));
        let due = events.due();
        assert_eq!(kinds(&due), [STARTED, ENDED]);
        assert_eq!(due[1].2["outcome"], "other");
        let dir = tempfile::tempdir().unwrap();
        let mut events = Events::new(dir.path().join(FILE));
        events.look(t, R + 1_500, || Some(status("fetching", 812, BEFORE)));
        let odd = Some(("C:/Users/someone", Some(R), R + 3_000));
        events.look(t + EVERY, R + 4_000, || Some(status("idle", 812, odd)));
        assert_eq!(events.due()[1].2["outcome"], "other");
    }

    /// The data holds the round, typed builds, a fixed outcome and times:
    /// no other key, and a build that is not digits, hex and `+` is cut to
    /// its sequence.
    #[test]
    fn the_data_holds_nothing_personal() {
        let dir = tempfile::tempdir().unwrap();
        let mut events = Events::new(dir.path().join(FILE));
        let mut fetching = status("fetching", 812, BEFORE);
        fetching.running = Some(UpdateBuild {
            sequence: 812,
            commit: "someone@example.com".into(),
        });
        events.look(Instant::now(), R + 1_500, || Some(fetching));
        let due = events.due();
        let mut keys: Vec<_> = due[0].2.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["at", "from", "round", "to"]);
        assert_eq!(due[0].2["from"], "812");
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(!text.contains("someone"), "{text}");
    }
}
