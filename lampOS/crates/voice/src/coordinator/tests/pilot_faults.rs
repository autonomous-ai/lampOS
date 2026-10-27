//! Supervised-pilot fault qualification through the production coordinator.
//!
//! Every test runs the real `conversation()` service loop (`Lifetime::Serve`,
//! V1-baseline turn-taking, default options) and `finish_active_run`, with
//! real child processes and real local IPC. The children are cooperative
//! fixtures; one test peer answers for capture, privacy, provider and speaker
//! on the production channels and injects exactly one fault at an observed
//! event, not after a fixed sleep. The speaker peer applies the production
//! `BoundaryGuard` to every chunk, as the real speaker does.
//!
//! Every fault here is injected. Nothing is a device, microphone, speaker,
//! cloud or acoustic measurement.
use super::*;
use crate::{
    camera_worker::{CameraEvent, Counts, Mode, PortMetadata, StopReason, Timing as CameraTiming},
    provider_flow::OutputSender,
    provider_worker::{FailedStage, Fault},
    ring_wire::{RingBlankReason, RingFeedback, RingFrame},
    wire::{CaptureTiming, ReferenceTiming},
};
use lamp_audio::ownership::PlaybackCursor;
use lamp_turn_owner::BoundaryGuard;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// The peers keep production freshness bounds (50 ms privacy, 100 ms
/// capture leases). Every scenario is stepped: logical time advances only
/// after every peer has run, so a loaded host cannot starve a peer past a
/// lease (#973).
/// Serialize sessions because SIGTERM is process-wide.
static ONE_SESSION: Mutex<()> = Mutex::new(());

pub(super) fn one_session() -> std::sync::MutexGuard<'static, ()> {
    ONE_SESSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Provider samples per answer packet: the production maximum, 40 ms at
/// 24 kHz.
const PACKET: usize = crate::provider_flow::PACKET_SAMPLES;
/// Upper bound on any single scenario, including cleanup. A run that needs
/// it has hung; the peer then reports instead of blocking the test binary.
const SCENARIO_LIMIT: Duration = Duration::from_secs(12);

/// What the provider peer does with one request after its End.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Answer {
    /// `packets` of audio, then GenerationComplete and TurnComplete.
    Complete(usize),
    /// TurnFailed(AwaitingResponse) before any audio.
    FailBeforeOutput,
    /// `packets` of audio, then TurnFailed(Responding) once the speaker has
    /// accepted at least one chunk of this request.
    FailDuringOutput(usize),
    /// `packets` of audio and then nothing: output stays pending.
    Hold(usize),
}

/// An observed point in the run at which the peer injects an action.
#[derive(Clone, Copy, Debug, PartialEq)]
enum When {
    /// The capture peer has sent this many frames.
    Block(u64),
    /// The provider peer has received End for this many requests.
    InputEnded(usize),
    /// The speaker has accepted this many chunks in total.
    Accepted(usize),
    /// The speaker has accepted this many chunks of the notice.
    NoticeAccepted(usize),
    /// The latest authority snapshot shows no owner, after this many
    /// requests have ended: the coordinator has settled them.
    Settled(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    /// Physical privacy reports muted from now on.
    Mute,
    /// Stepped-only fault: expire the accepted privacy lease, then queue a
    /// newly acquired mute before the coordinator resumes its next iteration.
    ExpirePrivacyThenMute,
    /// The capture child reports a worker fault.
    CaptureFault,
    /// The capture peer stops sending frames but stays connected.
    CaptureSilent,
    /// The capture peer sends nothing for half a second and then catches
    /// up, as a capture held up by the kernel re-tuning the SD/MMC host.
    CaptureStall,
    /// The capture worker is held up 400 ms, its authority lapses and its
    /// microphone closes: it says so, and opens it again under a new epoch,
    /// as the worker does after a stall (#784).
    CaptureReopen,
    /// The same with a 50 ms close, as an XRun the worker reopens at once.
    CaptureXRun,
    /// The provider peer closes its channels: the provider is gone.
    ProviderGone,
    /// Stop-only stall: the speaker consumes control messages but installs no
    /// authority and acknowledges nothing; it still honours Stop. This is no
    /// evidence of authority progress at the speaker.
    StallSpeaker,
    /// Control progress without settlement: the speaker keeps installing
    /// every authority snapshot but neither reads nor settles chunks.
    StallSpeakerInstalling,
    /// The speaker accepts half of its next chunk, then stalls as
    /// `StallSpeakerInstalling`.
    StallSpeakerPartial,
    /// Real SIGTERM, raised on the coordinator's thread, under an installed
    /// `Termination`.
    Sigterm,
    /// Speech starts on capture: a new user turn begins now.
    Speak,
    /// The provider child reports a worker fault on its control channel.
    ProviderFault,
    /// The provider link becomes terminal: the request in progress fails
    /// while responding, and the output stream ends.
    ProviderTerminal,
    /// The optional ring child reports a fault (an SPI write failure).
    RingFault,
    /// The optional ring child exits without writing black.
    RingGone,
    /// The optional camera stops reporting progress, as a stalled or
    /// unplugged sensor does, while its process keeps serving Stop.
    CameraSilent,
    /// The keeper pauses the lamp (`lamp/pause` says paused).
    Pause,
    /// The keeper resumes it.
    Resume,
}

/// How the speaker peer behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SpeakerMode {
    Healthy,
    StopOnly,
    Installing,
    Partial,
}

#[derive(Clone, Debug)]
struct Script {
    /// Capture frames scored as speech, by 1-based frame index. Later
    /// turns start with `Act::Speak`.
    speech: std::ops::Range<u64>,
    /// Answers in order of provider Start.
    answers: Vec<Answer>,
    actions: Vec<(When, Act)>,
    notices: bool,
    signals: bool,
    /// The pilot runs V1-baseline; other policies are cross-checks only.
    turn_taking: TurnTaking,
    /// The scenario expects verified cleanup to fail.
    cleanup_fails: bool,
    /// Run with the optional ring worker, as a pilot with the ring enabled.
    ring: bool,
    /// Run with the optional camera worker.
    camera: bool,
    /// The keeper has paused the lamp before the run begins.
    paused: bool,
    /// How the privacy worker's observations begin (lamp-4ace, 19 Oct
    /// 01:19: the first read stale as the voice started).
    privacy_start: PrivacyStart,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PrivacyStart {
    /// Fresh from the first.
    #[default]
    Fresh,
    /// The first 60 ms old when sent, as one that waited in the queue; then
    /// fresh every 5 ms.
    StaleFirst,
    /// The first 60 ms old, and then nothing.
    StaleThenSilent,
    /// The second fresh but acquired before the first: out of order.
    Backwards,
    /// Fresh, then the 40th (200 ms in, the microphone allowed) 60 ms old.
    StaleAfterAllowed,
}

impl Script {
    /// One user question, then the given answer, then a mute once the
    /// coordinator has settled the turn, on the scenario clock: synchronous
    /// steps and scoped time, with production freshness limits and the real
    /// process cleanup deadline. Cross-cycle privacy waits stay outside it.
    fn question(answer: Answer) -> Self {
        Self {
            speech: 20..80,
            answers: vec![answer],
            actions: vec![(When::Settled(1), Act::Mute)],
            notices: false,
            signals: false,
            turn_taking: TurnTaking::V1Baseline,
            cleanup_fails: false,
            ring: false,
            camera: false,
            paused: false,
            privacy_start: PrivacyStart::Fresh,
        }
    }
    fn then(mut self, when: When, act: Act) -> Self {
        self.actions.insert(0, (when, act));
        self
    }
}

/// One chunk as the speaker peer saw it.
#[derive(Clone, Debug)]
struct Chunk {
    /// The controller that issued the permit.
    boot: BootId,
    turn: u64,
    sequence: u64,
    frames: usize,
    end_of_speech: bool,
    /// The coordinator's authority view when it dispatched the chunk.
    snapshot_issued_us: u64,
    snapshot_revision: u64,
    /// When the speaker applied its boundary check, immediately before the
    /// write it allowed or refused.
    checked_us: u64,
    /// The authority the speaker's guard had installed at that check.
    guard: Option<Installed>,
    /// The production guard's verdict, as the real speaker would apply it.
    accepted: bool,
    notice: bool,
}

/// One authority state the speaker's guard installed.
#[derive(Clone, Copy, Debug)]
struct Installed {
    at_us: u64,
    revision: u64,
    owner: Option<u64>,
    microphone_allowed: bool,
}

/// Every item at most once, whatever the order: `[1, 2, 1]` is not distinct.
fn distinct(items: &[u64]) -> bool {
    items
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == items.len()
}

#[derive(Default)]
struct Seen {
    chunks: Vec<Chunk>,
    starts: Vec<u64>,
    ends: Vec<u64>,
    capture_frames: u64,
    /// Producer-side send attempts, not coordinator consumption timestamps.
    capture_attempt_gap_us: u64,
    capture_source_age_at_attempt_us: u64,
    last_capture_attempt_us: Option<u64>,
    /// When each action fired.
    fired: Vec<(Act, u64)>,
    /// Roles that observed priority Stop.
    stopped: Vec<&'static str>,
    /// Roles whose peer stopped answering by design.
    abandoned: Vec<&'static str>,
    /// Every authority state the speaker's guard installed, in order.
    installs: Vec<Installed>,
    /// When a Stop-only stall began: from then on the speaker installs no
    /// authority, so no revocation can be observed there.
    stop_only_from: Option<u64>,
    speaker_stopped_us: Option<u64>,
    ring_frames: u64,
    /// Longest interval between two privacy observations the peer sent, in
    /// microseconds: the peer's own share of any freshness loss.
    privacy_gap_us: u64,
    last_privacy_us: Option<u64>,
    /// Explicit fault's logical-clock bounds; normal steps remain 2 ms.
    privacy_expiry_jump: Option<(u64, u64)>,
    /// A successful send on the real privacy channel, after the expiry jump.
    fresh_mute_queued_at_us: Option<u64>,
    /// Actual host time immediately before sending SIGTERM, never policy time.
    sigterm_sent_at: Option<Instant>,
}

/// Packet, privacy and policy timestamps use the scenario clock.
/// `elapsed`, watchdogs and child exit observations always use real host time.
struct Run {
    result: Result<()>,
    trace: Vec<Value>,
    seen: Seen,
    elapsed: Duration,
    /// Whether the supervisor's child for each role had exited and been
    /// waited for once the run had finished.
    exits: Vec<(&'static str, bool)>,
    /// The scenario expects verified cleanup to fail.
    cleanup_fails: bool,
    /// The controller of this runtime cycle.
    boot: BootId,
    /// Before the first child was spawned, and after every child was reaped,
    /// on the host clock: real process events, compared across runs.
    started_us: u64,
    finished_us: u64,
    /// Actual host time after every real child exit/reap check.
    reaped_at: Instant,
}

impl Run {
    fn events(&self, kind: &str) -> Vec<&Value> {
        self.trace.iter().filter(|e| e["kind"] == kind).collect()
    }
    fn fired_at(&self, act: Act) -> u64 {
        self.seen
            .fired
            .iter()
            .find(|(fired, _)| *fired == act)
            .unwrap_or_else(|| panic!("{act:?} never fired"))
            .1
    }
    fn outcomes(&self) -> BTreeMap<u64, Vec<String>> {
        let mut outcomes: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        for event in self.events("turn_finished") {
            outcomes
                .entry(event["turn"].as_u64().unwrap())
                .or_default()
                .push(event["outcome"].as_str().unwrap().to_owned());
        }
        outcomes
    }
    fn accepted(&self, turn: u64) -> Vec<&Chunk> {
        self.seen
            .chunks
            .iter()
            .filter(|chunk| chunk.turn == turn && chunk.accepted)
            .collect()
    }
    /// When the speaker first installed authority without `turn` as owner
    /// (or with the microphone closed), after having installed it as owner.
    fn revoked_at(&self, turn: u64) -> Option<u64> {
        let mut owned = false;
        for install in &self.seen.installs {
            if install.owner == Some(turn) && install.microphone_allowed {
                owned = true;
            } else if owned {
                return Some(install.at_us);
            }
        }
        None
    }
    /// For each finished turn the speaker went on reading: how long after
    /// `turn_finished` its guard installed authority without that owner.
    fn revocations(&self) -> Vec<(u64, u64)> {
        self.events("turn_finished")
            .iter()
            .filter_map(|e| {
                let turn = e["turn"].as_u64().unwrap();
                let at = e["at_us"].as_u64().unwrap();
                self.revoked_at(turn)
                    .map(|revoked| (turn, revoked.saturating_sub(at)))
            })
            .collect()
    }
    /// Printed with every run, so a failure shows its own sequence.
    fn summary(&self) -> String {
        let mut kinds: Vec<String> = Vec::new();
        for event in &self.trace {
            let kind = event["kind"].as_str().unwrap_or("?");
            let detail = match kind {
                "turn_finished" => format!("({})", event["outcome"]),
                "local_endpoint" | "input_admitted" => format!("({})", event["turn"]),
                _ => String::new(),
            };
            let entry = format!("{kind}{detail}");
            if kinds.last() != Some(&entry) {
                kinds.push(entry);
            }
        }
        format!(
            "packet and policy clock logical, real conversation and cleanup {:?}\nresult {:?}\nkinds {kinds:?}\npeer privacy gap {} us, refused writes {}, end-to-speaker revocation {:?} us\ncapture send-attempt gap {} us, oldest source age at send attempt {} us (producer; consumption age unmeasured)\nframes {} starts {:?} ends {:?} chunks {} accepted {} fired {:?} stopped {:?} abandoned {:?}",
            self.elapsed,
            self.result.as_ref().map_err(|e| e.to_string()),
            self.seen.privacy_gap_us,
            self.seen.chunks.iter().filter(|c| !c.accepted).count(),
            self.revocations(),
            self.seen.capture_attempt_gap_us,
            self.seen.capture_source_age_at_attempt_us,
            self.seen.capture_frames,
            self.seen.starts,
            self.seen.ends,
            self.seen.chunks.len(),
            self.seen.chunks.iter().filter(|c| c.accepted).count(),
            self.seen.fired,
            self.seen.stopped,
            self.seen.abandoned,
        )
    }
    /// The invariants every scenario must keep, whatever its fault.
    fn assert_invariants(&self) {
        eprintln!("{}", self.summary());
        // No duplicate outcome for any turn, and no duplicate request.
        for (turn, outcomes) in self.outcomes() {
            assert_eq!(
                outcomes.len(),
                1,
                "turn {turn} finished twice: {outcomes:?}"
            );
        }
        assert!(distinct(&self.seen.starts), "a request started twice");
        let sequences: Vec<_> = self.seen.chunks.iter().map(|c| c.sequence).collect();
        assert!(distinct(&sequences), "a chunk was sent twice");
        // No stale dispatch: the coordinator never handed the speaker a chunk
        // under an authority view issued after its owner finished, privacy was
        // observed muted, or Stop was requested.
        let finished: BTreeMap<u64, u64> = self
            .events("turn_finished")
            .iter()
            .map(|e| (e["turn"].as_u64().unwrap(), e["at_us"].as_u64().unwrap()))
            .collect();
        let closed = self
            .events("physical_mute")
            .iter()
            .chain(self.events("service_stop_requested").iter())
            .map(|e| e["at_us"].as_u64().unwrap())
            .min();
        for chunk in &self.seen.chunks {
            if let Some(&at) = finished.get(&chunk.turn) {
                assert!(
                    chunk.snapshot_issued_us <= at,
                    "dispatch for finished turn {}: {chunk:?}",
                    chunk.turn
                );
            }
            if let Some(at) = closed {
                assert!(
                    chunk.snapshot_issued_us <= at,
                    "dispatch after mute or stop: {chunk:?}"
                );
            }
        }
        // No stale write: at every write the speaker allowed, its installed
        // authority named that owner with the microphone allowed, and the
        // speaker had not yet installed a state revoking that owner. Audio
        // already accepted before that point is counted, not hidden.
        for chunk in self.seen.chunks.iter().filter(|c| c.accepted) {
            let guard = chunk.guard.expect("an accepted write had authority");
            // The check used authority at least as new as the chunk's own.
            assert!(guard.revision >= chunk.snapshot_revision, "{chunk:?}");
            assert_eq!(guard.owner, Some(chunk.turn), "{chunk:?}");
            assert!(guard.microphone_allowed, "{chunk:?}");
            if let Some(revoked) = self.revoked_at(chunk.turn) {
                assert!(
                    chunk.checked_us < revoked,
                    "write after revocation: {chunk:?}"
                );
            }
        }
        // The policy bound, in the scenario clock: once the
        // coordinator ended a turn, an installing speaker must have authority
        // without that owner within one channel packet age (100 ms). It is
        // required, not merely checked when present. Exempt only a speaker
        // that installs nothing by design (a Stop-only stall or an abandoned
        // peer) or had already stopped.
        for (&turn, &at) in &finished {
            let installing = !self.seen.abandoned.contains(&"speaker")
                && self.seen.stop_only_from.is_none_or(|from| from > at)
                && self
                    .seen
                    .speaker_stopped_us
                    .is_none_or(|stopped| stopped > at);
            match self.revoked_at(turn) {
                Some(revoked) => assert!(
                    revoked.saturating_sub(at) <= 100_000,
                    "turn {turn} revoked at the speaker {} us after it ended",
                    revoked.saturating_sub(at)
                ),
                None => assert!(
                    !installing,
                    "turn {turn} ended but the installing speaker never saw it revoked"
                ),
            }
        }
        // No false readiness: once a session-ending fault, mute or stop was
        // recorded, nothing claims to listen again in this run.
        let ended = self
            .trace
            .iter()
            .filter(|e| {
                matches!(
                    e["kind"].as_str(),
                    Some("physical_mute" | "service_stop_requested" | "provider_output_ended")
                )
            })
            .map(|e| e["at_us"].as_u64().unwrap())
            .min();
        if let Some(at) = ended {
            for ready in self.events("listening_ready") {
                assert!(
                    ready["at_us"].as_u64().unwrap() <= at,
                    "readiness after the run ended: {ready}"
                );
            }
        }
        // No orphan. Every role saw Stop unless its peer was abandoned by
        // design; only an abandoned role may be force-killed; verified cleanup
        // succeeded unless the scenario says otherwise; and every role's child
        // had exited and been waited for when the run returned.
        for event in self.events("worker_forced_shutdown") {
            let role = event["worker"].as_str().unwrap();
            assert!(
                self.seen.abandoned.contains(&role),
                "{role} was force-killed: {event}"
            );
        }
        let roles = self.exits.iter().map(|(role, _)| *role);
        for role in roles {
            assert!(
                self.seen.stopped.contains(&role) || self.seen.abandoned.contains(&role),
                "{role} never saw Stop"
            );
        }
        let cleanup_failed = self
            .result
            .as_ref()
            .err()
            .is_some_and(|e| e.to_string().contains("verified cleanup failed"));
        assert_eq!(cleanup_failed, self.cleanup_fails, "{:?}", self.result);
        for (role, exited) in &self.exits {
            assert!(exited, "{role} child was not reaped");
        }
        assert!(self.elapsed < SCENARIO_LIMIT, "run took {:?}", self.elapsed);
    }
}

struct Peer {
    role: &'static str,
    channels: WorkerChannels,
    input: Option<std::process::ChildStdin>,
    child_pid: u32,
    snapshot: Option<Snapshot>,
    stopped: bool,
}

impl Peer {
    /// Observe the real child exit before the scenario clock advances again.
    /// WNOWAIT leaves the coordinator responsible for the actual reap. A
    /// shutdown step shares its one real deadline across every peer.
    fn close_input(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        let input = self.input.take();
        let closed = input.is_some();
        drop(input);
        if closed && let Some(deadline) = deadline {
            // Bounded by the scope's own watchdog, not the cleanup's real
            // deadline, which a busy host can use up first (#1235's review).
            let bound = crate::scenario_clock::host_deadline().unwrap_or(deadline);
            crate::scenario_clock::wait_child_exited(self.child_pid, bound)?;
        }
        Ok(())
    }
}

/// A continuous synthetic source starts when StartCapture is received. A late
/// delivery keeps each block's original acquisition time; catching up cannot
/// create a source gap or make old audio fresh again.
struct CaptureSchedule {
    origin_us: u64,
    next_sequence: u64,
}

impl CaptureSchedule {
    fn new(origin_us: u64) -> Self {
        Self {
            origin_us,
            next_sequence: 1,
        }
    }

