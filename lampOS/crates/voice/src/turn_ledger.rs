//! One record for each finished turn, made as the session's events pass
//! (`SessionState::record`), for an app holding `turns` (#8726, `docs/apps.md`,
//! "Lamp Doctor's grants"): the times, the outcome, the counts, each failed
//! call as a case, and nothing anyone said. The events carry words (the
//! transcripts); the ledger counts them as they pass and keeps no text, and
//! [`lamp_msgs::TurnRecord`] has no field to put one in.
//!
//! A failed call is told here by the code that saw it ([`failed`], beside its
//! journal line), with the request it was made for; the turn's record takes
//! the cases of its request when it finishes. A case nobody takes (a request
//! that never finishes) is dropped when more than [`MOST_WAITING`] wait.
//!
//! Published on `voice/turn` from a thread of its own, one `try_send` away
//! from the caller, never waited on, as `did_ring` does; nothing reads the
//! topic unless an installed, running app holds `turns` (lamp-control).
use lamp_msgs::doctor::{
    CallKind, CaseOutcome, FailedCall, Fault, MOST_WAIT_MS, Outcome, Proof, Route, TurnRecord,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, mpsc};

/// The switch: `on` only, off unless set, in no product unit. With it off
/// nothing is tallied or published, and a worker's cases are dropped.
pub const ENVIRONMENT: &str = "LAMP_DOCTOR_TURNS";

/// Whether [`ENVIRONMENT`] is `on` in this process.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Failed calls told and not yet taken by a turn that finished.
pub const MOST_WAITING: usize = 64;
/// Turns begun and not yet finished that the ledger keeps.
const MOST_OPEN: usize = 16;

/// Failed calls waiting for their turn to finish, and proofs, by request.
#[derive(Debug, Default)]
pub struct Cases {
    failed: Vec<(u64, FailedCall)>,
    proofs: Vec<(u64, Proof)>,
}

impl Cases {
    pub fn failed(&mut self, request: u64, call: FailedCall) {
        if self.failed.len() >= MOST_WAITING {
            self.failed.remove(0);
        }
        self.failed.push((request, call));
    }

    pub fn proved(&mut self, request: u64, proof: Proof) {
        if self.proofs.len() >= MOST_WAITING {
            self.proofs.remove(0);
        }
        self.proofs.push((request, proof));
    }

    /// The cases told for `request`, taken.
    fn take(&mut self, request: u64) -> (Vec<FailedCall>, Vec<Proof>) {
        let mut failed = Vec::new();
        self.failed.retain(|(of, call)| {
            let mine = *of == request;
            if mine {
                failed.push(*call);
            }
            !mine
        });
        let mut proofs = Vec::new();
        self.proofs.retain(|(of, proof)| {
            let mine = *of == request;
            if mine {
                proofs.push(*proof);
            }
            !mine
        });
        (failed, proofs)
    }
}

static CASES: Mutex<Cases> = Mutex::new(Cases {
    failed: Vec::new(),
    proofs: Vec::new(),
});

/// The waiting cases, locked; none if another thread panicked holding them.
pub(crate) fn cases() -> Option<std::sync::MutexGuard<'static, Cases>> {
    CASES.lock().ok()
}

/// A call made for `request` failed: what it was to, how, the fixed codes and
/// how long it waited. Say it beside the journal line, once.
pub fn failed(
    request: u64,
    kind: CallKind,
    outcome: CaseOutcome,
    http: Option<u16>,
    fault: Option<Fault>,
    waited_ms: u64,
) {
    failed_if(on(), request, kind, outcome, http, fault, waited_ms);
}

/// [`failed`] with the switch given: told only when on, so a lamp (or a test)
/// with it off keeps nothing, and two requests of the same number in one
/// process never meet in the statics.
fn failed_if(
    on: bool,
    request: u64,
    kind: CallKind,
    outcome: CaseOutcome,
    http: Option<u16>,
    fault: Option<Fault>,
    waited_ms: u64,
) {
    if !on {
        return;
    }
    if let Ok(mut cases) = CASES.lock() {
        cases.failed(
            request,
            FailedCall {
                kind,
                outcome,
                http,
                fault,
                // A wait past the most a case may say is told as that most, so the
                // report that carries it is never refused for the number.
                waited_ms: u32::try_from(waited_ms.min(u64::from(MOST_WAIT_MS)))
                    .unwrap_or(MOST_WAIT_MS),
                request,
            },
        );
    }
}

