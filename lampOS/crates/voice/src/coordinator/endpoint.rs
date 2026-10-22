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

/// The switch that sends a line too short for the commit's floor to the
/// model too (`ConfirmPolicy::short_verdict_blocks`, #2135): `on`, and
/// nothing else.
pub const SHORT_VERDICT_SWITCH: &str = "LAMP_ENDPOINT_SHORT_VERDICT";

/// Speech blocks a short line needs to be sent with the switch: 0.2 s, so
/// a word ("Yes.") is asked, and a click or a cough's scrap is not.
pub const SHORT_VERDICT_BLOCKS: u16 = 20;

/// The short lines' floor the switch's `value` asks for, under the
/// profile's own `least`: none with it off, or with no floor to go under.
fn short_verdict_blocks(value: Option<&str>, least: u16) -> u16 {
    if value == Some("on") && least > SHORT_VERDICT_BLOCKS {
        SHORT_VERDICT_BLOCKS
    } else {
        0
    }
}

/// The switch that lowers the commit's floor (`ConfirmPolicy::least_speech_blocks`)
/// to a number of milliseconds of speech, and counts the line's start toward
/// it (`ConfirmPolicy::count_start`): off unless an integer from
/// [`LEAST_SPEECH_LEAST_MS`] to the profile's own floor (800 ms with the
/// lamp's `--end-silence-ms 600`). On lamp-4ace a third of whole played day
/// lines ended as `silence_fallback: short`, never asked, at 600 ms, not the
/// 400 ms commit (the first builder on #2602, 22 Oct 04:28). The floor was
/// for "Hey Lamp," alone (about 0.6 s), taken for a finished turn at 400 ms
/// on 14 Oct; the call's wait (`--call-wait-ms`, #782) now holds a line
/// that is only the name.
pub const LEAST_SPEECH_SWITCH: &str = "LAMP_ENDPOINT_LEAST_SPEECH_MS";

/// The lowest floor the switch takes: 0.5 s. At 400 ms the replay committed
/// "Hey Lamp," said alone (about 0.6 s of speech, the start counted), the
/// very line the 0.8 s floor was set for on 14 Oct (the first builder's P2
/// on #2751, 22 Oct 06:10). At 500 a 0.6 s line is still over it: that
/// name alone is held by the call's wait (`--call-wait-ms`, #782), not by
/// the floor.
pub const LEAST_SPEECH_LEAST_MS: u16 = 500;

/// The floor, in ms of speech, the switch's `value` asks for under the
/// profile's own `least` blocks: none unless an integer of milliseconds
/// from [`LEAST_SPEECH_LEAST_MS`] to `least`'s.
fn least_speech_ms(value: Option<&str>, least: u16) -> Option<u16> {
    value
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|ms| (LEAST_SPEECH_LEAST_MS..=least.saturating_mul(10)).contains(ms))
}

/// The switch under whose probability a verdict holds the line to the
/// unfinished window (`ConfirmPolicy::incomplete_probability`, held to
/// `incomplete_blocks`, 1.5 s): a probability from
/// [`UNFINISHED_BELOW_RANGE`]; unset or `off`, 0.01 as before. A verdict
/// exactly at it is not under it. For the first builder's night A/B on
/// lamp-4ace (22 Oct): of 110 Ends there, 36 were uncertain verdicts ended
/// at the 600 ms window, 17 of them at p 0.2 or under, and 1 held as
/// unfinished, while made people's lines split into 2 to 4 pieces at
/// natural pauses of 0.6 to 1.4 s: a leading "Lamp," split off alone,
/// misheard and held as a short sound, then the rest held for want of the
/// name.
pub const UNFINISHED_BELOW_SWITCH: &str = "LAMP_ENDPOINT_UNFINISHED_BELOW";

/// The probabilities [`UNFINISHED_BELOW_SWITCH`] takes.
pub const UNFINISHED_BELOW_RANGE: std::ops::RangeInclusive<f32> = 0.01..=0.6;