    /// Nothing is read before `at_us`: the next frame is the first due then.
    fn skip_to(&mut self, at_us: u64) {
        let skipped = at_us.saturating_sub(self.origin_us).div_ceil(10_000);
        self.next_sequence = self.next_sequence.max(skipped + 1);
    }

    fn next_due(&mut self, now_us: u64) -> Option<(u64, u64)> {
        let sequence = self.next_sequence;
        let acquired_at_us = self.origin_us + 10_000 * (sequence - 1);
        if acquired_at_us > now_us {
            return None;
        }
        self.next_sequence += 1;
        Some((sequence, acquired_at_us))
    }
}

/// State owned either by one real-time thread or by the synchronous pump.
struct Peers {
    script: Script,
    peers: Vec<Peer>,
    seen: Arc<Mutex<Seen>>,
    guard: BoundaryGuard,
    output: OutputSender,
    queued: VecDeque<ProviderOutput>,
    /// Requests by Start order, with the samples their answer still owes.
    requests: Vec<u64>,
    pending_failure: Option<u64>,
    /// Shared with the privacy thread, which reports it.
    muted: Arc<AtomicBool>,
    /// What the coordinator reads as the keeper's pause.
    paused: Arc<AtomicBool>,
    fresh_mute_pending: bool,
    capture: Option<CaptureSchedule>,
    capture_silent: bool,
    capture_stalled_until: Option<Instant>,
    capture_epoch: u64,
    capture_reopen: bool,
    camera: Option<BootId>,
    camera_started: bool,
    camera_silent: bool,
    next_camera: Instant,
    speaker: SpeakerMode,
    provider_gone: bool,
    speech_offset: Option<u64>,
    /// The speaker models the PCM sample cadence; this is not an ALSA clock.
    next_write: Instant,
    cursor: u64,
    audio_sequence: u64,
    accepted: usize,
    notice_accepted: usize,
    ended: usize,
    settled_after: Option<usize>,
}

impl Peers {
    fn send_capture_started(&mut self, index: usize) {
        let at = monotonic_us();
        let generation = self.peers[index].snapshot.unwrap().microphone_generation();
        self.peers[index]
            .channels
            .control
            .send(WorkerEvent::CaptureStarted {
                epoch: self.capture_epoch,
                dsp_epoch: 1,
                noise_suppression: NoiseSuppression::On,
                privacy_generation: generation,
                rate: 16_000,
                channels: 1,
                period_frames: 160,
                buffer_frames: 640,
                monotonic_status_timestamps: false,
                open_started_at_us: at,
                prepared_at_us: at,
                capture_started_at_us: at,
                start_completed_at_us: at,
            })
            .unwrap();
    }
    fn fire(&mut self, act: Act, deadline: Option<Instant>) -> io::Result<()> {
        self.seen.lock().unwrap().fired.push((act, monotonic_us()));
        match act {
            Act::Mute => self.muted.store(true, Ordering::SeqCst),
            Act::ExpirePrivacyThenMute => {
                self.fresh_mute_pending = true;
            }
            Act::CaptureFault => {
                if let Some(capture) = self.peers.iter_mut().find(|p| p.role == "capture") {
                    let _ = capture.channels.control.send(WorkerEvent::Fault {
                        code: "injected capture fault".into(),
                    });
                }
            }
            Act::CaptureSilent => self.capture_silent = true,
            Act::CaptureStall => {
                // The device keeps its own 10 ms schedule (CaptureSchedule);
                // what it read while the worker was held up comes after.
                self.capture_stalled_until =
                    Some(crate::scenario_clock::instant() + Duration::from_millis(500));
            }
            Act::CaptureReopen | Act::CaptureXRun => {
                let ms = if act == Act::CaptureXRun { 50 } else { 400 };
                self.capture_stalled_until =
                    Some(crate::scenario_clock::instant() + Duration::from_millis(ms));
                // A closed microphone reads nothing: frames start again at
                // the reopen, not caught up.
                if let Some(schedule) = self.capture.as_mut() {
                    schedule.skip_to(monotonic_us() + ms * 1_000);
                }
                self.capture_reopen = true;
                let epoch = self.capture_epoch;
                if let Some(capture) = self.peers.iter_mut().find(|p| p.role == "capture") {
                    capture
                        .channels
                        .control
                        .send(WorkerEvent::CaptureGap {
                            epoch,
                            reason: "AuthorityExpired".to_owned(),
                            closed_at_us: monotonic_us(),
                        })
                        .unwrap();
                }
            }
            Act::ProviderGone => {
                self.provider_gone = true;
                if let Some(provider) = self.peers.iter_mut().find(|p| p.role == "provider") {
                    // Closing stdin ends the child; the sockets close with it.
                    provider.close_input(deadline)?;
                    provider.stopped = true;
                }
                self.seen.lock().unwrap().abandoned.push("provider");
            }
            Act::StallSpeaker => {
                self.speaker = SpeakerMode::StopOnly;
                self.seen.lock().unwrap().stop_only_from = Some(monotonic_us());
            }
            Act::StallSpeakerInstalling => self.speaker = SpeakerMode::Installing,
            Act::StallSpeakerPartial => self.speaker = SpeakerMode::Partial,
            Act::Sigterm => {
                self.seen.lock().unwrap().sigterm_sent_at = Some(Instant::now());
                // A real SIGTERM through the installed handler, raised on this
                // thread: its handler has run before raise returns. A signal
                // to the whole process may be taken by another thread later,
                // and a stepped run kept its scenario clock moving meanwhile
                // (0.27 to 1.7 s on a loaded host), long enough for the notice
                // to finish before the stop was seen.
                signal_hook::low_level::raise(signal_hook::consts::SIGTERM).unwrap();
            }
            Act::ProviderFault => {
                if let Some(provider) = self.peers.iter_mut().find(|p| p.role == "provider") {
                    let _ = provider.channels.control.send(WorkerEvent::Fault {
                        code: "injected provider worker fault".into(),
                    });
                }
            }
            Act::ProviderTerminal => {
                let request = self.requests.last().copied().unwrap_or(1);
                // Unsent answer audio is lost with the connection.
                self.queued
                    .retain(|output| !matches!(output, ProviderOutput::Audio { .. }));
                self.queued.push_back(ProviderOutput::TurnFailed {
                    request,
                    stage: FailedStage::Responding,
                    reason: Fault::Transport,
                });
                self.queued.push_back(ended_output());
                if let Some(provider) = self.peers.iter_mut().find(|p| p.role == "provider") {
                    let mut link = ready_link(2, 1);
                    link.state = crate::provider_link::ProviderLinkState::Terminal {
                        reason: crate::provider_link::LinkFailure::NoResumableContext,
                        fault: Fault::Transport,
                        attempts: 1,
                    };
                    let _ = provider
                        .channels
                        .control
                        .send(WorkerEvent::ProviderLink { link });
                }
            }
            Act::RingFault => {
                if let Some(ring) = self.peers.iter_mut().find(|p| p.role == "ring") {
                    let _ = ring.channels.control.send(WorkerEvent::Fault {
                        code: "injected ring SPI failure".into(),
                    });
                }
            }
            Act::RingGone => {
                if let Some(ring) = self.peers.iter_mut().find(|p| p.role == "ring") {
                    ring.close_input(deadline)?;
                    ring.stopped = true;
                }
                self.seen.lock().unwrap().abandoned.push("ring");
            }
            Act::CameraSilent => self.camera_silent = true,
            Act::Pause => self.paused.store(true, Ordering::SeqCst),
            Act::Resume => self.paused.store(false, Ordering::SeqCst),
            Act::Speak => {
                let frames = self.seen.lock().unwrap().capture_frames;
                self.speech_offset = Some(frames);
            }
        }
        Ok(())
    }

