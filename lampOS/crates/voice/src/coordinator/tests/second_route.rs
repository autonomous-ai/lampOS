//! The second voice route through the production coordinator (lampOS#754).
//!
//! Every test runs the real `conversation()` service loop and
//! `finish_active_run` with cooperative child processes and real local IPC,
//! as `pilot_faults` does. The peers (capture, the speaker with the
//! production `BoundaryGuard`, the ring, the Live provider, the second
//! route and privacy) run in the test thread under the stepped scenario
//! clock (`crate::scenario_clock`), as `pilot_faults`' stepped runs do, so
//! a busy host cannot starve a peer past a freshness bound (lampOS#890).
//! The Live peer answers with
//! samples of 1,000 and the second route's with 2,000, so the speaker tells
//! whose answer it played.
//!
//! Nothing here is a device, a cloud service or an acoustic measurement.
use super::*;
use crate::{
    provider_flow::OutputSender,
    provider_link::ProviderLinkState,
    provider_worker::{FailedStage, Fault},
    ring_wire::{RingBlankReason, RingFeedback, RingFrame, RingPhase},
    wire::{CaptureTiming, ReferenceTiming},
};
use lamp_audio::ownership::PlaybackCursor;
use lamp_turn_owner::BoundaryGuard;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const PACKET: usize = crate::provider_flow::PACKET_SAMPLES;
const LIVE: i16 = 1_000;
const SECOND: i16 = 2_000;
/// The second route's wait cue, played before its words.
const CUE: i16 = 500;
const LIMIT: Duration = Duration::from_secs(12);
/// The clock the run's `ExpressionOut` stamps with (`Script::expression`).
const EXPRESSION_CLOCK: &str = "second-route-boot";

/// What the Live peer does with one request, by Start order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Live {
    /// `packets` of audio at End, then completion.
    Answer(usize),
    /// TurnFailed(AwaitingResponse, fault) at End, before any audio.
    Fail(Fault),
    /// As `Fail`, then `packets` of audio for the same request: too late.
    FailThenLate(Fault, usize),
    /// The link becomes unavailable during the line, at capture frame 50.
    DropLink,
    /// As `DropLink` (at `link_drops_at` if set), and the line given to it
    /// again on its new session is answered with `packets`: a reconnect.
    Reconnect(usize),
    /// A skill's tool answered with a look (#941) in this session, then
    /// `packets` of audio.
    Look(usize, u64),
    /// A skill's tool answered with a look, then the answer opens with "...".
    LookSilent,
    /// A skill's tool answered with a look, then the turn is unclear.
    LookUnclear,
    /// The turn is unclear (heard as another language, #1417): no audio,
    /// then completion.
    Unclear,
    /// As `Unclear`, said of a cut-in over the lamp's answer (#828).
    UnclearCutIn,
    /// A skill's tool answered with a look, then as `FailThenLate`: the
    /// second route takes the turn.
    LookFail(Fault, usize),
    /// `packets` of a holding phrase ("One moment."), then `HoldingAnswer`
    /// for session 1 before completion (`LAMP_HOLDING_ANSWER`).
    Holding(usize),
}

/// How the Live route answers a turn the lamp wrote itself (a "Sorry?").
#[derive(Clone, Copy, Debug, PartialEq)]
enum Written {
    /// Not at all, as before #1421.
    Ignore,
    /// Started, then completion with no audio (#1417's empty "Sorry?").
    Silent,
    /// Started, the words "...", then completion: nothing said.
    Dots,
    /// Started, `packets` of audio, then completion: said.
    Said(usize),
}

/// How the second route, alone, answers a look's turn or the look's note
/// (`LAMP_SECOND_LOOK_NOTE`): as its engine tells them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LookAnswer {
    /// Calls `look_for` for the next place: `LookAsked`, before its audio,
    /// dropped as the engine drops it in a turn that is no search's.
    Next,
    /// Calls nothing: `LookNoCall`, after its audio.
    Nothing,
    /// Calls something else (a found place, another tool): neither.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum When {
    /// This many turns have been settled by the coordinator.
    Settled(usize),
    /// The speaker has accepted this many chunks.
    Accepted(usize),
}

#[derive(Clone)]
struct Script {
    second: bool,
    /// `LAMP_VOICE_ROUTE=second`: the second route alone, in `rig.provider`,
    /// with no Gemini worker (`second` is then not read).
    only: bool,
    live: Vec<Live>,
    /// Packets in each of the second route's answers.
    second_packets: usize,
    /// The second route's answers open with these words.
    second_says: Option<&'static str>,
    /// The second route's first answer comes after a skill's look step
    /// (`LAMP_SECOND_SKILL_LOOK`, #941): `LookAsked` for the session that
    /// began it, before its audio.
    second_looks: bool,
    /// `second_looks`' look is the route's own `take_a_look`, no skill's
    /// search.
    own_look: bool,
    /// Alone, how the second route answers each look's turn in order, then
    /// `Other`.
    look_answers: &'static [LookAnswer],
    /// Alone, how the second route answers the look's note.
    note_answer: LookAnswer,
    /// Packets of a wait cue the second route plays before its words.
    second_cue: usize,
    /// Speech starts again (a new turn, or speaking over an answer).
    speak_again: Option<When>,
    /// A murmur of two capture frames, too short to be a turn: the room is
    /// not quiet for a second after it.
    murmur: Option<When>,
    /// A hand Stop (the button's tap) comes through Controls this many
    /// capture frames after the murmur.
    hand_stop: Option<u64>,
    /// Privacy reports muted once this many turns are settled.
    turns: usize,
    /// The Live link is Ready again, with a new session, once this many
    /// turns are settled.
    live_back_after: Option<usize>,
    /// Gemini's link is not Ready at startup: it is Ready at this capture
    /// frame (10 ms each), or never.
    live_late: Option<Option<u64>>,
    /// The second route's worker exits as this many of its inputs arrive
    /// (1: on its Start) (#960).
    second_dies: Option<usize>,
    /// The standby's worker exits at this capture frame (#960).
    standby_dies_at: Option<u64>,
    /// Privacy reports muted a second after the second route's worker
    /// exited, instead of after `turns`.
    mute_after_second_dies: bool,
    /// The Live link is Ready again, with a new session, at this capture
    /// frame, whatever turn is in hand.
    live_back_at_frame: Option<u64>,
    /// The Live worker exits at this capture frame, with no report.
    live_dies_at: Option<u64>,
    /// The capture frame a `DropLink` or `Reconnect` link drops at (50).
    link_drops_at: Option<u64>,
    turn_taking: TurnTaking,
    /// How a written turn is answered.
    written: Written,
    /// Privacy reports muted at this capture frame, whatever has settled:
    /// for a run whose turns go on past the person's (the lamp's own).
    mute_at_frame: Option<u64>,
    /// The second route's answer packet by packet: its sample value, so its
    /// level rises and falls as speech does (#933). `None`: `SECOND`.
    second_shape: Option<fn(usize) -> i16>,
    /// Give the voice an `ExpressionOut` on a bus of the run's own, and keep
    /// what it says for Expression (`voice/progress`, `voice/sound`).
    expression: bool,
    /// The second route answers nothing at End: its chat is still
    /// streaming (#1620 on the second route).
    second_holds: bool,
    /// The second route fails each turn before any audio, as its worker
    /// does when its speech to text or chat cannot be reached.
    second_fails: Option<Fault>,
    /// This peer reads Stop and keeps its process running (#1620).
    ignores_stop: Option<&'static str>,
    /// The scenario's own bound, past which capture reports a fault.
    limit: Duration,
}

impl Script {
    fn question(live: Live) -> Self {
        Self {
            second: true,
            only: false,
            live: vec![live],
            second_packets: 12,
            second_says: None,
            second_looks: false,
            own_look: false,
            look_answers: &[],
            note_answer: LookAnswer::Other,
            second_cue: 0,
            speak_again: None,
            murmur: None,
            hand_stop: None,
            turns: 1,
            live_back_after: None,
            live_late: None,
            second_dies: None,
            standby_dies_at: None,
            mute_after_second_dies: false,
            live_back_at_frame: None,
            live_dies_at: None,
            link_drops_at: None,
            turn_taking: TurnTaking::V1Baseline,
            written: Written::Ignore,
            mute_at_frame: None,
            second_shape: None,
            expression: false,
            second_holds: false,
            second_fails: None,
            ignores_stop: None,
            limit: LIMIT,
        }
    }
}

#[derive(Clone, Debug)]
struct Chunk {
    turn: u64,
    value: i16,
    accepted: bool,
}

#[derive(Default)]
struct Seen {
    live: Vec<ProviderInput>,
    second: Vec<ProviderInput>,
    chunks: Vec<Chunk>,
    /// Owner and microphone of every authority the speaker's guard took.
    installs: Vec<(Option<u64>, bool)>,
    phases: Vec<RingPhase>,
    stopped: Vec<&'static str>,
}

struct Run {
    result: Result<()>,
    trace: Vec<Value>,
    seen: Seen,
    exits: Vec<(&'static str, bool)>,
    /// What the voice said for Expression, as Expression takes it, in the
    /// order it was said (`Script::expression`).
    expression: Vec<lamp_expression::Input>,
}

fn dedup<T: PartialEq + Clone>(items: &[T]) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for item in items {
        if out.last() != Some(item) {
            out.push(item.clone());
        }
    }
    out
}

impl Run {
    fn events(&self, kind: &str) -> Vec<&Value> {
        self.trace.iter().filter(|e| e["kind"] == kind).collect()
    }
    fn finished(&self, turn: u64) -> &Value {
        self.events("turn_finished")
            .into_iter()
            .find(|e| e["turn"] == turn)
            .unwrap_or_else(|| panic!("turn {turn} never finished: {}", self.summary()))
    }
    fn played(&self, turn: u64) -> Vec<i16> {
        self.seen
            .chunks
            .iter()
            .filter(|c| c.turn == turn && c.accepted)
            .map(|c| c.value)
            .collect()
    }
    fn summary(&self) -> String {
        let kinds: Vec<&str> = self
            .trace
            .iter()
            .map(|e| e["kind"].as_str().unwrap_or("?"))
            .collect();
        format!(
            "result {:?}\nkinds {:?}\nphases {:?}\ninstalls {:?}",
            self.result.as_ref().map_err(|e| e.to_string()),
            dedup(&kinds),
            dedup(&self.seen.phases),
            dedup(&self.seen.installs),
        )
    }
    /// What every run keeps: it ended by the scripted mute, every child saw
    /// Stop and was reaped, and the speaker wrote nothing without authority.
    fn assert_clean(&self) {
        eprintln!("{}", self.summary());
        assert!(
            self.result
                .as_ref()
                .err()
                .is_some_and(|e| e.downcast_ref::<PhysicalMute>().is_some()),
            "{}",
            self.summary()
        );
        for (role, exited) in &self.exits {
            assert!(exited, "{role} child was not reaped");
            assert!(self.seen.stopped.contains(role), "{role} never saw Stop");
        }
        assert!(self.events("worker_forced_shutdown").is_empty());
    }
}

/// Start, Audio 1..=n in order, End: one whole line.
fn whole_line(inputs: &[ProviderInput], request: u64) -> usize {
    let mine: Vec<_> = inputs
        .iter()
        .filter(|input| request_of(input) == request)
        .collect();
    assert!(
        matches!(mine.first(), Some(ProviderInput::Start { .. })),
        "{mine:?}"
    );
    assert!(
        matches!(mine.last(), Some(ProviderInput::End { .. })),
        "{mine:?}"
    );
    for (index, input) in mine[1..mine.len() - 1].iter().enumerate() {
        match input {
            ProviderInput::Audio { sequence, .. } => assert_eq!(*sequence, index as u64 + 1),
            other => panic!("{other:?} inside a line"),
        }
    }
    mine.len() - 2
}

fn request_of(input: &ProviderInput) -> u64 {
    match input {
        ProviderInput::Start { request, .. }
        | ProviderInput::Audio { request, .. }
        | ProviderInput::End { request, .. }
        | ProviderInput::Called { request, .. }
        | ProviderInput::Paused { request, .. }
        | ProviderInput::Written { request, .. }
        | ProviderInput::Reminder { request, .. } => *request,
    }
}

fn audio_count(inputs: &[ProviderInput], request: u64) -> usize {
    inputs
        .iter()
        .filter(|input| matches!(input, ProviderInput::Audio { request: r, .. } if *r == request))
        .count()
}

struct Peer {
    role: &'static str,
    channels: WorkerChannels,
    input: Option<std::process::ChildStdin>,
    child_pid: u32,
    stopped: bool,
    output: OutputSender,
    queued: VecDeque<ProviderOutput>,
    snapshot: Option<Snapshot>,
}

impl Peer {
    /// The child exits once its input closes; in a shutdown step it is
    /// waited for, as logical time does not pass there.
    fn close_input(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        if self.input.take().is_some()
            && let Some(deadline) = deadline
        {
            // Bounded by the scope's own watchdog, not the cleanup's real
            // deadline, which a busy host can use up first (#1235's review).
            let bound = crate::scenario_clock::host_deadline().unwrap_or(deadline);
            crate::scenario_clock::wait_child_exited(self.child_pid, bound)?;
        }
        Ok(())
    }
}