/// How a failed service call reads as a case: its outcome, the HTTP status it
/// answered if it did, and the fault category (the same ten names as
/// `second_provider::fault`, never the service's text).
pub fn case_of(
    error: &crate::second_provider::net::Error,
) -> (CaseOutcome, Option<u16>, Option<Fault>) {
    use crate::second_provider::net::Error;
    match error {
        Error::Url => (CaseOutcome::Error, None, Some(Fault::Local)),
        Error::Transport(_) => (CaseOutcome::Error, None, Some(Fault::Transport)),
        Error::Timeout(step) if step.contains("connect") => {
            (CaseOutcome::Timeout, None, Some(Fault::ConnectTimeout))
        }
        Error::Timeout(_) => (CaseOutcome::Timeout, None, Some(Fault::ResponseTimeout)),
        Error::Status(status) => {
            let (outcome, fault) = match status {
                401 | 403 => (CaseOutcome::Refused, Fault::Authentication),
                402 => (CaseOutcome::Refused, Fault::QuotaExceeded),
                429 => (CaseOutcome::Refused, Fault::RateLimited),
                500.. => (CaseOutcome::Error, Fault::ServerUnavailable),
                _ => (CaseOutcome::Error, Fault::ServerRejected),
            };
            (outcome, Some(*status), Some(fault))
        }
        Error::UsageLimit(_) => (CaseOutcome::Refused, Some(429), Some(Fault::RateLimited)),
        Error::KeyInvalid => (CaseOutcome::Refused, Some(400), Some(Fault::Authentication)),
        Error::Protocol(_) => (CaseOutcome::Error, None, Some(Fault::Protocol)),
    }
}

/// The cases told in THIS process for `request`, taken: the second worker's
/// side of them. A call fails in the worker, a process of its own, and the
/// ledger that makes the record lives in the coordinator's: the worker sends
/// what it took as `ProviderOutput::Cases` before the request's
/// `TurnComplete`, and the coordinator tells them again with [`told_cases`].
pub fn take(request: u64) -> (Vec<FailedCall>, Vec<Proof>) {
    CASES
        .lock()
        .map(|mut cases| cases.take(request))
        .unwrap_or_default()
}

/// The coordinator's side: the cases the worker sent for `request`, kept for
/// the turn's record. Dropped with the switch off.
pub fn told_cases(request: u64, failed: Vec<FailedCall>, proofs: Vec<Proof>) {
    told_cases_if(on(), request, failed, proofs);
}

fn told_cases_if(on: bool, request: u64, failed: Vec<FailedCall>, proofs: Vec<Proof>) {
    if !on {
        return;
    }
    if let Ok(mut cases) = CASES.lock() {
        for call in failed {
            cases.failed(request, FailedCall { request, ..call });
        }
        for proof in proofs {
            cases.proved(request, proof);
        }
    }
}

/// A call made for `request` failed with `error`, after `waited`.
pub fn failed_by(
    request: u64,
    kind: CallKind,
    error: &crate::second_provider::net::Error,
    waited: std::time::Duration,
) {
    let (outcome, http, fault) = case_of(error);
    failed(
        request,
        kind,
        outcome,
        http,
        fault,
        waited.as_millis() as u64,
    );
}

/// A call made for `request` waited `waited` for a first sound or answer and
/// did not get one.
pub fn timed_out(request: u64, kind: CallKind, waited: std::time::Duration) {
    failed(
        request,
        kind,
        CaseOutcome::Timeout,
        None,
        Some(Fault::ResponseTimeout),
        waited.as_millis() as u64,
    );
}

/// A call made for `request` that has a proof did, or did not, do what it was
/// for (the skill ran, the memory saved).
pub fn proved(request: u64, kind: CallKind, proved: bool) {
    proved_if(on(), request, kind, proved);
}

fn proved_if(on: bool, request: u64, kind: CallKind, proved: bool) {
    if !on {
        return;
    }
    if let Ok(mut cases) = CASES.lock() {
        cases.proved(request, Proof { kind, proved });
    }
}

