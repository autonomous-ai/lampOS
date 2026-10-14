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
    thread,
};

/// The peers keep production freshness bounds (50 ms privacy, 100 ms
/// capture leases). Threaded scenarios poll every millisecond; explicitly
/// stepped scenarios advance only after every peer has run.
/// Serialize sessions because SIGTERM is process-wide.
static ONE_SESSION: Mutex<()> = Mutex::new(());

fn one_session() -> std::sync::MutexGuard<'static, ()> {
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
    /// Real SIGTERM to this process, under an installed `Termination`.
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
}

/// How the speaker peer behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SpeakerMode {
    Healthy,
    StopOnly,
    Installing,
    Partial,
}

/// Single-cycle policy scenarios use synchronous steps and scoped time without
/// changing production freshness limits or the real process cleanup deadline.
/// Cross-cycle privacy waits and the stalled metadata writer exercise real
/// asynchronous dependencies, so those scenarios explicitly keep peer threads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fixture {
    Threaded,
    Stepped,
}

#[derive(Clone, Debug)]
struct Script {
    fixture: Fixture,
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
}

impl Script {
    /// One user question, then the given answer, then a mute once the
    /// coordinator has settled the turn. Every caller chooses its clock;
    /// no fixture silently changes between host and scenario time.
    fn question(fixture: Fixture, answer: Answer) -> Self {
        Self {
            fixture,
            speech: 20..80,
            answers: vec![answer],
            actions: vec![(When::Settled(1), Act::Mute)],
            notices: false,
            signals: false,
            turn_taking: TurnTaking::V1Baseline,
            cleanup_fails: false,
            ring: false,
            camera: false,
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
}

/// Packet, privacy and policy timestamps use the selected fixture clock.
/// `elapsed`, watchdogs and child exit observations always use real host time.
struct Run {
    fixture: Fixture,
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
    /// Before the first child was spawned, and after every child was reaped.
    /// Stepped runs use scenario time after startup, never elapsed host time.
    started_us: u64,
    finished_us: u64,
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
            "fixture {:?}, packet and policy clock {}, real conversation and cleanup {:?}\nresult {:?}\nkinds {kinds:?}\npeer privacy gap {} us, refused writes {}, end-to-speaker revocation {:?} us\nframes {} starts {:?} ends {:?} chunks {} accepted {} fired {:?} stopped {:?} abandoned {:?}",
            self.fixture,
            match self.fixture {
                Fixture::Threaded => "host",
                Fixture::Stepped => "logical",
            },
            self.elapsed,
            self.result.as_ref().map_err(|e| e.to_string()),
            self.seen.privacy_gap_us,
            self.seen.chunks.iter().filter(|c| !c.accepted).count(),
            self.revocations(),
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
        // The policy bound, in the selected fixture clock: once the
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
            crate::scenario_clock::wait_child_exited(self.child_pid, deadline)?;
        }
        Ok(())
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
    fresh_mute_pending: bool,
    capture_started: bool,
    capture_silent: bool,
    camera: Option<BootId>,
    camera_started: bool,
    camera_silent: bool,
    next_camera: Instant,
    speaker: SpeakerMode,
    provider_gone: bool,
    speech_offset: Option<u64>,
    next_capture: Instant,
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
    fn fire(&mut self, act: Act, deadline: Option<Instant>) -> io::Result<()> {
        self.seen.lock().unwrap().fired.push((act, monotonic_us()));
        match act {
            Act::Mute => self.muted.store(true, Ordering::SeqCst),
            Act::ExpirePrivacyThenMute => {
                if self.script.fixture != Fixture::Stepped {
                    return Err(io::Error::other(
                        "privacy expiry fault requires stepped peers",
                    ));
                }
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
                rustix::process::kill_process(
                    rustix::process::getpid(),
                    rustix::process::Signal::TERM,
                )
                .unwrap();
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
            // Consumes control, installs nothing, still sees Stop.
            if let Ok(Some(Control::Stop)) = self.peers[index].channels.control.receive::<Control>()
            {
                self.stop(index, deadline)?;
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
                Ok(Some(Control::StartCapture)) => {
                    self.capture_started = true;
                    let at = monotonic_us();
                    let generation = self.peers[index].snapshot.unwrap().microphone_generation();
                    self.peers[index]
                        .channels
                        .control
                        .send(WorkerEvent::CaptureStarted {
                            epoch: 1,
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
            "capture" if self.capture_started && !self.capture_silent => {
                // Frames follow a fixed 10 ms schedule, like a capture device.
                for _ in 0..64 {
                    if crate::scenario_clock::instant() < self.next_capture {
                        break;
                    }
                    let frame = {
                        let mut seen = self.seen.lock().unwrap();
                        seen.capture_frames += 1;
                        seen.capture_frames
                    };
                    let at = monotonic_us();
                    let speech = self.speech(frame);
                    let peer = &mut self.peers[index];
                    let sent = peer.channels.data.send(MicrophoneFrame {
                        epoch: 1,
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
                    self.next_capture += Duration::from_millis(10);
                    if sent.is_err() {
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
                Ok(Some(ProviderInput::Audio { .. } | ProviderInput::Written { .. })) => {}
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
        if !self.peer.stopped && crate::scenario_clock::instant() >= self.next {
            let at = monotonic_us();
            {
                let mut seen = self.seen.lock().unwrap();
                if let Some(last) = seen.last_privacy_us {
                    seen.privacy_gap_us = seen.privacy_gap_us.max(at - last);
                }
                seen.last_privacy_us = Some(at);
            }
            let _ = self.peer.channels.control.send(WorkerEvent::Privacy {
                acquired_at_us: at,
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

    fn run(mut self, finished: Arc<AtomicBool>) {
        self.next = Instant::now();
        while !finished.load(Ordering::SeqCst) {
            self.tick(None).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        self.peer.close_input(None).unwrap();
    }
}

fn run(script: Script) -> Run {
    run_in(script, &mut SessionState::default())
}

/// One runtime cycle in `trace`, which a service supervisor may share across
/// cycles. The returned run holds only this cycle's events.
fn run_in(script: Script, trace: &mut SessionState) -> Run {
    let first_event = trace.iter().count();
    let started_us = monotonic_us();
    let cleanup_fails = script.cleanup_fails;
    let fixture = script.fixture;
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
        let (worker, channels, input) = match fixture {
            Fixture::Threaded => Worker::cooperative_fixture(&directory.path, role),
            Fixture::Stepped => Worker::cooperative_ready_fixture(&directory.path, role),
        };
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
    let mut rig = Rig {
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        speaker: spawn("speaker"),
        capture: spawn("capture"),
        provider: spawn("provider"),
        privacy: spawn("privacy"),
        ring: ring.into(),
        camera,
        motor: None,
        ring_channel_ceiling,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: script.turn_taking,
        playout_start: PlayoutStart::default(),
        endpoint: None,
    };
    // The clock starts only after every real child has crossed its startup
    // barrier. All coordinator and peer timestamps below share this scope;
    // the real watchdog still bounds both the conversation and its cleanup.
    let mut clock_scope = (fixture == Fixture::Stepped)
        .then(|| crate::scenario_clock::Scope::enter(SCENARIO_LIMIT).unwrap());
    let mut owner = Controller::new(new_boot().unwrap(), now());
    let boot = owner.snapshot(now()).unwrap().boot();
    let mut startup = StartupEvents::default();
    startup
        .cloud
        .observe(ready_link(1, 1), monotonic_us())
        .unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let finished = Arc::new(AtomicBool::new(false));
    let muted = Arc::new(AtomicBool::new(false));
    let privacy_at = peers.iter().position(|p| p.role == "privacy").unwrap();
    let privacy = PrivacyPeer {
        peer: peers.remove(privacy_at),
        seen: seen.clone(),
        muted: muted.clone(),
        next: crate::scenario_clock::instant(),
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
        fresh_mute_pending: false,
        capture_started: false,
        capture_silent: false,
        camera: camera_boot,
        camera_started: false,
        camera_silent: false,
        next_camera: crate::scenario_clock::instant(),
        speaker: SpeakerMode::Healthy,
        provider_gone: false,
        speech_offset: None,
        next_capture: crate::scenario_clock::instant(),
        next_write: crate::scenario_clock::instant(),
        cursor: 0,
        audio_sequence: 0,
        accepted: 0,
        notice_accepted: 0,
        ended: 0,
        settled_after: None,
    };
    let threads = if let Some(scope) = clock_scope.as_mut() {
        let mut privacy = privacy;
        let mut state = state;
        scope
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
        None
    } else {
        // Real-dependency scenarios retain their independent privacy thread:
        // mute/reopen crosses host-clock waits, and the metadata case measures
        // a real asynchronous writer's shutdown/finish behavior.
        let privacy = thread::spawn({
            let finished = finished.clone();
            move || privacy.run(finished)
        });
        let worker = thread::spawn({
            let finished = finished.clone();
            move || {
                let mut state = state;
                // Preserve the threaded fixture's original schedule origin:
                // it begins when this peer thread starts, not at spawn.
                state.guard = BoundaryGuard::new(boot, now());
                state.next_camera = Instant::now();
                state.next_capture = Instant::now();
                state.next_write = Instant::now();
                let started = Instant::now();
                let mut limit_reported = false;
                // Endpoints stay open until the supervisor has finished, as
                // worker sockets do until the process is reaped.
                while !finished.load(Ordering::SeqCst) {
                    if started.elapsed() > SCENARIO_LIMIT && !limit_reported {
                        limit_reported = true;
                        if let Some(capture) = state.peers.iter_mut().find(|p| p.role == "capture")
                        {
                            let _ = capture.channels.control.send(WorkerEvent::Fault {
                                code: "pilot fault scenario limit".into(),
                            });
                        }
                    }
                    state.tick(None).unwrap();
                    thread::sleep(Duration::from_millis(1));
                }
                for peer in &mut state.peers {
                    peer.close_input(None).unwrap();
                }
            }
        });
        Some((worker, privacy))
    };
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
    let finished_us = monotonic_us();
    finished.store(true, Ordering::SeqCst);
    if let Some((worker, privacy)) = threads {
        worker.join().unwrap();
        privacy.join().unwrap();
    }
    drop(clock_scope);
    assert!(reply.is_none(), "an answer outlived its run");
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    Run {
        fixture,
        result,
        trace: trace.iter().skip(first_event).cloned().collect(),
        boot,
        started_us,
        finished_us,
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

#[test]
fn control_one_question_is_answered_once_then_mute_ends_the_run() {
    let _one = one_session();
    let run = run(Script::question(Fixture::Stepped, Answer::Complete(12)));
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
    let run = run(Script::question(Fixture::Stepped, Answer::Complete(12)));
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
    let run = run(Script::question(Fixture::Stepped, Answer::FailBeforeOutput));
    run.assert_invariants();
    assert!(is_mute(&run.result), "{:?}", run.result);
    assert_eq!(run.outcomes()[&first(&run)], ["provider_failed"]);
    assert!(run.seen.chunks.is_empty());
    assert!(run.events("speaker_first_write").is_empty());
}

#[test]
fn provider_failure_during_output_never_becomes_a_completed_answer() {
    let _one = one_session();
    let run = run(Script::question(
        Fixture::Stepped,
        Answer::FailDuringOutput(12),
    ));
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
        ..Script::question(Fixture::Stepped, Answer::FailBeforeOutput)
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
    let run = run(Script::question(Fixture::Stepped, Answer::Hold(40))
        .then(When::Accepted(3), Act::ProviderGone));
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
    let run = run(Script::question(Fixture::Stepped, Answer::Hold(40))
        .then(When::Accepted(3), Act::CaptureFault));
    run.assert_invariants();
    assert!(
        run.result.is_err() && !is_mute(&run.result),
        "{:?}",
        run.result
    );
    assert_eq!(run.outcomes()[&first(&run)], ["runtime_failed"]);
}

#[test]
fn capture_stall_during_a_question_ends_the_run_without_an_answer() {
    let _one = one_session();
    let run = run(Script {
        speech: 20..400,
        ..Script::question(Fixture::Stepped, Answer::Complete(12))
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
        ..Script::question(Fixture::Stepped, Answer::Complete(40))
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
        ..Script::question(Fixture::Stepped, Answer::Hold(40))
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
        ..Script::question(Fixture::Stepped, Answer::FailBeforeOutput)
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
    let run = run(Script::question(Fixture::Stepped, Answer::Complete(40))
        .then(When::Accepted(3), Act::StallSpeaker));
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
        ..Script::question(Fixture::Stepped, Answer::Complete(40))
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
        ..Script::question(Fixture::Stepped, Answer::FailBeforeOutput)
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
    let run = run(Script::question(Fixture::Stepped, Answer::Hold(40))
        .then(When::Accepted(3), Act::ProviderFault));
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
    let run = run(Script::question(Fixture::Stepped, Answer::Hold(40))
        .then(When::Accepted(3), Act::ProviderTerminal));
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
        ..Script::question(Fixture::Stepped, Answer::FailBeforeOutput)
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
        ..Script::question(Fixture::Stepped, Answer::FailBeforeOutput)
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
        ..Script::question(Fixture::Stepped, Answer::Complete(12))
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
        ..Script::question(Fixture::Stepped, Answer::Hold(12))
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

#[test]
fn ring_lost_without_blanking_keeps_the_answer_but_cleanup_fails_truthfully() {
    let _one = one_session();
    let run = run(Script {
        ring: true,
        cleanup_fails: true,
        ..Script::question(Fixture::Stepped, Answer::Complete(12))
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
        ..Script::question(Fixture::Stepped, Answer::Complete(12))
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
                    ..Script::question(Fixture::Threaded, Answer::Complete(40))
                },
                trace,
            );
            let mute = run
                .result
                .as_ref()
                .err()
                .and_then(|e| e.downcast_ref::<PhysicalMute>())
                .map(|mute| mute.observed_at_us);
            let result = match mute {
                Some(observed_at_us) => Err(Box::new(PhysicalMute { observed_at_us }) as _),
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

/// Metadata storage stalls from the first event (a stuck disk). Answer audio
/// must keep flowing, a real SIGTERM must still stop the runtime cleanly, and
/// the unacknowledged metadata must be reported within the finish bound
/// rather than hidden or waited on.
#[test]
fn sigterm_while_metadata_storage_is_stalled_stops_cleanly_and_reports_the_loss() {
    use crate::service_trace::{FINISH_WAIT, ServiceTrace};
    let _one = one_session();
    let (writer, entered, release) = ServiceTrace::stalled();
    let mut trace = SessionState::with_writer(writer);
    let run = run_in(
        Script {
            actions: vec![(When::Accepted(10), Act::Sigterm)],
            signals: true,
            ..Script::question(Fixture::Threaded, Answer::Hold(40))
        },
        &mut trace,
    );
    // The writer really was stalled during the run.
    entered.try_recv().expect("metadata writer never stalled");
    run.assert_invariants();
    assert!(run.result.is_ok(), "{:?}", run.result);
    // Ten chunks were accepted while every metadata write was blocked.
    assert!(run.seen.chunks.iter().filter(|c| c.accepted).count() >= 10);
    let stopped = run.fired_at(Act::Sigterm);
    let shutdown_ms = (run.finished_us - stopped) / 1_000;
    let finishing = Instant::now();
    let report = trace.finish_logging().expect("service metadata report");
    let finish_wait = finishing.elapsed();
    release.send(()).unwrap();
    eprintln!("sigterm to reaped {shutdown_ms} ms; metadata finish {finish_wait:?}; {report:?}");
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
    let run = run(Script::question(Fixture::Stepped, Answer::Complete(40))
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
        ..Script::question(Fixture::Stepped, Answer::Complete(40))
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
    let run = run(Script::question(Fixture::Stepped, Answer::Complete(40))
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
        ..Script::question(Fixture::Stepped, Answer::Complete(40))
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
        ..Script::question(Fixture::Stepped, Answer::Complete(12))
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
        ..Script::question(Fixture::Stepped, Answer::Complete(40))
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