/// The switch for the silence an uncertain verdict waits before the line
/// ends (`ConfirmPolicy::uncertain_blocks`): whole ms from
/// [`UNSURE_RANGE_MS`] in 10 ms steps; unset or `off`, the silence window
/// (600 ms on lamp-4ace) as before. The same night A/B.
pub const UNSURE_MS_SWITCH: &str = "LAMP_ENDPOINT_UNSURE_MS";

/// The waits [`UNSURE_MS_SWITCH`] takes, in ms: from the lamp's window to
/// the unfinished window.
pub const UNSURE_RANGE_MS: std::ops::RangeInclusive<u16> = 600..=1500;

/// A switch's value that asks for something: unset, empty and `off` are
/// today's, with nothing said.
fn asked_for(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "off")
}

/// The probability [`UNFINISHED_BELOW_SWITCH`]'s `value` asks for, if it
/// is one in [`UNFINISHED_BELOW_RANGE`].
fn unfinished_below(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|probability| UNFINISHED_BELOW_RANGE.contains(probability))
}

/// The wait in ms [`UNSURE_MS_SWITCH`]'s `value` asks for, if it is a
/// whole number in [`UNSURE_RANGE_MS`] and a multiple of 10.
fn unsure_ms(value: &str) -> Option<u16> {
    value
        .parse::<u16>()
        .ok()
        .filter(|ms| UNSURE_RANGE_MS.contains(ms) && ms % 10 == 0)
}

