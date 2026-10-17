//! Opt-in, one-reply offline provider for finite acoustic experiments.
//! This supplies PCM, never interaction authority, speaker permits, or a score.
use crate::{
    options::{DirectedOptions, SessionOptions},
    provider_flow::{MAX_REPLY_SAMPLES, OutputSender, PACKET_SAMPLES},
    provider_link::{LinkContext, ProviderLink, ProviderLinkState},
    provider_worker::{ProviderInput, ProviderOutput},
    transport::{Channel, WorkerChannels},
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Permission, Snapshot, TurnOwner};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{self, Cursor, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixDatagram,
    },
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_WAV_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_REPLY_FRAMES: usize = MAX_REPLY_SAMPLES;
const PACKET_FRAMES: usize = PACKET_SAMPLES;
const MAX_EVENTS: usize = 8;
const CONTROL_LEASE_US: u64 = 250_000;
const CONTROL_SLICE: usize = 16;
const RETAINED_INPUT_US: u64 = 100_000;
pub const CUE_MAX_BYTES: usize = 512;
pub const CUE_FRESH_US: u64 = 100_000;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Clone, Debug, Serialize)]
pub struct FixtureInfo {
    pub wav_sha256: String,
    pub pcm_sha256: String,
    pub rate: u32,
    pub channels: u16,
    pub frames: usize,
    pub rail_samples: usize,
}

pub struct Fixture {
    pub info: FixtureInfo,
    samples: Vec<i16>,
}
impl Fixture {
    /// Validate and own the entire immutable source before activating workers.
    /// No resampling, gain adjustment, silence insertion, or TTS regeneration.
    pub fn load(path: &Path, expected_sha256: &str) -> io::Result<Self> {
        if expected_sha256.len() != 64
            || !expected_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "fixture SHA256 must be 64 lowercase hexadecimal characters",
            ));
        }
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )?;
        let mut file = File::from(fd);
        let before = file.metadata()?;
        if !before.is_file() || !(44..=MAX_WAV_BYTES).contains(&before.len()) {
            return Err(invalid("fixture must be a bounded regular non-symlink WAV"));
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        (&mut file)
            .take(MAX_WAV_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != before.len() || file.metadata()?.len() != before.len() {
            return Err(invalid("fixture changed size during read"));
        }
        let wav_sha256 = format!("{:x}", Sha256::digest(&bytes));
        if wav_sha256 != expected_sha256 {
            return Err(invalid("fixture SHA256 mismatch"));
        }
        if &bytes[..4] != b"RIFF"
            || &bytes[8..12] != b"WAVE"
            || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 != bytes.len()
        {
            return Err(invalid("fixture requires a complete RIFF/WAVE container"));
        }
        let mut reader = hound::WavReader::new(Cursor::new(&bytes)).map_err(io::Error::other)?;
        let spec = reader.spec();
        let frames = reader.len() as usize;
        if spec.sample_rate != 24_000
            || spec.channels != 1
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
            || !(240..=MAX_REPLY_FRAMES).contains(&frames)
        {
            return Err(invalid(
                "fixture requires 10 ms through 30 s of mono 24 kHz PCM16",
            ));
        }
        let samples: Vec<i16> = reader
            .samples()
            .collect::<Result<_, _>>()
            .map_err(io::Error::other)?;
        if samples.len() != frames || samples.iter().all(|&sample| sample == 0) {
            return Err(invalid("fixture PCM is incomplete or entirely silent"));
        }
        let mut pcm_hash = Sha256::new();
        for sample in &samples {
            pcm_hash.update(sample.to_le_bytes());
        }
        let info = FixtureInfo {
            wav_sha256,
            pcm_sha256: format!("{:x}", pcm_hash.finalize()),
            rate: 24_000,
            channels: 1,
            frames,
            rail_samples: samples
                .iter()
                .filter(|&&s| matches!(s, i16::MIN | i16::MAX))
                .count(),
        };
        Ok(Self { info, samples })
    }
}

#[derive(Debug)]
pub struct FixtureOptions {
    pub directed: DirectedOptions,
    pub cue_socket: Option<PathBuf>,
}
impl FixtureOptions {
    pub fn parse(arguments: &[String]) -> io::Result<Self> {
        let SessionOptions {
            directed,
            cue_socket,
            camera_config,
            failure_notices,
            motor,
            controls,
        } = SessionOptions::parse(arguments)?;
        if controls.enabled() {
            return Err(invalid(
                "hand controls are for Gemini voice sessions, never reply fixtures",
            ));
        }
        if failure_notices.is_some() {
            return Err(invalid(
                "local failure notices are only for Gemini chat, never reply fixtures",
            ));
        }
        if camera_config.is_some() || motor.is_some() {
            return Err(invalid(
                "camera/motor components are supported only by directed Gemini chat",
            ));
        }
        if directed.provider_continuation != crate::provider_worker::Continuation::Off {
            // A reply fixture has no Gemini session to apply it to, and its
            // worker takes no such argument.
            return Err(invalid(
                "provider continuation is supported only by directed Gemini chat",
            ));
        }
        if !directed.diagnostics {
            return Err(invalid(
                "fixture experiments require explicit --diagnostics",
            ));
        }
        Ok(Self {
            directed,
            cue_socket,
        })
    }
}