struct Peers {
    script: Script,
    peers: Vec<Peer>,
    seen: Arc<Mutex<Seen>>,
    muted: Arc<AtomicBool>,
    guard: BoundaryGuard,
    capture_started: bool,
    frames: u64,
    next_capture: Instant,
    next_write: Instant,
    cursor: u64,
    accepted: usize,
    speech_offset: Option<u64>,
    /// Requests by Live Start order.
    live_requests: Vec<u64>,
    ended: BTreeSet<u64>,
    settled: usize,
    audio_sequence: u64,
    link_dropped: bool,
    link_back: bool,
    live_ready_sent: bool,
    /// The capture frame at which the second route's worker exited.
    second_died_at: Option<u64>,
    /// The session of the second route's last Start.
    second_session: u64,
    /// The second route has asked its look (`Script::second_looks`).
    second_looked: bool,
    /// Look's turns the second route has answered (`Script::look_answers`).
    look_turns: usize,
    /// The capture frame the murmur began at (`Script::murmur`).
    murmur_at: Option<u64>,
    /// Controls' queue into the voice's hand controls (`Script::hand_stop`).
    hand: Option<crate::hand_controls::TestControls>,
}

impl Peers {
    fn index(&self, role: &str) -> Option<usize> {
        self.peers.iter().position(|p| p.role == role)
    }

    fn speech(&self, frame: u64) -> bool {
        (20..80).contains(&frame)
            || self
                .speech_offset
                .is_some_and(|offset| (offset..offset + 60).contains(&frame))
            || self
                .murmur_at
                .is_some_and(|at| (at..at + 2).contains(&frame))
    }