    fn due(&self, when: When) -> bool {
        match when {
            When::Block(block) => self.seen.lock().unwrap().capture_frames >= block,
            When::InputEnded(count) => self.ended >= count,
            When::Accepted(count) => self.accepted >= count,
            When::NoticeAccepted(count) => self.notice_accepted >= count,
            When::Settled(count) => self.settled_after.is_some_and(|after| after >= count),
        }
    }

    fn speech(&self, frame: u64) -> bool {
        let scripted = self.script.speech.contains(&frame);
        let spoken = self
            .speech_offset
            .is_some_and(|offset| (offset..offset + 60).contains(&frame));
        scripted || spoken
    }

    fn tick(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        let actions = std::mem::take(&mut self.script.actions);
        let (due, waiting): (Vec<_>, Vec<_>) =
            actions.into_iter().partition(|&(when, _)| self.due(when));
        self.script.actions = waiting;
        for (_, act) in due {
            self.fire(act, deadline)?;
        }
        // The expiry fault belongs between coordinated steps. Do not create
        // extra capture or speaker progress after the deliberate time jump.
        if self.fresh_mute_pending {
            return Ok(());
        }
        for index in 0..self.peers.len() {
            self.tick_peer(index, deadline)?;
        }
        Ok(())
    }

    fn tick_peer(&mut self, index: usize, deadline: Option<Instant>) -> io::Result<()> {
        if self.peers[index].stopped {
            return Ok(());
        }
        let role = self.peers[index].role;
        if role == "speaker" && self.speaker == SpeakerMode::StopOnly {
            // Consumes control, installs nothing, still sees Stop. What is
            // queued is read in one step, as the other peers read theirs, so
            // the cleanup's revocation cannot hold its Stop back a step past
            // the cleanup's real deadline on a busy host (#1235's review).
            for _ in 0..16 {
                match self.peers[index].channels.control.receive::<Control>() {
                    Ok(Some(Control::Stop)) => {
                        self.stop(index, deadline)?;
                        break;
                    }
                    Ok(Some(_)) => {}
                    _ => break,
                }
            }
            return Ok(());
        }
        for _ in 0..16 {
            match self.peers[index].channels.control.receive::<Control>() {
                Ok(Some(Control::Authority { snapshot })) => {
                    self.peers[index].snapshot = Some(snapshot);
                    if role == "speaker" {
                        self.install(snapshot);
                    }
                    if snapshot.owner().is_none() && self.ended > 0 {
                        self.settled_after = Some(self.ended);
                    }
                }
                Ok(Some(Control::ProviderOutputCapacity { through })) => {
                    self.output.grant(through).unwrap()
                }
                Ok(Some(Control::StartCapture)) if role == "camera" => self.start_camera(index),
                Ok(Some(Control::StartCapture)) if role == "capture" && self.capture.is_none() => {
                    self.capture = Some(CaptureSchedule::new(monotonic_us()));
                    self.send_capture_started(index);
                }
                Ok(Some(Control::Stop)) => {
                    if role == "camera" {
                        self.stop_camera(index);
                    }
                    if role == "ring" {
                        // A ring that can still write blanks before it exits.
                        let at = monotonic_us();
                        let _ = self.peers[index].channels.control.send(WorkerEvent::Ring {
                            report: RingFeedback::Blanked {
                                reason: RingBlankReason::Shutdown,
                                write_started_at_us: at,
                                write_finished_at_us: at,
                            },
                        });
                    }
                    self.stop(index, deadline)?;
                    return Ok(());
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
        match role {
            "capture"
                if self.capture.is_some()
                    && !self.capture_silent
                    && self
                        .capture_stalled_until
                        .is_none_or(|until| crate::scenario_clock::instant() >= until) =>
            {
                if std::mem::take(&mut self.capture_reopen) {
                    self.capture_epoch += 1;
                    self.send_capture_started(index);
                }
                // Frames follow a fixed 10 ms schedule, like a capture device.
                for _ in 0..64 {
                    let attempted_at_us = monotonic_us();
                    let Some((frame, at)) =
                        self.capture.as_mut().unwrap().next_due(attempted_at_us)
                    else {
                        break;
                    };
                    {
                        let mut seen = self.seen.lock().unwrap();
                        seen.capture_frames = frame;
                        if let Some(previous) =
                            seen.last_capture_attempt_us.replace(attempted_at_us)
                        {
                            seen.capture_attempt_gap_us = seen
                                .capture_attempt_gap_us
                                .max(attempted_at_us.saturating_sub(previous));
                        }
                        seen.capture_source_age_at_attempt_us = seen
                            .capture_source_age_at_attempt_us
                            .max(attempted_at_us - at);
                    }
                    let speech = self.speech(frame);
                    let peer = &mut self.peers[index];
                    let sent = peer.channels.data.send(MicrophoneFrame {
                        raw: None,
                        linear: None,
                        epoch: self.capture_epoch,
                        dsp_epoch: 1,
                        privacy_generation: peer.snapshot.unwrap().microphone_generation(),
                        frame_sequence: frame,
                        read_completed_at_us: at,
                        probability: if speech { 0.99 } else { 0.0 },
                        echo: None,
                        reference: Default::default(),
                        samples: vec![if speech { 2_000 } else { 7 }; 160],
                        timing: CaptureTiming {
                            first_read_started_at_us: at,
                            last_read_started_at_us: at,
                            successful_reads: 1,
                            alsa_status_monotonic_us: None,
                            status_observed_at_us: at,
                            available_frames: 0,
                            delayed_frames: 0,
                            processing_completed_at_us: at,
                            aec_queue_delay_ms: 0,
                            reference: ReferenceTiming {
                                playback_epoch: 1,
                                clock_started_at_us: Some(at),
                                ..ReferenceTiming::default()
                            },
                        },
                    });
                    if sent.is_err() {
                        // A full socket (the catch-up after a stall can fill
                        // it) loses no block: the device still holds it, and
                        // it goes next step with its own acquisition time, so
                        // the sequence the coordinator sees has no hole.
                        self.capture.as_mut().unwrap().next_sequence = frame;
                        self.seen.lock().unwrap().capture_frames = frame - 1;
                        break;
                    }
                }
            }
            "provider" if !self.provider_gone => self.tick_provider(index),
            "speaker" if self.speaker != SpeakerMode::Installing => self.tick_speaker(index),
            "camera" if self.camera_started && !self.camera_silent => {
                if crate::scenario_clock::instant() >= self.next_camera {
                    let _ = self.peers[index].channels.data.send(CameraEvent::Progress {
                        at_us: monotonic_us(),
                        counts: Counts::default(),
                        last_read: None,
                        startup_discards: None,
                    });
                    self.next_camera = crate::scenario_clock::instant() + Duration::from_millis(50);
                }
            }
            "ring" => {
                for _ in 0..64 {
                    let Ok(Some(frame)) = self.peers[index].channels.data.receive::<RingFrame>()
                    else {
                        break;
                    };
                    self.seen.lock().unwrap().ring_frames += 1;
                    let at = monotonic_us();
                    let _ = self.peers[index].channels.control.send(WorkerEvent::Ring {
                        report: RingFeedback::Presented {
                            owner: frame.permit.owner(),
                            phase: frame.phase,
                            requested_at_us: frame.requested_at_us,
                            write_started_at_us: at,
                            write_finished_at_us: at,
                        },
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn tick_provider(&mut self, index: usize) {
        for _ in 0..64 {
            match self.peers[index].channels.data.receive::<ProviderInput>() {
                Ok(Some(ProviderInput::Start {
                    request, session, ..
                })) => {
                    self.seen.lock().unwrap().starts.push(request);
                    self.requests.push(request);
                    self.queued.push_back(ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    });
                }
                Ok(Some(ProviderInput::End { request, .. })) => {
                    self.seen.lock().unwrap().ends.push(request);
                    self.ended += 1;
                    let position = self.requests.iter().position(|&r| r == request);
                    let answer = position.and_then(|p| self.script.answers.get(p)).copied();
                    match answer {
                        Some(Answer::Complete(packets)) => {
                            self.audio(request, packets);
                            self.queued
                                .push_back(ProviderOutput::GenerationComplete { request });
                            self.queued.push_back(ProviderOutput::TurnComplete {
                                request,
                                idle: true,
                            });
                        }
                        Some(Answer::FailBeforeOutput) => {
                            self.queued.push_back(ProviderOutput::TurnFailed {
                                request,
                                stage: FailedStage::AwaitingResponse,
                                reason: Fault::Transport,
                            })
                        }
                        Some(Answer::FailDuringOutput(packets)) => {
                            self.audio(request, packets);
                            self.pending_failure = Some(request);
                        }
                        Some(Answer::Hold(packets)) => self.audio(request, packets),
                        None => {}
                    }
                }
                Ok(Some(
                    ProviderInput::Audio { .. }
                    | ProviderInput::Written { .. }
                    | ProviderInput::Called { .. }
                    | ProviderInput::Paused { .. }
                    | ProviderInput::CountOnly { .. }
                    | ProviderInput::StepCall { .. }
                    | ProviderInput::Onboarding { .. },
                )) => {}
                Ok(Some(
                    ProviderInput::FaceQuestion { .. } | ProviderInput::FaceConsentDelivery { .. },
                )) => {
                    panic!("face consent is disabled in this fixture")
                }
                Ok(Some(ProviderInput::Reminder { .. })) => {
                    panic!("reminders are disabled in this fixture")
                }
                Ok(None) | Err(_) => break,
            }
        }
        if let Some(request) = self.pending_failure
            && self.accepted > 0
        {
            self.pending_failure = None;
            self.queued.push_back(ProviderOutput::TurnFailed {
                request,
                stage: FailedStage::Responding,
                reason: Fault::Transport,
            });
        }
        let peer = &mut self.peers[index];
        for _ in 0..64 {
            let Some(event) = self.queued.front() else {
                break;
            };
            match self.output.try_send(|| peer.channels.data.send(event)) {
                Ok(true) => {
                    self.queued.pop_front();
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    fn audio(&mut self, request: u64, packets: usize) {
        for _ in 0..packets {
            self.audio_sequence += 1;
            self.queued.push_back(ProviderOutput::Audio {
                request,
                sequence: self.audio_sequence,
                provider_event_at_us: monotonic_us(),
                source_frames: PACKET,
                source_offset: 0,
                samples: vec![1_000; PACKET],
            });
        }
    }

    fn tick_speaker(&mut self, index: usize) {
        if crate::scenario_clock::instant() < self.next_write {
            return;
        }
        let Ok(Some(chunk)) = self.peers[index].channels.data.receive::<SpeakerChunk>() else {
            return;
        };
        // 24 kHz output: 24 samples per millisecond.
        self.next_write = self
            .next_write
            .max(crate::scenario_clock::instant() - Duration::from_millis(20))
            + Duration::from_micros(chunk.samples.len() as u64 * 1_000 / 24);
        // As the real speaker: install the chunk's own view (an older one is
        // ignored), then check the permit immediately before the write. A
        // partial stall writes half of this chunk and settles nothing more.
        self.install(chunk.snapshot);
        let checked_us = monotonic_us();
        let accepted = self
            .guard
            .check(
                MonoTime::from_micros(checked_us),
                chunk.permit,
                OutputKind::Speech,
            )
            .is_ok();
        let guard = self.guard.state().map(|state| Installed {
            at_us: checked_us,
            revision: state.revision(),
            owner: state.owner().map(|owner| owner.turn()),
            microphone_allowed: state.microphone_permission() == Permission::Allowed,
        });
        let owner = chunk.permit.owner();
        // The synthetic notice is constant 321; provider answers are 1,000.
        let notice = chunk.samples.contains(&321);
        self.seen.lock().unwrap().chunks.push(Chunk {
            boot: owner.boot(),
            turn: owner.turn(),
            sequence: chunk.chunk_sequence,
            frames: chunk.samples.len(),
            end_of_speech: chunk.end_of_speech,
            snapshot_issued_us: chunk.snapshot.issued_at().as_micros(),
            snapshot_revision: chunk.snapshot.revision(),
            checked_us,
            guard,
            accepted,
            notice,
        });
        let channels = &mut self.peers[index].channels;
        if !accepted {
            let _ = channels.control.send(WorkerEvent::PlaybackRejected {
                turn: owner.turn(),
                sequence: chunk.chunk_sequence,
                reason: "fixture guard refused the permit".into(),
                lapsed: false,
            });
            return;
        }
        self.accepted += 1;
        if notice {
            self.notice_accepted += 1;
        }
        let frames = if self.speaker == SpeakerMode::Partial {
            self.speaker = SpeakerMode::Installing;
            chunk.samples.len() / 2
        } else {
            chunk.samples.len()
        };
        let first = PlaybackCursor {
            epoch: 1,
            frame: self.cursor,
        };
        self.cursor += frames as u64;
        let end = PlaybackCursor {
            epoch: 1,
            frame: self.cursor,
        };
        let at = monotonic_us();
        let _ = channels.control.send(WorkerEvent::Playback {
            owner,
            first_sample: first,
            end_sample: end,
            queue_observed_at_us: at,
            turn: owner.turn(),
            sequence: chunk.chunk_sequence,
            queued_frames: 0,
            accepted_frames: frames,
            observed_at_us: at,
        });
        if frames < chunk.samples.len() {
            // Part of the chunk was written; the rest is never settled.
            return;
        }
        let _ = channels.control.send(WorkerEvent::SpeechDrained {
            owner,
            sequence: chunk.chunk_sequence,
            last_sample: end,
            observed_at_us: at,
        });
        if chunk.end_of_speech {
            let _ = channels.control.send(WorkerEvent::SpeechRetired {
                owner,
                sequence: chunk.chunk_sequence,
                final_sample: end,
                observed_at_us: at,
            });
        }
    }

    /// Opening, then Started in the configured mode, as a sensor that opens.
    fn start_camera(&mut self, index: usize) {
        let worker = self.camera.expect("camera boot");
        let opened = monotonic_us();
        let data = &mut self.peers[index].channels.data;
        let _ = data.send(CameraEvent::Opening {
            at_us: opened,
            deadline_us: opened + lamp_camera::START_BUDGET_US,
        });
        let at = monotonic_us();
        let _ = data.send(CameraEvent::Started {
            at_us: at,
            worker,
            capture_epoch: 1,
            mode: Mode {
                source: "desk".into(),
                format: *b"MJPG",
                width: 320,
                height: 240,
                interval: None,
                size_image: 64 * 1024,
                buffers: 4,
            },
            timing: CameraTiming {
                started_at_us: opened,
                completed_at_us: at,
            },
            port: PortMetadata::default(),
        });
        self.camera_started = true;
    }

    /// Local revocation, then driver teardown, within their budgets.
    fn stop_camera(&mut self, index: usize) {
        let worker = self.camera.expect("camera boot");
        let revoked = monotonic_us();
        let data = &mut self.peers[index].channels.data;
        let _ = data.send(CameraEvent::Revoked {
            at_us: revoked,
            worker,
            reason: StopReason::Requested,
            teardown_deadline_us: revoked + lamp_camera::DRIVER_TEARDOWN_BUDGET_US,
            error: None,
            timestamp_rejection: None,
            startup_discards: None,
        });
        let at = monotonic_us();
        let _ = data.send(CameraEvent::Stopped {
            at_us: at,
            reason: StopReason::Requested,
            counts: Counts::default(),
            stop: Some(CameraTiming {
                started_at_us: revoked,
                completed_at_us: at,
            }),
            port: PortMetadata::default(),
            error: None,
        });
    }

    /// Installs authority in the speaker's guard and records what changed.
    /// An older or duplicate state is refused by the guard and not recorded.
    fn install(&mut self, snapshot: Snapshot) {
        let at_us = monotonic_us();
        if self
            .guard
            .install(MonoTime::from_micros(at_us), snapshot)
            .is_ok()
        {
            self.seen.lock().unwrap().installs.push(Installed {
                at_us,
                revision: snapshot.revision(),
                owner: snapshot.owner().map(|owner| owner.turn()),
                microphone_allowed: snapshot.microphone_permission() == Permission::Allowed,
            });
        }
    }

    fn stop(&mut self, index: usize, deadline: Option<Instant>) -> io::Result<()> {
        let peer = &mut self.peers[index];
        peer.stopped = true;
        peer.close_input(deadline)?;
        let mut seen = self.seen.lock().unwrap();
        seen.stopped.push(peer.role);
        if peer.role == "speaker" {
            seen.speaker_stopped_us = Some(monotonic_us());
        }
        Ok(())
    }
}

/// Fresh privacy observations every 5 ms, muted once a scenario says so.
/// The same bounded step serves the real thread and the scoped-clock pump.
struct PrivacyPeer {
    peer: Peer,
    seen: Arc<Mutex<Seen>>,
    muted: Arc<AtomicBool>,
    next: Instant,
    start: PrivacyStart,
    sent: u64,
    first_at: u64,
}

impl PrivacyPeer {
    fn tick(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        for _ in 0..16 {
            if self.peer.stopped {
                break;
            }
            let Ok(Some(control)) = self.peer.channels.control.receive::<Control>() else {
                break;
            };
            if matches!(control, Control::Stop) {
                self.peer.stopped = true;
                self.peer.close_input(deadline)?;
                self.seen.lock().unwrap().stopped.push("privacy");
            }
        }
        let silent = self.start == PrivacyStart::StaleThenSilent && self.sent > 0;
        if !self.peer.stopped && !silent && crate::scenario_clock::instant() >= self.next {
            let at = monotonic_us();
            let stale = matches!(
                self.start,
                PrivacyStart::StaleFirst | PrivacyStart::StaleThenSilent
            ) && self.sent == 0
                || self.start == PrivacyStart::StaleAfterAllowed && self.sent == 39;
            let backwards = self.start == PrivacyStart::Backwards && self.sent == 1;
            if self.sent == 0 {
                self.first_at = at;
            }
            self.sent += 1;
            {
                let mut seen = self.seen.lock().unwrap();
                if let Some(last) = seen.last_privacy_us {
                    seen.privacy_gap_us = seen.privacy_gap_us.max(at - last);
                }
                seen.last_privacy_us = Some(at);
            }
            let _ = self.peer.channels.control.send(WorkerEvent::Privacy {
                receipt: None,
                acquired_at_us: if stale {
                    at - 60_000
                } else if backwards {
                    self.first_at - 1_000
                } else {
                    at
                },
                muted: self.muted.load(Ordering::SeqCst),
            });
            self.next = crate::scenario_clock::instant() + Duration::from_millis(5);
        }
        Ok(())
    }

    fn queue_fresh_mute(&mut self) -> io::Result<()> {
        if self.peer.stopped {
            return Err(io::Error::other("privacy peer stopped before expiry fault"));
        }
        self.muted.store(true, Ordering::SeqCst);
        let at = monotonic_us();
        self.peer.channels.control.send(WorkerEvent::Privacy {
            receipt: None,
            acquired_at_us: at,
            muted: true,
        })?;
        let mut seen = self.seen.lock().unwrap();
        if let Some(last) = seen.last_privacy_us {
            seen.privacy_gap_us = seen.privacy_gap_us.max(at - last);
        }
        seen.last_privacy_us = Some(at);
        seen.fresh_mute_queued_at_us = Some(at);
        self.next = crate::scenario_clock::instant() + Duration::from_millis(5);
        Ok(())
    }
}

fn run(script: Script) -> Run {
    run_in(script, &mut SessionState::default())
}

/// One runtime cycle in `trace`, which a service supervisor may share across
/// cycles. The returned run holds only this cycle's events.
fn run_in(script: Script, trace: &mut SessionState) -> Run {
    let first_event = trace.iter().count();
    // Spawn and reap are real process events: on the host clock even in a
    // stepped run, whose logical time is its own and not comparable across
    // runs (scenario_clock.rs).
    let started_us = lamp_ipc::monotonic_us();
    let cleanup_fails = script.cleanup_fails;
    let notices = if script.notices {
        FailureNotices::synthetic()
    } else {
        FailureNotices::none()
    };
    let termination = if script.signals {
        Termination::install().unwrap()
    } else {
        Termination::inactive()
    };
    let directory = SessionDirectory::create().unwrap();
    let mut peers = Vec::new();
    let mut spawn = |role| {
        let (worker, channels, input) = Worker::cooperative_ready_fixture(&directory.path, role);
        peers.push(Peer {
            role,
            channels,
            input: Some(input),
            child_pid: worker.fixture_pid(),
            snapshot: None,
            stopped: false,
        });
        worker
    };
    let ring = script.ring.then(|| spawn("ring"));
    let ring_channel_ceiling = script.ring.then_some(24);
    let camera_worker = script.camera.then(|| spawn("camera"));
    let camera_boot = camera_worker.as_ref().map(|worker| worker.boot);
    let camera = camera_worker.map(|worker| {
        let source = VisualSource {
            socket: directory.path.join("camera.frames"),
            camera: worker.boot,
            source: "desk".into(),
            width: 320,
            height: 240,
            max_frame_bytes: 128 * 1024,
        };
        OptionalCamera::new(worker, source)
    });
    let paused = Arc::new(AtomicBool::new(script.paused));
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        speaker: spawn("speaker"),
        capture: spawn("capture"),
        provider: spawn("provider"),
        privacy: spawn("privacy"),
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
        ring: ring.into(),
        camera,
        motor: None,
        ring_channel_ceiling,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: script.turn_taking,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new({
            let paused = paused.clone();
            move || paused.load(Ordering::SeqCst)
        }),
    };
    // The clock starts only after every real child has crossed its startup
    // barrier. All coordinator and peer timestamps below share this scope;
    // the real watchdog still bounds both the conversation and its cleanup.
    let clock_scope = crate::scenario_clock::Scope::enter(SCENARIO_LIMIT).unwrap();
    let mut owner = Controller::new(new_boot().unwrap(), now());
    let boot = owner.snapshot(now()).unwrap().boot();
    let mut startup = StartupEvents::default();
    startup
        .cloud
        .observe(ready_link(1, 1), monotonic_us())
        .unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let muted = Arc::new(AtomicBool::new(false));
    let privacy_at = peers.iter().position(|p| p.role == "privacy").unwrap();
    let privacy = PrivacyPeer {
        peer: peers.remove(privacy_at),
        seen: seen.clone(),
        muted: muted.clone(),
        next: crate::scenario_clock::instant(),
        start: script.privacy_start,
        sent: 0,
        first_at: 0,
    };
    let state = Peers {
        script,
        peers,
        seen: seen.clone(),
        guard: BoundaryGuard::new(boot, now()),
        output: OutputSender::default(),
        queued: VecDeque::new(),
        requests: Vec::new(),
        pending_failure: None,
        muted,
        paused,
        fresh_mute_pending: false,
        capture: None,
        capture_silent: false,
        capture_stalled_until: None,
        capture_epoch: 1,
        capture_reopen: false,
        camera: camera_boot,
        camera_started: false,
        camera_silent: false,
        next_camera: crate::scenario_clock::instant(),
        speaker: SpeakerMode::Healthy,
        provider_gone: false,
        speech_offset: None,
        next_write: crate::scenario_clock::instant(),
        cursor: 0,
        audio_sequence: 0,
        accepted: 0,
        notice_accepted: 0,
        ended: 0,
        settled_after: None,
    };
    {
        let mut privacy = privacy;
        let mut state = state;
        clock_scope
            .set_pump(move |phase| {
                // Real child exit cannot be simulated. One deadline bounds
                // all children this step stops, and shutdown supplies its
                // existing shared budget instead of granting a new one.
                let deadline = match phase {
                    crate::scenario_clock::Phase::Run => {
                        Instant::now() + Duration::from_millis(300)
                    }
                    crate::scenario_clock::Phase::Shutdown { deadline } => deadline,
                };
                privacy.tick(Some(deadline))?;
                state.tick(Some(deadline))?;
                if std::mem::take(&mut state.fresh_mute_pending) {
                    let before = monotonic_us();
                    crate::scenario_clock::elapse_for_fault(Duration::from_millis(50))?;
                    let after = monotonic_us();
                    privacy.queue_fresh_mute()?;
                    state.seen.lock().unwrap().privacy_expiry_jump = Some((before, after));
                }
                Ok(())
            })
            .unwrap();
    }
    let mut reply = None;
    let started = Instant::now();
    let result = conversation(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        trace,
        startup,
        &mut reply,
        &mut None,
        &termination,
        &notices,
    );
    let result = finish_active_run(&mut rig, &mut owner, &mut reply, trace, &mut None, result);
    let elapsed = started.elapsed();
    let mut exits = vec![
        ("speaker", matches!(rig.speaker.try_exit(), Ok(Some(_)))),
        ("capture", matches!(rig.capture.try_exit(), Ok(Some(_)))),
        ("provider", matches!(rig.provider.try_exit(), Ok(Some(_)))),
        ("privacy", matches!(rig.privacy.try_exit(), Ok(Some(_)))),
    ];
    for ring in rig.ring.iter_mut() {
        exits.push(("ring", matches!(ring.try_exit(), Ok(Some(_)))));
    }
    if let Some(camera) = rig.camera.as_mut() {
        exits.push(("camera", matches!(camera.worker.try_exit(), Ok(Some(_)))));
    }
    let reaped_at = Instant::now();
    let finished_us = lamp_ipc::monotonic_us();
    drop(clock_scope);
    assert!(reply.is_none(), "an answer outlived its run");
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    Run {
        result,
        trace: trace.iter().skip(first_event).cloned().collect(),
        boot,
        started_us,
        finished_us,
        reaped_at,
        seen,
        elapsed,
        exits,
        cleanup_fails,
    }
}

fn is_mute(result: &Result<()>) -> bool {
    result
        .as_ref()
        .err()
        .is_some_and(|error| error.downcast_ref::<PhysicalMute>().is_some())
}

mod capture_schedule_tests {
    use super::*;

    fn observed((sequence, captured_at_us): (u64, u64)) -> ObservedAudio {
        ObservedAudio {
            sequence,
            captured_at_us,
            samples: [7; 160],
        }
    }

    #[test]
    fn a_delivery_pause_keeps_contiguous_original_acquisition_times() {
        let origin_us = 1_000_000;
        let mut schedule = CaptureSchedule::new(origin_us);
        let mut detector = TurnDetector::v1_baseline();
        assert!(schedule.next_due(origin_us - 1).is_none());
        let first = schedule.next_due(origin_us).unwrap();
        assert_eq!(first, (1, origin_us));
        assert!(matches!(
            detector.push(observed(first), 0.0),
            Activity::Quiet
        ));
        assert!(schedule.next_due(origin_us + 9_999).is_none());
        // The delivery peer was not run for 60 ms. The source still acquired
        // all six blocks at 10 ms intervals, so the real detector sees no gap.
        for sequence in 2..=7 {
            let next = schedule.next_due(origin_us + 60_000).unwrap();
            assert_eq!(next, (sequence, origin_us + 10_000 * (sequence - 1)));
            assert!(matches!(
                detector.push(observed(next), 0.0),
                Activity::Quiet
            ));
        }
        assert!(schedule.next_due(origin_us + 60_000).is_none());
    }

    #[test]
    fn late_delivery_cannot_refresh_a_stale_admission_trigger() {
        let origin_us = 1_000_000;
        let mut schedule = CaptureSchedule::new(origin_us);
        assert_eq!(schedule.next_due(origin_us), Some((1, origin_us)));
        let delivered_at_us = origin_us + 110_000;
        let late = schedule.next_due(delivered_at_us).unwrap();
        assert_eq!(late, (2, origin_us + 10_000));
        assert_eq!(delivered_at_us - late.1, 100_000);

        let at = MonoTime::from_micros(delivered_at_us - 1);
        let until = MonoTime::from_micros(delivered_at_us + 100_000);
        let mut owner = Controller::new(BootId::new([1; 16]).unwrap(), at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let authority = owner.snapshot(at).unwrap();
        let context = AdmissionContext {
            capture: CaptureLineage {
                worker: BootId::new([2; 16]).unwrap(),
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: authority.microphone_generation(),
            },
            authority,
        };
        let fresh = InputAdmission::default()
            .observe(
                Activity::Start(vec![observed(late)]),
                context,
                delivered_at_us - 1,
            )
            .unwrap();
        assert!(matches!(fresh.step, AdmissionStep::Candidate(_)));
        let stale = InputAdmission::default()
            .observe(
                Activity::Start(vec![observed(late)]),
                context,
                delivered_at_us,
            )
            .unwrap_err();
        assert_eq!(stale.to_string(), "candidate trigger is stale or future");
    }

    #[test]
    fn a_dropped_block_or_real_source_gap_still_faults() {
        let origin_us = 1_000_000;
        let mut schedule = CaptureSchedule::new(origin_us);
        let first = schedule.next_due(origin_us).unwrap();
        // Deliberately lose block 2; no delivery catch-up can renumber block 3.
        assert_eq!(
            schedule.next_due(origin_us + 10_000),
            Some((2, origin_us + 10_000))
        );
        let after_drop = schedule.next_due(origin_us + 20_000).unwrap();
        assert_eq!(after_drop, (3, origin_us + 20_000));
        for broken in [after_drop, (2, origin_us + 50_001)] {
            let mut detector = TurnDetector::v1_baseline();
            assert!(matches!(
                detector.push(observed(first), 0.0),
                Activity::Quiet
            ));
            assert!(matches!(
                detector.push(observed(broken), 0.0),
                Activity::Fault("capture discontinuity")
            ));
        }
    }
}

#[test]
fn control_one_question_is_answered_once_then_mute_ends_the_run() {
    let _one = one_session();
    let run = run(Script::question(Answer::Complete(12)));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.seen.starts.len(), 1);
    let turn = run.seen.starts[0];
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    // 12 packets of 960 samples are exactly 48 blocks of 240: every sample
    // was written once, with no padding, and only the last block is final.
    let accepted = run.accepted(turn);
    assert_eq!(
        accepted.iter().map(|c| c.frames).sum::<usize>(),
        12 * PACKET
    );
    assert_eq!(accepted.len(), 48);
    assert!(accepted.iter().all(|c| c.frames == 240));
    assert!(accepted[..47].iter().all(|c| !c.end_of_speech));
    assert!(accepted[47].end_of_speech);
    assert!(run.seen.chunks.iter().all(|c| c.accepted));
    assert_eq!(run.events("speech_final_sample_retired").len(), 1);
    assert_eq!(run.events("listening_ready").len(), 1);
}

#[test]
fn readiness_publishes_the_link_state_it_was_validated_against() {
    let _one = one_session();
    let run = run(Script::question(Answer::Complete(12)));
    run.assert_invariants();
    let current = run.events("provider_link_current");
    assert_eq!(current.len(), 1);
    let at = |kind: &str| {
        run.trace
            .iter()
            .position(|event| event["kind"] == kind)
            .unwrap()
    };
    // Recorded with readiness, at the same tick and just before it.
    assert_eq!(at("provider_link_current") + 1, at("listening_ready"));
    let ready = run.events("listening_ready")[0];
    assert_eq!(current[0]["at_us"], ready["at_us"]);
    assert_eq!(current[0]["state"], "ready");
    // The fixture's one provider session.
    assert_eq!(current[0]["session"], 1);
}

#[test]
fn distinct_rejects_an_item_that_returns_later() {
    // A sort-free `dedup` would miss this order.
    assert!(!distinct(&[1, 2, 1]));
    assert!(distinct(&[3, 1, 2]));
    assert!(distinct(&[]));
}

/// Turn of the first request.
fn first(run: &Run) -> u64 {
    *run.seen.starts.first().expect("a request started")
}

#[test]
fn provider_failure_before_output_settles_failed_without_speech() {
    let _one = one_session();
    let run = run(Script::question(Answer::FailBeforeOutput));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.outcomes()[&first(&run)], ["provider_failed"]);
    assert!(run.seen.chunks.is_empty());
    assert!(run.events("speaker_first_write").is_empty());
}

#[test]
fn provider_failure_during_output_never_becomes_a_completed_answer() {
    let _one = one_session();
    let run = run(Script::question(Answer::FailDuringOutput(12)));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let turn = first(&run);
    // Audio that arrived before the failure may play; the turn stays failed.
    assert!(!run.accepted(turn).is_empty());
    assert_eq!(run.outcomes()[&turn], ["provider_failed"]);
}

#[test]
fn provider_process_lost_before_output_ends_the_run_without_speech() {
    let _one = one_session();
    let run = run(Script {
        answers: vec![],
        ..Script::question(Answer::FailBeforeOutput)
    }
    .then(When::InputEnded(1), Act::ProviderGone));
    run.assert_invariants();
    assert!(
        run.result.is_err() && !is_mute(&run.result),
        "{:?}",
        run.result
    );
    assert!(run.seen.chunks.is_empty());
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
}

#[test]
fn provider_process_lost_during_output_stops_speech_and_fails_the_turn() {
    let _one = one_session();
    let run = run(Script::question(Answer::Hold(40)).then(When::Accepted(3), Act::ProviderGone));
    run.assert_invariants();
    assert!(
        run.result.is_err() && !is_mute(&run.result),
        "{:?}",
        run.result
    );
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["runtime_failed"]);
    let lost = run.fired_at(Act::ProviderGone);
    // Only chunks already accepted around the loss; the 40-packet answer is cut.
    let frames: usize = run.accepted(turn).iter().map(|c| c.frames).sum();
    assert!(frames < 40 * PACKET, "{frames}");
    assert!(
        run.seen
            .chunks
            .iter()
            .all(|c| c.snapshot_issued_us <= lost + 100_000),
        "dispatch long after the provider was lost"
    );
}

#[test]
fn capture_fault_during_output_cancels_the_answer_and_ends_the_run() {
    let _one = one_session();
    let run = run(Script::question(Answer::Hold(40)).then(When::Accepted(3), Act::CaptureFault));
    run.assert_invariants();
    assert!(
        run.result.is_err() && !is_mute(&run.result),
        "{:?}",
        run.result
    );
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
}

/// A capture held up for half a second, as the SD/MMC host's re-tune held
/// lamp-4ace's microphone (#784), is a gap: the voice goes on listening and
/// answers the question asked after it.
#[test]
fn a_half_second_capture_stall_is_a_gap_and_the_voice_goes_on() {
    let _one = one_session();
    let run = run(Script {
        speech: 80..140,
        ..Script::question(Answer::Complete(12))
    }
    .then(When::Block(30), Act::CaptureStall));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let gaps = run.events("capture_gap");
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert!(
        // The harness's capture schedule (lamp-voice's own since #835's
        // base) resumes on its 10 ms grid, a little inside the 500 ms hold.
        gaps[0]["stalled_us"].as_u64().unwrap() >= 400_000,
        "{gaps:?}"
    );
    assert_eq!(run.seen.starts.len(), 1);
    let turn = run.seen.starts[0];
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
}

/// A capture worker held up 400 ms whose microphone closed (#784) opens it
/// again under a new epoch; the voice goes on and answers the question
/// asked after it.
#[test]
fn a_microphone_closed_by_a_stall_reopens_under_a_new_epoch_and_the_voice_goes_on() {
    let _one = one_session();
    let run = run(Script {
        speech: 80..140,
        ..Script::question(Answer::Complete(12))
    }
    .then(When::Block(30), Act::CaptureReopen));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.events("capture_gap_closed").len(), 1);
    let started: Vec<_> = run
        .events("capture_started")
        .iter()
        .map(|event| event["details"]["epoch"].as_u64().unwrap())
        .collect();
    assert_eq!(started, [1, 2]);
    let gaps = run.events("capture_gap");
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert!(
        gaps[0]["stalled_us"].as_u64().unwrap() >= 350_000,
        "{gaps:?}"
    );
    assert_eq!(run.seen.starts.len(), 1);
    let turn = run.seen.starts[0];
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
}

/// A stall inside someone's question: what was heard is abandoned as a hand
/// Stop abandons it, never glued to what follows the gap, and the voice
/// goes on.
#[test]
fn a_stall_inside_a_question_abandons_it_and_the_voice_goes_on() {
    let _one = one_session();
    let run = run(Script {
        speech: 40..90,
        actions: vec![
            (When::Block(60), Act::CaptureReopen),
            (When::Block(250), Act::Mute),
        ],
        ..Script::question(Answer::Complete(12))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let cancels = run.events("hand_control_cancel");
    assert_eq!(cancels.len(), 1, "{cancels:?}");
    assert_eq!(cancels[0]["reason"], "capture_gap");
    assert!(
        run.seen.ends.is_empty(),
        "a question cut by a gap was sent as finished: {:?}",
        run.seen.ends
    );
}

/// The microphone closed and reopened while the lamp answers (#784): the
/// reply is cut as a hand Stop cuts it, and the voice goes on.
#[test]
fn a_reopen_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![
            (When::Accepted(3), Act::CaptureReopen),
            (When::Block(400), Act::Mute),
        ],
        ..Script::question(Answer::Hold(40))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let cancels = run.events("hand_control_cancel");
    assert_eq!(cancels.len(), 1, "{cancels:?}");
    assert_eq!(cancels[0]["reason"], "capture_gap");
}

/// A capture held up half a second while the lamp answers, with no close
/// reported: the lapsed lease cuts the reply, and the voice goes on.
#[test]
fn a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![
            (When::Accepted(3), Act::CaptureStall),
            (When::Block(400), Act::Mute),
        ],
        ..Script::question(Answer::Hold(40))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let cancels = run.events("hand_control_cancel");
    assert_eq!(cancels.len(), 1, "{cancels:?}");
    assert_eq!(cancels[0]["reason"], "capture_gap");
}

/// An XRun closes and reopens the microphone within 50 ms, inside a
/// question: the question is abandoned and the voice goes on.
#[test]
fn a_short_xrun_reopen_inside_a_question_abandons_it_and_the_voice_goes_on() {
    let _one = one_session();
    let run = run(Script {
        speech: 40..90,
        actions: vec![
            (When::Block(60), Act::CaptureXRun),
            (When::Block(250), Act::Mute),
        ],
        ..Script::question(Answer::Complete(12))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let cancels = run.events("hand_control_cancel");
    assert_eq!(cancels.len(), 1, "{cancels:?}");
    assert_eq!(cancels[0]["reason"], "capture_gap");
    assert!(run.seen.ends.is_empty(), "{:?}", run.seen.ends);
}

#[test]
fn capture_stall_during_a_question_ends_the_run_without_an_answer() {
    let _one = one_session();
    let run = run(Script {
        speech: 20..400,
        ..Script::question(Answer::Complete(12))
    }
    .then(When::Block(60), Act::CaptureSilent));
    run.assert_invariants();
    assert!(
        run.result.is_err() && !is_mute(&run.result),
        "{:?}",
        run.result
    );
    assert!(run.seen.chunks.is_empty());
    assert!(
        run.seen.ends.is_empty(),
        "a stalled question was sent as finished"
    );
}

#[test]
fn mute_while_answer_speech_is_queued_cuts_it_and_never_resumes() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![(When::Accepted(3), Act::Mute)],
        ..Script::question(Answer::Complete(40))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["privacy_muted"]);
    let frames: usize = run.accepted(turn).iter().map(|c| c.frames).sum();
    assert!(frames < 40 * PACKET, "{frames}");
}

#[test]
fn sigterm_while_answer_speech_is_pending_stops_cleanly() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![(When::Accepted(3), Act::Sigterm)],
        signals: true,
        ..Script::question(Answer::Hold(40))
    });
    run.assert_invariants();
    assert!(run.result.is_ok(), "{:?}", run.result);
    assert_eq!(run.events("service_stop_requested").len(), 1);
    assert_eq!(run.outcomes()[&first(&run)], ["service_stopped"]);
}

#[test]
fn failure_notice_plays_once_keeps_the_failure_and_yields_to_the_next_owner() {
    let _one = one_session();
    let run = run(Script {
        answers: vec![Answer::FailBeforeOutput, Answer::Complete(12)],
        actions: vec![
            (When::NoticeAccepted(2), Act::Speak),
            (When::Settled(2), Act::Mute),
        ],
        notices: true,
        ..Script::question(Answer::FailBeforeOutput)
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.seen.starts.len(), 2, "the next owner was not served");
    let (failed, next) = (run.seen.starts[0], run.seen.starts[1]);
    assert_eq!(run.outcomes()[&failed], ["provider_failed"]);
    assert_eq!(run.events("failure_notice_attempted").len(), 1);
    // No notice audio after the next owner was admitted.
    let admitted = run
        .events("input_admitted")
        .iter()
        .find(|e| e["turn"] == next)
        .map(|e| e["at_us"].as_u64().unwrap())
        .unwrap();
    for chunk in run.seen.chunks.iter().filter(|c| c.notice) {
        assert_eq!(chunk.turn, failed);
        assert!(chunk.snapshot_issued_us <= admitted, "{chunk:?}");
    }
    assert!(!run.accepted(next).is_empty());
}

#[test]
fn stalled_speaker_during_output_is_bounded_and_never_completes_the_answer() {
    let _one = one_session();
    let run =
        run(Script::question(Answer::Complete(40)).then(When::Accepted(3), Act::StallSpeaker));
    run.assert_invariants();
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["runtime_failed"]);
    // V1-baseline's existing policy bound: write evidence within 200 ms
    // of each dispatch, checked on every modeled capture block. These
    // timestamps are scenario time, not measured host or lamp latency.
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(error.contains("speaker write evidence expired"), "{error}");
    let stalled = run.fired_at(Act::StallSpeaker);
    let ended = run.events("turn_finished")[0]["at_us"].as_u64().unwrap();
    let after_ms = (ended - stalled) / 1_000;
    eprintln!("scenario stall to turn end {after_ms} ms");
    // The last unacknowledged chunk may have been dispatched just before the
    // stall; detection follows at the next capture block after its budget.
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
}

/// F1. Not the pilot policy: `vad-only`, the option default, has no
/// write-evidence lease. Before the speaker progress bound, a speaker that
/// keeps reading control but stops acknowledging chunks left the answer open
/// until something else ended the run (only the 12 s scenario limit here).
/// The bound now ends it like V1-baseline's own lease, in the same window.
#[test]
fn stalled_speaker_under_vad_only_is_bounded_too() {
    let _one = one_session();
    let run = run(Script {
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Answer::Complete(40))
    }
    .then(When::Accepted(3), Act::StallSpeaker));
    run.assert_invariants();
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(error.contains("speaker acknowledgment expired"), "{error}");
    let stalled = run.fired_at(Act::StallSpeaker);
    let ended = run.events("turn_finished")[0]["at_us"].as_u64().unwrap();
    let after_ms = (ended - stalled) / 1_000;
    eprintln!("scenario stall to turn end {after_ms} ms");
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
}

/// Not the pilot policy: without the V1-baseline playback mask, speech over
/// the notice admits the next owner while the notice is still playing.
#[test]
fn failure_notice_yields_mid_play_to_a_new_owner_under_vad_only() {
    let _one = one_session();
    let run = run(Script {
        answers: vec![Answer::FailBeforeOutput, Answer::Complete(12)],
        actions: vec![
            (When::NoticeAccepted(1), Act::Speak),
            (When::Settled(2), Act::Mute),
        ],
        notices: true,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Answer::FailBeforeOutput)
    });
    run.assert_invariants();
    assert_eq!(run.seen.starts.len(), 2, "the next owner was not served");
    let (failed, next) = (run.seen.starts[0], run.seen.starts[1]);
    assert_eq!(run.outcomes()[&failed].len(), 1);
    assert_ne!(run.outcomes()[&failed][0], "audio_written_unscored");
    // N1: the output was cut by the next owner, and the record says so. The
    // provider's failure stays in the same record, unchanged, and the notice
    // is visible as attempted but never retired: neither became an answer.
    let record = run
        .events("turn_finished")
        .into_iter()
        .find(|e| e["turn"] == failed)
        .unwrap();
    assert_eq!(record["outcome"], "user_interrupted", "{record}");
    assert_eq!(record["provider_outcome"]["kind"], "failed", "{record}");
    assert_eq!(record["provider_audio_seen"], false, "{record}");
    assert_eq!(record["failure_notice"]["answer"], false, "{record}");
    assert_eq!(record["failure_notice"]["state"], "attempted", "{record}");
    assert!(
        record["failure_notice"]["retired_at_us"].is_null(),
        "{record}"
    );
    assert_eq!(run.events("provider_turn_failed").len(), 1);
    assert_eq!(run.events("failure_notice_attempted").len(), 1);
    let admitted = run
        .events("input_admitted")
        .iter()
        .find(|e| e["turn"] == next)
        .map(|e| e["at_us"].as_u64().unwrap())
        .unwrap();
    for chunk in run.seen.chunks.iter().filter(|c| c.notice) {
        assert!(chunk.snapshot_issued_us <= admitted, "{chunk:?}");
    }
    assert!(!run.accepted(next).is_empty());
}

#[test]
fn provider_worker_fault_during_output_fails_the_turn_and_ends_the_run() {
    let _one = one_session();
    let run = run(Script::question(Answer::Hold(40)).then(When::Accepted(3), Act::ProviderFault));
    run.assert_invariants();
    assert!(
        run.result.is_err() && !is_mute(&run.result),
        "{:?}",
        run.result
    );
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
}

#[test]
fn terminal_provider_link_during_output_settles_failed_and_ends_service() {
    let _one = one_session();
    let run =
        run(Script::question(Answer::Hold(40)).then(When::Accepted(3), Act::ProviderTerminal));
    run.assert_invariants();
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(
        error.contains("terminal provider output settled"),
        "{error}"
    );
    assert_eq!(run.outcomes()[&first(&run)], ["provider_failed"]);
    assert!(!run.events("provider_output_ended").is_empty());
}

#[test]
fn mute_while_the_failure_notice_plays_cuts_it_and_keeps_the_failure() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![(When::NoticeAccepted(3), Act::Mute)],
        notices: true,
        ..Script::question(Answer::FailBeforeOutput)
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let turn = first(&run);
    assert_ne!(run.outcomes()[&turn][0], "audio_written_unscored");
    assert!(run.events("failure_notice_final_sample_retired").is_empty());
    // 7,200 notice samples are 30 chunks; the mute cut it.
    let notice: usize = run
        .seen
        .chunks
        .iter()
        .filter(|c| c.notice && c.accepted)
        .count();
    assert!(notice < 30, "{notice}");
}

#[test]
fn sigterm_while_the_failure_notice_plays_stops_cleanly() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![(When::NoticeAccepted(3), Act::Sigterm)],
        notices: true,
        signals: true,
        ..Script::question(Answer::FailBeforeOutput)
    });
    run.assert_invariants();
    assert!(run.result.is_ok(), "{:?}", run.result);
    assert_eq!(run.events("service_stop_requested").len(), 1);
    let turn = first(&run);
    assert_ne!(run.outcomes()[&turn][0], "audio_written_unscored");
    assert!(run.events("failure_notice_final_sample_retired").is_empty());
}

#[test]
fn ring_fault_during_an_answer_keeps_the_conversation_and_cleans_up() {
    let _one = one_session();
    let run = run(Script {
        ring: true,
        ..Script::question(Answer::Complete(12))
    }
    .then(When::Accepted(3), Act::RingFault));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert!(run.seen.ring_frames > 0, "the ring never presented");
    // The optional failure revokes only the ring: the answer completes in
    // full, readiness is not claimed again, and the ring blanks on Stop.
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    let frames: usize = run.accepted(turn).iter().map(|c| c.frames).sum();
    assert_eq!(frames, 12 * PACKET);
    assert_eq!(run.events("listening_ready").len(), 1);
    assert!(!run.events("ring_service_fault").is_empty());
}

#[test]
fn expired_accepted_privacy_rejects_a_fresh_queued_mute_in_stepped_run() {
    let _one = one_session();
    let run = run(Script {
        actions: vec![(When::Accepted(1), Act::ExpirePrivacyThenMute)],
        ..Script::question(Answer::Hold(12))
    });
    run.assert_invariants();
    let (before, after) = run.seen.privacy_expiry_jump.expect("privacy lease expired");
    assert_eq!(after - before, 50_000, "exact production privacy boundary");
    assert_eq!(run.fired_at(Act::ExpirePrivacyThenMute), before);
    assert_eq!(run.seen.fresh_mute_queued_at_us, Some(after));
    // An accepted answer proves the coordinator had already installed a
    // permissive sensor lease before the injected expiry; startup grace is
    // not the condition under test.
    assert!(
        run.accepted(first(&run))
            .iter()
            .any(|chunk| chunk.checked_us < before)
    );
    assert_eq!(
        run.result.as_ref().unwrap_err().to_string(),
        "service privacy freshness lost before next observation"
    );
    // A fresh mute was already waiting on the real channel. The expired
    // accepted lease must still fail before that packet can manufacture a
    // healthy, restartable physical-mute lifecycle.
    assert!(!is_mute(&run.result));
    assert!(run.events("physical_mute").is_empty());
}

/// Lamp-4ace, 19 Oct 01:19: 0.7 s after a start the voice ended on
/// `invalid or stale service privacy observation`, the first one read after
/// it had waited in the queue while the workers started, and came back
/// muted by the restart guard. Before the microphone is first allowed, such
/// an observation now closes the gate and is waited past: the next, fresh,
/// opens it as usual, and the run goes on to its answer and its mute.
#[test]
fn a_privacy_observation_read_stale_as_the_voice_starts_is_waited_past() {
    let _one = one_session();
    let run = run(Script {
        privacy_start: PrivacyStart::StaleFirst,
        ..Script::question(Answer::Complete(12))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.events("privacy_stale_at_start").len(), 1);
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    assert_eq!(run.events("listening_ready").len(), 1);
}

/// The same stale first observation with nothing after it: the gate stays
/// closed, so nothing is heard, and the run ends on the lost freshness once
/// the starting second has passed, as it did before on the observation
/// itself.
#[test]
fn a_privacy_worker_silent_after_a_stale_first_observation_still_ends_the_run() {
    let _one = one_session();
    let run = run(Script {
        privacy_start: PrivacyStart::StaleThenSilent,
        ..Script::question(Answer::Complete(12))
    });
    assert_eq!(
        run.result.as_ref().unwrap_err().to_string(),
        "service privacy freshness lost before next observation"
    );
    assert!(!is_mute(&run.result));
    assert_eq!(run.events("privacy_stale_at_start").len(), 1);
    assert!(
        run.events("listening_ready").is_empty(),
        "nothing was heard"
    );
    assert!(run.seen.chunks.is_empty());
}

/// Only staleness is waited past while the voice starts: an observation
/// out of order (acquired before the one before it) still ends the run at
/// once, as before.
#[test]
fn a_privacy_observation_out_of_order_as_the_voice_starts_still_ends_the_run() {
    let _one = one_session();
    let run = run(Script {
        privacy_start: PrivacyStart::Backwards,
        ..Script::question(Answer::Complete(12))
    });
    assert_eq!(
        run.result.as_ref().unwrap_err().to_string(),
        "invalid or stale service privacy observation"
    );
    assert!(run.events("privacy_stale_at_start").is_empty());
    assert!(
        run.events("listening_ready").is_empty(),
        "nothing was heard"
    );
}

/// Once the microphone has been allowed, a stale observation ends the run
/// at once as before, though the starting second has not passed.
#[test]
fn a_privacy_observation_read_stale_after_the_microphone_was_allowed_still_ends_the_run() {
    let _one = one_session();
    let run = run(Script {
        privacy_start: PrivacyStart::StaleAfterAllowed,
        ..Script::question(Answer::Hold(12))
    });
    assert_eq!(
        run.result.as_ref().unwrap_err().to_string(),
        "invalid or stale service privacy observation"
    );
    assert!(run.events("privacy_stale_at_start").is_empty());
    assert_eq!(
        run.events("listening_ready").len(),
        1,
        "it was allowed first"
    );
}

#[test]
fn ring_lost_without_blanking_keeps_the_answer_but_cleanup_fails_truthfully() {
    let _one = one_session();
    let run = run(Script {
        ring: true,
        cleanup_fails: true,
        ..Script::question(Answer::Complete(12))
    }
    .then(When::Accepted(3), Act::RingGone));
    run.assert_invariants();
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    let frames: usize = run.accepted(turn).iter().map(|c| c.frames).sum();
    assert_eq!(frames, 12 * PACKET);
    // Without a black write, the mute is not a restartable clean stop.
    assert!(!is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.events("listening_ready").len(), 1);
}

#[test]
fn camera_loss_during_an_answer_revokes_only_sight() {
    let _one = one_session();
    let run = run(Script {
        camera: true,
        ..Script::question(Answer::Complete(12))
    }
    .then(When::Accepted(3), Act::CameraSilent));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.events("camera_unavailable").len(), 1);
    // Audio is untouched: the answer completes in full and readiness is
    // neither lost nor claimed again.
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    let frames: usize = run.accepted(turn).iter().map(|c| c.frames).sum();
    assert_eq!(frames, 12 * PACKET);
    assert_eq!(run.events("listening_ready").len(), 1);
}

/// Physical mute during an answer, then privacy reopened at once, through
/// the production service supervisor and privacy wait, four times. Each
/// cycle must start only after the previous one's children were reaped, and
/// a new cycle's speaker must never see the previous controller's audio.
#[test]
fn rapid_mute_and_reopen_never_overlaps_workers_or_carries_speech_across() {
    use crate::service_privacy::{
        self, WaitOutcome,
        tests::{Script as PrivacyScript, mock},
    };
    let _one = one_session();
    const CYCLES: usize = 4;
    let mut trace = SessionState::default();
    let runs = std::cell::RefCell::new(Vec::<Run>::new());
    let waits = std::cell::Cell::new(0usize);
    let result = crate::coordinator::service::supervise(
        &mut trace,
        &mut None,
        &Termination::inactive(),
        |_, _| {
            if waits.get() == CYCLES {
                return Ok(WaitOutcome::Stopped);
            }
            let scratch = SessionDirectory::create().unwrap();
            let seen = Arc::new(AtomicBool::new(false));
            let (worker, peer) = mock(&scratch.path, PrivacyScript::Unmute, seen);
            let outcome = service_privacy::wait_worker(
                worker,
                new_boot().unwrap(),
                &Termination::inactive(),
                |_, _, _| Ok(()),
            );
            assert!(peer.join().unwrap().received_stop);
            waits.set(waits.get() + 1);
            outcome
        },
        |trace, _| {
            let mut run = run_in(
                Script {
                    actions: vec![(When::Accepted(3), Act::Mute)],
                    ..Script::question(Answer::Complete(40))
                },
                trace,
            );
            let mute = run
                .result
                .as_ref()
                .err()
                .and_then(|e| e.downcast_ref::<PhysicalMute>())
                .map(|mute| mute.observed_at_us);
            if mute.is_none() {
                eprintln!(
                    "runtime cycle {} did not end in physical mute:\n{}",
                    runs.borrow().len() + 1,
                    run.summary()
                );
            }
            let result = match mute {
                Some(observed_at_us) => Err(Box::new(PhysicalMute {
                    onboarding: None,
                    observed_at_us,
                }) as _),
                None => std::mem::replace(&mut run.result, Ok(())),
            };
            runs.borrow_mut().push(run);
            result
        },
    );
    assert!(result.is_ok(), "{result:?}");
    let runs = runs.into_inner();
    assert_eq!(runs.len(), CYCLES);
    for run in &runs {
        run.assert_invariants();
        assert!(is_mute(&run.result), "{:?}", run.result);
        assert_eq!(run.seen.starts.len(), 1);
        assert_eq!(run.outcomes()[&first(run)], ["privacy_muted"]);
        // Every chunk this cycle's speaker saw was issued by this cycle's
        // controller: nothing from a previous runtime reached it.
        assert!(run.seen.chunks.iter().all(|c| c.boot == run.boot));
    }
    for pair in runs.windows(2) {
        assert_ne!(pair[0].boot, pair[1].boot);
        // The next runtime's children were spawned only after every child of
        // the previous one had been reaped.
        assert!(pair[1].started_us > pair[0].finished_us);
    }
    let mutes = trace
        .iter()
        .filter(|e| e["kind"] == "service_cycle_muted")
        .count();
    assert_eq!(mutes, CYCLES);
    // Readiness is only ever claimed inside a cycle, never while waiting.
    let mut open = false;
    for event in trace.iter() {
        match event["kind"].as_str() {
            Some("service_cycle_start") => open = true,
            Some("service_cycle_muted") => open = false,
            Some("listening_ready") => assert!(open, "readiness while waiting"),
            _ => {}
        }
    }
    let reopen_ms: Vec<u64> = runs
        .windows(2)
        .map(|pair| (pair[1].started_us - pair[0].finished_us) / 1_000)
        .collect();
    eprintln!("reaped-to-next-spawn {reopen_ms:?} ms");
}

/// The release token must survive every assertion until it is sent. Dropping a
/// sender without sending would make the real stalled writer panic on recv.
struct MetadataRelease(Option<std::sync::mpsc::Sender<()>>);

impl MetadataRelease {
    fn release(&mut self) -> io::Result<()> {
        self.0
            .take()
            .ok_or_else(|| io::Error::other("metadata writer already released"))?
            .send(())
            .map_err(io::Error::other)
    }
}

impl Drop for MetadataRelease {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            // This unbounded-channel send cannot wait. Do not replace an
            // existing test failure with a second panic during unwinding.
            let _ = release.send(());
        }
    }
}