struct Input {
    owner: TurnOwner,
    open: bool,
    sequence: u64,
    read_at_us: Option<u64>,
    frames: usize,
}
struct Reply {
    owner: TurnOwner,
    offset: usize,
    stage: u8,
    audio: bool,
}

struct RetainedInput {
    command: ProviderInput,
    received_at_us: u64,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum ControlDrain {
    Empty,
    Deferred,
    Stopped,
}

/// Bounded protocol state. The source is reserved for the first admitted
/// owner, including when cancellation overtakes its queued Start/End messages.
/// Later inputs are observed but never trigger another fixture playback.
pub struct FixtureProvider {
    fixture: Fixture,
    guard: BoundaryGuard,
    first_owner: Option<TurnOwner>,
    input: Option<Input>,
    reply: Option<Reply>,
    events: VecDeque<ProviderOutput>,
    pending: Option<ProviderOutput>,
    retained_input: Option<RetainedInput>,
    highest_seen: u64,
    retired_through: u64,
    reminder_refused_through: u64,
    output_sequence: u64,
    output_flow: OutputSender,
    last_control_us: u64,
    last_now_us: u64,
    saw_allowed: bool,
    stopped: bool,
}
impl FixtureProvider {
    pub fn new(fixture: Fixture, boot: BootId, now_us: u64) -> Self {
        Self {
            fixture,
            guard: BoundaryGuard::new(boot, MonoTime::from_micros(now_us)),
            first_owner: None,
            input: None,
            reply: None,
            events: VecDeque::with_capacity(MAX_EVENTS),
            pending: Some(ProviderOutput::Ready { setup_us: 0 }),
            retained_input: None,
            highest_seen: 0,
            retired_through: 0,
            reminder_refused_through: 0,
            output_sequence: 0,
            output_flow: OutputSender::default(),
            last_control_us: now_us,
            last_now_us: now_us,
            saw_allowed: false,
            stopped: false,
        }
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.guard.invalidate();
        self.input = None;
        self.reply = None;
        self.pending = None;
        self.retained_input = None;
        self.events.clear();
    }
    fn checked<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            self.stop();
        }
        result
    }
    /// False means privacy ended this finite worker; reopening cannot revive it.
    pub fn install(&mut self, now_us: u64, state: Snapshot) -> io::Result<bool> {
        let result = self.install_inner(now_us, state);
        self.checked(result)
    }
    fn install_inner(&mut self, now_us: u64, state: Snapshot) -> io::Result<bool> {
        // A new packet cannot revive a worker that missed its previous lease.
        self.fresh(now_us)?;
        let changed = self
            .guard
            .state()
            .is_some_and(|old| old.microphone_generation() != state.microphone_generation());
        self.guard
            .install(MonoTime::from_micros(now_us), state)
            .map_err(io::Error::other)?;
        self.last_control_us = now_us;
        if self.saw_allowed && (changed || state.microphone_permission() != Permission::Allowed) {
            self.stop();
            return Ok(false);
        }
        self.saw_allowed |= state.microphone_permission() == Permission::Allowed;
        if let Some(owner) = state.owner() {
            if owner.turn() <= self.retired_through || owner.turn() < self.highest_seen {
                return Err(invalid("fixture authority resurrected a retired owner"));
            }
            if owner.turn() > self.reminder_refused_through {
                self.first_owner.get_or_insert(owner);
            }
            self.highest_seen = owner.turn();
            self.retired_through = self.retired_through.max(owner.turn() - 1);
        } else {
            self.retired_through = self.retired_through.max(self.highest_seen);
        }
        if self
            .input
            .as_ref()
            .is_some_and(|input| Some(input.owner) != state.owner())
        {
            self.input = None;
        }
        if self
            .reply
            .as_ref()
            .is_some_and(|reply| Some(reply.owner) != state.owner())
        {
            self.reply = None;
        }
        if self
            .pending
            .as_ref()
            .and_then(output_request)
            .is_some_and(|r| r <= self.retired_through)
        {
            self.pending = None;
        }
        self.events
            .retain(|event| output_request(event).is_none_or(|r| r > self.retired_through));
        Ok(true)
    }
    fn clock(&mut self, now_us: u64) -> io::Result<()> {
        if self.stopped || now_us < self.last_now_us {
            return Err(invalid("fixture stopped or monotonic clock regressed"));
        }
        self.last_now_us = now_us;
        Ok(())
    }
    fn fresh(&mut self, now_us: u64) -> io::Result<()> {
        self.clock(now_us)?;
        if now_us.saturating_sub(self.last_control_us) >= CONTROL_LEASE_US {
            return Err(invalid("fixture controller heartbeat expired"));
        }
        if let Some(state) = self.guard.state()
            && (now_us >= state.expires_at().as_micros()
                || (state.owner().is_some()
                    && !state.listening_ready(MonoTime::from_micros(now_us))))
        {
            return Err(invalid("fixture privacy or input lease expired"));
        }
        Ok(())
    }
    fn event(&mut self, event: ProviderOutput) -> io::Result<()> {
        if self.events.len() == MAX_EVENTS {
            return Err(invalid("fixture event queue full"));
        }
        self.events.push_back(event);
        Ok(())
    }
    pub fn submit(&mut self, now_us: u64, command: ProviderInput) -> io::Result<()> {
        let result = self.submit_inner(now_us, command);
        self.checked(result)
    }
    fn submit_inner(&mut self, now_us: u64, command: ProviderInput) -> io::Result<()> {
        self.fresh(now_us)?;
        let state = self
            .guard
            .state()
            .ok_or_else(|| invalid("fixture has no authority"))?;
        let (request, privacy) = match &command {
            ProviderInput::Start {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Audio {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::End {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Called {
                request,
                privacy_generation,
            }
            | ProviderInput::Written {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Reminder {
                request,
                privacy_generation,
                ..
            } => (*request, *privacy_generation),
        };
        if request == 0
            || !state.listening_ready(MonoTime::from_micros(now_us))
            || privacy != state.microphone_generation()
        {
            return Err(invalid("fixture input lost privacy or retention authority"));
        }
        if request <= self.retired_through || request <= self.reminder_refused_through {
            return Ok(());
        }
        let owner = state
            .owner()
            .filter(|owner| owner.turn() == request)
            .ok_or_else(|| invalid("fixture input has no matching admitted owner"))?;
        match command {
            ProviderInput::Reminder { session, .. } => {
                if self.input.is_none() && self.reply.is_none() && self.first_owner == Some(owner) {
                    self.first_owner = None;
                }
                self.reminder_refused_through = request;
                self.input = None;
                self.reply = None;
                if self.pending.as_ref().and_then(output_request) == Some(request) {
                    self.pending = None;
                }
                self.events
                    .retain(|event| output_request(event) != Some(request));
                self.event(ProviderOutput::ReminderDeferred {
                    request,
                    session,
                    reason: crate::provider_worker::ReminderDeferral::Disabled,
                })?;
            }
            ProviderInput::Start { session, .. } => {
                if session != 1 {
                    return Err(invalid(
                        "fixture Start selected an unknown provider session",
                    ));
                }
                if self.input.is_some() {
                    return Err(invalid("fixture input already started"));
                }
                self.input = Some(Input {
                    owner,
                    open: true,
                    sequence: 0,
                    read_at_us: None,
                    frames: 0,
                });
                self.event(ProviderOutput::Started {
                    request,
                    session: 1,
                    waiting_for_barrier: false,
                })?;
            }
            ProviderInput::Audio {
                sequence,
                read_completed_at_us,
                samples,
                ..
            } => {
                let input = self
                    .input
                    .as_mut()
                    .filter(|input| input.owner == owner && input.open)
                    .ok_or_else(|| invalid("fixture audio without open input"))?;
                if samples.len() != lamp_audio::CAPTURE_SAMPLES
                    || read_completed_at_us == 0
                    || input.sequence.checked_add(1) != Some(sequence)
                    || now_us
                        .checked_sub(read_completed_at_us)
                        .is_none_or(|age| age >= 1_000_000)
                    || input
                        .read_at_us
                        .is_some_and(|previous| read_completed_at_us < previous)
                    || input.frames >= 120 * 16_000
                {
                    return Err(invalid(
                        "fixture audio sequence, time, shape or duration invalid",
                    ));
                }
                input.sequence = sequence;
                input.read_at_us = Some(read_completed_at_us);
                input.frames += samples.len();
                // The fixture needs protocol continuity, not retained microphone PCM.
            }
            // A note beside the request: the fixture has no model to tell.
            ProviderInput::Called { .. } => {}
            ProviderInput::End { .. } => {
                let input = self
                    .input
                    .as_mut()
                    .filter(|input| input.owner == owner && input.open && input.frames > 0)
                    .ok_or_else(|| invalid("fixture endpoint without captured open input"))?;
                input.open = false;
                self.reply = Some(Reply {
                    owner,
                    offset: 0,
                    stage: 0,
                    audio: self.first_owner == Some(owner),
                });
            }
            // The fixture answers what it hears. It has no written turns.
            ProviderInput::Written { .. } => {
                return Err(invalid("the fixture provider takes no written request"));
            }
        }
        Ok(())
    }
    /// One bounded worker tick. Control is checked again after data receipt:
    /// Authority-before-Start on separate sockets is not receive-ordering.
    /// False means Stop/privacy ended the worker. A full control slice defers
    /// input and output, retaining at most one command with its original age.
    pub fn service_step(
        &mut self,
        mut now: impl FnMut() -> u64,
        mut receive_control: impl FnMut() -> io::Result<Option<Control>>,
        data: &mut Channel,
    ) -> io::Result<bool> {
        let result = self.service_inner(&mut now, &mut receive_control, data);
        self.checked(result)
    }
    fn service_inner(
        &mut self,
        now: &mut impl FnMut() -> u64,
        receive_control: &mut impl FnMut() -> io::Result<Option<Control>>,
        data: &mut Channel,
    ) -> io::Result<bool> {
        let at = now();
        self.fresh(at)?;
        self.check_retained_input(at)?;
        match self.drain_control(now, receive_control)? {
            ControlDrain::Stopped => return Ok(false),
            ControlDrain::Deferred => return Ok(true),
            ControlDrain::Empty => {}
        }
        if self.retained_input.is_none()
            && let Some(command) = data.receive()?
        {
            self.retained_input = Some(RetainedInput {
                command,
                received_at_us: now(),
            });
        }
        // The parent sends authority before its dependent input. It can do
        // both after our first empty read, so the data read must precede this
        // second priority drain. Never consume another input while deferred.
        match self.drain_control(now, receive_control)? {
            ControlDrain::Stopped => return Ok(false),
            ControlDrain::Deferred => return Ok(true),
            ControlDrain::Empty => {}
        }
        let at = now();
        self.check_retained_input(at)?;
        if let Some(retained) = self.retained_input.take() {
            // No speculative wait for missing owner authority: once control
            // is empty, the existing admission/privacy checks remain final.
            self.submit(at, retained.command)?;
        }
        if self.output_flow.can_send() && self.send_one(now(), |event| data.send(event))? {
            self.output_flow.record_sent()?;
        }
        Ok(true)
    }
    fn check_retained_input(&self, now_us: u64) -> io::Result<()> {
        if self.retained_input.as_ref().is_some_and(|input| {
            now_us
                .checked_sub(input.received_at_us)
                .is_none_or(|age| age >= RETAINED_INPUT_US)
        }) {
            return Err(invalid("fixture retained input expired or clock regressed"));
        }
        Ok(())
    }
    fn drain_control(
        &mut self,
        now: &mut impl FnMut() -> u64,
        receive: &mut impl FnMut() -> io::Result<Option<Control>>,
    ) -> io::Result<ControlDrain> {
        for _ in 0..CONTROL_SLICE {
            match receive()? {
                Some(Control::Stop) => {
                    self.stop();
                    return Ok(ControlDrain::Stopped);
                }
                Some(Control::Authority { snapshot }) => {
                    if !self.install(now(), snapshot)? {
                        return Ok(ControlDrain::Stopped);
                    }
                }
                Some(Control::ProviderOutputCapacity { through }) => {
                    self.output_flow.grant(through)?;
                }
                Some(Control::StartCapture | Control::ConnectReference { .. }) => {
                    return Err(invalid("invalid fixture provider control"));
                }
                None => return Ok(ControlDrain::Empty),
            }
        }
        Ok(ControlDrain::Deferred)
    }
    /// At most one bounded datagram per call. WouldBlock retains the original
    /// pending packet; priority authority installation can discard it first.
    pub fn send_one(
        &mut self,
        now_us: u64,
        mut send: impl FnMut(&ProviderOutput) -> io::Result<()>,
    ) -> io::Result<bool> {
        let result = self.send_inner(now_us, &mut send);
        self.checked(result)
    }
    fn send_inner(
        &mut self,
        now_us: u64,
        send: &mut impl FnMut(&ProviderOutput) -> io::Result<()>,
    ) -> io::Result<bool> {
        self.fresh(now_us)?;
        if self.pending.is_none() {
            self.pending = self.events.pop_front();
        }
        if self.pending.is_none()
            && let Some(reply) = self.reply.as_mut()
        {
            let request = reply.owner.turn();
            self.pending = Some(
                if reply.audio && reply.offset < self.fixture.samples.len() {
                    let end = (reply.offset + PACKET_FRAMES).min(self.fixture.samples.len());
                    let samples = self.fixture.samples[reply.offset..end].to_vec();
                    reply.offset = end;
                    self.output_sequence = self
                        .output_sequence
                        .checked_add(1)
                        .ok_or_else(|| invalid("fixture output sequence exhausted"))?;
                    // Each fixed-size event is its own bounded provider packet.
                    ProviderOutput::Audio {
                        request,
                        sequence: self.output_sequence,
                        provider_event_at_us: now_us,
                        source_frames: samples.len(),
                        source_offset: 0,
                        samples,
                    }
                } else if reply.stage == 0 {
                    reply.stage = 1;
                    ProviderOutput::GenerationComplete { request }
                } else {
                    self.reply = None;
                    ProviderOutput::TurnComplete {
                        request,
                        idle: true,
                    }
                },
            );
        }
        let Some(event) = self.pending.as_ref() else {
            return Ok(false);
        };
        if let Some(request) = output_request(event) {
            let state = self
                .guard
                .state()
                .ok_or_else(|| invalid("fixture output has no authority"))?;
            if !state.listening_ready(MonoTime::from_micros(now_us))
                || state.owner().map(|owner| owner.turn()) != Some(request)
            {
                return Err(invalid("fixture output lost original owner"));
            }
        }
        match send(event) {
            Ok(()) => {
                self.pending = None;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(error),
        }
    }
}
fn output_request(event: &ProviderOutput) -> Option<u64> {
    match event {
        ProviderOutput::Ready { .. }
        | ProviderOutput::TurnFailed { .. }
        | ProviderOutput::ReminderDeferred { .. }
        // A fixture keeps no input and never reports one.
        | ProviderOutput::InputDiscarded { .. }
        | ProviderOutput::OwnershipAssumed { .. }
        | ProviderOutput::Ended { .. } => None,
        ProviderOutput::Started { request, .. }
        | ProviderOutput::Audio { request, .. }
        | ProviderOutput::GenerationComplete { request }
        | ProviderOutput::Interrupted { request }
        | ProviderOutput::TurnComplete { request, .. }
        | ProviderOutput::Unclear { request, .. }
        | ProviderOutput::LookAsked { request, .. }
        | ProviderOutput::NewsAnswered { request, .. } => Some(*request),
        ProviderOutput::Transcript { request, .. } => *request,
    }
}

#[cfg(test)]
mod reminder_tests {
    use super::*;
    use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller};

    #[test]
    fn fixture_refuses_a_reminder_once_without_audio_or_disabling_later_ordinary_input() {
        let now_us = 10_000;
        let at = MonoTime::from_micros(now_us);
        let boot = BootId::new([7; 16]).unwrap();
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_capture(
                at,
                CaptureState::RetainingUntil(MonoTime::from_micros(now_us + 100_000)),
            )
            .unwrap();
        owner
            .set_admission(
                at,
                AdmissionState::OpenUntil(MonoTime::from_micros(now_us + 100_000)),
            )
            .unwrap();
        let mut fixture = FixtureProvider::new(
            Fixture {
                info: FixtureInfo {
                    wav_sha256: String::new(),
                    pcm_sha256: String::new(),
                    rate: 24_000,
                    channels: 1,
                    frames: 240,
                    rail_samples: 0,
                },
                samples: vec![1; 240],
            },
            boot,
            now_us,
        );
        let first = owner.admit(at, AdmittedInput::NewTurn).unwrap();
        let state = owner.snapshot(at).unwrap();
        fixture.install(now_us, state).unwrap();
        for _ in 0..3 {
            fixture
                .submit(
                    now_us,
                    ProviderInput::Reminder {
                        request: first.turn(),
                        session: 1,
                        privacy_generation: state.microphone_generation(),
                        text: "Tea".into(),
                        late: false,
                        late_by_ms: None,
                    },
                )
                .unwrap();
            fixture
                .install(now_us, owner.snapshot(at).unwrap())
                .unwrap();
        }
        assert!(!fixture.stopped);
        assert!(fixture.input.is_none() && fixture.reply.is_none());
        assert_eq!(fixture.events.len(), 1);
        assert!(matches!(
            fixture.events.front(),
            Some(ProviderOutput::ReminderDeferred {
                reason: crate::provider_worker::ReminderDeferral::Disabled,
                ..
            })
        ));
        owner.input_ended(at, first).unwrap();
        owner.complete_turn(at, first).unwrap();
        fixture
            .install(now_us, owner.snapshot(at).unwrap())
            .unwrap();
        let next = owner.admit(at, AdmittedInput::NewTurn).unwrap();
        let state = owner.snapshot(at).unwrap();
        fixture.install(now_us, state).unwrap();
        fixture
            .submit(
                now_us,
                ProviderInput::Start {
                    request: next.turn(),
                    session: 1,
                    privacy_generation: state.microphone_generation(),
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                },
            )
            .unwrap();
        assert_eq!(fixture.first_owner, Some(next));
        assert_eq!(fixture.input.as_ref().unwrap().owner, next);
    }
}

/// Separate worker, still using the ordinary provider IPC protocol. Control is
/// drained before every input/output step; PCM never bypasses the speaker worker.
pub fn run(mut channels: WorkerChannels, boot: BootId, wav: &Path, sha256: &str) -> io::Result<()> {
    let started_us = monotonic_us();
    let fixture = Fixture::load(wav, sha256)?;
    let loaded_us = monotonic_us();
    let mut provider = FixtureProvider::new(fixture, boot, loaded_us);
    // Unit fixtures may expose a legacy data Ready, but the actual worker uses
    // exactly the coordinated control-plane readiness gate as the live model.
    provider.pending = None;
    channels.control.send(WorkerEvent::Ready)?;
    let ready = ProviderLink {
        revision: 1,
        session: 1,
        observed_at_us: loaded_us,
        source_clock: None,
        state: ProviderLinkState::Ready {
            setup_us: loaded_us - started_us,
            attempts: 1,
            context: LinkContext::Initial,
            outage_started_at_us: None,
            after: None,
        },
    };
    ready.validate(monotonic_us())?;
    channels
        .control
        .send(WorkerEvent::ProviderLink { link: ready })?;
    loop {
        if !provider.service_step(
            monotonic_us,
            || channels.control.receive(),
            &mut channels.data,
        )? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CueKind {
    ListeningReady,
    InputAdmitted,
    LocalEndpoint,
    SpeakerFirstWrite,
    Cancelled,
    SpeechRetired,
    TurnCompleted,
    RunEnd,
    /// Unowned readiness evidence for event-driven next-turn scheduling.
    InputAvailability,
    ProviderLink,
    InputCandidate,
}
#[derive(Serialize)]
struct Cue<'a> {
    schema: u8,
    sequence: u64,
    kind: CueKind,
    boot: BootId,
    turn: Option<u64>,
    generation: Option<u64>,
    capture_epoch: Option<u64>,
    reference_epoch_context: Option<u64>,
    event_us: u64,
    sent_us: u64,
    expires_us: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    playback_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    playback_gaps: Option<u64>,
    /// `input_availability` only: the policy's prospective input availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    available: Option<bool>,
    /// `provider_link` only: `ready`, `unavailable` or `terminal`.
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<&'static str>,
}
struct FixedBytes {
    bytes: [u8; CUE_MAX_BYTES],
    len: usize,
}
impl Write for FixedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > CUE_MAX_BYTES - self.len {
            return Err(invalid("cue exceeds fixed buffer"));
        }
        self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Optional scheduling-only channel. The connected socket pins the peer endpoint;
/// no filesystem operation, heap output buffer or blocking write occurs in emit.
pub struct CueSink {
    socket: UnixDatagram,
    boot: Option<BootId>,
    sequence: u64,
    capture_epoch: Option<u64>,
    reference_epoch: Option<u64>,
    trace_offset: usize,
    fault: Option<&'static str>,
    /// This boot's listening readiness was published. Before it, retained
    /// startup state is history, not a live update.
    listening: bool,
    historical_state_skipped: u64,
}
impl CueSink {
    pub fn connect(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid("cue socket must be absolute"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| invalid("cue socket needs a private parent"))?;
        let before_dir = fs::symlink_metadata(parent)?;
        let before = fs::symlink_metadata(path)?;
        let uid = rustix::process::geteuid().as_raw();
        if !before_dir.is_dir()
            || before_dir.mode() & 0o077 != 0
            || before_dir.uid() != uid
            || !before.file_type().is_socket()
            || before.uid() != uid
        {
            return Err(invalid(
                "cue requires a same-user socket in a private non-symlink directory",
            ));
        }
        let socket = UnixDatagram::unbound()?;
        socket.set_nonblocking(true)?;
        socket.connect(path)?;
        let after = fs::symlink_metadata(path)?;
        let after_dir = fs::symlink_metadata(parent)?;
        if (before.dev(), before.ino()) != (after.dev(), after.ino())
            || (before_dir.dev(), before_dir.ino()) != (after_dir.dev(), after_dir.ino())
        {
            return Err(invalid("cue endpoint changed during connection"));
        }
        Ok(Self {
            socket,
            boot: None,
            sequence: 0,
            capture_epoch: None,
            reference_epoch: None,
            trace_offset: 0,
            fault: None,
            listening: false,
            historical_state_skipped: 0,
        })
    }
    pub fn set_boot(&mut self, boot: BootId) {
        if self.boot != Some(boot) {
            self.capture_epoch = None;
            self.reference_epoch = None;
            self.listening = false;
        }
        self.boot = Some(boot);
    }
    /// Startup state records not published because they were already older
    /// than the cue bound before this boot's listening readiness.
    pub fn historical_state_skipped(&self) -> u64 {
        self.historical_state_skipped
    }
    pub fn fault(&self) -> Option<&'static str> {
        self.fault
    }
    /// Each event is processed once, even after an error. Failure latches and
    /// returns once so the coordinator can add one trace receipt without failing audio.
    pub fn scan(&mut self, trace: &[Value], sent_us: u64) -> Option<&'static str> {
        let previous = self.fault;
        while self.trace_offset < trace.len() {
            let event = &trace[self.trace_offset];
            self.trace_offset += 1;
            self.observe_event(event, sent_us);
        }
        if previous.is_none() { self.fault } else { None }
    }
    /// Append-only service batches; context and datagram sequence survive rotation.
    pub(crate) fn scan_batch(&mut self, batch: &[Value], sent_us: u64) -> Option<&'static str> {
        let previous = self.fault;
        for event in batch {
            self.observe_event(event, sent_us);
        }
        if previous.is_none() { self.fault } else { None }
    }
    pub(crate) fn invalidate(&mut self, reason: &'static str) {
        self.fault.get_or_insert(reason);
    }
    fn observe_event(&mut self, event: &Value, sent_us: u64) {
        match event["kind"].as_str() {
            Some("capture_started") => self.capture_epoch = event["details"]["epoch"].as_u64(),
            Some("reference_clock") => {
                self.reference_epoch = event["details"]["playback_epoch"].as_u64()
            }
            Some("listening_ready") => {
                self.emit(CueKind::ListeningReady, event, sent_us);
                self.listening = true;
            }
            Some("input_admitted") => self.emit(CueKind::InputAdmitted, event, sent_us),
            Some("local_endpoint") => self.emit(CueKind::LocalEndpoint, event, sent_us),
            Some("speaker_first_write") => self.emit(CueKind::SpeakerFirstWrite, event, sent_us),
            Some("speech_final_sample_retired") => {
                self.emit(CueKind::SpeechRetired, event, sent_us)
            }
            Some("turn_finished") if event["owner"].is_object() => {
                self.emit(CueKind::Cancelled, event, sent_us)
            }
            Some("turn_finished") => self.emit(CueKind::TurnCompleted, event, sent_us),
            Some("run_end") => self.emit(CueKind::RunEnd, event, sent_us),
            Some("turn_taking_availability") if !self.historical(event, sent_us) => {
                self.emit(CueKind::InputAvailability, event, sent_us)
            }
            Some("provider_ready" | "provider_unavailable" | "provider_terminal")
                if !self.historical(event, sent_us) =>
            {
                self.emit(CueKind::ProviderLink, event, sent_us)
            }
            // A current-state publication is never history: strict bound.
            Some("provider_link_current") => self.emit(CueKind::ProviderLink, event, sent_us),
            Some("input_candidate") => self.emit(CueKind::InputCandidate, event, sent_us),
            _ => {}
        }
    }
    /// A state record retained during startup (for example a provider Ready
    /// received while later workers were still starting) and flushed after
    /// the cue bound is history: it is neither sent nor a fault, and it is
    /// never re-stamped. Only state kinds qualify, only before this boot's
    /// listening readiness; afterwards every record keeps the strict bound.
    fn historical(&mut self, event: &Value, sent_us: u64) -> bool {
        if self.listening {
            return false;
        }
        let observed = event["coordinator_observed_at_us"]
            .as_u64()
            .or_else(|| event["at_us"].as_u64());
        let stale = observed
            .and_then(|at| at.checked_add(CUE_FRESH_US))
            .is_some_and(|expires| sent_us >= expires);
        if stale {
            self.historical_state_skipped = self.historical_state_skipped.saturating_add(1);
        }
        stale
    }
    fn emit(&mut self, kind: CueKind, event: &Value, sent_us: u64) {
        if self.fault.is_some() {
            return;
        }
        let result = (|| -> io::Result<()> {
            let boot = self
                .boot
                .ok_or_else(|| invalid("cue has no session identity"))?;
            // A provider link is known here when the coordinator observed it;
            // the provider worker's own observation can be older.
            let event_us = match kind {
                CueKind::ProviderLink => event["coordinator_observed_at_us"]
                    .as_u64()
                    .or_else(|| event["at_us"].as_u64()),
                _ => event["at_us"].as_u64(),
            }
            .ok_or_else(|| invalid("cue has no event timestamp"))?;
            let expires_us = event_us
                .checked_add(CUE_FRESH_US)
                .ok_or_else(|| invalid("cue deadline overflow"))?;
            if sent_us < event_us || sent_us >= expires_us {
                return Err(invalid("cue event expired before send"));
            }
            let sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| invalid("cue sequence exhausted"))?;
            let playback_sequence =
                if matches!(kind, CueKind::SpeakerFirstWrite | CueKind::SpeechRetired) {
                    Some(
                        event["playback_sequence"]
                            .as_u64()
                            .filter(|n| *n > 0)
                            .ok_or_else(|| invalid("cue has no playback occurrence"))?,
                    )
                } else {
                    None
                };
            let (outcome, playback_gaps) = if matches!(kind, CueKind::TurnCompleted) {
                let outcome = event["outcome"]
                    .as_str()
                    .filter(|outcome| {
                        matches!(
                            *outcome,
                            "no_audio_answer"
                                | "audio_written_unscored"
                                | "audio_written_with_playback_gaps_unscored"
                                | "provider_failed"
                        )
                    })
                    .ok_or_else(|| invalid("cue has no completion outcome"))?;
                let gaps = event["playback_gaps"]
                    .as_u64()
                    .ok_or_else(|| invalid("cue has no completion gap count"))?;
                if event["generation"].as_u64().is_none_or(|n| n == 0) {
                    return Err(invalid("cue has no completed owner generation"));
                }
                (Some(outcome), Some(gaps))
            } else {
                (None, None)
            };
            let cue = Cue {
                schema: 1,
                sequence,
                kind,
                boot,
                turn: event["turn"].as_u64(),
                generation: event["owner"]["generation"]
                    .as_u64()
                    .or_else(|| event["generation"].as_u64()),
                capture_epoch: self.capture_epoch,
                reference_epoch_context: self.reference_epoch,
                event_us,
                sent_us,
                expires_us,
                reason: if matches!(kind, CueKind::Cancelled) {
                    event["outcome"].as_str()
                } else {
                    None
                },
                playback_sequence,
                outcome,
                playback_gaps,
                available: if matches!(kind, CueKind::InputAvailability) {
                    Some(
                        event["prospective_input_available"]
                            .as_bool()
                            .ok_or_else(|| invalid("cue has no input availability"))?,
                    )
                } else {
                    None
                },
                link: match (kind, event["kind"].as_str()) {
                    (CueKind::ProviderLink, Some("provider_ready")) => Some("ready"),
                    (CueKind::ProviderLink, Some("provider_unavailable")) => Some("unavailable"),
                    (CueKind::ProviderLink, Some("provider_terminal")) => Some("terminal"),
                    (CueKind::ProviderLink, Some("provider_link_current")) => {
                        match event["state"].as_str() {
                            Some("ready") => Some("ready"),
                            Some("unavailable") => Some("unavailable"),
                            Some("terminal") => Some("terminal"),
                            _ => return Err(invalid("current link record has no valid state")),
                        }
                    }
                    _ => None,
                },
            };
            let mut bytes = FixedBytes {
                bytes: [0; CUE_MAX_BYTES],
                len: 0,
            };
            serde_json::to_writer(&mut bytes, &cue).map_err(io::Error::other)?;
            if self.socket.send(&bytes.bytes[..bytes.len])? != bytes.len {
                return Err(invalid("partial cue datagram"));
            }
            self.sequence = sequence;
            Ok(())
        })();
        if let Err(error) = result {
            self.fault = Some(if error.kind() == io::ErrorKind::WouldBlock {
                "cue_backpressure"
            } else {
                "cue_invalid_or_unavailable"
            });
        }
    }
}