    fn tick(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        if let Some(when) = self.script.speak_again {
            let due = match when {
                When::Settled(count) => self.settled >= count,
                When::Accepted(count) => self.accepted >= count,
            };
            if due {
                self.script.speak_again = None;
                self.speech_offset = Some(self.frames);
            }
        }
        if let Some(when) = self.script.murmur {
            let due = match when {
                When::Settled(count) => self.settled >= count,
                When::Accepted(count) => self.accepted >= count,
            };
            if due {
                self.script.murmur = None;
                self.murmur_at = Some(self.frames);
            }
        }
        if let (Some(after), Some(at)) = (self.script.hand_stop, self.murmur_at)
            && self.frames >= at + after
            && let Some(hand) = self.hand.as_ref()
        {
            self.script.hand_stop = None;
            hand.send(hand_stop_state(monotonic_us()));
        }
        // A voice still serving a second after its second route died can
        // be muted then, which ends the run cleanly.
        if self
            .script
            .mute_at_frame
            .is_some_and(|frame| self.frames >= frame)
        {
            self.muted.store(true, Ordering::SeqCst);
        }
        if self.script.mute_at_frame.is_none() && self.settled >= self.script.turns
            || (self.script.mute_after_second_dies
                && self
                    .second_died_at
                    .is_some_and(|frame| self.frames >= frame + 100))
        {
            self.muted.store(true, Ordering::SeqCst);
        }
        let live_back_due = self
            .script
            .live_back_after
            .is_some_and(|after| self.settled >= after)
            || self
                .script
                .live_back_at_frame
                .is_some_and(|frame| self.frames >= frame);
        if live_back_due
            && self.link_dropped
            && !self.link_back
            && let Some(index) = self.index("provider")
        {
            self.link_back = true;
            let _ = self.peers[index]
                .channels
                .control
                .send(WorkerEvent::ProviderLink {
                    link: ready_link(3, 2),
                });
        }
        if let Some(Some(frame)) = self.script.live_late
            && self.frames >= frame
            && !self.live_ready_sent
            && let Some(index) = self.index("provider")
        {
            self.live_ready_sent = true;
            let _ = self.peers[index]
                .channels
                .control
                .send(WorkerEvent::ProviderLink {
                    link: ready_link(1, 1),
                });
        }
        if self
            .script
            .live_dies_at
            .is_some_and(|frame| self.frames >= frame)
            && let Some(index) = self.index("provider")
            && !self.peers[index].stopped
        {
            let peer = &mut self.peers[index];
            peer.stopped = true;
            peer.input.take();
        }
        if self
            .script
            .standby_dies_at
            .is_some_and(|frame| self.frames >= frame)
            && let Some(index) = self.index("second")
            && !self.peers[index].stopped
        {
            let peer = &mut self.peers[index];
            peer.stopped = true;
            peer.input.take();
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
        for _ in 0..16 {
            match self.peers[index].channels.control.receive::<Control>() {
                Ok(Some(Control::Authority { snapshot })) => {
                    self.peers[index].snapshot = Some(snapshot);
                    if role == "speaker" {
                        self.install(snapshot);
                    }
                    if snapshot.owner().is_none() {
                        self.settled = self.settled.max(self.ended.len());
                    }
                }
                Ok(Some(Control::ProviderOutputCapacity { through })) => {
                    self.peers[index].output.grant(through).unwrap()
                }
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
                Ok(Some(Control::Stop)) if self.script.ignores_stop == Some(role) => {
                    // Read, and its process goes on: only a kill ends it.
                    self.peers[index].stopped = true;
                    self.seen.lock().unwrap().stopped.push(role);
                    return Ok(());
                }
                Ok(Some(Control::Stop)) => {
                    if role == "ring" {
                        let at = monotonic_us();
                        let _ = self.peers[index].channels.control.send(WorkerEvent::Ring {
                            report: RingFeedback::Blanked {
                                reason: RingBlankReason::Shutdown,
                                write_started_at_us: at,
                                write_finished_at_us: at,
                            },
                        });
                    }
                    let peer = &mut self.peers[index];
                    peer.stopped = true;
                    peer.close_input(deadline)?;
                    self.seen.lock().unwrap().stopped.push(role);
                    return Ok(());
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
        match role {
            "capture" if self.capture_started => self.tick_capture(index),
            "speaker" => self.tick_speaker(index),
            "provider" | "second" => self.tick_provider(index),
            "ring" => {
                while let Ok(Some(frame)) = self.peers[index].channels.data.receive::<RingFrame>() {
                    self.seen.lock().unwrap().phases.push(frame.phase);
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

    fn tick_capture(&mut self, index: usize) {
        while crate::scenario_clock::instant() >= self.next_capture {
            self.frames += 1;
            let frame = self.frames;
            if frame == self.script.link_drops_at.unwrap_or(50)
                && !self.link_dropped
                && matches!(
                    self.script.live.first(),
                    Some(Live::DropLink | Live::Reconnect(_))
                )
                && let Some(live) = self.index("provider")
            {
                self.link_dropped = true;
                let link = ProviderLink {
                    revision: 2,
                    session: 1,
                    observed_at_us: monotonic_us(),
                    source_clock: None,
                    state: ProviderLinkState::Unavailable {
                        reason: Fault::Transport,
                    },
                };
                let _ = self.peers[live]
                    .channels
                    .control
                    .send(WorkerEvent::ProviderLink { link });
            }
            let at = monotonic_us();
            let speech = self.speech(frame);
            let peer = &mut self.peers[index];
            let sent = peer.channels.data.send(MicrophoneFrame {
                raw: None,
                linear: None,
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

    fn audio(&mut self, index: usize, request: u64, packets: usize, value: i16) {
        for _ in 0..packets {
            self.audio_sequence += 1;
            self.peers[index].queued.push_back(ProviderOutput::Audio {
                request,
                sequence: self.audio_sequence,
                provider_event_at_us: monotonic_us(),
                source_frames: PACKET,
                source_offset: 0,
                samples: vec![value; PACKET],
            });
        }
    }

    fn finish(&mut self, index: usize, request: u64) {
        let queued = &mut self.peers[index].queued;
        queued.push_back(ProviderOutput::GenerationComplete { request });
        queued.push_back(ProviderOutput::TurnComplete {
            request,
            idle: true,
        });
    }

    fn tick_provider(&mut self, index: usize) {
        let live = self.peers[index].role == "provider";
        for _ in 0..64 {
            let Ok(Some(input)) = self.peers[index].channels.data.receive::<ProviderInput>() else {
                break;
            };
            if !live
                && self
                    .script
                    .second_dies
                    .is_some_and(|after| self.seen.lock().unwrap().second.len() + 1 >= after)
            {
                // Its process exits; it reads and says nothing more.
                self.second_died_at.get_or_insert(self.frames);
                let peer = &mut self.peers[index];
                peer.stopped = true;
                peer.input.take();
                return;
            }
            {
                let mut seen = self.seen.lock().unwrap();
                let log = if live {
                    &mut seen.live
                } else {
                    &mut seen.second
                };
                log.push(input.clone());
            }
            match input {
                ProviderInput::Start {
                    request, session, ..
                } => {
                    // Alone, the second route's requests are the script's.
                    if live || self.script.only {
                        self.live_requests.push(request);
                    }
                    if !live {
                        self.second_session = session;
                    }
                    self.peers[index].queued.push_back(ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    });
                }
                // Alone, the second route answers the lamp's own turns as
                // the Live route would, with its own samples.
                ProviderInput::Written {
                    request,
                    session,
                    text,
                    look,
                    search,
                    ..
                } if live || self.script.only => {
                    let queued = &mut self.peers[index].queued;
                    let started = ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    };
                    match self.script.written {
                        Written::Ignore => {}
                        Written::Silent => {
                            queued.push_back(started);
                            self.finish(index, request);
                        }
                        Written::Dots => {
                            queued.push_back(started);
                            queued.push_back(ProviderOutput::Transcript {
                                request: Some(request),
                                text: "...".into(),
                                finished: true,
                            });
                            self.finish(index, request);
                        }
                        Written::Said(packets) => {
                            queued.push_back(started);
                            // Alone, a look's turn or the look's note is
                            // answered as the script says.
                            let answer = if live {
                                None
                            } else if look {
                                self.look_turns += 1;
                                Some(
                                    self.script
                                        .look_answers
                                        .get(self.look_turns - 1)
                                        .copied()
                                        .unwrap_or(LookAnswer::Other),
                                )
                            } else if text == super::look_note::NOTE {
                                Some(self.script.note_answer)
                            } else {
                                None
                            };
                            // As the engine: the skill's next look is taken
                            // only in a search's own turn, and dropped in any
                            // other the lamp wrote (#1146).
                            if answer == Some(LookAnswer::Next) {
                                self.peers[index]
                                    .queued
                                    .push_back(ProviderOutput::LookAsked {
                                        request,
                                        session,
                                        dropped: (!search).then(|| {
                                            "the turn is one the lamp wrote itself".to_owned()
                                        }),
                                        search: true,
                                    });
                            }
                            self.audio(index, request, packets, if live { LIVE } else { SECOND });
                            if answer == Some(LookAnswer::Nothing) {
                                self.peers[index]
                                    .queued
                                    .push_back(ProviderOutput::LookNoCall { request, session });
                            }
                            self.finish(index, request);
                        }
                    }
                }
                ProviderInput::End { request, .. } => {
                    self.ended.insert(request);
                    if !live && self.script.second_holds {
                        continue;
                    }
                    if !live && let Some(reason) = self.script.second_fails {
                        self.peers[index]
                            .queued
                            .push_back(ProviderOutput::TurnFailed {
                                request,
                                stage: FailedStage::AwaitingResponse,
                                reason,
                            });
                        continue;
                    }
                    // Alone, a holding phrase is answered as the second worker
                    // answers one (the holding answers, #1722).
                    if !live
                        && self.script.only
                        && let Some(Live::Holding(packets)) = self
                            .live_requests
                            .iter()
                            .position(|&r| r == request)
                            .and_then(|p| self.script.live.get(p).copied())
                    {
                        self.audio(index, request, packets, SECOND);
                        self.peers[index]
                            .queued
                            .push_back(ProviderOutput::HoldingAnswer {
                                request,
                                session: 1,
                            });
                        self.finish(index, request);
                        continue;
                    }
                    if !live {
                        // Alone, a line the script makes unclear is held
                        // as the second worker holds one (the hums, #828).
                        let unclear = self
                            .live_requests
                            .iter()
                            .position(|&r| r == request)
                            .and_then(|p| self.script.live.get(p).copied())
                            .filter(|answer| {
                                self.script.only
                                    && matches!(answer, Live::Unclear | Live::UnclearCutIn)
                            });
                        if let Some(answer) = unclear {
                            self.peers[index].queued.push_back(ProviderOutput::Unclear {
                                request,
                                why: "fixture".into(),
                                cut_in: answer == Live::UnclearCutIn,
                            });
                            self.finish(index, request);
                            continue;
                        }
                        if self.script.second_looks
                            && !std::mem::replace(&mut self.second_looked, true)
                        {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::LookAsked {
                                    request,
                                    session: self.second_session,
                                    dropped: None,
                                    search: !self.script.own_look,
                                });
                        }
                        // A wait cue first: audio with no words.
                        if self.script.second_cue > 0 {
                            self.audio(index, request, self.script.second_cue, CUE);
                        }
                        if let Some(text) = self.script.second_says {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::Transcript {
                                    request: Some(request),
                                    text: text.into(),
                                    finished: true,
                                });
                        }
                        match self.script.second_shape {
                            Some(shape) => {
                                for packet in 0..self.script.second_packets {
                                    self.audio(index, request, 1, shape(packet));
                                }
                            }
                            None => self.audio(index, request, self.script.second_packets, SECOND),
                        }
                        self.finish(index, request);
                        continue;
                    }
                    let position = self.live_requests.iter().position(|&r| r == request);
                    // After the scripted ones, the Live route answers.
                    let answer = position
                        .and_then(|p| self.script.live.get(p).copied())
                        .unwrap_or(Live::Answer(12));
                    let fail = |reason| ProviderOutput::TurnFailed {
                        request,
                        stage: FailedStage::AwaitingResponse,
                        reason,
                    };
                    match answer {
                        Live::Answer(packets) => {
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Fail(reason) => self.peers[index].queued.push_back(fail(reason)),
                        Live::FailThenLate(reason, packets) => {
                            self.peers[index].queued.push_back(fail(reason));
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::DropLink => {}
                        Live::Reconnect(packets) => {
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Look(packets, session) => {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::LookAsked {
                                    request,
                                    session,
                                    dropped: None,
                                    search: false,
                                });
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Holding(packets) => {
                            self.audio(index, request, packets, LIVE);
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::HoldingAnswer {
                                    request,
                                    session: 1,
                                });
                            self.finish(index, request);
                        }
                        Live::Unclear | Live::UnclearCutIn => {
                            self.peers[index].queued.push_back(ProviderOutput::Unclear {
                                request,
                                why: "fixture".into(),
                                cut_in: answer == Live::UnclearCutIn,
                            });
                            self.finish(index, request);
                        }
                        Live::LookSilent | Live::LookUnclear | Live::LookFail(..) => {
                            let queued = &mut self.peers[index].queued;
                            queued.push_back(ProviderOutput::LookAsked {
                                request,
                                session: 1,
                                dropped: None,
                                search: false,
                            });
                            match answer {
                                Live::LookSilent => {
                                    queued.push_back(ProviderOutput::Transcript {
                                        request: Some(request),
                                        text: "...".into(),
                                        finished: true,
                                    });
                                    self.finish(index, request);
                                }
                                Live::LookUnclear => {
                                    queued.push_back(ProviderOutput::Unclear {
                                        request,
                                        why: "fixture".into(),
                                        cut_in: false,
                                    });
                                    self.finish(index, request);
                                }
                                Live::LookFail(reason, packets) => {
                                    queued.push_back(fail(reason));
                                    self.audio(index, request, packets, LIVE);
                                    self.finish(index, request);
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let peer = &mut self.peers[index];
        while let Some(event) = peer.queued.front() {
            match peer.output.try_send(|| peer.channels.data.send(event)) {
                Ok(true) => {
                    peer.queued.pop_front();
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    fn tick_speaker(&mut self, index: usize) {
        if crate::scenario_clock::instant() < self.next_write {
            return;
        }
        let Ok(Some(chunk)) = self.peers[index].channels.data.receive::<SpeakerChunk>() else {
            return;
        };
        self.next_write = self
            .next_write
            .max(crate::scenario_clock::instant() - Duration::from_millis(20))
            + Duration::from_micros(chunk.samples.len() as u64 * 1_000 / 24);
        self.install(chunk.snapshot);
        let accepted = self
            .guard
            .check(now(), chunk.permit, OutputKind::Speech)
            .is_ok();
        let owner = chunk.permit.owner();
        self.seen.lock().unwrap().chunks.push(Chunk {
            turn: owner.turn(),
            value: chunk.samples[0],
            accepted,
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
        let first = PlaybackCursor {
            epoch: 1,
            frame: self.cursor,
        };
        self.cursor += chunk.samples.len() as u64;
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
            accepted_frames: chunk.samples.len(),
            observed_at_us: at,
        });
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

    fn install(&mut self, snapshot: Snapshot) {
        if self.guard.install(now(), snapshot).is_ok() {
            self.seen.lock().unwrap().installs.push((
                snapshot.owner().map(|owner| owner.turn()),
                snapshot.microphone_permission() == Permission::Allowed,
            ));
        }
    }
}

struct PrivacyPeer {
    peer: Peer,
    seen: Arc<Mutex<Seen>>,
    muted: Arc<AtomicBool>,
    next: Instant,
}

impl PrivacyPeer {
    /// A fresh observation every 5 ms of scenario time.
    fn tick(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        while !self.peer.stopped
            && let Ok(Some(control)) = self.peer.channels.control.receive::<Control>()
        {
            if matches!(control, Control::Stop) {
                self.peer.stopped = true;
                self.peer.close_input(deadline)?;
                self.seen.lock().unwrap().stopped.push("privacy");
            }
        }
        if !self.peer.stopped && crate::scenario_clock::instant() >= self.next {
            let _ = self.peer.channels.control.send(WorkerEvent::Privacy {
                acquired_at_us: monotonic_us(),
                muted: self.muted.load(Ordering::SeqCst),
            });
            self.next = crate::scenario_clock::instant() + Duration::from_millis(5);
        }
        Ok(())
    }
}

/// The clock the run's hand controls are stamped with (`Script::hand_stop`).
const HAND_CLOCK: &str = "second-route-hand";

/// Controls' state with one tap of the button that stops, at `at_us`.
fn hand_stop_state(at_us: u64) -> lamp_msgs::ControlsState {
    use lamp_msgs::{ClockStamp, ControlGesture, ControlIntent, ControlSource, Gesture, HoldTier};
    let stamp = || ClockStamp {
        clock_id: HAND_CLOCK.into(),
        at_us,
        sequence: 1,
    };
    let state = lamp_msgs::ControlsState {
        stamp: stamp(),
        session_id: "second-route-controls".into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: vec![],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: Some(ControlGesture {
            stamp: stamp(),
            session_id: "second-route-controls".into(),
            source: ControlSource::Button,
            name: "primary".into(),
            gesture: Gesture::Tap,
            intent: ControlIntent::Stop,
            duration_us: 100_000,
            released_us: at_us,
            enabled: true,
        }),
        latest_mute: None,
        event_sequence: 1,
        rejected_inputs: 0,
        privacy: None,
        no_privacy_slide: false,
    };
    state.check().unwrap();
    state
}

fn run(script: Script) -> Run {
    let directory = SessionDirectory::create().unwrap();
    let mut peers = Vec::new();
    let mut spawn = |role| {
        let (worker, channels, input) = Worker::cooperative_ready_fixture(&directory.path, role);
        peers.push(Peer {
            role,
            channels,
            input: Some(input),
            child_pid: worker.fixture_pid(),
            stopped: false,
            output: OutputSender::default(),
            queued: VecDeque::new(),
            snapshot: None,
        });
        worker
    };
    // The workers are the ones the service's routes start (`spawn_rig`).
    let routes = if script.only {
        second::Routes::SecondOnly(PathBuf::from("/etc/second.json"))
    } else {
        second::Routes::Live {
            standby: script.second.then(|| PathBuf::from("/etc/second.json")),
        }
    };
    let workers = routes
        .workers(
            vec!["/etc/provider.json".to_owned()],
            second::RouteFlags::default(),
        )
        .unwrap();
    let mut rig = Rig {
        expression_out: None,
        second: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        faint_level: None,
        speaker: spawn("speaker"),
        capture: spawn("capture"),
        provider: spawn(workers.role),
        privacy: spawn("privacy"),
        privacy_controls: false,
        ring: Some(spawn("ring")).into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: Some(24),
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: script.turn_taking,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
    };
    if workers.standby.is_some() {
        let worker = spawn("second");
        rig.second = Some(second::Route::standing_by(worker, ready_link(1, 1)));
    }
    // What the voice says for Expression, on a bus of the run's own, kept by
    // a thread as it comes so that no subscriber's queue fills.
    let expression = script.expression.then(|| {
        use lamp_msgs::{VoiceProgress, VoiceSound};
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(format!(
            "/tmp/lv-route-expr-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        rig.expression_out = Some(
            crate::expression_out::ExpressionOut::on(&bus, EXPRESSION_CLOCK, "voice-route")
                .unwrap(),
        );
        let progress = bus
            .subscribe_typed(VoiceProgress::TOPIC, "test", VoiceProgress::TYPE)
            .unwrap();
        let sound = bus
            .subscribe_typed(VoiceSound::TOPIC, "test", VoiceSound::TYPE)
            .unwrap();
        let done = Arc::new(AtomicBool::new(false));
        // One thread for each topic, so that neither waits on the other: a
        // Linux socket holds about ten datagrams, and the bus drops one sent
        // to a full queue, so a keeper busy with progress lost sound pieces
        // (Second's read on #1722, 20 Oct 00:01; Eighth's 4 of 6 on Linux).
        let keep = |stop: Arc<AtomicBool>,
                    mut subscriber: lamp_bus::Subscriber,
                    input: fn(lamp_bus::Message) -> lamp_expression::Input| {
            std::thread::spawn(move || {
                let mut kept = Vec::new();
                loop {
                    let stopping = stop.load(Ordering::Acquire);
                    while let Ok(Some(message)) = subscriber.recv(Duration::from_millis(5)) {
                        kept.push(input(message));
                    }
                    if stopping {
                        return kept;
                    }
                }
            })
        };
        let progress = keep(done.clone(), progress, |message| {
            lamp_expression::Input::Progress(message.parse().unwrap())
        });
        let sound = keep(done.clone(), sound, |message| {
            lamp_expression::Input::Sound(message.parse().unwrap())
        });
        let keeper = std::thread::spawn(move || {
            let mut kept = progress.join().unwrap();
            kept.extend(sound.join().unwrap());
            kept
        });
        (root, done, keeper)
    });
    // The clock starts once every child has crossed its startup barrier;
    // the coordinator and the peers share it from here.
    let limit = script.limit;
    let clock_scope = crate::scenario_clock::Scope::enter(limit * 2).unwrap();
    let mut owner = Controller::new(new_boot().unwrap(), now());
    let boot = owner.snapshot(now()).unwrap().boot();
    let mut startup = StartupEvents::default();
    if script.live_late.is_none() {
        startup
            .cloud
            .observe(ready_link(1, 1), monotonic_us())
            .unwrap();
    }
    let seen = Arc::new(Mutex::new(Seen::default()));
    let muted = Arc::new(AtomicBool::new(false));
    let (mut controls, hand) = match script.hand_stop {
        Some(_) => {
            let (controls, hand) =
                HandControls::with_test_sender(HAND_CLOCK, monotonic_us()).unwrap();
            (Some(controls), Some(hand))
        }
        None => (None, None),
    };
    let privacy_at = peers.iter().position(|p| p.role == "privacy").unwrap();
    let mut privacy = PrivacyPeer {
        peer: peers.remove(privacy_at),
        seen: seen.clone(),
        muted: muted.clone(),
        next: crate::scenario_clock::instant(),
    };
    let mut state = Peers {
        script,
        peers,
        seen: seen.clone(),
        muted,
        guard: BoundaryGuard::new(boot, now()),
        capture_started: false,
        frames: 0,
        next_capture: crate::scenario_clock::instant(),
        next_write: crate::scenario_clock::instant(),
        cursor: 0,
        accepted: 0,
        speech_offset: None,
        live_requests: Vec::new(),
        ended: BTreeSet::new(),
        settled: 0,
        audio_sequence: 0,
        link_dropped: false,
        link_back: false,
        live_ready_sent: false,
        second_died_at: None,
        second_session: 0,
        second_looked: false,
        look_turns: 0,
        murmur_at: None,
        hand,
    };
    let started = crate::scenario_clock::instant();
    let mut reported = false;
    clock_scope
        .set_pump(move |phase| {
            // A real child's exit is not simulated: one deadline bounds the
            // children a step stops, and shutdown brings its own budget.
            let deadline = match phase {
                crate::scenario_clock::Phase::Run => Instant::now() + Duration::from_millis(300),
                crate::scenario_clock::Phase::Shutdown { deadline } => deadline,
            };
            if crate::scenario_clock::instant() - started > limit && !reported {
                reported = true;
                if let Some(index) = state.index("capture") {
                    let _ = state.peers[index]
                        .channels
                        .control
                        .send(WorkerEvent::Fault {
                            code: "second route scenario limit".into(),
                        });
                }
            }
            privacy.tick(Some(deadline))?;
            state.tick(Some(deadline))
        })
        .unwrap();
    let mut reply = None;
    let mut trace = SessionState::default();
    let result = conversation_with_controls(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        startup,
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
        &mut controls,
        &mut None,
    );
    let result = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut None, result,
    );
    let mut exits = vec![
        ("speaker", matches!(rig.speaker.try_exit(), Ok(Some(_)))),
        ("capture", matches!(rig.capture.try_exit(), Ok(Some(_)))),
        ("privacy", matches!(rig.privacy.try_exit(), Ok(Some(_)))),
    ];
    for ring in rig.ring.iter_mut() {
        exits.push(("ring", matches!(ring.try_exit(), Ok(Some(_)))));
    }
    // After a switch either route's worker may be `rig.provider`.
    let mut routes = vec![&mut rig.provider];
    routes.extend(rig.second.as_mut().map(second::Route::worker_mut));
    for worker in routes {
        let role = if worker.role == "second" {
            "second"
        } else {
            "provider"
        };
        exits.push((role, matches!(worker.try_exit(), Ok(Some(_)))));
    }
    drop(clock_scope);
    let expression = expression.map_or_else(Vec::new, |(root, done, keeper)| {
        done.store(true, Ordering::Release);
        let kept = keeper.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        kept
    });
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    Run {
        result,
        trace: trace.iter().cloned().collect(),
        seen,
        exits,
        expression,
    }
}

/// The records whose kinds and fields are compared across runs: what a turn
/// leaves, with neither timing nor the route's own records.
fn shape(run: &Run) -> BTreeSet<(String, Vec<String>)> {
    // Nor when an answer began to be heard: on the second route that needs
    // its words, which these peers send only when a script asks
    // (`the_answers_first_audio_is_recorded_after_a_wait_cue_never_at_it`).
    run.trace
        .iter()
        .filter(|e| e["kind"] != "voice_route_switched" && e["kind"] != "answer_first_audio")
        .map(|e| {
            let mut keys: Vec<String> = e.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            (e["kind"].as_str().unwrap().to_owned(), keys)
        })
        .collect()
}

/// The Live packets with their times left out.
fn timeless(inputs: &[ProviderInput]) -> Vec<String> {
    inputs
        .iter()
        .map(|input| match input {
            ProviderInput::Audio {
                request,
                privacy_generation,
                sequence,
                samples,
                ..
            } => format!("audio {request} {privacy_generation} {sequence} {samples:?}"),
            other => format!("{other:?}"),
        })
        .collect()
}

/// (a) Without a second route nothing of the switch is seen, and a route
/// standing by changes nothing while Gemini is well: the Live packets and
/// the kinds and fields of every record are the same.
/// #941, the reviewer's 22:23 P2 and P3 on #1146: a skill call with a look,
/// through the loop, asks the eyes for one still, and only once the turn's
/// first audio is played, as the head lamp-skills sent down is on its way;
/// a plain call asks for none.
#[test]
fn a_tool_s_look_asks_the_eyes_once_its_first_audio_plays_and_a_plain_call_asks_none() {
    let still =
        std::env::temp_dir().join(format!("lamp-voice-loop-look-{}.jpg", std::process::id()));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still));
    let looked = run(Script {
        second: false,
        ..Script::question(Live::Look(12, 1))
    });
    let plain = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    // The first builder's 22:32 P2: a call from a session that is no
    // longer the one ready is dropped, and says so.
    let elsewhere = run(Script {
        second: false,
        ..Script::question(Live::Look(12, 99))
    });
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let at = |run: &Run, kind: &str| {
        run.trace
            .iter()
            .position(|e| e["kind"] == kind)
            .unwrap_or_else(|| panic!("no {kind}"))
    };
    let asks = |run: &Run| {
        run.trace
            .iter()
            .filter(|e| e["kind"] == "look_full_asked")
            .count()
    };
    assert_eq!(asks(&looked), 1);
    assert!(
        looked
            .trace
            .iter()
            .any(|e| e["kind"] == "look_asked" && e["by"] == "tool")
    );
    assert!(at(&looked, "speaker_first_write") < at(&looked, "look_full_asked"));
    assert_eq!(asks(&plain), 0);
    assert!(!plain.trace.iter().any(|e| e["kind"] == "look_asked"));
    let dropped = |run: &Run| {
        run.trace
            .iter()
            .filter(|e| e["kind"] == "look_dropped")
            .map(|e| e["why"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(dropped(&looked).is_empty(), "{:?}", dropped(&looked));
    assert_eq!(
        dropped(&elsewhere),
        ["the session that made the call has gone"]
    );
    assert!(
        !elsewhere
            .trace
            .iter()
            .any(|e| e["kind"] == "look_full_still")
    );
    // The reviewer's 23:47 P2 on #1172: dropped before the eyes are asked.
    assert_eq!(asks(&elsewhere), 0);
}

/// The reviewer's 23:47 P3 on #1172: a tool's look whose turn says nothing,
/// is unclear, or is taken by the second route asks the eyes for nothing,
/// and says why.
#[test]
fn a_tool_s_look_on_an_unsaid_unclear_or_handed_over_turn_asks_the_eyes_nothing() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-dropped-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still));
    let cases = [
        (Live::LookSilent, false, "its answer said nothing"),
        (Live::LookUnclear, false, "its turn is asked again"),
        (
            Live::LookFail(Fault::RateLimited, 6),
            true,
            "the session that made the call has gone",
        ),
    ];
    let runs: Vec<_> = cases
        .iter()
        .map(|(live, second, _)| {
            run(Script {
                second: *second,
                ..Script::question(*live)
            })
        })
        .collect();
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    for ((live, _, why), run) in cases.iter().zip(&runs) {
        let kinds = |kind: &str| run.trace.iter().filter(|e| e["kind"] == kind).count();
        assert_eq!(kinds("look_asked"), 1, "{live:?}");
        assert_eq!(kinds("look_full_asked"), 0, "{live:?}");
        let dropped: Vec<_> = run
            .trace
            .iter()
            .filter(|e| e["kind"] == "look_dropped")
            .map(|e| e["why"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(dropped, [*why], "{live:?}");
    }
}

/// The reviewer's 01:57 P3 on #1172: once a tool's look is dropped because
/// the second route took its turn, the second route's own "Let me take a
/// look." on that turn asks for its own look.
#[test]
fn the_second_route_s_own_phrase_asks_its_look_after_the_tool_s_is_dropped() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-second-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still));
    let run = run(Script {
        second_says: Some("Let me take a look."),
        ..Script::question(Live::LookFail(Fault::RateLimited, 6))
    });
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let asked: Vec<_> = run
        .trace
        .iter()
        .filter(|e| e["kind"] == "look_asked")
        .map(|e| (e["turn"].as_u64(), e["by"].as_str()))
        .collect();
    assert_eq!(asked, [(Some(1), Some("tool")), (Some(1), None)]);
    let dropped: Vec<_> = run
        .trace
        .iter()
        .filter(|e| e["kind"] == "look_dropped")
        .map(|e| e["by"].as_str().unwrap())
        .collect();
    assert_eq!(dropped, ["tool"]);
    assert_eq!(
        run.trace
            .iter()
            .filter(|e| e["kind"] == "look_full_asked")
            .count(),
        1
    );
}

/// A skill's look step on the second route (`LAMP_SECOND_SKILL_LOOK`,
/// #941): the look its worker asks for its own session is taken once the
/// answer's first audio plays, never before it, and its still goes to that
/// session as the `lamp_looks` written turn with the picture. Standing by
/// with Gemini's link gone, and as the only route. Its answer plain, no
/// look is asked.
#[test]
fn a_skills_look_on_the_second_route_takes_the_still_for_its_own_session() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-skill-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    let script = |only: bool, looks: bool| Script {
        only,
        second: !only,
        second_looks: looks,
        // The look's turn has time to go before the run ends.
        mute_at_frame: Some(500),
        written: Written::Said(6),
        ..Script::question(if only {
            Live::Answer(12)
        } else {
            Live::DropLink
        })
    };
    let standing_by = run(script(false, true));
    let alone = run(script(true, true));
    let plain = run(script(false, false));
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    for (case, run) in [("standing by", &standing_by), ("alone", &alone)] {
        let at = |kind: &str| {
            run.trace
                .iter()
                .position(|e| e["kind"] == kind)
                .unwrap_or_else(|| panic!("{case}: no {kind}: {}", run.summary()))
        };
        assert!(
            run.trace
                .iter()
                .any(|e| e["kind"] == "look_asked" && e["by"] == "tool" && e["turn"] == 1),
            "{case}"
        );
        let dropped: Vec<_> = run.events("look_dropped");
        assert!(dropped.is_empty(), "{case}: {dropped:?}");
        // Asked of the eyes once the answer is heard, not before.
        assert!(at("speaker_first_write") < at("look_full_asked"), "{case}");
        let still: Vec<_> = run.events("look_full_still");
        assert_eq!(still.len(), 1, "{case}");
        assert_eq!(still[0]["still"], true, "{case}");
        // The look's written turn, on the second route's own session.
        let session = run.seen.second.iter().find_map(|input| match input {
            ProviderInput::Start { session, .. } => Some(*session),
            _ => None,
        });
        let looks: Vec<_> = run
            .seen
            .second
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written {
                    session,
                    text,
                    look,
                    ..
                } => Some((Some(*session), text.as_str(), *look)),
                _ => None,
            })
            .collect();
        assert_eq!(
            looks,
            [(session, crate::speak_up::LOOK_FULL_NOTE, true)],
            "{case}"
        );
        assert!(
            run.trace
                .iter()
                .any(|e| e["kind"] == "input_admitted" && e["admission_basis"] == "lamp_looks"),
            "{case}"
        );
    }
    assert!(plain.events("look_asked").is_empty());
    assert!(plain.events("look_full_asked").is_empty());
    assert!(
        !plain
            .seen
            .second
            .iter()
            .any(|input| matches!(input, ProviderInput::Written { .. }))
    );
}

/// The first builder on this change's plan (20 Oct): a skill's look on
/// the second route is dropped the moment the person speaks, as every look
/// is when someone speaking first takes the turn; no look's turn goes.
#[test]
fn a_skills_look_on_the_second_route_is_dropped_when_the_person_speaks() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-skill-cut-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        written: Written::Said(6),
        ..Script::question(Live::Answer(60))
    });
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    assert_eq!(run.finished(1)["outcome"], "user_interrupted");
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["by"].clone()))
        .collect();
    assert_eq!(dropped, [(json!(1), json!("tool"))], "{}", run.summary());
    assert!(run.events("look_full_still").is_empty());
    assert!(
        !run.seen
            .second
            .iter()
            .any(|input| matches!(input, ProviderInput::Written { look: true, .. }))
    );
}

/// A skill's search on the second route alone (`LAMP_SECOND_SKILL_LOOK`),
/// its look's turns answered as `look_answers` says, the note's as `note`,
/// with `LAMP_SECOND_LOOK_NOTE` on or off.
fn search(
    on: bool,
    look_answers: &'static [LookAnswer],
    note: LookAnswer,
    speak_again: Option<When>,
) -> Run {
    search_with(on, look_answers, note, speak_again, None)
}

/// As [`search`], with a murmur and a hand Stop that many capture frames
/// after it (`Script::murmur`, `Script::hand_stop`).
fn search_with(
    on: bool,
    look_answers: &'static [LookAnswer],
    note: LookAnswer,
    speak_again: Option<When>,
    murmur_then_stop: Option<(When, u64)>,
) -> Run {
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-note-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    TEST_LOOK_NOTE.set(on);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        look_answers,
        note_answer: note,
        speak_again,
        murmur: murmur_then_stop.map(|(murmur, _)| murmur),
        hand_stop: murmur_then_stop.map(|(_, after)| after),
        turns: if speak_again.is_some() { 3 } else { 1 },
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(1_100),
        written: Written::Said(6),
        ..Script::question(Live::Answer(12))
    });
    TEST_LOOK_NOTE.set(false);
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    run
}

/// The look's notes the second route was sent, by request.
fn look_notes(run: &Run) -> Vec<u64> {
    run.seen
        .second
        .iter()
        .filter_map(|input| match input {
            ProviderInput::Written { request, text, .. } if text == super::look_note::NOTE => {
                Some(*request)
            }
            _ => None,
        })
        .collect()
}

/// The written turns sent to the second route as a search's own.
fn search_turns(run: &Run) -> Vec<u64> {
    run.seen
        .second
        .iter()
        .filter_map(|input| match input {
            ProviderInput::Written {
                request,
                search: true,
                ..
            } => Some(*request),
            _ => None,
        })
        .collect()
}

/// The look's turns (`lamp_looks`) the second route was sent, by request.
fn look_turns(run: &Run) -> Vec<u64> {
    run.trace
        .iter()
        .filter(|e| e["kind"] == "input_admitted" && e["admission_basis"] == "lamp_looks")
        .map(|e| e["turn"].as_u64().unwrap())
        .collect()
}

/// (a) The first builder's step 2 for look-for-things on the second route
/// (20 Oct): a skill's search whose look got an answer that called nothing
/// is sent one note to go on, once that answer has played and the room has
/// been quiet a second, on the same session; the note's answer is said.
#[test]
fn a_skills_look_answered_with_no_call_gets_one_note_after_it_plays() {
    let _one = super::pilot_faults::one_session();
    let run = search(true, &[LookAnswer::Nothing], LookAnswer::Other, None);
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    let notes = look_notes(&run);
    assert_eq!(notes.len(), 1, "{:?}", run.events("look_note_dropped"));
    let sent = run.events("look_note_sent");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["turn"], looks[0]);
    assert_eq!(sent[0]["next_turn"], notes[0]);
    // After the look's answer played whole, and a second of quiet.
    let at = |kind: &str, turn: u64| {
        run.trace
            .iter()
            .position(|e| e["kind"] == kind && e["turn"] == turn)
            .unwrap_or_else(|| panic!("no {kind} for {turn}: {}", run.summary()))
    };
    assert!(at("turn_finished", looks[0]) < at("look_note_sent", looks[0]));
    assert_eq!(run.played(looks[0]), vec![SECOND; 24]);
    assert_eq!(run.played(notes[0]), vec![SECOND; 24]);
    let session = run.seen.second.iter().find_map(|input| match input {
        ProviderInput::Start { session, .. } => Some(*session),
        _ => None,
    });
    assert!(run.seen.second.iter().any(|input| matches!(input,
        ProviderInput::Written { request, session: of, look: false, .. }
            if *request == notes[0] && Some(*of) == session)));
    assert!(run.events("look_note_dropped").is_empty());
}

/// (b) Once a search: the note's answer calls the next place, its look is
/// taken there (the note's turn goes on with the search), and that look's
/// answer calls nothing again: no second note.
#[test]
fn a_second_look_left_in_the_same_search_gets_no_second_note() {
    let _one = super::pilot_faults::one_session();
    let run = search(
        true,
        &[LookAnswer::Nothing, LookAnswer::Nothing],
        LookAnswer::Next,
        None,
    );
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 2, "{}", run.summary());
    let notes = look_notes(&run);
    assert_eq!(notes.len(), 1, "{}", run.summary());
    // The search's own turns, in order: look, note, look.
    assert_eq!(search_turns(&run), [looks[0], notes[0], looks[1]]);
    assert!(run.events("look_dropped").is_empty());
    let dropped: Vec<_> = run
        .events("look_note_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(json!(looks[1]), json!("its search had its note"))]
    );
}

/// (c) The chain: the look's answer calls `look_for` for the next place,
/// that look is taken in the look's own turn and the next `lamp_looks` turn
/// goes, also the search's; its answer called something else: no note.
#[test]
fn a_look_answer_that_called_gets_no_note() {
    let _one = super::pilot_faults::one_session();
    // The chain needs only `LAMP_SECOND_SKILL_LOOK`; the note's switch
    // changes nothing of it.
    for on in [true, false] {
        let run = search(
            on,
            &[LookAnswer::Next, LookAnswer::Other],
            LookAnswer::Other,
            None,
        );
        run.assert_clean();
        let looks = look_turns(&run);
        assert_eq!(looks.len(), 2, "note {on}: {}", run.summary());
        assert_eq!(search_turns(&run), looks, "note {on}");
        assert!(run.events("look_dropped").is_empty(), "note {on}");
        assert_eq!(run.events("look_full_still").len(), 2, "note {on}");
        assert!(look_notes(&run).is_empty(), "note {on}");
        assert!(run.events("look_note_sent").is_empty(), "note {on}");
    }
}

/// The voice's own bound on a search (`look_note::SEARCH_LOOKS_AT_MOST`,
/// lamp-skills' 8): every look's answer calls the next place, the search's
/// looks are taken in its own turns up to the 8th, and the 9th, asked in
/// the 8th look's turn, is dropped as in any turn the lamp wrote (#1146),
/// saying the search took its looks. No note follows.
#[test]
fn a_searchs_ninth_chained_look_is_dropped() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-cap-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    TEST_LOOK_NOTE.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        look_answers: &[LookAnswer::Next; 8],
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(3_000),
        written: Written::Said(6),
        limit: Duration::from_secs(40),
        ..Script::question(Live::Answer(12))
    });
    TEST_LOOK_NOTE.set(false);
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    run.assert_clean();
    let at_most = super::look_note::SEARCH_LOOKS_AT_MOST as usize;
    let looks = look_turns(&run);
    assert_eq!(looks.len(), at_most, "{}", run.summary());
    assert_eq!(run.events("look_full_still").len(), at_most);
    // Every look's turn but the last is the search's own.
    assert_eq!(search_turns(&run), looks[..at_most - 1]);
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(
            json!(looks[at_most - 1]),
            json!("the search took its looks")
        )]
    );
    assert!(look_notes(&run).is_empty());
}

/// #1146 kept: a look's turn that is no skill's search (the route's own
/// `take_a_look`) is a turn the lamp wrote itself like any other: a skill's
/// look its answer asks for is dropped, and no second still goes.
#[test]
fn a_look_asked_in_a_turn_that_is_no_searchs_is_still_dropped() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-own-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        own_look: true,
        look_answers: &[LookAnswer::Next],
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(1_100),
        written: Written::Said(6),
        ..Script::question(Live::Answer(12))
    });
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    assert!(search_turns(&run).is_empty());
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(
            json!(looks[0]),
            json!("the turn is one the lamp wrote itself")
        )]
    );
    assert_eq!(run.events("look_full_still").len(), 1);
}