struct StalledMetadata {
    trace: SessionState,
    entered: std::sync::mpsc::Receiver<()>,
    release: MetadataRelease,
}

impl StalledMetadata {
    fn new() -> Self {
        let deadline = Instant::now() + Duration::from_secs(2);
        let (writer, entered, release) = crate::service_trace::ServiceTrace::stalled();
        let mut fixture = Self {
            trace: SessionState::with_writer(writer),
            entered,
            release: MetadataRelease(Some(release)),
        };
        // Prime the production submission path before any child or policy
        // clock starts. The writer cannot complete this first event until the
        // release token is sent, including throughout the later audio run.
        fixture
            .trace
            .record(json!({"kind":"fixture_metadata_stall","at_us":monotonic_us()}))
            .unwrap();
        fixture.trace.flush(&mut None, monotonic_us());
        fixture
            .entered
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("metadata writer never stalled");
        fixture
    }

    fn release_and_finish(&mut self) -> crate::service_trace::Report {
        self.release.release().expect("metadata writer disappeared");
        self.trace
            .finish_logging()
            .expect("metadata writer cleanup report")
    }
}

impl Drop for StalledMetadata {
    fn drop(&mut self) {
        if self.release.0.is_some() {
            let _ = self.release.release();
            // Preserve the original assertion failure while attempting bounded
            // real cleanup. Keep the entered receiver alive during this wait.
            // Successful scenarios check this receipt explicitly instead.
            let _ = self.trace.finish_logging();
        }
    }
}

