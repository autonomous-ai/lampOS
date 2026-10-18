//! Opt-in end-of-turn confirmation for the directed session.
//!
//! The model runs only in the separate `lamp-turn-end` process. The
//! coordinator holds a non-blocking client: loading, a missing model, failure,
//! recovery and shutdown are observed states, and every pause without a usable
//! verdict ends on the unchanged silence window. Nothing here waits for the
//! worker; its process lifetime is supervised by the client, not by `Rig`.
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use crate::{
    activity::{ConfirmPolicy, TurnDetector},
    options::{DirectedOptions, Endpoint, TurnTaking},
};
#[cfg(not(test))]
use lamp_ipc::monotonic_us;
use lamp_turn_end_wire::{
    Availability, Classifier, Limits, Monitor, ProcessClassifier, Report, SMART_TURN_MODEL_FILE,
    SMART_TURN_MODEL_SHA256, Teardown, WORKER_EXECUTABLE, WorkerCommand,
};
use serde_json::{Value, json};
use std::{io, path::Path, time::Duration};

/// The switch that starts the fast worker with ONNX Runtime's threads not
/// spinning while they wait for work (#1608): off unless `on`. The worker
/// runs at the voice's real-time priority, where a spinning thread holds its
/// core from everything ordinary there, `lamp-name` among them.
pub const NO_SPIN_SWITCH: &str = "LAMP_TURN_END_NO_SPIN";

/// The switch that lowers the worker to a real-time priority below the
/// voice's own round-robin 20 (#1608): a number from 1 to 99; unset, it keeps
/// the voice's. The voice's capture, playback and coordinator threads then
/// take its core back at once, while a verdict still runs ahead of every
/// ordinary process. The worker only ever lowers a priority.
pub const PRIORITY_SWITCH: &str = "LAMP_TURN_END_PRIORITY";

/// The switch's priority, if it is one from 1 to 99.
fn priority(value: Option<&str>) -> Option<u8> {
    value
        .and_then(|value| value.trim().parse::<u8>().ok())
        .filter(|priority| (1..=99).contains(priority))
}

/// The worker's command for `endpoint`, its threads not spinning with
/// `no_spin` (the fast worker only: the exact model has one thread and no
/// pool) and lowered to real-time `priority`, or none for the silence
/// window alone.
fn worker_command(
    endpoint: Endpoint,
    program: std::path::PathBuf,
    model: std::path::PathBuf,
    no_spin: bool,
    priority: Option<u8>,
) -> Option<WorkerCommand> {
    let command = match endpoint {
        Endpoint::Silence => return None,
        Endpoint::SmartTurn => WorkerCommand::smart_turn(program, model),
        Endpoint::SmartTurnFast if no_spin => {
            WorkerCommand::smart_turn_fast(program, model).without_spinning()
        }
        Endpoint::SmartTurnFast => WorkerCommand::smart_turn_fast(program, model),
    };
    Some(match priority {
        Some(priority) => command.with_realtime_priority(priority),
        None => command,
    })
}

pub(super) struct EndpointRig {
    /// Moved into the session's detector when the conversation starts.
    classifier: Option<Box<dyn Classifier>>,
    /// Absent for in-process test doubles.
    pub(super) monitor: Option<Monitor>,
    command: Option<WorkerCommand>,
    /// The pause after which a confident verdict ends the turn, if the
    /// options chose one (`--endpoint-commit-ms`, #782).
    commit_blocks: Option<u16>,
}

