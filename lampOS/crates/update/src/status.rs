//! What lamp-update is doing and last did, said on `update/status` for the
//! app's Lamp 2 page (#2194; lamp-cloud answers `update.status` from it).
//! Read from its own records and from where the round is, and kept nowhere
//! else: the records stay its own, 0700, and no part reads them.
//!
//! The last result is read back from the records' log when lamp-update
//! starts, so a restart, a reboot or its own switch (`self-switched`, then
//! the new build in its place) never says it as none. The value is said
//! again every minute and is stale after three: a lamp-update that died
//! mid-round is not taken as still fetching.
use crate::{records::Records, round::Outcome};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{UpdateBuild, UpdateResult, UpdateStatus};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// The status now: the build the records say the lamp runs, the phase, and
/// the last round's end.
pub fn status(records: &Path, now: &'static str, last: Option<&UpdateResult>) -> UpdateStatus {
    let running = Records::new(records.to_owned())
        .peek_state()
        .installed
        .map(|build| UpdateBuild {
            sequence: build.sequence,
            commit: build.commit,
        });
    UpdateStatus {
        running,
        now: now.to_owned(),
        last: last.cloned(),
    }
}

/// A round's end, by its outcome's name and build: never its reason, which
/// may name paths.
pub fn result(outcome: &Outcome, at_unix_ms: u64) -> UpdateResult {
    UpdateResult {
        outcome: outcome.kind().to_owned(),
        sequence: outcome.sequence(),
        at_unix_ms,
        reason: None,
        started_at_unix_ms: None,
    }
}

/// [`result`], with when the round began and, for a round that refused,
/// failed, rolled back, was unreachable or found no quiet moment, where it
/// stopped: `phase`, the last of [`UpdateStatus::NOW`] it said, or `models`.
/// Only a name of [`UpdateResult::REASONS`] is given.
pub fn ended(
    outcome: &Outcome,
    at_unix_ms: u64,
    phase: Option<&'static str>,
    began_unix_ms: Option<u64>,
) -> UpdateResult {
    let reason = match outcome {
        Outcome::Switched(_)
        | Outcome::SelfSwitched { .. }
        | Outcome::Climbs { .. }
        | Outcome::UpToDate(_)
        | Outcome::NothingPublished
        | Outcome::Would(_) => None,
        Outcome::WaitsForModels(_) => Some("models"),
        _ => phase,
    }
    .filter(|name| UpdateResult::REASONS.contains(name));
    UpdateResult {
        reason: reason.map(str::to_owned),
        started_at_unix_ms: began_unix_ms,
        ..result(outcome, at_unix_ms)
    }
}

/// The last round's end, as the records' log has it.
pub fn last_in_records(records: &Path) -> Option<UpdateResult> {
    let (outcome, sequence, at_unix_ms) = Records::new(records.to_owned()).last_round()?;
    Some(UpdateResult {
        outcome,
        sequence,
        at_unix_ms,
        reason: None,
        started_at_unix_ms: None,
    })
}

/// What `lamp-update serve` and `once` say on `update/status`: `idle` with
/// the records' last result when it starts, each phase of a round, and
/// `idle` with the round's end after it; the last said again every
/// [`UpdateStatus::EVERY`] meanwhile. A bus that cannot be used is said once
/// and nothing is said: an update never waits on its own status.
pub struct Telling {
    records: PathBuf,
    last: Option<UpdateResult>,
    said: Option<Arc<Mutex<(Publisher, UpdateStatus)>>>,
    /// The round under way: the last phase it said, and when it began.
    phase: Option<&'static str>,
    began_unix_ms: Option<u64>,
}

impl Telling {
    pub fn start(bus_root: &Path, records: &Path, say: &mut dyn FnMut(&str)) -> Self {
        Self::start_every(bus_root, records, UpdateStatus::EVERY, say)
    }

    /// As [`Telling::start`], said again every `every` (shorter in tests).
    pub fn start_every(
        bus_root: &Path,
        records: &Path,
        every: Duration,
        say: &mut dyn FnMut(&str),
    ) -> Self {
        let last = last_in_records(records);
        let first = status(records, "idle", last.as_ref());
        let publisher = Bus::new(bus_root).and_then(|bus| {
            bus.publisher_declared(
                UpdateStatus::TOPIC,
                &UpdateStatus::declaration(),
                crate::world::NAME,
            )
        });
        let said = match publisher {
            Ok(mut publisher) => {
                let _ = publisher.publish_json(0, &first);
                let said = Arc::new(Mutex::new((publisher, first)));
                let again = Arc::downgrade(&said);
                let _ = std::thread::Builder::new()
                    .name("update-status".into())
                    .spawn(move || {
                        loop {
                            std::thread::sleep(every);
                            let Some(said) = again.upgrade() else {
                                return;
                            };
                            if let Ok(mut said) = said.lock() {
                                let (publisher, status) = &mut *said;
                                let _ = publisher.publish_json(0, &*status);
                            }
                        }
                    });
                Some(said)
            }
            Err(error) => {
                say(&format!("{} cannot be said: {error}", UpdateStatus::TOPIC));
                None
            }
        };
        Self {
            records: records.to_owned(),
            last,
            said,
            phase: None,
            began_unix_ms: None,
        }
    }