#[test]
fn metadata_release_on_unwind_unblocks_the_real_writer_and_keeps_the_panic() {
    use crate::service_trace::{FINISH_WAIT, ServiceTrace};
    struct ScenarioFailure;

    let (mut writer, entered, release) = ServiceTrace::stalled();
    let release = MetadataRelease(Some(release));
    writer.submit(&json!({"kind":"fixture_metadata_stall"}));
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _release_on_unwind = release;
        std::panic::panic_any(ScenarioFailure);
    }));
    assert!(failed.unwrap_err().is::<ScenarioFailure>());
    let report = writer.finish(FINISH_WAIT);
    assert!(report.acknowledged, "{report:?}");
    assert!(!report.writer_failed, "{report:?}");
    assert_eq!(report.written_events, 1);
}

/// Metadata storage stalls from the first event (a stuck disk). Answer audio
/// must keep flowing, a real SIGTERM must still stop the runtime cleanly, and
/// the unacknowledged metadata must be reported within the finish bound
/// rather than hidden or waited on.
#[test]
fn sigterm_while_metadata_storage_is_stalled_stops_cleanly_and_reports_the_loss() {
    use crate::service_trace::FINISH_WAIT;
    let _one = one_session();
    let mut metadata = StalledMetadata::new();
    let run = run_in(
        Script {
            actions: vec![(When::Accepted(10), Act::Sigterm)],
            signals: true,
            ..Script::question(Answer::Hold(40))
        },
        &mut metadata.trace,
    );
    // The real writer entered its blocked first event before this run began.
    run.assert_invariants();
    assert!(run.result.is_ok(), "{:?}", run.result);
    // Ten chunks were accepted while every metadata write was blocked.
    assert!(run.seen.chunks.iter().filter(|c| c.accepted).count() >= 10);
    let stopped = run.seen.sigterm_sent_at.expect("SIGTERM was sent");
    let shutdown_ms = run.reaped_at.duration_since(stopped).as_millis();
    let finishing = Instant::now();
    let report = metadata
        .trace
        .finish_logging()
        .expect("service metadata report");
    let finish_wait = finishing.elapsed();
    let cleanup = metadata.release_and_finish();
    assert!(cleanup.acknowledged, "{cleanup:?}");
    assert_eq!(report.written_events, 0, "metadata was blocked throughout");
    eprintln!(
        "real SIGTERM-send to all children reaped {shutdown_ms} ms; real metadata finish {finish_wait:?}; {report:?}"
    );
    assert!(
        !report.acknowledged && report.finish_attempted,
        "{report:?}"
    );
    assert!(
        finish_wait < FINISH_WAIT + Duration::from_millis(250),
        "{finish_wait:?}"
    );
    assert!(shutdown_ms < 1_000, "{shutdown_ms} ms");
}