/// (d) The person speaks while the look's answer plays (the speaker has
/// taken 48 chunks of the question's answer and 12 of the look's 24): the
/// answer is cut, no note goes, and why is recorded.
#[test]
fn the_person_speaking_after_the_look_drops_the_note() {
    let _one = super::pilot_faults::one_session();
    let run = search(
        true,
        &[LookAnswer::Nothing],
        LookAnswer::Other,
        Some(When::Accepted(60)),
    );
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    assert!(look_notes(&run).is_empty(), "{}", run.summary());
    let dropped = run.events("look_note_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["turn"], looks[0]);
    assert!(
        [
            "the person is speaking",
            "another turn began",
            "its answer was not heard whole"
        ]
        .contains(&dropped[0]["why"].as_str().unwrap()),
        "{dropped:?}"
    );
}

/// A hand Stop after the look's answer has played whole ends the search
/// (the reviewer's P3 on #2266): the note owed for it is not sent once the
/// room is quiet, after the person said stop, nor recorded as dropped.
#[test]
fn a_hand_stop_after_a_looks_answer_drops_its_note() {
    let _one = super::pilot_faults::one_session();
    let run = search_with(
        true,
        &[LookAnswer::Nothing],
        LookAnswer::Other,
        None,
        Some((When::Accepted(70), 50)),
    );
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    // The look's answer played whole, so the note was owed; the murmur
    // near its end kept it waiting for a quiet second.
    assert_eq!(run.played(looks[0]), vec![SECOND; 24]);
    let at = |kind: &str| {
        run.trace
            .iter()
            .position(|e| {
                e["kind"] == kind
                    && (kind != "turn_finished" || e["turn"] == looks[0])
                    && (kind != "hand_control_cancel" || e["reason"] == "hand_stop")
            })
            .unwrap_or_else(|| panic!("no {kind}: {}", run.summary()))
    };
    // The Stop came by hand once the look's turn had finished.
    assert!(at("turn_finished") < at("hand_control_cancel"));
    // The quiet came after it: no note, and no turn written for one.
    assert!(look_notes(&run).is_empty(), "{}", run.summary());
    assert!(run.events("look_note_sent").is_empty());
    assert!(
        !run.trace
            .iter()
            .any(|e| e["kind"] == "input_admitted" && e["admission_basis"] == "look_note"),
        "{}",
        run.summary()
    );
    // The search ended with the Stop: nothing owed is left to drop.
    assert!(run.events("look_note_dropped").is_empty());
}

/// (e) Without `LAMP_SECOND_LOOK_NOTE`, a look left with no call is as
/// before: no note, nothing recorded of one.
#[test]
fn without_the_switch_a_look_left_gets_no_note() {
    let _one = super::pilot_faults::one_session();
    let run = search(false, &[LookAnswer::Nothing], LookAnswer::Other, None);
    run.assert_clean();
    assert_eq!(look_turns(&run).len(), 1, "{}", run.summary());
    assert!(look_notes(&run).is_empty());
    assert!(run.events("look_note_sent").is_empty());
    assert!(run.events("look_note_dropped").is_empty());
}

#[test]
fn without_a_second_route_the_packets_and_records_are_as_before() {
    let _one = super::pilot_faults::one_session();
    let plain = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    plain.assert_clean();
    assert!(plain.events("voice_route_switched").is_empty());
    assert!(
        plain
            .trace
            .iter()
            .all(|e| e.get("standby").is_none() && e.get("route").is_none())
    );
    let standing_by = run(Script::question(Live::Answer(12)));
    standing_by.assert_clean();
    assert!(standing_by.events("voice_route_switched").is_empty());
    assert!(standing_by.seen.second.is_empty());
    assert_eq!(timeless(&plain.seen.live), timeless(&standing_by.seen.live));
    assert_eq!(shape(&plain), shape(&standing_by));
    assert_eq!(plain.played(1), standing_by.played(1));
    assert!(plain.played(1).iter().all(|&v| v == LIVE));
}

/// (b) A spent quota before any audio: the second route is given the whole
/// line and answers it, and the rest of the lamp sees the turn it would
/// have seen had Gemini answered: the same authority at the speaker, the
/// same ring phases, the same record of the finished turn.
#[test]
fn a_spent_quota_moves_the_line_to_the_second_route_and_nothing_else_changes() {
    let _one = super::pilot_faults::one_session();
    let live = run(Script::question(Live::Answer(12)));
    live.assert_clean();
    let switched = run(Script::question(Live::Fail(Fault::QuotaExceeded)));
    switched.assert_clean();
    let blocks = whole_line(&switched.seen.live, 1);
    assert_eq!(whole_line(&switched.seen.second, 1), blocks);
    // Over, and back to Gemini at rest if the run lasts long enough: its
    // link never went. On the ARM64 runner the run once ended before the
    // idle tick that goes back; going back has its own test below.
    let switch = switched.events("voice_route_switched");
    assert!(matches!(switch.len(), 1 | 2), "{switch:?}");
    assert_eq!(switch[0]["why"], "live_turn_failed");
    if let Some(back) = switch.get(1) {
        assert_eq!(back["why"], "live_link_ready");
    }
    assert_eq!(switch[0]["reason"], json!(Fault::QuotaExceeded));
    assert_eq!(switch[0]["replayed_commands"], blocks + 2);
    assert_eq!(switched.played(1), vec![SECOND; 48]);
    assert!(switched.events("provider_turn_failed").is_empty());
    for run in [&live, &switched] {
        assert_eq!(run.finished(1)["outcome"], "audio_written_unscored");
    }
    let keys = |run: &Run| {
        let mut keys: Vec<_> = run
            .finished(1)
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&live), keys(&switched));
    assert_eq!(dedup(&live.seen.installs), dedup(&switched.seen.installs));
    assert_eq!(dedup(&live.seen.phases), dedup(&switched.seen.phases));
    assert!(dedup(&switched.seen.phases).contains(&RingPhase::Speaking));
}