impl EndpointRig {
    /// Starts loading at once; the session never waits for it.
    pub(super) fn start(options: &DirectedOptions, executable: &Path) -> Option<Self> {
        let directory = executable.parent().unwrap_or(Path::new("/"));
        let (program, model) = (
            directory.join(WORKER_EXECUTABLE),
            directory.join(SMART_TURN_MODEL_FILE),
        );
        let no_spin = crate::ring_asleep::enabled(std::env::var(NO_SPIN_SWITCH).ok().as_deref());
        let asked = std::env::var(PRIORITY_SWITCH).ok();
        let lowered = priority(asked.as_deref());
        if asked.is_some() && lowered.is_none() {
            eprintln!(
                "lamp-voice: {PRIORITY_SWITCH} is not a priority from 1 to 99; the end-of-turn worker keeps the voice's"
            );
        }
        let command = worker_command(options.endpoint, program, model, no_spin, lowered)?;
        let mut rig = Self::process(command, Limits::default());
        rig.commit_blocks = options.endpoint_commit_blocks;
        Some(rig)
    }

    pub(super) fn process(command: WorkerCommand, limits: Limits) -> Self {
        let classifier = ProcessClassifier::spawn(command.clone(), limits);
        Self {
            monitor: Some(classifier.monitor()),
            classifier: Some(Box::new(classifier)),
            command: Some(command),
            commit_blocks: None,
        }
    }

    #[cfg(test)]
    pub(super) fn double(classifier: Box<dyn Classifier>) -> Self {
        Self {
            classifier: Some(classifier),
            monitor: None,
            command: None,
            commit_blocks: None,
        }
    }

    /// Moves the classifier into the detector and describes the policy.
    pub(super) fn attach(
        endpoint: Option<&mut Self>,
        turn_taking: TurnTaking,
        mut detector: TurnDetector,
    ) -> io::Result<(TurnDetector, Value)> {
        let Some(endpoint) = endpoint else {
            let silence = detector.endpoint_silence_ms();
            return Ok((detector, policy_event(turn_taking, silence, None, None)));
        };
        let classifier = endpoint
            .classifier
            .take()
            .ok_or_else(|| io::Error::other("endpoint classifier already in use"))?;
        // The parity profile ends a confident turn after one second, which
        // only makes sense under a longer silence window. A window chosen
        // shorter than that asks for a quicker lamp, so a confident verdict
        // ends the turn at 400 ms and the window itself decides the rest.
        let mut policy = if detector.endpoint_silence_ms()
            > u64::from(ConfirmPolicy::default().commit_blocks) * 10
        {
            ConfirmPolicy::default()
        } else {
            ConfirmPolicy::responsive()
        };
        // A longer commit keeps a person who pauses at a comma (#782); the
        // record of the policy says which was in force.
        if let Some(commit) = endpoint.commit_blocks {
            policy.commit_blocks = commit;
        }
        detector = detector
            .with_confirmation(classifier, policy)
            .map_err(io::Error::other)?;
        let event = policy_event(
            turn_taking,
            detector.endpoint_silence_ms(),
            detector.confirmation_policy(),
            endpoint.command.as_ref(),
        );
        Ok((detector, event))
    }
}

fn policy_event(
    turn_taking: TurnTaking,
    silence_ms: u64,
    confirmation: Option<ConfirmPolicy>,
    command: Option<&WorkerCommand>,
) -> Value {
    let mut event = json!({"kind":"endpoint_policy","at_us":monotonic_us(),
        "turn_taking":turn_taking,"silence_wait_ms":silence_ms,
        "basis":"consecutive_low_vad_scores","semantic_confirmation":false});
    // The baseline record keeps exactly its existing fields.
    if let Some(confirmation) = confirmation {
        event["basis"] = json!("smart_turn_with_silence_fallback");
        event["semantic_confirmation"] = json!(true);
        event["confirmation"] = json!(confirmation);
        event["worker"] = json!(command.map(|command| json!({"program":command.program,
            "args":command.args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>()})));
        event["model"] = json!({"file":SMART_TURN_MODEL_FILE,"sha256":SMART_TURN_MODEL_SHA256});
    }
    event
}

pub(super) fn availability_json(availability: &Availability) -> Value {
    let (state, detail) = match availability {
        Availability::Ready => ("ready", None),
        Availability::Loading => ("loading", None),
        Availability::Recovering(reason) => ("recovering", Some(reason)),
        Availability::Failed(reason) => ("failed", Some(reason)),
        Availability::Closed => ("closed", None),
    };
    json!({"state":state,"detail":detail})
}