/// `policy` with the night's two endpoint switches' values (`unfinished`
/// for [`UNFINISHED_BELOW_SWITCH`], `unsure` for [`UNSURE_MS_SWITCH`]):
/// each unset or `off` keeps today's; one out of its range is left as
/// today's too. With the journal's lines for them, said once, as the
/// policy is built.
fn with_night_switches(
    mut policy: ConfirmPolicy,
    unfinished: Option<&str>,
    unsure: Option<&str>,
) -> (ConfirmPolicy, Vec<String>) {
    let mut said = Vec::new();
    if let Some(value) = asked_for(unfinished) {
        match unfinished_below(value) {
            Some(probability) => {
                policy.incomplete_probability = probability;
                said.push(format!(
                    "lamp-voice: a verdict under {probability} holds the line to the {} ms unfinished window ({UNFINISHED_BELOW_SWITCH})",
                    u64::from(policy.incomplete_blocks) * 10
                ));
            }
            None => said.push(format!(
                "lamp-voice: {UNFINISHED_BELOW_SWITCH} is not a probability from {} to {}; a verdict under {} is unfinished, as before",
                UNFINISHED_BELOW_RANGE.start(),
                UNFINISHED_BELOW_RANGE.end(),
                policy.incomplete_probability
            )),
        }
    }
    if let Some(value) = asked_for(unsure) {
        match unsure_ms(value) {
            Some(ms) => {
                policy.uncertain_blocks = ms / 10;
                said.push(format!(
                    "lamp-voice: an uncertain verdict ends the line after {ms} ms of silence, or the silence window if longer ({UNSURE_MS_SWITCH})"
                ));
            }
            None => said.push(format!(
                "lamp-voice: {UNSURE_MS_SWITCH} is not a whole number of ms from {} to {} in steps of 10; an uncertain verdict ends on the silence window, as before",
                UNSURE_RANGE_MS.start(),
                UNSURE_RANGE_MS.end()
            )),
        }
    }
    (policy, said)
}

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
        // A lower floor, the line's start counted toward it
        // (`LAMP_ENDPOINT_LEAST_SPEECH_MS`, #2602); first, so the short
        // lines' floor below is taken under it.
        let asked = std::env::var(LEAST_SPEECH_SWITCH).ok();
        match least_speech_ms(asked.as_deref(), policy.least_speech_blocks) {
            Some(ms) => {
                policy = policy.with_floor_ms(ms);
                eprintln!(
                    "lamp-voice: a line of {ms} ms of speech or more, its start counted, ends on a complete verdict at the {} ms commit ({LEAST_SPEECH_SWITCH})",
                    u64::from(policy.commit_blocks) * 10
                );
            }
            None if asked.is_some() => eprintln!(
                "lamp-voice: {LEAST_SPEECH_SWITCH} is not a whole number of ms from {LEAST_SPEECH_LEAST_MS} to {}; the commit's floor stays",
                u64::from(policy.least_speech_blocks) * 10
            ),
            None => {}
        }
        // A line too short for that floor is asked too, and its verdict ends
        // it no sooner (`LAMP_ENDPOINT_SHORT_VERDICT`, #2135).
        policy.short_verdict_blocks = short_verdict_blocks(
            std::env::var(SHORT_VERDICT_SWITCH).ok().as_deref(),
            policy.least_speech_blocks,
        );
        if policy.short_verdict_blocks > 0 {
            eprintln!(
                "lamp-voice: a line of {} to {} ms of speech is sent to the end-of-turn model too; only a clearly unfinished one waits past the silence window, to {} ms ({SHORT_VERDICT_SWITCH})",
                u64::from(policy.short_verdict_blocks) * 10,
                u64::from(policy.least_speech_blocks) * 10,
                u64::from(policy.incomplete_blocks) * 10
            );
        }
        // The night's A/B on lamp-4ace (22 Oct): an unfinished verdict's
        // bar and an uncertain verdict's wait, each today's unless set.
        let said;
        (policy, said) = with_night_switches(
            policy,
            std::env::var(UNFINISHED_BELOW_SWITCH).ok().as_deref(),
            std::env::var(UNSURE_MS_SWITCH).ok().as_deref(),
        );
        for line in said {
            eprintln!("{line}");
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
        "last_exit":report.last_exit,"last_failure":report.last_failure,
        "kill_requested_after_us":report.kill_requested_after_us,
        "native_exit_after_us":report.native_exit_after_us,"native_reaped":report.native_reaped,
        "reader_finished_after_us":report.reader_finished_after_us})
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

    /// #2135: only `on` sends short lines to the model, and only under a
    /// floor there is: 20 blocks with the responsive profile's 80, none
    /// with the parity profile's 0.
    #[test]
    fn short_lines_are_asked_only_with_the_switch_and_a_floor() {
        assert_eq!(short_verdict_blocks(Some("on"), 80), SHORT_VERDICT_BLOCKS);
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert_eq!(short_verdict_blocks(value, 80), 0, "{value:?}");
        }
        assert_eq!(short_verdict_blocks(Some("on"), 0), 0);
        assert_eq!(short_verdict_blocks(Some("on"), SHORT_VERDICT_BLOCKS), 0);
        let policy = ConfirmPolicy {
            short_verdict_blocks: short_verdict_blocks(Some("on"), 80),
            ..ConfirmPolicy::responsive()
        };
        assert!(policy.validate().is_ok());
    }

    /// #2602: the floor is lowered only by a whole number of ms from 500 to
    /// the profile's own (800 with the responsive profile's 80 blocks); unset,
    /// not a number, under 500 (the first builder's P2 on #2751) or over the
    /// profile's, it stays. The short
    /// lines' floor taken under it still passes the policy's check.
    #[test]
    fn the_floor_is_lowered_only_by_ms_from_500_to_the_profiles() {
        for (value, taken) in [
            (None, None),
            (Some(""), None),
            (Some("on"), None),
            (Some("500ms"), None),
            (Some("0.5"), None),
            (Some("-500"), None),
            (Some("200"), None),
            (Some("499"), None),
            (Some("810"), None),
            (Some("600"), Some(600)),
            (Some("500"), Some(500)),
            (Some(" 550 "), Some(550)),
            (Some("800"), Some(800)),
        ] {
            assert_eq!(least_speech_ms(value, 80), taken, "{value:?}");
        }
        // The parity profile has no floor to lower.
        assert_eq!(least_speech_ms(Some("600"), 0), None);
        // What it sets: the floor, the start counted, and the short lines'
        // floor under it, before and after `LAMP_ENDPOINT_SHORT_VERDICT`.
        for ms in [500, 650, 800] {
            let mut policy = ConfirmPolicy {
                short_verdict_blocks: SHORT_VERDICT_BLOCKS,
                ..ConfirmPolicy::responsive()
            }
            .with_floor_ms(ms);
            assert_eq!(policy.least_speech_blocks, ms / 10);
            assert!(policy.count_start);
            assert!(policy.validate().is_ok(), "{ms}");
            policy.short_verdict_blocks =
                short_verdict_blocks(Some("on"), policy.least_speech_blocks);
            assert!(policy.validate().is_ok(), "{ms}");
        }
    }

    /// The night's A/B on lamp-4ace (22 Oct): `LAMP_ENDPOINT_UNFINISHED_BELOW`
    /// takes a probability from 0.01 to 0.6 and `LAMP_ENDPOINT_UNSURE_MS`
    /// whole ms from 600 to 1500 in 10 ms steps. Unset, empty or `off`,
    /// today's, with nothing said; any other value keeps today's and is
    /// said. What they set passes the policy's check.
    #[test]
    fn the_nights_endpoint_switches_take_only_their_ranges_and_keep_todays_otherwise() {
        let today = ConfirmPolicy {
            fallback_blocks: 60,
            ..ConfirmPolicy::responsive()
        };
        for (unfinished, unsure) in [
            (None, None),
            (Some(""), Some("")),
            (Some("off"), Some(" off ")),
        ] {
            assert_eq!(
                with_night_switches(today, unfinished, unsure),
                (today, Vec::new()),
                "{unfinished:?} {unsure:?}"
            );
        }
        for bad in [
            "on", "0", "0.009", "0.61", "1", "-0.2", "NaN", "inf", "0.2p", "20%",
        ] {
            let (policy, said) = with_night_switches(today, Some(bad), None);
            assert_eq!(policy, today, "{bad}");
            assert_eq!(said.len(), 1, "{bad}");
            assert!(said[0].contains(UNFINISHED_BELOW_SWITCH), "{bad}");
        }
        for bad in [
            "on", "599", "590", "1510", "605", "1001", "1.2", "1000ms", "-600",
        ] {
            let (policy, said) = with_night_switches(today, None, Some(bad));
            assert_eq!(policy, today, "{bad}");
            assert_eq!(said.len(), 1, "{bad}");
            assert!(said[0].contains(UNSURE_MS_SWITCH), "{bad}");
        }
        for (value, taken) in [("0.01", 0.01), ("0.3", 0.3), (" 0.25 ", 0.25), ("0.6", 0.6)] {
            let (policy, said) = with_night_switches(today, Some(value), None);
            assert_eq!(
                policy,
                ConfirmPolicy {
                    incomplete_probability: taken,
                    ..today
                },
                "{value}"
            );
            assert_eq!(said.len(), 1);
            assert!(policy.validate().is_ok(), "{value}");
        }
        for (value, blocks) in [("600", 60), ("1000", 100), (" 1210 ", 121), ("1500", 150)] {
            let (policy, _) = with_night_switches(today, None, Some(value));
            assert_eq!(
                policy,
                ConfirmPolicy {
                    uncertain_blocks: blocks,
                    ..today
                },
                "{value}"
            );
            assert!(policy.validate().is_ok(), "{value}");
        }
        // Both, and the policy's record says them; off, the record is as
        // before.
        let (both, said) = with_night_switches(today, Some("0.3"), Some("1000"));
        assert_eq!(said.len(), 2);
        let record = json!(both);
        assert_eq!(record["incomplete_probability"], json!(0.3_f32));
        assert_eq!(record["uncertain_blocks"], json!(100));
        assert!(json!(today).get("uncertain_blocks").is_none());
    }

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

    #[test]
    fn worker_report_keeps_native_reap_separate_from_reader_completion() {
        let report = Report {
            worker: 3,
            kill_requested_after_us: Some(300_000),
            native_exit_after_us: Some(301_000),
            native_reaped: Some(true),
            ..Report::default()
        };
        let value = report_json(&report);
        assert_eq!(value["worker"], 3);
        assert_eq!(value["kill_requested_after_us"], 300_000);
        assert_eq!(value["native_exit_after_us"], 301_000);
        assert_eq!(value["native_reaped"], true);
        assert!(value["reader_finished_after_us"].is_null());
        assert!(value["last_exit"].is_null());
        let finished = report_json(&Report {
            reader_finished_after_us: Some(302_000),
            ..report
        });
        assert_eq!(finished["reader_finished_after_us"], 302_000);
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