/// What the ledger knows of a turn before it finishes.
#[derive(Debug, Default)]
struct Open {
    endpoint_us: Option<u64>,
    first_sound_us: Option<u64>,
    tool_calls: u32,
    words_said: Option<u32>,
}

/// The ledger of one session's events.
#[derive(Debug)]
pub struct Ledger {
    open: BTreeMap<u64, Open>,
    /// The newest turn admitted: what the input's transcription is of.
    admitted: Option<u64>,
    heard: BTreeMap<u64, u32>,
    build: u32,
}

/// A transcription's words, counted: the text is not kept.
fn words(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

/// How the session's `turn_finished` outcome reads for a scorer.
fn outcome_of(why: &str) -> Outcome {
    match why {
        "provider_failed" => Outcome::Error,
        "no_audio_answer" => Outcome::NoAnswer,
        why if why.starts_with("audio_written") => Outcome::Answered,
        _ => Outcome::Cancelled,
    }
}

impl Ledger {
    /// `build` is the lamp's build number, 0 when it could not be read.
    pub fn new(build: u32) -> Self {
        Self {
            open: BTreeMap::new(),
            admitted: None,
            heard: BTreeMap::new(),
            build,
        }
    }

    /// The record this event finishes, if it is a turn's end. `at_min` is
    /// the time now in whole minutes since the Unix epoch.
    pub fn observe(&mut self, event: &Value, cases: &mut Cases, at_min: u64) -> Option<TurnRecord> {
        let turn = event["turn"].as_u64();
        let at = event["at_us"].as_u64();
        match (event["kind"].as_str()?, turn, at) {
            ("local_endpoint", Some(turn), Some(at)) => {
                self.entry(turn).endpoint_us.get_or_insert(at);
            }
            ("input_admitted", Some(turn), _) => {
                self.admitted = Some(turn);
                self.entry(turn);
            }
            ("speaker_first_write", Some(turn), Some(at)) => {
                self.entry(turn).first_sound_us.get_or_insert(at);
            }
            ("function_named", Some(turn), _) => {
                let open = self.entry(turn);
                open.tool_calls = open.tool_calls.saturating_add(1);
            }
            ("transcript", Some(turn), _) if event["finished"] == true => {
                if let Some(text) = event["text"].as_str() {
                    let open = self.entry(turn);
                    open.words_said =
                        Some(open.words_said.unwrap_or(0).saturating_add(words(text)));
                }
            }
            ("transcript", None, _) if event["finished"] == true => {
                if let (Some(turn), Some(text)) = (self.admitted, event["text"].as_str()) {
                    let count = self.heard.entry(turn).or_insert(0);
                    *count = count.saturating_add(words(text));
                }
            }
            ("turn_finished", Some(turn), _) => {
                return Some(self.finish(turn, event, cases, at_min));
            }
            _ => {}
        }
        None
    }

    fn entry(&mut self, turn: u64) -> &mut Open {
        if !self.open.contains_key(&turn)
            && self.open.len() >= MOST_OPEN
            && let Some(oldest) = self.open.keys().next().copied()
        {
            self.open.remove(&oldest);
            self.heard.remove(&oldest);
        }
        self.open.entry(turn).or_default()
    }

    fn finish(&mut self, turn: u64, event: &Value, cases: &mut Cases, at_min: u64) -> TurnRecord {
        let open = self.open.remove(&turn).unwrap_or_default();
        let heard = self.heard.remove(&turn);
        let (failed, proofs) = cases.take(turn);
        let timeouts = failed
            .iter()
            .filter(|call| call.kind == CallKind::Tool && call.outcome == CaseOutcome::Timeout)
            .count();
        let errors = failed
            .iter()
            .filter(|call| call.kind == CallKind::Tool && call.outcome == CaseOutcome::Error)
            .count();
        let sound = open
            .first_sound_us
            .zip(open.endpoint_us)
            .and_then(|(sound, endpoint)| sound.checked_sub(endpoint))
            .map(|us| u32::try_from(us / 1_000).unwrap_or(u32::MAX));
        let mut record = TurnRecord {
            turn,
            at_min,
            heard_to_first_sound_ms: sound,
            cut_ins: None,
            tool_calls: open.tool_calls,
            tool_timeouts: u32::try_from(timeouts).unwrap_or(u32::MAX),
            tool_errors: u32::try_from(errors).unwrap_or(u32::MAX),
            outcome: outcome_of(event["outcome"].as_str().unwrap_or_default()),
            words_heard: heard,
            words_said: open.words_said,
            heard_wrongly: false,
            name_missed: false,
            held_then_repeated: false,
            failed,
            proofs,
            route: Route::Unknown,
            build: self.build,
        };
        record.failed.truncate(TurnRecord::MOST_CASES);
        record.proofs.truncate(TurnRecord::MOST_CASES);
        record
    }
}

static OUT: OnceLock<Option<mpsc::SyncSender<TurnRecord>>> = OnceLock::new();

/// Tell a finished turn's record: one `try_send` to the thread that publishes
/// (started on the first call; the bus is opened there, never by the
/// caller), never waited on. A full queue drops the record.
pub fn told(record: TurnRecord) {
    if !on() {
        return;
    }
    let out = OUT.get_or_init(|| {
        let (to_thread, from_callers) = mpsc::sync_channel::<TurnRecord>(8);
        let spawned = std::thread::Builder::new()
            .name("turn-ledger".into())
            .spawn(move || publish(from_callers))
            .is_ok();
        spawned.then_some(to_thread)
    });
    if let Some(out) = out {
        let _ = out.try_send(record);
    }
}

fn publish(records: mpsc::Receiver<TurnRecord>) {
    let Ok(bus) = lamp_bus::Bus::from_environment() else {
        return;
    };
    let Ok(mut out) =
        bus.publisher_declared(TurnRecord::TOPIC, &TurnRecord::declaration(), "voice")
    else {
        return;
    };
    while let Ok(record) = records.recv() {
        if record.check().is_ok()
            && let Err(error) = out.publish_json(lamp_ipc::monotonic_us(), &record)
        {
            eprintln!("lamp-voice: a turn record was not told: {error}");
        }
    }
}

/// This lamp's build number from `lamp-update`'s `state.json`
/// (`{"installed":{"sequence":N}}`), 0 when it can't be read.
pub fn build_number() -> u32 {
    std::fs::read_to_string("/var/lib/lampos-update/state.json")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim()).ok())
        .and_then(|state| state["installed"]["sequence"].as_u64())
        .and_then(|sequence| u32::try_from(sequence).ok())
        .unwrap_or(0)
}