    /// A phase of a round begins; the first is when the round began.
    pub fn phase(&mut self, now: &'static str) {
        self.phase = Some(now);
        self.began_unix_ms.get_or_insert_with(unix_ms);
        let status = status(&self.records, now, self.last.as_ref());
        self.tell(status);
    }

    /// A round ended: `idle`, with how, where it stopped and when it began.
    pub fn ended(&mut self, outcome: &Outcome, at_unix_ms: u64) {
        self.last = Some(ended(
            outcome,
            at_unix_ms,
            self.phase.take(),
            self.began_unix_ms.take(),
        ));
        let status = status(&self.records, "idle", self.last.as_ref());
        self.tell(status);
    }

    fn tell(&mut self, status: UpdateStatus) {
        if let Some(said) = &self.said
            && let Ok(mut said) = said.lock()
        {
            let _ = said.0.publish_json(0, &status);
            said.1 = status;
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{Build, State};

    #[test]
    fn the_status_is_the_records_build_the_phase_and_the_last_end_without_its_reason() {
        let folder = tempfile::tempdir().unwrap();
        let records = Records::new(folder.path().to_owned());
        let empty = status(folder.path(), "idle", None);
        assert_eq!(
            empty,
            UpdateStatus {
                running: None,
                now: "idle".into(),
                last: None
            }
        );
        records
            .save_state(&State {
                installed: Some(Build {
                    sequence: 3309,
                    commit: "4a39f0412".into(),
                }),
                ..State::default()
            })
            .unwrap();
        let ended = result(
            &Outcome::Refused("/etc/lampOS/x cannot be read".into()),
            1_760_950_000_000,
        );
        assert_eq!(ended.outcome, "refused");
        assert_eq!(ended.sequence, None);
        let told = status(folder.path(), "fetching", Some(&ended));
        assert_eq!(
            told.running,
            Some(UpdateBuild {
                sequence: 3309,
                commit: "4a39f0412".into()
            })
        );
        assert_eq!(told.now, "fetching");
        let text = serde_json::to_string(&told).unwrap();
        assert!(!text.contains("/etc/lampOS"), "no reason text: {text}");
        assert_eq!(result(&Outcome::Switched(3310), 1).sequence, Some(3310));
        for now in UpdateStatus::NOW {
            assert_eq!(status(folder.path(), now, None).now, now);
        }
    }

    /// #1683's update alerts: a round that did not switch says where it
    /// stopped by a fixed name, and every end says when its round began;
    /// a switch or nothing to do says no reason, and what the records give
    /// back at start says neither. An older reader takes the new fields'
    /// value, and the new one takes an older value without them.
    #[test]
    fn an_end_says_where_a_round_stopped_by_name_and_when_it_began() {
        let refused = Outcome::Refused("/etc/lampOS/x cannot be read".into());
        for (outcome, phase, reason) in [
            (refused.clone(), Some("checking"), Some("checking")),
            (
                Outcome::Failed("/var/lib/x: denied".into()),
                Some("fetching"),
                Some("fetching"),
            ),
            (
                Outcome::NotQuiet("someone spoke".into()),
                Some("switching"),
                Some("switching"),
            ),
            (
                Outcome::WaitsForModels("models 3 to 4".into()),
                Some("checking"),
                Some("models"),
            ),
            (
                Outcome::Unreachable("no route".into()),
                Some("checking"),
                Some("checking"),
            ),
            (Outcome::Switched(3312), Some("watching"), None),
            (Outcome::UpToDate(3312), Some("checking"), None),
            (refused.clone(), None, None),
            (refused.clone(), Some("not a phase"), None),
        ] {
            let end = ended(&outcome, 9_000, phase, Some(1_000));
            assert_eq!(end.reason.as_deref(), reason, "{outcome:?} in {phase:?}");
            assert_eq!(end.started_at_unix_ms, Some(1_000));
            assert_eq!(
                (end.outcome.clone(), end.sequence),
                (outcome.kind().to_owned(), outcome.sequence())
            );
            let text = serde_json::to_string(&end).unwrap();
            assert!(!text.contains('/'), "no reason text: {text}");
        }
        for name in UpdateResult::REASONS {
            assert!(
                UpdateStatus::NOW.contains(&name) || name == "models",
                "{name}"
            );
        }

        // Absent fields are not written, and either reader takes either value.
        let plain = result(&Outcome::Switched(3312), 9_000);
        let text = serde_json::to_string(&plain).unwrap();
        assert!(
            !text.contains("reason") && !text.contains("started_at"),
            "{text}"
        );
        let older: UpdateResult =
            serde_json::from_str(r#"{"outcome":"switched","sequence":3312,"at_unix_ms":9000}"#)
                .unwrap();
        assert_eq!(older, plain);
        #[derive(serde::Deserialize)]
        struct Older {
            outcome: String,
        }
        let newer =
            serde_json::to_string(&ended(&refused, 9_000, Some("fetching"), Some(1))).unwrap();
        assert_eq!(
            serde_json::from_str::<Older>(&newer).unwrap().outcome,
            "refused"
        );
    }

    /// The reviewers on #2271: the last result survives a restart (read
    /// back from the log, the self-switch's included), each round's end is
    /// said, and the value is said again while nothing changes. Read off a
    /// real bus.
    #[test]
    fn a_restart_keeps_the_last_result_each_end_is_said_and_it_is_said_again() {
        let records = tempfile::tempdir().unwrap();
        let bus_root = tempfile::tempdir().unwrap();
        let bus = Bus::new(bus_root.path()).unwrap();
        let kept = || {
            bus.latest(UpdateStatus::TOPIC)
                .unwrap()
                .unwrap()
                .parse::<UpdateStatus>()
                .unwrap()
        };
        let log = Records::new(records.path().to_owned());
        log.log("checked", "not a round's end");
        log.log_round(
            "self-switched",
            "lamp-update switched to its build",
            Some(3311),
        );
        log.log("journal-set-aside", "not a round's end either");

        let mut said = Vec::new();
        let mut telling = Telling::start_every(
            bus_root.path(),
            records.path(),
            Duration::from_millis(50),
            &mut |line| said.push(line.to_owned()),
        );
        assert!(said.is_empty(), "{said:?}");
        let started = kept();
        assert_eq!(started.now, "idle");
        let last = started.last.expect("the self-switch, read back");
        assert_eq!(
            (last.outcome.as_str(), last.sequence),
            ("self-switched", Some(3311))
        );

        assert_eq!((last.reason, last.started_at_unix_ms), (None, None));

        telling.phase("checking");
        telling.phase("fetching");
        assert_eq!(kept().now, "fetching");
        telling.ended(&Outcome::Switched(3312), 7);
        let ended = kept();
        assert_eq!(ended.now, "idle");
        let switched = ended.last.unwrap();
        assert_eq!(switched.sequence, Some(3312));
        assert_eq!(switched.reason, None);
        let began = switched.started_at_unix_ms.expect("its checking's time");
        // The next round has its own start, and says where it stopped.
        telling.phase("checking");
        telling.phase("fetching");
        telling.ended(&Outcome::Failed("the disk is full".into()), 8);
        let failed = kept().last.unwrap();
        assert_eq!(failed.reason.as_deref(), Some("fetching"));
        assert!(failed.started_at_unix_ms.unwrap() >= began);
        // A round that ends before its first phase (its lock taken, #4490:
        // not a failure) says neither, and takes nothing from the round
        // before.
        telling.ended(
            &Outcome::NotQuiet("another lamp-update is at work".into()),
            9,
        );
        let locked = kept().last.unwrap();
        assert_eq!(
            (
                locked.outcome.as_str(),
                locked.reason,
                locked.started_at_unix_ms
            ),
            ("not-quiet", None, None)
        );

        // Said again: a newer value of the same status within a few periods.
        let first = bus.latest(UpdateStatus::TOPIC).unwrap().unwrap().header.seq;
        std::thread::sleep(Duration::from_millis(250));
        let again = bus.latest(UpdateStatus::TOPIC).unwrap().unwrap().header.seq;
        assert!(again > first, "said again: {first} then {again}");
    }

    #[test]
    fn the_value_lasts_three_periods_so_a_dead_updater_reads_stale() {
        let declared = UpdateStatus::declaration();
        assert_eq!(declared.period(), Some(UpdateStatus::EVERY));
        assert_eq!(declared.lifespan(), Some(UpdateStatus::LASTING));
        assert!(UpdateStatus::LASTING >= UpdateStatus::EVERY * 3);
    }
}