/// (c) The link drops while the person is still speaking: the line is not
/// abandoned but replayed to the second route, which hears all of it.
#[test]
fn a_link_dropped_mid_line_loses_nothing_of_the_question() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script::question(Live::DropLink));
    run.assert_clean();
    let sent_live = audio_count(&run.seen.live, 1);
    assert!(sent_live > 0, "the drop came before any audio");
    assert!(
        !run.seen
            .live
            .iter()
            .any(|i| matches!(i, ProviderInput::End { .. }))
    );
    let heard = whole_line(&run.seen.second, 1);
    assert!(heard > sent_live, "{heard} <= {sent_live}");
    assert!(run.events("provider_input_abandoned").is_empty());
    assert_eq!(
        run.events("voice_route_switched")[0]["why"],
        "live_link_changed"
    );
    assert_eq!(run.finished(1)["outcome"], "audio_written_unscored");
    assert_eq!(run.played(1), vec![SECOND; 48]);
}

/// (d) Gemini's answer that comes after its failure is never heard.
#[test]
fn gemini_audio_after_its_failure_is_dropped() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script::question(Live::FailThenLate(Fault::RateLimited, 6)));
    run.assert_clean();
    assert_eq!(run.played(1), vec![SECOND; 48]);
    assert!(run.seen.chunks.iter().all(|c| c.value != LIVE));
}