#[test]
fn speaker_progress_is_due_strictly_after_lease_plus_report_age() {
    let (mut owner, reply, permit) = active_reply();
    let _ = (&mut owner, reply);
    let pending = PendingSpeaker {
        origin: SpeechOrigin::Provider,
        sequence: 1,
        permit,
        accepted: 0,
        final_chunk: false,
        report_due_us: 1_000 + SPEAKER_PERMIT_LEASE_US + SPEAKER_REPORT_AGE_US,
    };
    let due = pending.report_due_us;
    assert!(check_speaker_progress(None, u64::MAX).is_ok());
    assert!(check_speaker_progress(Some(&pending), due).is_ok());
    let error = check_speaker_progress(Some(&pending), due + 1).unwrap_err();
    assert_eq!(error.to_string(), "speaker acknowledgment expired");
    // Equal to V1-baseline's write-evidence budget, so that lease reports
    // first whenever it applies.
    assert_eq!(
        SPEAKER_PERMIT_LEASE_US + SPEAKER_REPORT_AGE_US,
        crate::admission::DECISION_BUDGET_US
    );
}

// Further speaker policy controls in scenario time, thresholds unchanged.
// Each run still measures real cleanup wall time and proves actual child reap.

fn stall_end(run: &Run, act: Act) -> (String, u64) {
    let error = run.result.as_ref().unwrap_err().to_string();
    let stalled = run.fired_at(act);
    let ended = run.events("turn_finished")[0]["at_us"].as_u64().unwrap();
    (error, (ended - stalled) / 1_000)
}