pub(super) fn report_json(report: &Report) -> Value {
    json!({"worker":report.worker,"pid":report.pid,"spawned":report.spawned,"ready":report.ready,
        "load_us":report.load_us,"cold_us":report.cold_us,"ready_after_us":report.ready_after_us,
        "model_sha256":report.model_sha256,"requests":report.requests,"verdicts":report.verdicts,
        "errors":report.errors,"discarded":report.discarded,"lost":report.lost,"kills":report.kills,
        "last_exit":report.last_exit,"last_failure":report.last_failure})
}

/// Records each change of classifier availability with the worker report.
#[derive(Default)]
pub(super) struct Watch {
    last: Option<Availability>,
}

impl Watch {
    pub(super) fn observe(
        &mut self,
        detector: &TurnDetector,
        monitor: Option<&Monitor>,
    ) -> Option<Value> {
        let availability = detector.classifier_availability()?;
        if self.last.as_ref() == Some(&availability) {
            return None;
        }
        let event = json!({"kind":"endpoint_worker","at_us":monotonic_us(),
            "availability":availability_json(&availability),
            "report":monitor.map(|monitor| report_json(&monitor.report()))});
        self.last = Some(availability);
        Some(event)
    }
}

impl EndpointRig {
    /// Runtime teardown, never the live loop: closes a client the session did
    /// not take, then waits at most `limit` for its last worker to be reaped,
    /// so a later runtime cannot overlap it. Returns the final record.
    ///
    /// Returns the record, and an error unless every worker process was
    /// confirmed reaped: an unconfirmed exit must keep a later runtime, such
    /// as a service cycle after a physical mute, from starting.
    pub(super) fn finish(mut self, limit: Duration) -> Option<(Value, io::Result<()>)> {
        drop(self.classifier.take());
        let monitor = self.monitor?;
        let teardown = monitor.wait_finished(limit);
        let (reaped, result) = match &teardown {
            Teardown::Reaped => (true, Ok(())),
            Teardown::Pending => (
                false,
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "endpoint worker exit not confirmed within {} ms",
                        limit.as_millis()
                    ),
                )),
            ),
            Teardown::Unreaped(reason) => (
                false,
                Err(io::Error::other(format!("endpoint worker {reason}"))),
            ),
        };
        let event = json!({"kind":"endpoint_worker_final","at_us":monotonic_us(),
            "reaped":reaped,"teardown":format!("{teardown:?}"),
            "availability":availability_json(&monitor.availability()),
            "report":report_json(&monitor.report())});
        Some((event, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_turn_end_wire::{Outcome, Submit, Token};

    /// #1608: `--no-spin` only with the switch, and only for the fast worker.
    #[test]
    fn the_fast_worker_stops_spinning_only_with_its_switch() {
        let spins = |endpoint, no_spin| {
            worker_command(endpoint, "w".into(), "m".into(), no_spin, None)
                .map(|command| command.args.iter().any(|arg| arg == "--no-spin"))
        };
        assert_eq!(spins(Endpoint::SmartTurnFast, true), Some(true));
        assert_eq!(spins(Endpoint::SmartTurnFast, false), Some(false));
        assert_eq!(spins(Endpoint::SmartTurn, true), Some(false));
        assert_eq!(spins(Endpoint::Silence, true), None);
        let fast =
            worker_command(Endpoint::SmartTurnFast, "w".into(), "m".into(), true, None).unwrap();
        assert!(fast.args.iter().any(|arg| arg == "--fast"));
    }

    /// #1608: the worker is lowered only with a priority from 1 to 99, for
    /// either model; anything else leaves it at the voice's.
    #[test]
    fn the_worker_is_lowered_only_with_a_priority_from_1_to_99() {
        for (value, taken) in [
            (Some("10"), Some(10)),
            (Some(" 1 "), Some(1)),
            (Some("99"), Some(99)),
            (Some("0"), None),
            (Some("100"), None),
            (Some("on"), None),
            (None, None),
        ] {
            assert_eq!(priority(value), taken, "{value:?}");
        }
        for endpoint in [Endpoint::SmartTurn, Endpoint::SmartTurnFast] {
            let lowered =
                worker_command(endpoint, "w".into(), "m".into(), false, Some(10)).unwrap();
            let args: Vec<_> = lowered
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            let at = args
                .iter()
                .position(|arg| arg == "--realtime-priority")
                .unwrap();
            assert_eq!(args[at + 1], "10", "{endpoint:?}");
            let kept = worker_command(endpoint, "w".into(), "m".into(), false, None).unwrap();
            assert!(!kept.args.iter().any(|arg| arg == "--realtime-priority"));
        }
    }

    struct Silent;
    impl Classifier for Silent {
        fn submit(&mut self, _: Token, _: &[i16]) -> Submit {
            Submit::Unavailable
        }
        fn poll(&mut self) -> Option<Outcome> {
            None
        }
        fn availability(&self) -> Availability {
            Availability::Loading
        }
        fn cancel(&mut self) {}
    }

    fn attached(detector: TurnDetector) -> io::Result<ConfirmPolicy> {
        let mut rig = EndpointRig::double(Box::new(Silent));
        let (detector, _) = EndpointRig::attach(Some(&mut rig), TurnTaking::V1Baseline, detector)?;
        Ok(detector.confirmation_policy().unwrap())
    }

    #[test]
    fn a_short_silence_window_selects_the_quicker_profile_and_stays_the_fallback() {
        // The baseline window keeps the parity profile exactly.
        let parity = attached(TurnDetector::v1_baseline()).unwrap();
        assert_eq!(
            (
                parity.commit_blocks,
                parity.fallback_blocks,
                parity.incomplete_blocks
            ),
            (100, 120, 150)
        );
        // 700 ms chosen: a confident verdict ends at 400 ms, an unfinished one
        // holds to 1.5 s, and everything else ends on the 700 ms chosen.
        let quick = attached(TurnDetector::v1_baseline().with_end_silence(70).unwrap()).unwrap();
        assert_eq!(
            (
                quick.commit_blocks,
                quick.fallback_blocks,
                quick.incomplete_blocks
            ),
            (40, 70, 150)
        );
        assert_eq!(quick.complete_probability, parity.complete_probability);
        // A window no longer than the quick commit leaves nothing to confirm.
        assert!(attached(TurnDetector::v1_baseline().with_end_silence(40).unwrap()).is_err());
    }

    #[test]
    fn a_chosen_commit_changes_only_when_a_confident_verdict_ends_the_turn() {
        let with = |commit: Option<u16>, detector: TurnDetector| {
            let mut rig = EndpointRig::double(Box::new(Silent));
            rig.commit_blocks = commit;
            EndpointRig::attach(Some(&mut rig), TurnTaking::Duplex, detector)
                .map(|(detector, event)| (detector.confirmation_policy().unwrap(), event))
        };
        // lamp-4ace's 600 ms window: none chosen is the quick profile as it was.
        let window = || TurnDetector::v1_baseline().with_end_silence(60).unwrap();
        let (none, _) = with(None, window()).unwrap();
        assert_eq!(
            none,
            ConfirmPolicy {
                fallback_blocks: 60,
                ..ConfirmPolicy::responsive()
            }
        );
        let (later, event) = with(Some(55), window()).unwrap();
        assert_eq!(
            later,
            ConfirmPolicy {
                commit_blocks: 55,
                ..none
            }
        );
        assert_eq!(event["confirmation"]["commit_blocks"], json!(55));
        // A commit at or past the window cannot be.
        assert!(with(Some(60), window()).is_err());
    }
}
