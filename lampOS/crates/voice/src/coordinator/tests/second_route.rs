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
        atomic::{AtomicBool, Ordering},
    },
};

const PACKET: usize = crate::provider_flow::PACKET_SAMPLES;
const LIVE: i16 = 1_000;
const SECOND: i16 = 2_000;
const LIMIT: Duration = Duration::from_secs(12);

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
    /// A skill's tool answered with a look, then as `FailThenLate`: the
    /// second route takes the turn.
    LookFail(Fault, usize),
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
    live: Vec<Live>,
    /// Packets in each of the second route's answers.
    second_packets: usize,
    /// The second route's answers open with these words.
    second_says: Option<&'static str>,
    /// Speech starts again (a new turn, or speaking over an answer).
    speak_again: Option<When>,
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
}

impl Script {
    fn question(live: Live) -> Self {
        Self {
            second: true,
            live: vec![live],
            second_packets: 12,
            second_says: None,
            speak_again: None,
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
                    if live {
                        self.live_requests.push(request);
                    }
                    self.peers[index].queued.push_back(ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    });
                }
                ProviderInput::Written {
                    request, session, ..
                } if live => {
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
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                    }
                }
                ProviderInput::End { request, .. } => {
                    self.ended.insert(request);
                    if !live {
                        if let Some(text) = self.script.second_says {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::Transcript {
                                    request: Some(request),
                                    text: text.into(),
                                    finished: true,
                                });
                        }
                        self.audio(index, request, self.script.second_packets, SECOND);
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
                                });
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Unclear => {
                            self.peers[index].queued.push_back(ProviderOutput::Unclear {
                                request,
                                why: "fixture".into(),
                            });
                            self.finish(index, request);
                        }
                        Live::LookSilent | Live::LookUnclear | Live::LookFail(..) => {
                            let queued = &mut self.peers[index].queued;
                            queued.push_back(ProviderOutput::LookAsked {
                                request,
                                session: 1,
                                dropped: None,
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
    let mut rig = Rig {
        second: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        speaker: spawn("speaker"),
        capture: spawn("capture"),
        provider: spawn("provider"),
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
        endpoint: None,
    };
    if script.second {
        let worker = spawn("second");
        rig.second = Some(second::Route::standing_by(worker, ready_link(1, 1)));
    }
    // The clock starts once every child has crossed its startup barrier;
    // the coordinator and the peers share it from here.
    let clock_scope = crate::scenario_clock::Scope::enter(LIMIT * 2).unwrap();
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
            if crate::scenario_clock::instant() - started > LIMIT && !reported {
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
    let result = conversation(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        startup,
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
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
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    Run {
        result,
        trace: trace.iter().cloned().collect(),
        seen,
        exits,
    }
}

/// The records whose kinds and fields are compared across runs: what a turn
/// leaves, with neither timing nor the route's own records.
fn shape(run: &Run) -> BTreeSet<(String, Vec<String>)> {
    run.trace
        .iter()
        .filter(|e| e["kind"] != "voice_route_switched")
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
    let with = |on: bool, written| {
        TEST_UNCLEAR_HUM.with(|hum| hum.set(on));
        let run = run(Script {
            second: false,
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