/// (e) Once Gemini's link is Ready again, the next turn at rest is its.
#[test]
fn the_next_turn_goes_back_to_gemini_once_it_is_ready() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        speak_again: Some(When::Settled(1)),
        turns: 2,
        live_back_after: Some(1),
        ..Script::question(Live::DropLink)
    });
    run.assert_clean();
    let whys: Vec<_> = run
        .events("voice_route_switched")
        .iter()
        .map(|e| e["why"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(whys, ["live_link_changed", "live_link_ready"]);
    assert_eq!(run.played(1), vec![SECOND; 48]);
    let second_turn = run.seen.live.iter().rev().find_map(|i| match i {
        ProviderInput::Start {
            request, session, ..
        } => Some((*request, *session)),
        _ => None,
    });
    let (turn, session) = second_turn.unwrap();
    assert!(turn > 1);
    assert_eq!(session, 2);
    whole_line(&run.seen.live, turn);
    assert!(!run.seen.second.iter().any(|i| request_of(i) == turn));
    assert_eq!(run.played(turn), vec![LIVE; 48]);
}

/// (g) Spoken over while the second route answers, the answer stops as
/// one of Gemini's does.
#[test]
fn spoken_over_the_second_routes_answer_stops_as_gemini_does() {
    let _one = super::pilot_faults::one_session();
    let script = |live| Script {
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(live)
    };
    let live = run(script(Live::Answer(60)));
    live.assert_clean();
    let switched = run(script(Live::Fail(Fault::ServerRejected)));
    switched.assert_clean();
    for run in [&live, &switched] {
        assert_eq!(run.finished(1)["outcome"], "user_interrupted");
        assert!(run.played(1).len() < 240, "{}", run.played(1).len());
        // The speaker took an authority without turn 1 once it was over.
        let installs = dedup(&run.seen.installs);
        let owned = installs.iter().position(|i| i.0 == Some(1)).unwrap();
        assert!(installs[owned..].iter().any(|i| i.0 != Some(1)));
    }
    assert!(switched.played(1).iter().all(|&v| v == SECOND));
    assert_eq!(dedup(&live.seen.installs), dedup(&switched.seen.installs));
}

/// (h) Gemini is Ready 1 s after the voice starts and the question began
/// before that: it is held, then Gemini's, and the second route hears none
/// of it (lampOS#779).
#[test]
fn a_question_before_geminis_first_ready_waits_for_it() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        live_late: Some(Some(100)),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    assert!(run.events("voice_route_switched").is_empty());
    let held = run.events("voice_route_held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["to"], "live");
    assert!(held[0]["held_ms"].as_u64().unwrap() < 3_000);
    assert!(run.seen.second.is_empty());
    whole_line(&run.seen.live, 1);
    assert_eq!(run.played(1), vec![LIVE; 48]);
}

/// (i) Gemini is never Ready: after the hold the second route is given the
/// whole line and answers it.
#[test]
fn a_question_goes_to_the_second_route_once_the_startup_hold_is_over() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        live_late: Some(None),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let held = run.events("voice_route_held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["to"], "second");
    assert!(
        held[0]["held_ms"].as_u64().unwrap() >= crate::coordinator::second::STARTUP_HOLD_US / 1_000
    );
    let switch = run.events("voice_route_switched");
    assert_eq!(switch.len(), 1, "{switch:?}");
    assert_eq!(switch[0]["why"], "live_link_not_ready");
    assert_eq!(switch[0]["turn"], 1);
    let blocks = whole_line(&run.seen.second, 1);
    assert_eq!(switch[0]["replayed_commands"], blocks + 2);
    assert!(run.seen.live.is_empty());
    assert_eq!(run.played(1), vec![SECOND; 48]);
}

/// lampOS#892: Gemini's worker is told to wait for room in its input queue
/// only when a second route is configured, right after `--keep-link`.
#[test]
fn the_live_worker_waits_for_room_only_with_a_second_route() {
    let options = DirectedOptions::parse(&[]).unwrap();
    let config = std::path::Path::new("/private/provider.json");
    let without = provider_arguments(
        ProviderSource::Gemini(config),
        &options,
        Lifetime::Serve,
        None,
        false,
    )
    .unwrap();
    assert!(
        !without
            .iter()
            .any(|flag| flag == crate::provider_worker::WAIT_FOR_ROOM_FLAG)
    );
    let with = provider_arguments(
        ProviderSource::Gemini(config),
        &options,
        Lifetime::Serve,
        None,
        true,
    )
    .unwrap();
    let keep = with
        .iter()
        .position(|flag| flag == crate::provider_worker::KEEP_LINK_FLAG)
        .unwrap();
    assert_eq!(with[keep + 1], crate::provider_worker::WAIT_FOR_ROOM_FLAG);
    let mut stripped = with.clone();
    stripped.remove(keep + 1);
    assert_eq!(stripped, without);
}

/// (j) #960: the second route's worker exits while it holds the turn. The
/// route is dropped and that turn ends, and the voice goes on: the next
/// turn is Gemini's, and the run ends by its scripted mute, not by the
/// worker's exit.
#[test]
fn a_second_worker_that_exits_is_dropped_and_the_voice_goes_on() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second_dies: Some(1),
        speak_again: Some(When::Settled(1)),
        turns: 2,
        ..Script::question(Live::Fail(Fault::QuotaExceeded))
    });
    run.assert_clean();
    assert_eq!(
        run.events("voice_route_switched")[0]["why"],
        "live_turn_failed"
    );
    let dropped = run.events("voice_route_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["active"], true);
    assert_eq!(run.finished(1)["outcome"], "second_route_failed");
    assert!(run.played(1).is_empty());
    let turn = run
        .seen
        .live
        .iter()
        .rev()
        .find_map(|i| match i {
            ProviderInput::Start { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap();
    assert!(turn > 1);
    whole_line(&run.seen.live, turn);
    assert_eq!(run.played(turn), vec![LIVE; 48]);
}

/// (k) #960: the standby's worker exits while Gemini is well: it is
/// dropped, and Gemini's answer is untouched.
#[test]
fn a_standby_that_exits_is_dropped_and_gemini_answers() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        standby_dies_at: Some(5),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let dropped = run.events("voice_route_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["active"], false);
    assert!(run.events("voice_route_switched").is_empty());
    whole_line(&run.seen.live, 1);
    assert_eq!(run.played(1), vec![LIVE; 48]);
}

/// (l) #960, the reviewer's P2: the active second worker exits mid-line
/// while the question is still being said, with input queued for it that
/// it never took. That input goes with the turn, and the voice goes on.
#[test]
fn a_second_worker_that_exits_mid_line_leaves_no_queued_input_behind() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second_dies: Some(40),
        mute_after_second_dies: true,
        ..Script::question(Live::DropLink)
    });
    run.assert_clean();
    let dropped = run.events("voice_route_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["active"], true);
    assert_eq!(run.finished(1)["outcome"], "second_route_failed");
    assert!(run.played(1).is_empty());
}

/// (m) #960, lamp-4ace 09:03-09:06: on the second route in a busy room the
/// lamp is never between turns. Gemini's link is Ready again while the
/// second route still answers, and someone speaks over it: that new turn
/// is Gemini's, not the second route's.
#[test]
fn a_turn_begun_while_the_second_route_answers_goes_to_gemini_once_ready() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        live_back_at_frame: Some(85),
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::DropLink)
    });
    run.assert_clean();
    let switches = run.events("voice_route_switched");
    let whys: Vec<_> = switches
        .iter()
        .map(|e| e["why"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        whys,
        ["live_link_changed", "live_link_ready"],
        "{switches:?}"
    );
    let (turn, session) = run
        .seen
        .live
        .iter()
        .rev()
        .find_map(|i| match i {
            ProviderInput::Start {
                request, session, ..
            } => Some((*request, *session)),
            _ => None,
        })
        .unwrap();
    assert!(turn > 1);
    assert_eq!(session, 2);
    assert_eq!(switches[1]["turn"], turn);
    whole_line(&run.seen.live, turn);
    assert!(!run.seen.second.iter().any(|i| request_of(i) == turn));
    assert!(run.played(1).iter().all(|&v| v == SECOND));
    assert_eq!(run.played(turn), vec![LIVE; 48]);
}

/// (n) The reviewer's P2 on #962: Gemini's worker, waiting while the second
/// route is active, exits with no report. It is still a mandatory worker:
/// the voice ends on it (and is restarted), rather than staying on the
/// second route for good.
#[test]
fn geminis_worker_exiting_while_the_second_route_is_active_still_ends_the_voice() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        live_dies_at: Some(60),
        ..Script::question(Live::DropLink)
    });
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(
        error.contains("provider worker exited"),
        "{}",
        run.summary()
    );
}

/// `LAMP_VOICE_ROUTE=second` (the owner, 19 Oct: Gemini Live against the
/// cheaper second route, an A/B on lamp-4ace): the second route is the only
/// voice from the first turn. No Gemini worker is started and nothing is
/// sent to one; the second route is sent what Gemini would have been sent,
/// answers, and the rest of the lamp sees the turn as Gemini's: the same
/// authorities at the speaker, the same ring phases, the same kinds and
/// fields of every record, and no switch.
#[test]
fn the_second_route_alone_answers_a_spoken_request_and_no_gemini_worker_starts() {
    let _one = super::pilot_faults::one_session();
    let gemini = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    gemini.assert_clean();
    let alone = run(Script {
        second: false,
        only: true,
        ..Script::question(Live::Answer(12))
    });
    alone.assert_clean();
    assert!(alone.seen.live.is_empty(), "{:?}", alone.seen.live);
    assert!(
        alone.exits.iter().all(|(role, _)| *role != "provider"),
        "{:?}",
        alone.exits
    );
    assert!(alone.exits.contains(&("second", true)), "{:?}", alone.exits);
    whole_line(&alone.seen.second, 1);
    assert_eq!(timeless(&gemini.seen.live), timeless(&alone.seen.second));
    assert_eq!(alone.played(1), vec![SECOND; 48]);
    assert_eq!(gemini.played(1), vec![LIVE; 48]);
    for kind in [
        "voice_route_switched",
        "voice_route_held",
        "voice_route_dropped",
    ] {
        assert!(alone.events(kind).is_empty(), "{kind}: {}", alone.summary());
    }
    assert_eq!(shape(&gemini), shape(&alone));
    assert_eq!(gemini.finished(1)["outcome"], alone.finished(1)["outcome"]);
    assert_eq!(dedup(&gemini.seen.installs), dedup(&alone.seen.installs));
    assert_eq!(dedup(&gemini.seen.phases), dedup(&alone.seen.phases));
    assert!(dedup(&alone.seen.phases).contains(&RingPhase::Speaking));
}

/// The first builder, 19 Oct: on the second route the first sound had been
/// its wait cue, not its answer. The session's records say when each turn's
/// answer began to be heard (`answer_first_audio`): on the second route the
/// first audio after its words, a cue before them being no answer; on the
/// Live route its first audio.
#[test]
fn the_answers_first_audio_is_recorded_after_a_wait_cue_never_at_it() {
    let _one = super::pilot_faults::one_session();
    let order = |run: &Run| -> Vec<&'static str> {
        run.trace
            .iter()
            .filter_map(|e| match e["kind"].as_str() {
                Some("provider_first_audio") if e["turn"] == 1 => Some("provider_first_audio"),
                Some("answer_first_audio") if e["turn"] == 1 => Some("answer_first_audio"),
                Some("transcript") if e["turn"] == 1 => Some("words"),
                _ => None,
            })
            .collect()
    };
    let alone = run(Script {
        second: false,
        only: true,
        second_cue: 4,
        second_says: Some("Sure."),
        ..Script::question(Live::Answer(12))
    });
    alone.assert_clean();
    assert_eq!(
        order(&alone),
        ["provider_first_audio", "words", "answer_first_audio"],
        "{}",
        alone.summary()
    );
    assert_eq!(alone.events("answer_first_audio").len(), 1);
    let gemini = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    gemini.assert_clean();
    let at = |run: &Run, kind: &str| run.events(kind)[0]["at_us"].as_u64().unwrap();
    assert_eq!(gemini.events("answer_first_audio").len(), 1);
    assert!(at(&gemini, "answer_first_audio") >= at(&gemini, "provider_first_audio"));
    assert_eq!(
        order(&gemini),
        ["provider_first_audio", "answer_first_audio"]
    );
}

/// Alone, the second route's worker is the lamp's only voice: its exit
/// ends the voice (and the service is restarted), as Gemini's does, where a
/// standby's is dropped and the voice goes on with Gemini.
#[test]
fn the_second_route_alone_exiting_ends_the_voice_as_gemini_does() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        second_dies: Some(1),
        ..Script::question(Live::Answer(12))
    });
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(error.contains("second worker exited"), "{}", run.summary());
    assert!(run.events("voice_route_dropped").is_empty());
    assert!(run.seen.live.is_empty());
}

/// The first builder's 20:15 on #1722: a task's news told unasked on the
/// second route alone is said once, at the next quiet moment, as on Live.
/// After the person's question is answered, the news waiting is begun as
/// the lamp's own turn at the first pause (`LAMP_TASK_NEWS_AT_PAUSE=on`),
/// sent to the voice that answers, said whole, and only then handed on as
/// heard; it is never sent again.
#[test]
fn news_is_told_once_at_the_next_quiet_moment_on_the_second_route_as_on_live() {
    let _one = super::pilot_faults::one_session();
    const NOTE: &str = "Claude Code just finished the story you asked for.";
    let told = |only: bool| {
        let (news, heard) = crate::task_news::TaskNews::waiting_for_test_heard(
            crate::task_news::News {
                key: "s1:2:4".into(),
                note: NOTE.into(),
                until: None,
                progress: None,
                finished: None,
                also: Vec::new(),
            },
            Duration::from_secs(30),
        );
        TEST_TASK_NEWS.with(|given| *given.borrow_mut() = Some(news));
        TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(true));
        let run = run(Script {
            second: false,
            only,
            written: Written::Said(12),
            mute_at_frame: Some(800),
            ..Script::question(Live::Answer(12))
        });
        TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(false));
        TEST_TASK_NEWS.with(|given| given.borrow_mut().take());
        (run, heard.try_iter().collect::<Vec<_>>())
    };
    for (only, voice) in [(false, LIVE), (true, SECOND)] {
        let (run, heard) = told(only);
        run.assert_clean();
        let sent = if only {
            &run.seen.second
        } else {
            &run.seen.live
        };
        let notes: Vec<u64> = sent
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written { request, text, .. } if text == NOTE => Some(*request),
                _ => None,
            })
            .collect();
        assert_eq!(notes, [2], "only {only}: {}", run.summary());
        assert_eq!(run.events("task_news").len(), 1, "only {only}");
        assert_eq!(run.events("task_news")[0]["key"], "s1:2:4");
        // At the first pause after the answer, not after 15 s with no turn.
        let since = run.events("task_news")[0]["since_turn_ms"]
            .as_u64()
            .unwrap();
        assert!(
            (crate::task_news::PAUSE_US / 1_000..crate::task_news::QUIET_US / 1_000)
                .contains(&since),
            "only {only}: {since} ms"
        );
        assert_eq!(run.played(1), vec![voice; 48], "only {only}");
        assert_eq!(run.played(2), vec![voice; 48], "only {only}");
        assert_eq!(heard, ["s1:2:4"], "only {only}: {}", run.summary());
        if only {
            assert!(run.seen.live.is_empty(), "{:?}", run.seen.live);
        }
    }
}

