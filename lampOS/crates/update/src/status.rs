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
use crate::{
    engines::Engines,
    records::{Build, Records},
    round::Outcome,
};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{UpdateBuild, UpdateResult, UpdateRound, UpdateStatus};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// How often the status thread looks at Harness's record (#5197), so a
/// change the one-shot writes between rounds is said within this.
pub const HARNESS_LOOK: Duration = Duration::from_secs(2);

/// How long the status thread sleeps between looks: [`HARNESS_LOOK`] at
/// most with Harness's switch on, else its `every`, as before #5197.
fn look_every(every: Duration, harness: bool) -> Duration {
    if harness {
        HARNESS_LOOK.min(every)
    } else {
        every
    }
}

/// The status now: the build the records say the lamp runs, the phase, the
/// last round's end, and the round under way, if any (#5473).
pub fn status(
    records: &Path,
    now: &'static str,
    last: Option<&UpdateResult>,
    round: Option<UpdateRound>,
) -> UpdateStatus {
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
        harness: crate::harness_install::record(records),
        round,
        engines: None,
    }
}

/// [`status`], with the lamp's agents (#6240) when `engines` is given
/// (lamp-update's `LAMP_APP_ENGINES=on`), else none.
pub fn status_with(
    records: &Path,
    now: &'static str,
    last: Option<&UpdateResult>,
    round: Option<UpdateRound>,
    engines: Option<&Engines>,
) -> UpdateStatus {
    UpdateStatus {
        engines: engines.map(|engines| engines.read(records)),
        ..status(records, now, last, round)
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
        | Outcome::Held { .. }
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
    /// The round under way: the last phase it said, when it began, when
    /// that phase began, and the build it goes to once read (#5473).
    phase: Option<&'static str>,
    began_unix_ms: Option<u64>,
    step_began_unix_ms: u64,
    target: Option<UpdateBuild>,
    /// The lamp's agents (#6240), with `LAMP_APP_ENGINES=on`.
    engines: Option<Engines>,
}

impl Telling {
    /// `harness`: Harness's switch is on, so its record is looked at
    /// every [`HARNESS_LOOK`] (#5197); off, the thread wakes each `every`.
    /// `engines` (#6240, `LAMP_APP_ENGINES=on`): the lamp's agents are said
    /// too, and looked at every [`HARNESS_LOOK`] as well; none, `engines`
    /// is absent.
    pub fn start(
        bus_root: &Path,
        records: &Path,
        harness: bool,
        engines: Option<Engines>,
        say: &mut dyn FnMut(&str),
    ) -> Self {
        Self::start_every(
            bus_root,
            records,
            UpdateStatus::EVERY,
            harness,
            engines,
            say,
        )
    }

    /// As [`Telling::start`], said again every `every` (shorter in tests).
    pub fn start_every(
        bus_root: &Path,
        records: &Path,
        every: Duration,
        harness: bool,
        engines: Option<Engines>,
        say: &mut dyn FnMut(&str),
    ) -> Self {
        let last = last_in_records(records);
        let first = status_with(records, "idle", last.as_ref(), None, engines.as_ref());
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
                let folder = records.to_owned();
                let looked = engines.clone();
                let _ = std::thread::Builder::new()
                    .name("update-status".into())
                    .spawn(move || {
                        let mut waited = Duration::ZERO;
                        loop {
                            let step = look_every(every, harness || looked.is_some());
                            std::thread::sleep(step);
                            waited += step;
                            let Some(said) = again.upgrade() else {
                                return;
                            };
                            if let Ok(mut said) = said.lock() {
                                let (publisher, status) = &mut *said;
                                // Harness's one-shot writes its record
                                // between rounds (#5197): said as soon as
                                // it changes, and with the rest each `every`.
                                let mut changed = false;
                                if harness {
                                    let now = crate::harness_install::record(&folder);
                                    changed = now != status.harness;
                                    status.harness = now;
                                }
                                // The agents (#6240): Hermes's record, its
                                // freeze and the app's tap, said at once.
                                if let Some(engines) = &looked {
                                    let now = Some(engines.read(&folder));
                                    changed |= now != status.engines;
                                    status.engines = now;
                                }
                                if changed || waited >= every {
                                    waited = Duration::ZERO;
                                    let _ = publisher.publish_json(0, &*status);
                                }
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
            step_began_unix_ms: 0,
            target: None,
            engines,
        }
    }

    /// A phase of a round begins; the first is when the round began.
    pub fn phase(&mut self, now: &'static str) {
        self.phase_at(now, unix_ms());
    }

    /// [`Telling::phase`], begun at `at_unix_ms`.
    pub fn phase_at(&mut self, now: &'static str, at_unix_ms: u64) {
        self.phase = Some(now);
        self.began_unix_ms.get_or_insert(at_unix_ms);
        self.step_began_unix_ms = at_unix_ms;
        self.tell_round();
    }

    /// The round read the build it goes to (#5473): said at once, in the
    /// phase it is in. Outside a round it is neither said nor kept.
    pub fn target(&mut self, build: &Build) {
        if self.phase.is_none() {
            return;
        }
        self.target = Some(UpdateBuild {
            sequence: build.sequence,
            commit: build.commit.clone(),
        });
        self.tell_round();
    }

    /// Where the round is now, with when it and its phase began and its
    /// target.
    fn tell_round(&mut self) {
        let (Some(now), Some(began)) = (self.phase, self.began_unix_ms) else {
            return;
        };
        let round = UpdateRound {
            started_at_unix_ms: began,
            step_started_at_unix_ms: self.step_began_unix_ms.max(began),
            target: self.target.clone(),
        };
        let status = status_with(
            &self.records,
            now,
            self.last.as_ref(),
            Some(round),
            self.engines.as_ref(),
        );
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
        self.target = None;
        let status = status_with(
            &self.records,
            "idle",
            self.last.as_ref(),
            None,
            self.engines.as_ref(),
        );
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
        let empty = status(folder.path(), "idle", None, None);
        assert_eq!(
            empty,
            UpdateStatus {
                running: None,
                now: "idle".into(),
                last: None,
                harness: None,
                round: None,
                engines: None,
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
        let told = status(folder.path(), "fetching", Some(&ended), None);
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
            assert_eq!(status(folder.path(), now, None, None).now, now);
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
            false,
            None,
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

    /// #5473, for the app's Software update screen: while a round runs, the
    /// status says when the round and its step began and, once read, the
    /// build it goes to; between rounds it says none of them, and the next
    /// round starts with no target of the last. Read off a real bus.
    #[test]
    fn a_round_says_its_target_and_when_it_and_its_step_began() {
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
        let mut telling = Telling::start_every(
            bus_root.path(),
            records.path(),
            Duration::from_secs(600),
            false,
            None,
            &mut |_| {},
        );
        assert_eq!(kept().round, None, "idle at start");
        let two = Build {
            sequence: 10_580,
            commit: "e3b6593873".into(),
        };
        telling.phase_at("checking", 1_000);
        assert_eq!(
            kept().round,
            Some(UpdateRound {
                started_at_unix_ms: 1_000,
                step_started_at_unix_ms: 1_000,
                target: None,
            })
        );
        telling.target(&two);
        let read = kept();
        assert_eq!(read.now, "checking");
        assert_eq!(
            read.round.as_ref().and_then(|round| round.target.clone()),
            Some(UpdateBuild {
                sequence: 10_580,
                commit: "e3b6593873".into(),
            })
        );
        telling.phase_at("fetching", 4_000);
        telling.phase_at("switching", 9_000);
        let switching = kept();
        assert_eq!(switching.now, "switching");
        let round = switching.round.unwrap();
        assert_eq!(
            (round.started_at_unix_ms, round.step_started_at_unix_ms),
            (1_000, 9_000)
        );
        assert_eq!(round.target.map(|build| build.sequence), Some(10_580));
        // Ended: idle, with none of it.
        telling.ended(&Outcome::Switched(10_580), 12_000);
        let ended = kept();
        assert_eq!((ended.now.as_str(), ended.round.clone()), ("idle", None));
        let text = serde_json::to_string(&ended).unwrap();
        assert!(!text.contains("round"), "{text}");
        // The next round has its own start and no target until it reads one.
        telling.phase_at("checking", 20_000);
        assert_eq!(
            kept().round,
            Some(UpdateRound {
                started_at_unix_ms: 20_000,
                step_started_at_unix_ms: 20_000,
                target: None,
            })
        );
        telling.ended(&Outcome::UpToDate(10_580), 21_000);
        // A target with no round under way is neither said nor kept.
        let before = bus.latest(UpdateStatus::TOPIC).unwrap().unwrap().header.seq;
        telling.target(&two);
        let after = bus.latest(UpdateStatus::TOPIC).unwrap().unwrap().header.seq;
        assert_eq!((before, kept().round), (after, None));
        telling.phase_at("checking", 30_000);
        assert_eq!(kept().round.and_then(|round| round.target), None);
    }

    /// #5197: Harness's record, written between rounds by its one-shot, is
    /// said within a few looks, not at the next minute.
    #[test]
    fn a_harness_change_between_rounds_is_said_at_once() {
        let records = tempfile::tempdir().unwrap();
        let lamp = tempfile::tempdir().unwrap();
        let bus_root = tempfile::tempdir().unwrap();
        let bus = Bus::new(bus_root.path()).unwrap();
        let _telling = Telling::start_every(
            bus_root.path(),
            records.path(),
            Duration::from_secs(600),
            true,
            None,
            &mut |_| {},
        );
        let kept = || {
            bus.latest(UpdateStatus::TOPIC)
                .unwrap()
                .unwrap()
                .parse::<UpdateStatus>()
                .unwrap()
        };
        assert_eq!(kept().harness, None);
        crate::harness_install::at_start(lamp.path(), records.path(), true, 5);
        let until = std::time::Instant::now() + HARNESS_LOOK * 3;
        while kept().harness.is_none() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
        let said = kept().harness.expect("said within three looks");
        assert_eq!((said.install.as_str(), said.revision), ("absent", 1));
    }

    /// #6240: with `LAMP_APP_ENGINES` off there is no `engines`; on, Hermes
    /// is said from its record at start, and a change of its record, its
    /// freeze or the app's tap is said within a few looks.
    #[test]
    fn the_engines_are_said_only_with_their_switch_and_at_once_when_they_change() {
        let records = tempfile::tempdir().unwrap();
        assert_eq!(status(records.path(), "idle", None, None).engines, None);
        assert_eq!(
            status_with(records.path(), "idle", None, None, None).engines,
            None
        );
        let bus_root = tempfile::tempdir().unwrap();
        let bus = Bus::new(bus_root.path()).unwrap();
        let kept = || {
            bus.latest(UpdateStatus::TOPIC)
                .unwrap()
                .unwrap()
                .parse::<UpdateStatus>()
                .unwrap()
        };
        let other = tempfile::tempdir().unwrap();
        let _off = Telling::start_every(
            other.path(),
            records.path(),
            Duration::from_secs(600),
            false,
            None,
            &mut |_| {},
        );
        let off = Bus::new(other.path())
            .unwrap()
            .latest(UpdateStatus::TOPIC)
            .unwrap()
            .unwrap()
            .parse::<UpdateStatus>()
            .unwrap();
        assert_eq!(off.engines, None, "absent with the switch off");
        let engines = Engines {
            hermes_on: true,
            ..Engines::default()
        };
        let _telling = Telling::start_every(
            bus_root.path(),
            records.path(),
            Duration::from_secs(600),
            false,
            Some(engines.clone()),
            &mut |_| {},
        );
        let state = || kept().engines.expect("said with the switch on")["hermes"].clone();
        assert_eq!(state().state, "not_installed");
        let said_within = |want: &str| {
            let until = std::time::Instant::now() + HARNESS_LOOK * 3;
            while state().state != want && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
            }
            assert_eq!(state().state, want);
        };
        engines
            .pending
            .store(true, std::sync::atomic::Ordering::Relaxed);
        said_within("waiting");
        std::fs::write(
            records.path().join(crate::hermes_install::RECORD),
            r#"{"install":"installing","revision":2,"at_unix_ms":9}"#,
        )
        .unwrap();
        engines.settle(records.path());
        said_within("installing");
        assert_eq!(state().at_unix_ms, Some(9));
        engines
            .frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
        said_within("frozen");
    }

    /// #5241's reader: with Harness's switch off the thread sleeps its
    /// `every` as before #5197, not waking each [`HARNESS_LOOK`].
    #[test]
    fn the_status_thread_wakes_for_harness_only_with_its_switch_on() {
        assert_eq!(look_every(UpdateStatus::EVERY, false), UpdateStatus::EVERY);
        assert_eq!(look_every(UpdateStatus::EVERY, true), HARNESS_LOOK);
        let short = Duration::from_millis(500);
        assert_eq!(look_every(short, true), short);
        assert_eq!(look_every(short, false), short);
    }

    #[test]
    fn the_value_lasts_three_periods_so_a_dead_updater_reads_stale() {
        let declared = UpdateStatus::declaration();
        assert_eq!(declared.period(), Some(UpdateStatus::EVERY));
        assert_eq!(declared.lifespan(), Some(UpdateStatus::LASTING));
        assert!(UpdateStatus::LASTING >= UpdateStatus::EVERY * 3);
    }
}