#[test]
fn partial_write_then_stall_is_bounded_under_v1_baseline() {
    let _one = one_session();
    let run =
        run(Script::question(Answer::Complete(40))
            .then(When::Accepted(3), Act::StallSpeakerPartial));
    run.assert_invariants();
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
    let (error, after_ms) = stall_end(&run, Act::StallSpeakerPartial);
    eprintln!("scenario partial stall: {error}; {after_ms} ms");
    assert!(error.contains("speaker write evidence expired"), "{error}");
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
}

#[test]
fn partial_write_then_stall_is_bounded_under_vad_only() {
    let _one = one_session();
    let run = run(Script {
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Answer::Complete(40))
    }
    .then(When::Accepted(3), Act::StallSpeakerPartial));
    run.assert_invariants();
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
    let (error, after_ms) = stall_end(&run, Act::StallSpeakerPartial);
    eprintln!("scenario partial stall: {error}; {after_ms} ms");
    assert!(error.contains("speaker acknowledgment expired"), "{error}");
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
}

#[test]
fn control_progress_without_settlement_is_bounded_under_v1_baseline() {
    let _one = one_session();
    let run =
        run(Script::question(Answer::Complete(40))
            .then(When::Accepted(3), Act::StallSpeakerInstalling));
    // The installing speaker must have seen the turn revoked.
    run.assert_invariants();
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
    let (error, after_ms) = stall_end(&run, Act::StallSpeakerInstalling);
    eprintln!("scenario installing stall: {error}; {after_ms} ms");
    assert!(error.contains("speaker write evidence expired"), "{error}");
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
    assert!(run.revoked_at(first(&run)).is_some());
}