/// `LAMP_SAY` (#939) on the second route alone, as on Live: a skill's say
/// is begun as the lamp's own turn once the person's answer is over and the
/// room is quiet, sent to the voice that answers, said whole, and its asker
/// told `begun` and then `spoken`, once.
#[test]
fn a_skills_say_is_said_and_answered_spoken_on_the_second_route_as_on_live() {
    use crate::say::{Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    const NOTE: &str = "Remind the person to stretch.";
    let said = |only: bool| {
        let (says, answered) =
            crate::say::Says::note_for_test("stretch", NOTE, crate::say::URGENT_FROM);
        TEST_SAYS.with(|given| *given.borrow_mut() = Some(says));
        let run = run(Script {
            second: false,
            only,
            written: Written::Said(12),
            mute_at_frame: Some(800),
            ..Script::question(Live::Answer(12))
        });
        TEST_SAYS.with(|given| given.borrow_mut().take());
        (run, answered.try_iter().collect::<Vec<_>>())
    };
    for (only, voice) in [(false, LIVE), (true, SECOND)] {
        let (run, answered) = said(only);
        run.assert_clean();
        let sent = if only {
            &run.seen.second
        } else {
            &run.seen.live
        };
        let notes: Vec<u64> = sent
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written { request, text, .. } if text == NOTE => Some(*request),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 1, "only {only}: {}", run.summary());
        let turn = notes[0];
        assert_eq!(run.played(turn), vec![voice; 48], "only {only}");
        assert!(
            matches!(
                answered.as_slice(),
                [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
            ),
            "only {only}: {answered:?}"
        );
        if only {
            assert!(run.seen.live.is_empty(), "{:?}", run.seen.live);
        }
    }
}

/// The holding answers on the second route (the first builder's 22:54 on
/// #1722), as on Live: with `LAMP_HOLDING_ANSWER=on`, an answer that only
/// asked the person to wait is followed, once the room is quiet, by one
/// written turn of `holding_answer::NOTE` to the same voice, which says it.
/// Without the switch, nothing follows.
#[test]
fn a_holding_answer_is_asked_to_go_on_once_on_the_second_route_as_on_live() {
    let _one = super::pilot_faults::one_session();
    let with = |on: bool, only: bool| {
        TEST_HOLDING_ANSWER.with(|holding| holding.set(on));
        let run = run(Script {
            second: false,
            only,
            written: Written::Said(12),
            mute_at_frame: Some(900),
            ..Script::question(Live::Holding(12))
        });
        TEST_HOLDING_ANSWER.with(|holding| holding.set(false));
        run
    };
    for (only, voice) in [(false, LIVE), (true, SECOND)] {
        let run = with(true, only);
        run.assert_clean();
        let sent = if only {
            &run.seen.second
        } else {
            &run.seen.live
        };
        let notes: Vec<u64> = sent
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written { request, text, .. }
                    if text == crate::holding_answer::NOTE =>
                {
                    Some(*request)
                }
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 1, "only {only}: {}", run.summary());
        assert_eq!(run.events("holding_answer_nudged").len(), 1, "only {only}");
        assert_eq!(run.played(1), vec![voice; 48], "only {only}");
        assert_eq!(run.played(notes[0]), vec![voice; 48], "only {only}");
        assert_eq!(run.seen.live.is_empty(), only, "only {only}");

        let off = with(false, only);
        off.assert_clean();
        let sent = if only {
            &off.seen.second
        } else {
            &off.seen.live
        };
        assert!(
            !sent
                .iter()
                .any(|input| matches!(input, ProviderInput::Written { .. })),
            "only {only}: nothing follows without the switch"
        );
        assert!(off.events("holding_answer_nudged").is_empty());
    }
}

/// (p) #960, lamp-4ace (`server_go_away`): Gemini's link drops with no turn
/// in hand and is Ready again 500 ms later; the question asked meanwhile
/// waits for it, not for the second route.
#[test]
fn a_question_during_a_reconnect_at_rest_waits_for_gemini() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        link_drops_at: Some(10),
        live_back_at_frame: Some(60),
        ..Script::question(Live::Reconnect(12))
    });
    run.assert_clean();
    assert!(
        run.events("voice_route_switched").is_empty(),
        "{}",
        run.summary()
    );
    let held = run.events("voice_route_held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["to"], "live");
    assert!(run.seen.second.is_empty());
    assert_eq!(run.played(1), vec![LIVE; 48]);
}

/// #1421, the Seventh's P2: the loop's own wiring, from the asking to the
/// hum. The person's turn is unclear, the lamp asks "Sorry?" in writing, and
/// that turn comes back with nothing said (#1417). With `LAMP_UNCLEAR_HUM=on`
/// the lamp hums its "hm?" as a sound turn of its own, to the speaker;
/// without it, silence, as before. A "Sorry?" whose only words are "..." is
/// as silent and hums too; one the model said doesn't.
#[test]
fn an_empty_sorry_in_the_loop_hums_with_the_switch_and_only_then() {
    let _one = super::pilot_faults::one_session();
    // On Live and on the second route alone (the hums, #1722).
    for only in [false, true] {
        let with = |on: bool, written| {
            TEST_UNCLEAR_HUM.with(|hum| hum.set(on));
            let run = run(Script {
                second: false,
                only,
                written,
                mute_at_frame: Some(900),
                ..Script::question(Live::Unclear)
            });
            TEST_UNCLEAR_HUM.with(|hum| hum.set(false));
            run
        };
        let rows = |run: &Run, kind: &str| -> Vec<Value> {
            run.trace
                .iter()
                .filter(|event| event["kind"] == kind)
                .cloned()
                .collect()
        };
        let asked = |run: &Run| {
            run.trace
                .iter()
                .filter(|event| {
                    event["kind"] == "input_admitted" && event["admission_basis"] == "asked_again"
                })
                .count()
        };

        // The transcript's two ends, then the hum, played to the speaker.
        let silent = with(true, Written::Silent);
        assert_eq!(silent.seen.live.is_empty(), only, "only {only}");
        assert_eq!(rows(&silent, "provider_turn_unclear").len(), 1);
        assert_eq!(asked(&silent), 1, "the lamp asked Sorry?");
        let owed = rows(&silent, "unclear_hum_owed");
        assert_eq!(owed.len(), 1, "{:?}", rows(&silent, "turn_finished"));
        let played = rows(&silent, "unclear_hum");
        assert_eq!(played.len(), 1);
        assert_eq!(played[0]["plays"], true, "{played:?}");
        assert_eq!(rows(&silent, "unclear_hum_attempted").len(), 1);
        assert!(rows(&silent, "head_tap_cue_attempted").is_empty());
        // Its ring frames show the listening face, every one of them while its
        // turn lasts, as the tap's hum's do (the Seventh's P3 on #1421): the
        // first is set in the pass that admits it, the rest by the ring's own
        // reading of the turn (`tell_ring_of_turn`).
        let hum = rows(&silent, "input_admitted")
            .into_iter()
            .find(|row| row["admission_basis"] == "unclear_hum")
            .expect("the hum's own turn");
        let faces: Vec<bool> = rows(&silent, "ring_requested")
            .iter()
            .filter(|row| row["owner"] == hum["owner"])
            .map(|row| row["frame"]["listening"] == true)
            .collect();
        assert!(faces.len() > 1, "{faces:?}");
        assert!(faces.iter().all(|listening| *listening), "{faces:?}");

        // The switch off: asked the same, and silence, as before.
        let off = with(false, Written::Silent);
        assert_eq!(asked(&off), 1);
        assert!(rows(&off, "unclear_hum_owed").is_empty());
        assert!(rows(&off, "unclear_hum").is_empty());
        assert!(rows(&off, "unclear_hum_attempted").is_empty());

        // "..." is nothing said: cut as such, and the hum owed after it.
        let dots = with(true, Written::Dots);
        let owed = rows(&dots, "unclear_hum_owed");
        assert_eq!(owed.len(), 1);
        assert_eq!(owed[0]["after"], "nothing_said");
        assert_eq!(rows(&dots, "unclear_hum")[0]["plays"], true);

        // A "Sorry?" the model said: nothing owed, nothing hummed.
        let said = with(true, Written::Said(6));
        assert_eq!(asked(&said), 1);
        assert!(rows(&said, "unclear_hum_owed").is_empty());
        assert!(rows(&said, "unclear_hum").is_empty());

        // Said again once the first "hm?" plays, heard as unclear again, and
        // the second empty "Sorry?" hums too, well inside the tap's 30 s: it
        // answers the person's words, so it doesn't go blank the second time
        // (the Seventh's P2 on #1421).
        TEST_UNCLEAR_HUM.with(|hum| hum.set(true));
        let again = run(Script {
            second: false,
            only,
            written: Written::Silent,
            live: vec![Live::Unclear, Live::Unclear],
            speak_again: Some(When::Accepted(1)),
            mute_at_frame: Some(1_100),
            ..Script::question(Live::Unclear)
        });
        TEST_UNCLEAR_HUM.with(|hum| hum.set(false));
        assert_eq!(rows(&again, "provider_turn_unclear").len(), 2);
        assert_eq!(asked(&again), 2);
        let hums: Vec<bool> = rows(&again, "unclear_hum")
            .iter()
            .map(|row| row["plays"] == true)
            .collect();
        assert_eq!(hums, [true, true], "{:?}", rows(&again, "unclear_hum"));
        assert_eq!(rows(&again, "unclear_hum_attempted").len(), 2);
    }
}

/// #828, the first builder at 05:07 on 19 October: with
/// `LAMP_MISHEARD_HUM=on`, a cut-in the worker held as misheard gets the
/// lamp's "hm?" as a sound turn of its own, with the listening face, and no
/// written "Sorry?" turn goes to the model; within 100 ms of the hold. A
/// line held as unclear that was no cut-in is asked again as before, and so
/// is a misheard cut-in without the switch.
#[test]
fn with_the_switch_a_misheard_cut_in_hums_and_sends_no_model_turn() {
    let _one = super::pilot_faults::one_session();
    // On Live and on the second route alone (the hums, #1722).
    for only in [false, true] {
        let with = |on: bool, live: Live| {
            TEST_MISHEARD_HUM.with(|hum| hum.set(on));
            let run = run(Script {
                second: false,
                only,
                written: Written::Said(6),
                mute_at_frame: Some(900),
                ..Script::question(live)
            });
            TEST_MISHEARD_HUM.with(|hum| hum.set(false));
            run
        };
        let rows = |run: &Run, kind: &str| -> Vec<Value> {
            run.trace
                .iter()
                .filter(|event| event["kind"] == kind)
                .cloned()
                .collect()
        };
        let asked = |run: &Run| {
            run.trace
                .iter()
                .filter(|event| {
                    event["kind"] == "input_admitted" && event["admission_basis"] == "asked_again"
                })
                .count()
        };

        let hummed = with(true, Live::UnclearCutIn);
        assert_eq!(hummed.seen.live.is_empty(), only, "only {only}");
        let unclear = rows(&hummed, "provider_turn_unclear");
        assert_eq!(unclear.len(), 1);
        assert_eq!(unclear[0]["hum"], true, "{unclear:?}");
        assert_eq!(asked(&hummed), 0, "no model turn for the misheard line");
        let owed = rows(&hummed, "unclear_hum_owed");
        assert_eq!(owed.len(), 1, "{owed:?}");
        assert_eq!(owed[0]["after"], "misheard_cut_in");
        let played = rows(&hummed, "unclear_hum");
        assert_eq!(played.len(), 1, "{played:?}");
        assert_eq!(played[0]["plays"], true, "{played:?}");
        assert_eq!(played[0]["misheard_turn"], unclear[0]["turn"]);
        let held_us = unclear[0]["at_us"].as_u64().unwrap();
        let hum_us = played[0]["at_us"].as_u64().unwrap();
        assert!(
            hum_us.saturating_sub(held_us) < 100_000,
            "the hum {} us after the hold",
            hum_us.saturating_sub(held_us)
        );
        // Its ring frames show the listening face, as the empty Sorry?'s hum's do.
        let hum = rows(&hummed, "input_admitted")
            .into_iter()
            .find(|row| row["admission_basis"] == "unclear_hum")
            .expect("the hum's own turn");
        let faces: Vec<bool> = rows(&hummed, "ring_requested")
            .iter()
            .filter(|row| row["owner"] == hum["owner"])
            .map(|row| row["frame"]["listening"] == true)
            .collect();
        assert!(faces.len() > 1, "{faces:?}");
        assert!(faces.iter().all(|listening| *listening), "{faces:?}");

        // A line held as unclear that was no cut-in: asked again, as before.
        let other = with(true, Live::Unclear);
        assert_eq!(asked(&other), 1);
        assert!(rows(&other, "unclear_hum_owed").is_empty());
        assert!(rows(&other, "unclear_hum").is_empty());

        // The switch off: the misheard cut-in is asked again, as before.
        let off = with(false, Live::UnclearCutIn);
        assert_eq!(asked(&off), 1);
        assert!(rows(&off, "unclear_hum_owed").is_empty());
        assert!(rows(&off, "unclear_hum").is_empty());
    }
}