#[cfg(test)]
mod service_cue_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn readiness_records_become_unowned_sequenced_cues() {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let path = directory.path.join("cue.sock");
        let receiver = UnixDatagram::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        receiver.set_nonblocking(true).unwrap();
        let mut sink = CueSink::connect(&path).unwrap();
        sink.set_boot(crate::process::new_boot().unwrap());
        let at = lamp_ipc::monotonic_us();
        // The provider's own observation is older than the cue bound; the
        // coordinator observed it just now, which is what the cue carries.
        let records = [
            serde_json::json!({"kind":"turn_taking_availability","at_us":at,"policy":"v1_baseline",
                "prospective_input_available":false,"hard_capture_authority_unchanged":true}),
            serde_json::json!({"kind":"provider_ready","at_us":at - 500_000,
                "coordinator_observed_at_us":at,"link":{}}),
            serde_json::json!({"kind":"provider_terminal","at_us":at,"link":{}}),
            serde_json::json!({"kind":"input_candidate","at_us":at,
                "candidate":{"id":{"serial":1}}}),
            serde_json::json!({"kind":"turn_taking_availability","at_us":at,
                "prospective_input_available":true}),
        ];
        assert_eq!(sink.scan(&records, at + 1), None);
        let mut cues = Vec::new();
        let mut bytes = [0; 512];
        while let Ok(n) = receiver.recv(&mut bytes) {
            cues.push(serde_json::from_slice::<Value>(&bytes[..n]).unwrap());
        }
        let shape: Vec<_> = cues
            .iter()
            .map(|cue| {
                (
                    cue["kind"].clone(),
                    cue["available"].clone(),
                    cue["link"].clone(),
                )
            })
            .collect();
        let (null, text) = (Value::Null, |v: &str| Value::from(v));
        assert_eq!(
            shape,
            [
                (text("input_availability"), Value::from(false), null.clone()),
                (text("provider_link"), null.clone(), text("ready")),
                (text("provider_link"), null.clone(), text("terminal")),
                (text("input_candidate"), null.clone(), null.clone()),
                (text("input_availability"), Value::from(true), null),
            ]
        );
        for (index, cue) in cues.iter().enumerate() {
            assert_eq!(cue["sequence"], index as u64 + 1);
            assert!(
                cue["turn"].is_null() && cue["generation"].is_null(),
                "{cue}"
            );
        }
        assert_eq!(cues[1]["event_us"], at);
        // An availability record without its value is a cue fault, never a guess.
        let mut faulty = CueSink::connect(&path).unwrap();
        faulty.set_boot(crate::process::new_boot().unwrap());
        let missing = [serde_json::json!({"kind":"turn_taking_availability","at_us":at})];
        assert_eq!(
            faulty.scan(&missing, at + 1),
            Some("cue_invalid_or_unavailable")
        );
    }

    #[test]
    fn new_service_boot_clears_old_epochs_without_reusing_sequence_or_repairing_fault() {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let path = directory.path.join("cue.sock");
        let receiver = UnixDatagram::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        receiver.set_nonblocking(true).unwrap();
        let mut sink = CueSink::connect(&path).unwrap();
        let first = crate::process::new_boot().unwrap();
        let next = crate::process::new_boot().unwrap();
        sink.set_boot(first);
        sink.capture_epoch = Some(7);
        sink.reference_epoch = Some(9);
        sink.sequence = 41;
        sink.set_boot(next);
        assert_eq!(sink.capture_epoch, None);
        assert_eq!(sink.reference_epoch, None);
        let at = lamp_ipc::monotonic_us();
        sink.scan_batch(
            &[serde_json::json!({"kind":"listening_ready","at_us":at})],
            at,
        );
        let mut bytes = [0; 512];
        let n = receiver.recv(&mut bytes).unwrap();
        let cue: Value = serde_json::from_slice(&bytes[..n]).unwrap();
        assert_eq!(cue["boot"], serde_json::to_value(next).unwrap());
        assert_eq!(cue["sequence"], 42);
        assert!(cue["capture_epoch"].is_null());
        assert!(cue["reference_epoch_context"].is_null());
        sink.invalidate("service_cue_history_lost");
        sink.set_boot(crate::process::new_boot().unwrap());
        assert_eq!(sink.fault(), Some("service_cue_history_lost"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn separate_service_batches_preserve_source_context_and_sequence_without_row_indexes() {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let path = directory.path.join("cue.sock");
        let receiver = UnixDatagram::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        receiver.set_nonblocking(true).unwrap();
        let mut sink = CueSink::connect(&path).unwrap();
        sink.set_boot(crate::process::new_boot().unwrap());
        let at = lamp_ipc::monotonic_us();
        assert_eq!(
            sink.scan_batch(
                &[
                    serde_json::json!({"kind":"capture_started","details":{"epoch":3}}),
                    serde_json::json!({"kind":"reference_clock","details":{"playback_epoch":5}}),
                    serde_json::json!({"kind":"listening_ready","at_us":at}),
                ],
                at
            ),
            None
        );
        let mut bytes = [0; 512];
        let count = receiver.recv(&mut bytes).unwrap();
        let first: Value = serde_json::from_slice(&bytes[..count]).unwrap();
        assert_eq!(first["sequence"], 1);
        assert_eq!(first["capture_epoch"], 3);
        assert_eq!(first["reference_epoch_context"], 5);
        assert_eq!(
            sink.scan_batch(&[serde_json::json!({"kind":"run_end","at_us":at})], at),
            None
        );
        let count = receiver.recv(&mut bytes).unwrap();
        let second: Value = serde_json::from_slice(&bytes[..count]).unwrap();
        assert_eq!(second["sequence"], 2);
        assert_eq!(second["capture_epoch"], 3);
        sink.invalidate("service_cue_history_lost");
        sink.scan_batch(
            &[serde_json::json!({"kind":"listening_ready","at_us":at})],
            at,
        );
        assert_eq!(
            receiver.recv(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(sink.fault(), Some("service_cue_history_lost"));
    }
}