#[test]
fn control_progress_without_settlement_is_bounded_under_vad_only() {
    let _one = one_session();
    let run = run(Script {
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Answer::Complete(40))
    }
    .then(When::Accepted(3), Act::StallSpeakerInstalling));
    run.assert_invariants();
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
    let (error, after_ms) = stall_end(&run, Act::StallSpeakerInstalling);
    eprintln!("scenario installing stall: {error}; {after_ms} ms");
    assert!(error.contains("speaker acknowledgment expired"), "{error}");
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
    assert!(run.revoked_at(first(&run)).is_some());
}

#[test]
fn duplex_control_answers_once() {
    let _one = one_session();
    let run = run(Script {
        turn_taking: TurnTaking::Duplex,
        ..Script::question(Answer::Complete(12))
    });
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    let accepted = run.accepted(turn);
    assert_eq!(
        accepted.iter().map(|c| c.frames).sum::<usize>(),
        12 * PACKET
    );
}

#[test]
fn duplex_stalled_speaker_is_bounded() {
    let _one = one_session();
    let run = run(Script {
        turn_taking: TurnTaking::Duplex,
        ..Script::question(Answer::Complete(40))
    }
    .then(When::Accepted(3), Act::StallSpeakerInstalling));
    run.assert_invariants();
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
    let (error, after_ms) = stall_end(&run, Act::StallSpeakerInstalling);
    eprintln!("scenario duplex installing stall: {error}; {after_ms} ms");
    // Duplex keeps V1-baseline's playback mask, so its lease reports first.
    assert!(error.contains("speaker write evidence expired"), "{error}");
    assert!((190..=300).contains(&after_ms), "{after_ms} ms");
}

/// The control run's question, with the lamp paused by the keeper
/// (`lamp/pause`) from the start.
fn paused_question() -> Script {
    Script {
        paused: true,
        actions: vec![(When::Block(160), Act::Mute)],
        ..Script::question(Answer::Complete(12))
    }
}

#[test]
fn a_paused_lamp_takes_no_question_and_says_why() {
    let _one = one_session();
    let run = run(paused_question());
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert!(run.seen.starts.is_empty(), "a request reached the provider");
    assert!(run.seen.chunks.is_empty());
    assert!(run.events("input_admitted").is_empty());
    assert_eq!(run.events("input_not_taken_paused").len(), 1);
    let pause = run.events("keeper_pause");
    assert_eq!(pause.len(), 1);
    assert_eq!(pause[0]["paused"], true);
}

#[test]
fn a_turn_under_way_when_paused_is_finished_and_the_next_is_not_taken() {
    let _one = one_session();
    let script = Script {
        actions: vec![
            // Paused once the question has gone to the provider, before
            // its answer is spoken; someone speaks again once it is.
            (When::InputEnded(1), Act::Pause),
            (When::Settled(1), Act::Speak),
            (When::Block(500), Act::Mute),
        ],
        ..Script::question(Answer::Complete(12))
    };
    let run = run(script);
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.seen.starts.len(), 1, "a second request was taken");
    let turn = first(&run);
    assert_eq!(run.outcomes()[&turn], ["audio_written_unscored"]);
    assert_eq!(
        run.accepted(turn).iter().map(|c| c.frames).sum::<usize>(),
        12 * PACKET,
        "the answer under way was cut"
    );
    assert_eq!(run.events("input_admitted").len(), 1);
    assert_eq!(run.events("input_not_taken_paused").len(), 1);
}

#[test]
fn resumed_the_lamp_answers_again() {
    let _one = one_session();
    let script = Script {
        paused: true,
        actions: vec![
            (When::Block(120), Act::Resume),
            (When::Block(130), Act::Speak),
            (When::Settled(1), Act::Mute),
        ],
        ..Script::question(Answer::Complete(12))
    };
    let run = run(script);
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.seen.starts.len(), 1);
    assert_eq!(run.outcomes()[&first(&run)], ["audio_written_unscored"]);
    // What was said while paused was not taken; what was said after is.
    let admitted = run.events("input_admitted");
    assert_eq!(admitted.len(), 1);
    assert!(admitted[0]["at_us"].as_u64().unwrap() > run.fired_at(Act::Resume));
    let pause: Vec<_> = run
        .events("keeper_pause")
        .iter()
        .map(|event| event["paused"].clone())
        .collect();
    assert_eq!(pause, [true, false]);
}

/// Release the worker even if a conversation assertion unwinds. The worker's
/// own timeout also bounds a regression that blocks the coordinator itself.
struct HeldOnboardingStorage {
    release: std::sync::mpsc::Sender<()>,
    timed_out: Arc<AtomicBool>,
    accepted: Arc<std::sync::atomic::AtomicU64>,
    polls: Arc<std::sync::atomic::AtomicU64>,
    directory: PathBuf,
}

impl Drop for HeldOnboardingStorage {
    fn drop(&mut self) {
        let _ = self.release.send(());
    }
}

fn held_onboarding_storage() -> (SessionState, HeldOnboardingStorage) {
    held_onboarding_storage_at(false)
}

/// The held operation is the progress commit, or with `near_call` the
/// near-call bar's synchronous write and fsync (#1803).
fn held_onboarding_storage_at(near_call: bool) -> (SessionState, HeldOnboardingStorage) {
    use crate::onboarding::storage::{Paths, ServiceStorage, Submit};
    let directory = std::env::temp_dir().join(format!(
        "lamp-held-onboarding-{}-{}",
        std::process::id(),
        monotonic_us()
    ));
    fs::create_dir(&directory).unwrap();
    let file = directory.join("progress.json");
    let (release, released) = std::sync::mpsc::channel();
    let (entered, entering) = std::sync::mpsc::channel();
    let timed_out = Arc::new(AtomicBool::new(false));
    let failed = timed_out.clone();
    let storage = ServiceStorage::start_with_hook(
        Paths {
            progress: file.clone(),
            checkpoint: directory.join("privacy.json"),
            mailbox: directory.join("mailbox"),
            near_call: directory.join("near-call.json"),
            room_sounds: directory.join("room-sounds.json"),
            room_sounds_group: None,
        },
        Box::new(move |tag| {
            if tag.revision == 2 {
                entered.send(()).unwrap();
                if released.recv_timeout(SCENARIO_LIMIT).is_err() {
                    failed.store(true, Ordering::SeqCst);
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "test storage gate expired",
                    ));
                }
            }
            Ok(())
        }),
    )
    .unwrap();
    let client = storage.client();
    let deadline = Instant::now() + SCENARIO_LIMIT;
    loop {
        if client.snapshot().unwrap().ready {
            break;
        }
        assert!(Instant::now() < deadline, "storage initialization stalled");
        std::thread::yield_now();
    }
    let held = if near_call {
        let tries = [22.4, 25.0, 23.1].map(|rise_db| lamp_near_call::Try {
            rise_db,
            score: Some(0.98),
            heard_by: lamp_near_call::HeardBy::Name,
        });
        client.near_call(lamp_near_call::Calibrated::of(tries.to_vec(), 1))
    } else {
        client.commit(crate::onboarding::Progress::default(), false)
    };
    assert!(matches!(held.unwrap(), Submit::Queued(tag) if tag.revision == 2));
    entering
        .recv_timeout(SCENARIO_LIMIT)
        .expect("commit reached the held worker");
    let accepted = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut trace = SessionState::default();
    trace.onboarding_storage = Some(storage);
    trace.onboarding_test_file = Some(file);
    trace.onboarding_test_capture_accepted = Some(accepted.clone());
    let polls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    trace.onboarding_test_polls = Some(polls.clone());
    (
        trace,
        HeldOnboardingStorage {
            release,
            timed_out,
            accepted,
            polls,
            directory,
        },
    )
}

fn assert_held_storage_progress(run: &Run, trace: &SessionState, held: &HeldOnboardingStorage) {
    run.assert_invariants();
    assert!(
        held.polls.load(Ordering::SeqCst) > 1,
        "the real onboarding owner must poll held storage during conversation"
    );
    assert!(
        !held.timed_out.load(Ordering::SeqCst),
        "storage released only by watchdog"
    );
    assert!(
        trace
            .onboarding_storage
            .as_ref()
            .unwrap()
            .client()
            .snapshot()
            .unwrap()
            .busy,
        "the real conversation ran with commit still pending"
    );
    // The capture fixture generates contiguous 10 ms blocks; these are counted
    // only after the real coordinator validates/consumes them, not when sent.
    assert!(
        held.accepted.load(Ordering::SeqCst) >= 30,
        "less than 300 ms of capture accepted while storage was held"
    );
    assert!(!run.events("listening_ready").is_empty());
    assert!(run.events("capture_gap").is_empty());
    assert!(
        run.exits.iter().all(|(_, reaped)| *reaped),
        "audio reaped before storage release"
    );
    for role in ["speaker", "capture", "provider", "privacy"] {
        assert!(
            run.seen.stopped.contains(&role),
            "{role} did not receive Stop"
        );
    }
}

fn release_onboarding_storage(trace: &mut SessionState, held: &HeldOnboardingStorage) {
    held.release.send(()).unwrap();
    assert_eq!(
        trace
            .onboarding_storage
            .as_mut()
            .unwrap()
            .finish(SCENARIO_LIMIT)
            .unwrap(),
        crate::onboarding::storage::StorageShutdown::Finished
    );
    fs::remove_dir_all(&held.directory).unwrap();
}

#[test]
fn onboarding_storage_held_does_not_interrupt_capture_or_privacy() {
    let _one = one_session();
    let (mut trace, held) = held_onboarding_storage();
    let run = run_in(Script::question(Answer::Complete(12)), &mut trace);
    assert_held_storage_progress(&run, &trace, &held);
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.outcomes()[&first(&run)], ["audio_written_unscored"]);
    // The walk's inputs are recorded once, as nothing here changes them,
    // storage started (row 10 on lamp-52e6).
    let inputs = run.events("onboarding_inputs");
    assert_eq!(inputs.len(), 1, "{inputs:?}");
    assert_eq!(inputs[0]["storage"], true, "{inputs:?}");
    release_onboarding_storage(&mut trace, &held);
}

#[test]
fn stop_and_physical_mute_do_not_wait_for_onboarding_storage() {
    let _one = one_session();
    for action in [Act::Sigterm, Act::Mute] {
        let (mut trace, held) = held_onboarding_storage();
        let run = run_in(
            Script {
                actions: vec![(When::Block(40), action)],
                signals: action == Act::Sigterm,
                ..Script::question(Answer::Hold(12))
            },
            &mut trace,
        );
        assert_held_storage_progress(&run, &trace, &held);
        if action == Act::Sigterm {
            assert!(run.result.is_ok(), "{:?}", run.result);
            assert_eq!(run.events("service_stop_requested").len(), 1);
        } else {
            assert!(is_mute(&run.result), "{:?}", run.result);
            assert_eq!(run.events("physical_mute").len(), 1);
        }
        release_onboarding_storage(&mut trace, &held);
    }
}

#[test]
fn a_held_near_call_write_does_not_interrupt_capture_privacy_or_stop() {
    let _one = one_session();
    let (mut trace, held) = held_onboarding_storage_at(true);
    let run = run_in(Script::question(Answer::Complete(12)), &mut trace);
    assert_held_storage_progress(&run, &trace, &held);
    assert!(is_mute(&run.result), "{:?}", run.result);
    release_onboarding_storage(&mut trace, &held);
    for action in [Act::Sigterm, Act::Mute] {
        let (mut trace, held) = held_onboarding_storage_at(true);
        let run = run_in(
            Script {
                actions: vec![(When::Block(40), action)],
                signals: action == Act::Sigterm,
                ..Script::question(Answer::Hold(12))
            },
            &mut trace,
        );
        assert_held_storage_progress(&run, &trace, &held);
        if action == Act::Sigterm {
            assert!(run.result.is_ok(), "{:?}", run.result);
        } else {
            assert_eq!(run.events("physical_mute").len(), 1);
        }
        release_onboarding_storage(&mut trace, &held);
    }
}