/// #933 on the second route (the first builder, 19:48 on #1722): an answer
/// spoken by the second route alone, its level rising on stressed words and
/// falling between sentences, makes the head nod and sway. What the voice
/// says for Expression (`voice/progress`, `voice/sound`) comes from the loop,
/// whichever route answers, so this is the whole path: the second route's
/// audio, the speaker, `expression_out`, and Expression's planner with the
/// product's `speaking` section.
#[test]
fn the_second_routes_answer_nods_and_sways_the_head_as_it_is_spoken() {
    let _one = super::pilot_faults::one_session();
    // 40 ms packets: a stressed word about every 1.1 s, syllables between,
    // and a half-second pause every 3 s. On the meter's scale (-48 to -14
    // dBFS, `ring_face::level_of`) the syllables read about 0.3 and 0.45 and a
    // stressed word 1.0, so each stressed word stands out, as in speech. (At
    // 3,000 and 11,000 the syllables filled the meter too, so only a
    // sentence's onset could nod, and a slow start that lost one failed the
    // count: Seventh and Eighth on #1722, 19 Oct.)
    fn shape(packet: usize) -> i16 {
        let into = (packet * 40) % 3_000;
        if into >= 2_500 {
            0
        } else if into % 1_100 < 120 {
            8_000
        } else if packet.is_multiple_of(3) {
            422
        } else {
            760
        }
    }
    let run = run(Script {
        second: false,
        only: true,
        second_packets: 200,
        second_shape: Some(shape),
        expression: true,
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let sounds = run
        .expression
        .iter()
        .filter(|input| matches!(input, lamp_expression::Input::Sound(_)))
        .count();
    assert!(sounds > 100, "{sounds} pieces of sound said for Expression");
    // Pieces lost between the voice and this test, by their stamps'
    // sequence: said with the counts below, so a loss is told from the
    // planner.
    let mut sequences: Vec<u64> = run
        .expression
        .iter()
        .filter_map(|input| match input {
            lamp_expression::Input::Sound(sound) => Some(sound.playout.stamp.sequence),
            _ => None,
        })
        .collect();
    sequences.sort_unstable();
    let lost = sequences.windows(2).map(|w| w[1] - w[0] - 1).sum::<u64>();
    // Through Expression's planner, with the product's speaking section.
    let mut catalogue = lamp_expression::Catalogue::from_json(include_str!(
        "../../../../../deploy/product/settings/expression.json"
    ))
    .unwrap();
    // The catalogue's own body lag (300 ms), past the 200 ms the voice
    // says its sound ahead: the speaking motion takes the farthest piece
    // said (#1759, 20 Oct). At 0 here, it once hid that no lamp nodded.
    if catalogue.speaking.is_none() {
        catalogue.speaking = lamp_expression::Catalogue::from_json(include_str!(
            "../../../../expression/alive-behaviours.json"
        ))
        .unwrap()
        .speaking;
    }
    let mut engine = lamp_expression::Engine::new(catalogue, true, EXPRESSION_CLOCK).unwrap();
    let at_of = |input: &lamp_expression::Input| match input {
        lamp_expression::Input::Progress(p) => p.timing.stamp.at_us,
        lamp_expression::Input::Sound(s) => s.playout.stamp.at_us,
        _ => 0,
    };
    let mut inputs = run.expression.clone();
    inputs.sort_by_key(at_of);
    let (first, last) = (at_of(&inputs[0]), at_of(inputs.last().unwrap()));
    let mut behaviours: Vec<Vec<String>> = Vec::new();
    let mut next = inputs.into_iter().peekable();
    let mut sequence = 0;
    let mut at = first;
    while at <= last + 1_500_000 {
        if at % 100_000 < 20_000 {
            sequence += 1;
            for context in [
                serde_json::json!({"kind":"room","stamp":{"clock_id":EXPRESSION_CLOCK,"at_us":at,"sequence":sequence},"value":{"state":"quiet"}}),
                serde_json::json!({"kind":"light","stamp":{"clock_id":EXPRESSION_CLOCK,"at_us":at,"sequence":sequence},"value":{"on":true,"colour":"warm","level":3}}),
                serde_json::json!({"kind":"joints","stamp":{"clock_id":EXPRESSION_CLOCK,"at_us":at,"sequence":sequence},
                    "value":{"name":["base_yaw","shoulder","elbow","head_roll","head_pitch"],"position":[0,0,0,0,0],"velocity":[],"effort":[],"commanded":[]}}),
            ] {
                engine
                    .ingest(at, serde_json::from_value(context).unwrap())
                    .unwrap();
            }
        }
        while next.peek().is_some_and(|input| at_of(input) <= at) {
            let input = next.next().unwrap();
            // A piece the planner refuses (out of its turn) is not a fault here.
            let _ = engine.ingest(at, input);
        }
        behaviours.push(engine.tick(at).state.behaviours);
        at += 20_000;
    }
    let starts = |name: &str| {
        behaviours
            .windows(2)
            .filter(|w| !w[0].iter().any(|b| b == name) && w[1].iter().any(|b| b == name))
            .count()
    };
    let nods = starts("nodding on a stressed word");
    let sways = starts("swaying between sentences");
    // A stressed word about every 1.1 s over 8 s in three sentences: a nod
    // on one in each, never two within 4 s (the first builder on #2030),
    // so two; a flat answer (`second_shape: None`) never sways.
    assert!(nods >= 2, "{nods} nods ({sounds} pieces, {lost} lost)");
    assert!(sways >= 1, "{sways} sways ({sounds} pieces, {lost} lost)");
}

/// #1620 on the second route (`LAMP_VOICE_ROUTE=second`, the owner, 19 Oct:
/// every voice feature on it). The voice is muted while the second route's
/// worker holds the request, its chat still streaming: the worker is sent
/// Stop with the others, reads it and is reaped inside the shared deadline,
/// and nothing is forced, failed or left behind, as on Live.
#[test]
fn a_stop_while_the_second_route_alone_is_mid_request_is_clean() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        second_holds: true,
        mute_at_frame: Some(300),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    assert!(run.exits.contains(&("second", true)), "{:?}", run.exits);
    assert!(run.seen.stopped.contains(&"second"));
    // The whole line reached it and nothing came back before the stop.
    whole_line(&run.seen.second, 1);
    assert!(run.played(1).is_empty(), "{}", run.summary());
    assert!(run.events("worker_exit_failed").is_empty());
    assert!(run.events("stop_delivery_error").is_empty());
}

/// #1620 on the second route: a worker that reads Stop and goes on running
/// is killed at the shared deadline, reaped, and named as the cleanup's
/// cause (`ForcedStop`, its own role), exactly as Gemini's worker is. The
/// rest of the rig still stops cleanly, and the conversation's own end
/// (the mute) is kept beside the cleanup failure.
#[test]
fn a_second_worker_that_ignores_stop_is_killed_and_named_as_geminis_is() {
    let _one = super::pilot_faults::one_session();
    for (only, role) in [(false, "provider"), (true, "second")] {
        let run = run(Script {
            second: false,
            only,
            ignores_stop: Some(role),
            ..Script::question(Live::Answer(12))
        });
        eprintln!("{}", run.summary());
        let error = run.result.as_ref().unwrap_err().to_string();
        let forced = run.events("worker_forced_shutdown");
        assert_eq!(forced.len(), 1, "{role}: {}", run.summary());
        assert_eq!(forced[0]["worker"], role);
        let cause = forced[0]["error"].as_str().unwrap();
        for said in [
            format!("{role} worker did not stop within shared 300 ms deadline"),
            "forced termination required; reaped with".to_owned(),
        ] {
            assert!(cause.contains(&said), "{role}: {cause}");
            assert!(error.contains(&said), "{role}: {error}");
        }
        assert!(
            error.contains("PhysicalMute") || error.contains("mute"),
            "{role}: {error}"
        );
        for (exited_role, exited) in &run.exits {
            assert!(exited, "{exited_role} child was not reaped ({role})");
            assert!(
                run.seen.stopped.contains(exited_role),
                "{exited_role} never saw Stop"
            );
        }
        assert!(run.exits.contains(&(role, true)), "{:?}", run.exits);
    }
}

/// #1567/#1864 on the second route: its worker says its link is Ready as
/// it starts, before any service is asked
/// (`the_second_workers_link_is_ready_before_any_service_is_asked`), so a
/// speech to text, chat or voice that refuses or does not answer can never
/// hold the voice's readiness. Each line then fails on its own
/// (`TurnFailed`, before any audio), and a service runs past the 25 s
/// startup budget to its mute, with `LAMP_START_OFFLINE` on or off; nothing
/// waits for a link.
#[test]
fn the_second_route_alone_with_its_services_refused_stays_up_past_the_startup_budget() {
    let _one = super::pilot_faults::one_session();
    let past = INPUT_READINESS_TIMEOUT + Duration::from_secs(3);
    for start_offline in [true, false] {
        crate::session_lifetime::set_start_offline_for_test(Some(start_offline));
        let run = run(Script {
            second: false,
            only: true,
            second_fails: Some(Fault::Transport),
            mute_at_frame: Some(past.as_millis() as u64 / 10),
            limit: past + Duration::from_secs(5),
            ..Script::question(Live::Answer(12))
        });
        crate::session_lifetime::set_start_offline_for_test(None);
        run.assert_clean();
        let error = run.result.as_ref().unwrap_err().to_string();
        assert!(!error.contains("never became ready"), "{error}");
        assert!(run.events("waiting_for_link").is_empty());
        assert_eq!(
            run.finished(1)["outcome"],
            "provider_failed",
            "{}",
            run.summary()
        );
        // Muted past the budget, not ended at it.
        let at = |kind: &str| run.events(kind)[0]["at_us"].as_u64().unwrap();
        assert!(
            at("physical_mute") - at("capture_started")
                >= INPUT_READINESS_TIMEOUT.as_micros() as u64,
            "{}",
            run.summary()
        );
        whole_line(&run.seen.second, 1);
        assert!(run.played(1).is_empty());
    }
}

/// Services that refuse every call, and count each.
#[derive(Default)]
struct Refusing {
    asked: std::sync::atomic::AtomicUsize,
}

impl crate::second_provider::Services for Refusing {
    fn listen(
        &self,
    ) -> futures_util::future::BoxFuture<
        'static,
        crate::second_provider::net::Result<Box<dyn crate::second_provider::Listener>>,
    > {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(crate::second_provider::net::Error::Transport(
                "speech to text",
            ))
        })
    }
    fn chat(
        &self,
        _: Vec<crate::second_provider::net::Message>,
    ) -> futures_util::stream::BoxStream<
        'static,
        crate::second_provider::net::Result<crate::second_provider::net::ChatEvent>,
    > {
        use futures_util::StreamExt;
        self.asked.fetch_add(1, Ordering::SeqCst);
        futures_util::stream::once(async {
            Err(crate::second_provider::net::Error::Transport("chat"))
        })
        .boxed()
    }
    fn speak(
        &self,
        _: String,
    ) -> futures_util::stream::BoxStream<'static, crate::second_provider::net::Result<Vec<i16>>>
    {
        use futures_util::StreamExt;
        self.asked.fetch_add(1, Ordering::SeqCst);
        futures_util::stream::once(async {
            Err(crate::second_provider::net::Error::Transport("speech"))
        })
        .boxed()
    }
}

/// The worker side of
/// `the_second_route_alone_with_its_services_refused_stays_up_past_the_startup_budget`,
/// through the second route's own `run`: it says Ready and its link Ready
/// (session 1, `Initial`) before any service is asked, and returns at once,
/// cleanly, on Stop.
#[test]
fn the_second_workers_link_is_ready_before_any_service_is_asked() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, worker) = super::pair(&directory, "second");
    let services = Arc::new(Refusing::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    parent.control.send(Control::Stop).unwrap();
    let started = Instant::now();
    let engine_services = services.clone();
    runtime
        .block_on(async move {
            let engine = crate::second_provider::Engine::new(
                engine_services,
                || crate::second_provider::Told::from(String::new()),
                Arc::new(|_, _, _, _| Box::pin(async { serde_json::json!({}) })),
            );
            crate::second_provider::worker::run(worker, new_boot().unwrap(), engine, None, None)
                .await
        })
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(300));
    assert!(matches!(
        parent.control.receive::<WorkerEvent>().unwrap(),
        Some(WorkerEvent::Ready)
    ));
    match parent.control.receive::<WorkerEvent>().unwrap() {
        Some(WorkerEvent::ProviderLink { link }) => {
            assert_eq!(link.session, 1);
            assert!(matches!(
                link.state,
                ProviderLinkState::Ready {
                    context: crate::provider_link::LinkContext::Initial,
                    ..
                }
            ));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(services.asked.load(Ordering::SeqCst), 0);
}

/// #1620 on the second route, the gap this closes: the second worker's
/// process (`worker::serve`) runs on a current-thread tokio runtime, ended
/// when `run` returns on Stop. Its async network I/O (the
/// chat's stream, the voice's download, speech to text's socket) is
/// dropped with it at once, but tokio's drop waits, with no bound, for
/// every `spawn_blocking` still running (a plain drop): each tool call (`run_call`, up to
/// `tools::ASK_DEADLINE`, 1.5 s, for the executive and the skills, longer
/// for the tasks part), a text's tool call (`texts_ends`), and a DNS lookup
/// (hyper-util's and tokio's resolvers both use the blocking pool). A stop
/// during one holds the worker past the shared 300 ms (750 ms with
/// diagnostics), so it is killed and the run's cleanup fails, which keeps
/// the restart guard's mute, #1620's symptom. The Live worker runs its
/// calls on detached threads, which never hold its exit.
///
/// Built as `serve` builds its runtime and ended by `serve`'s own
/// `worker::finish`, with the real `tool_runner` asking an executive that
/// takes the call and does not answer. With a plain drop it took 1.50 s.
#[test]
fn a_stop_during_a_slow_tool_call_lets_the_second_worker_exit_within_its_deadline() {
    use std::os::unix::net::UnixListener;
    let directory = SessionDirectory::create().unwrap();
    let executive = UnixListener::bind(directory.path.join(crate::tools::CALL_SOCKET)).unwrap();
    // A chat stream that has said nothing yet, as DeepSeek thinking.
    let chat = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let chat_at = chat.local_addr().unwrap();
    let settings = crate::tools::Settings {
        exec: Some(directory.path.clone()),
        ..Default::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (taken, held) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let (call, _) = executive.accept().unwrap();
        let (stream, _) = chat.accept().unwrap();
        taken.send(()).unwrap();
        // Held open, unanswered, until the test ends.
        std::thread::sleep(Duration::from_secs(3));
        drop((call, stream));
    });
    runtime.block_on(async move {
        let runner = crate::second_provider::worker::tool_runner(settings, None, None, false);
        tokio::spawn(runner(1, false, "light_on".into(), serde_json::json!({})));
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut stream = tokio::net::TcpStream::connect(chat_at).await.unwrap();
            let _ = stream.read(&mut [0; 16]).await;
        });
        // Both are in flight: the call in the blocking pool, the stream
        // waiting for its first byte. Then `run` returns on Stop.
        tokio::task::spawn_blocking(move || held.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
    });
    let stopped = Instant::now();
    crate::second_provider::worker::finish(runtime);
    let took = stopped.elapsed();
    holder.join().unwrap();
    assert!(
        took < Duration::from_millis(300),
        "the second worker's runtime took {took:?} to end after Stop"
    );
}