/// The time now in whole minutes since the Unix epoch.
pub fn now_min() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn finished(ledger: &mut Ledger, cases: &mut Cases, events: &[Value]) -> Vec<TurnRecord> {
        events
            .iter()
            .filter_map(|event| ledger.observe(event, cases, 29_000_000))
            .collect()
    }

    #[test]
    fn a_wait_past_two_minutes_is_told_as_the_most_a_case_may_say() {
        // A request number no other test uses: the statics are shared.
        let request = 4_000_000_017;
        failed_if(
            true,
            request,
            CallKind::Llm,
            CaseOutcome::Timeout,
            None,
            None,
            600_000,
        );
        let (failed, _) = take(request);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].waited_ms, MOST_WAIT_MS);
        assert_eq!(failed[0].request, request);
    }

    #[test]
    fn a_turn_makes_one_record_with_its_times_counts_and_no_words() {
        let mut ledger = Ledger::new(15_800);
        let mut cases = Cases::default();
        let made = finished(
            &mut ledger,
            &mut cases,
            &[
                json!({"kind":"input_admitted","turn":3,"at_us":1_000_000}),
                json!({"kind":"transcript","turn":null,"text":"set a timer for ten minutes","finished":true,"at_us":1_100_000}),
                json!({"kind":"local_endpoint","turn":3,"at_us":2_000_000}),
                json!({"kind":"function_named","turn":3,"at_us":2_400_000}),
                json!({"kind":"speaker_first_write","turn":3,"at_us":3_850_000}),
                json!({"kind":"transcript","turn":3,"text":"Timer set for ten minutes.","finished":true,"at_us":4_000_000}),
                json!({"kind":"turn_finished","turn":3,"outcome":"audio_written_unscored","at_us":5_000_000}),
            ],
        );
        assert_eq!(made.len(), 1);
        let record = &made[0];
        assert_eq!(record.turn, 3);
        assert_eq!(record.heard_to_first_sound_ms, Some(1_850));
        assert_eq!(record.outcome, Outcome::Answered);
        assert_eq!(record.tool_calls, 1);
        assert_eq!(record.words_heard, Some(6));
        assert_eq!(record.words_said, Some(5));
        assert_eq!(record.build, 15_800);
        // Nothing of what was said is in what goes out.
        let sent = serde_json::to_string(record).unwrap();
        for word in ["timer", "Timer", "ten", "minutes", "set a"] {
            assert!(!sent.contains(word), "{word} in {sent}");
        }
    }

    #[test]
    fn the_outcome_reads_the_sessions_and_a_turn_with_no_answer_has_no_sound() {
        let mut ledger = Ledger::new(0);
        let mut cases = Cases::default();
        for (why, want) in [
            ("provider_failed", Outcome::Error),
            ("no_audio_answer", Outcome::NoAnswer),
            (
                "audio_written_with_playback_gaps_unscored",
                Outcome::Answered,
            ),
            ("stopped by the person", Outcome::Cancelled),
        ] {
            let made = finished(
                &mut ledger,
                &mut cases,
                &[
                    json!({"kind":"local_endpoint","turn":4,"at_us":1_000}),
                    json!({"kind":"turn_finished","turn":4,"outcome":why,"at_us":9_000}),
                ],
            );
            assert_eq!(made[0].outcome, want, "{why}");
            assert_eq!(made[0].heard_to_first_sound_ms, None);
            assert_eq!(made[0].words_heard, None);
        }
    }

    #[test]
    fn a_failed_call_is_a_case_of_its_turn_and_only_of_its_turn() {
        let mut ledger = Ledger::new(1);
        let mut cases = Cases::default();
        let call = |request, kind, outcome| FailedCall {
            kind,
            outcome,
            http: None,
            fault: Some(Fault::ResponseTimeout),
            waited_ms: 4_500,
            request,
        };
        cases.failed(5, call(5, CallKind::Tool, CaseOutcome::Timeout));
        cases.failed(5, call(5, CallKind::Tool, CaseOutcome::Error));
        cases.failed(6, call(6, CallKind::Llm, CaseOutcome::Timeout));
        cases.proved(
            5,
            Proof {
                kind: CallKind::Memory,
                proved: false,
            },
        );
        let made = finished(
            &mut ledger,
            &mut cases,
            &[json!({"kind":"turn_finished","turn":5,"outcome":"no_audio_answer","at_us":1})],
        );
        assert_eq!(made[0].failed.len(), 2);
        assert_eq!((made[0].tool_timeouts, made[0].tool_errors), (1, 1));
        assert_eq!(made[0].proofs.len(), 1);
        // Turn 6's case waits for turn 6, and is taken once.
        let later = finished(
            &mut ledger,
            &mut cases,
            &[
                json!({"kind":"turn_finished","turn":6,"outcome":"no_audio_answer","at_us":2}),
                json!({"kind":"turn_finished","turn":5,"outcome":"no_audio_answer","at_us":3}),
            ],
        );
        assert_eq!(later[0].failed.len(), 1);
        assert_eq!(later[0].failed[0].kind, CallKind::Llm);
        assert!(later[1].failed.is_empty());
    }

    #[test]
    fn a_failed_service_call_reads_as_a_case_with_fixed_codes_only() {
        use crate::second_provider::net::Error;
        let case = |error: Error| case_of(&error);
        assert_eq!(
            case(Error::Status(429)),
            (CaseOutcome::Refused, Some(429), Some(Fault::RateLimited))
        );
        assert_eq!(
            case(Error::Status(503)),
            (
                CaseOutcome::Error,
                Some(503),
                Some(Fault::ServerUnavailable)
            )
        );
        assert_eq!(
            case(Error::Status(404)),
            (CaseOutcome::Error, Some(404), Some(Fault::ServerRejected))
        );
        assert_eq!(
            case(Error::Timeout("speech socket connect")),
            (CaseOutcome::Timeout, None, Some(Fault::ConnectTimeout))
        );
        assert_eq!(
            case(Error::Timeout("chat stream data")),
            (CaseOutcome::Timeout, None, Some(Fault::ResponseTimeout))
        );
        assert_eq!(
            case(Error::Transport("speech to text send")),
            (CaseOutcome::Error, None, Some(Fault::Transport))
        );
        assert_eq!(
            case(Error::KeyInvalid),
            (CaseOutcome::Refused, Some(400), Some(Fault::Authentication))
        );
    }

    /// The worker is a process of its own (#8766's read): a call that fails
    /// there is told in its static, which the coordinator's ledger never sees.
    /// What crosses is `ProviderOutput::Cases`, as the wire writes it.
    #[test]
    fn a_failed_call_in_the_worker_reaches_the_coordinators_record_over_the_wire() {
        use crate::provider_worker::ProviderOutput;
        use std::time::Duration;
        // Request numbers no other test uses: the statics are shared.
        const REQUEST: u64 = 9_000_001;
        // The worker's side: the call fails, and is told in ITS process.
        let (outcome, http, fault) = case_of(&crate::second_provider::net::Error::Status(429));
        failed_if(
            true,
            REQUEST,
            CallKind::Llm,
            outcome,
            http,
            fault,
            Duration::from_millis(800).as_millis() as u64,
        );
        proved_if(true, REQUEST, CallKind::Memory, false);
        // With the switch off a worker tells nothing at all.
        failed_if(
            false,
            REQUEST + 1,
            CallKind::Tts,
            CaseOutcome::Error,
            None,
            None,
            1,
        );
        assert_eq!(take(REQUEST + 1), (Vec::new(), Vec::new()));
        let (failed, proofs) = take(REQUEST);
        assert_eq!((failed.len(), proofs.len()), (1, 1));
        // It left the worker's static: the record cannot take it from there.
        assert_eq!(take(REQUEST), (Vec::new(), Vec::new()));
        // What goes over the channel is the wire's text.
        let sent = serde_json::to_string(&ProviderOutput::Cases {
            request: REQUEST,
            failed,
            proofs,
        })
        .unwrap();
        assert!(!sent.contains("Mai"));
        // The coordinator's side, a process with nothing in its static.
        let ProviderOutput::Cases {
            request,
            failed,
            proofs,
        } = serde_json::from_str(&sent).unwrap()
        else {
            panic!("not the cases: {sent}");
        };
        // Off, they are dropped at the coordinator.
        told_cases_if(false, request, failed.clone(), proofs.clone());
        assert_eq!(take(REQUEST), (Vec::new(), Vec::new()));
        told_cases_if(true, request, failed, proofs);
        let mut ledger = Ledger::new(1);
        let mut guard = cases().expect("the statics");
        let made = ledger
            .observe(
                &json!({"kind":"turn_finished","turn":REQUEST,"outcome":"provider_failed","at_us":1}),
                &mut guard,
                0,
            )
            .expect("the turn's record");
        assert_eq!(made.failed.len(), 1);
        assert_eq!(made.failed[0].http, Some(429));
        assert_eq!(made.failed[0].fault, Some(Fault::RateLimited));
        assert_eq!(made.failed[0].kind, CallKind::Llm);
        assert_eq!(made.proofs.len(), 1);
        assert_eq!(made.outcome, Outcome::Error);
    }

    #[test]
    fn waiting_cases_and_open_turns_are_bounded() {
        let mut cases = Cases::default();
        for request in 0..(MOST_WAITING as u64 * 2) {
            cases.failed(
                request,
                FailedCall {
                    kind: CallKind::Stt,
                    outcome: CaseOutcome::Error,
                    http: Some(500),
                    fault: None,
                    waited_ms: 1,
                    request,
                },
            );
        }
        assert_eq!(cases.failed.len(), MOST_WAITING);
        let mut ledger = Ledger::new(0);
        for turn in 0..(MOST_OPEN as u64 * 3) {
            ledger.observe(
                &json!({"kind":"input_admitted","turn":turn,"at_us":1}),
                &mut cases,
                0,
            );
        }
        assert!(ledger.open.len() <= MOST_OPEN);
    }
}
