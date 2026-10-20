//! The capture worker's own loop with the capture hold, over a scripted
//! microphone: the controller's side of its channels and the speaker's side of
//! its reference link are played here, in real time, through the same
//! sockets and messages the lamp uses.
//!
//! The microphone is given the lamp's situation with `mic1`: the sound of a
//! render block is in the microphone signal 30 ms after the speaker accepted
//! it, which is two capture blocks before the capture-clocked reference
//! gives that block to the canceller.
//!
//! This is the production loop, handshake and canceller. It is not a lamp:
//! the microphone is scripted, the room is a delay and a gain, and the clock
//! is this host's.
#![cfg(target_os = "linux")]
use lamp_audio::{
    CAPTURE_SAMPLES, RENDER_SAMPLES,
    duplex::Hearing,
    testing::{CaptureControl, ScriptedCapture},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission, TurnOwner};
use lamp_voice::{
    audio_workers::capture_on,
    options::{Microphone, NoiseSuppression, ReferenceClock, ReferenceRecovery},
    process::{SessionDirectory, new_boot},
    reference::{PRIME_FRAMES, hold},
    stop_words::Source,
    transport::{ReferenceChannels, WorkerChannels},
    wire::{
        ClockPrimed, Control, MicrophoneFrame, ReferenceControl, ReferenceFaultKind,
        ReferenceTrust, RenderPayload, RenderReference, WorkerEvent,
    },
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Once,
    thread::{JoinHandle, sleep},
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_millis(10);
/// The speaker's sound reaches the microphone this many ticks after the
/// speaker accepted it; the reference ring gives it to the canceller after
/// five.
const ECHO_TICKS: usize = 3;

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}

fn level(power: f64) -> f64 {
    10.0 * (power.max(1e-3) / 32_768f64.powi(2)).log10()
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// Noise under 2 kHz at 48 kHz, read at the speaker's rate and at the
/// microphone's.
struct Signal {
    base: Vec<f32>,
}

impl Signal {
    fn new(ticks: usize) -> Self {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut white = vec![0f32; (ticks + 8) * 480 + 24];
        for sample in &mut white {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *sample = (state >> 40) as f32 / (1u64 << 23) as f32 - 1.0;
        }
        let smooth = |input: &[f32]| -> Vec<f32> {
            input
                .windows(12)
                .map(|window| window.iter().sum::<f32>() / 12.0)
                .collect()
        };
        let mut base = smooth(&smooth(&white));
        let rms = (base.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / base.len() as f64)
            .sqrt() as f32;
        for sample in &mut base {
            *sample *= 3_000.0 / rms;
        }
        Self { base }
    }

    /// What the speaker accepts in a tick.
    fn render(&self, tick: usize) -> Vec<i16> {
        (0..RENDER_SAMPLES)
            .map(|i| self.base[tick * 480 + 2 * i] as i16)
            .collect()
    }

    /// What the microphone holds in a tick: half of what the speaker
    /// accepted [`ECHO_TICKS`] earlier.
    fn microphone(&self, tick: usize) -> [i16; CAPTURE_SAMPLES] {
        let mut block = [0; CAPTURE_SAMPLES];
        if let Some(start) = (tick * 480).checked_sub(ECHO_TICKS * 480) {
            for (j, sample) in block.iter_mut().enumerate() {
                *sample = (0.5 * self.base[start + 3 * j]) as i16;
            }
        }
        block
    }
}

struct Heard {
    sequence: u64,
    read_at: u64,
    processed_at: u64,
    power: f64,
    explained: Option<u8>,
    /// The block before the canceller's power, when the worker was asked
    /// for raw blocks (#1047).
    raw_power: Option<f64>,
    /// The canceller's linear output's power, when the worker was asked
    /// for linear blocks (#828).
    linear_power: Option<f64>,
    /// Whether `samples` passed the canceller (the reference trusted or
    /// warming).
    cancelled: bool,
}

/// One rig at a time. Each runs the capture worker's loop in real time,
/// canceller included, against a 40 ms bound on unread microphone and old
/// render; with the file's tests in parallel on a loaded machine those
/// workers starved each other, and 2 to 4 of 11 ended on that bound at their
/// first frames (Eighth on #705, 18 October: `ReferenceBacklog after 0
/// frames`, `Discontinuity(DeviceError) after 1 frames`). The lamp runs one.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Rig {
    _directory: SessionDirectory,
    controller: Controller,
    parent: WorkerChannels,
    speaker: ReferenceChannels,
    device: CaptureControl,
    worker: Option<JoinHandle<Result<(), String>>>,
    /// Where the worker records, when it does.
    recording: Option<std::path::PathBuf>,
    owner: TurnOwner,
    privacy: u64,
    origin: Instant,
    authority_at: Option<Instant>,
    pushed_at: Vec<u64>,
    /// Blocks the speaker has accepted, in the stream and in a hold-up.
    references: u64,
    frames: Vec<Heard>,
    ready: bool,
    prepared: bool,
    started: bool,
    stopped: bool,
    /// The epoch of each start the worker reported, and its gaps.
    epochs: Vec<u64>,
    gaps: usize,
    /// A reference fault and a refused send are recorded, and a refused
    /// receive ends the drain, not the test (#1178, #1351): the worker is
    /// expected to end.
    tolerate_fault: bool,
    fault: Option<ReferenceFaultKind>,
    refused: Option<std::io::Error>,
    /// The worker's own fault, recorded the same way.
    worker_fault: Option<String>,
    /// The worker has been sent what ends it, and nothing more is sent: on
    /// Linux a send that finds it gone drops its `CaptureStopped` unread
    /// (`a_refused_send_drops_what_the_gone_peer_had_sent`, #1359).
    ending: bool,
    /// Held for the rig's life ([`ONE_AT_A_TIME`]). The last field, so a
    /// rig dropped mid-test closes its channels before the next rig starts.
    _alone: std::sync::MutexGuard<'static, ()>,
}

impl Rig {
    /// Everything up to the speaker's start notice: the worker has been given
    /// authority, the reference peer and the start request, has prepared the
    /// microphone, and has taken the speaker's prime.
    fn start(device_name: &'static str) -> Self {
        Self::start_busy(device_name, 0)
    }

    /// The same, with the microphone's first `busy_opens` opens answered
    /// "busy", as a voice that ran just before can leave it.
    fn start_busy(device_name: &'static str, busy_opens: usize) -> Self {
        Self::start_with(device_name, busy_opens, ReferenceRecovery::Off)
    }

    fn start_with(
        device_name: &'static str,
        busy_opens: usize,
        recovery: ReferenceRecovery,
    ) -> Self {
        Self::start_asking(device_name, busy_opens, recovery, Source::Processed)
    }

    /// The same, with the microphone's first `gone_opens` opens answered "no
    /// such device", as a microphone off USB for a moment leaves it.
    fn start_gone(device_name: &'static str, gone_opens: usize) -> Self {
        Self::start_failing(
            device_name,
            (gone_opens, lamp_audio::device::errno::NODEV),
            ReferenceRecovery::Off,
            Source::Processed,
        )
    }

    /// The same, with the stop words' blocks asked for or not
    /// (`--raw-blocks`, #1047; `--linear-blocks`, #828).
    fn start_asking(
        device_name: &'static str,
        busy_opens: usize,
        recovery: ReferenceRecovery,
        stop_words: Source,
    ) -> Self {
        Self::start_failing(
            device_name,
            (busy_opens, lamp_audio::device::errno::BUSY),
            recovery,
            stop_words,
        )
    }

    fn start_failing(
        device_name: &'static str,
        failed: (usize, i32),
        recovery: ReferenceRecovery,
        stop_words: Source,
    ) -> Self {
        let mut rig = Self::spawn(device_name, failed, recovery, stop_words, false, false);
        rig.handshake();
        rig
    }

    /// The same, with the loop waiting on the microphone between passes
    /// (`LAMP_CAPTURE_WAIT=on`, #705) or ticking.
    fn start_waiting(device_name: &'static str, wait: bool) -> Self {
        let mut rig = Self::spawn(
            device_name,
            (0, lamp_audio::device::errno::BUSY),
            ReferenceRecovery::Off,
            Source::Processed,
            false,
            wait,
        );
        rig.handshake();
        rig
    }

    /// The same, with the worker recording (`--diagnostics`) beside it.
    fn start_recording(device_name: &'static str) -> Self {
        let mut rig = Self::spawn(
            device_name,
            (0, lamp_audio::device::errno::BUSY),
            ReferenceRecovery::Off,
            Source::Processed,
            true,
            false,
        );
        rig.handshake();
        rig
    }

    /// The worker started and asked to capture, before its handshake.
    fn spawn(
        device_name: &'static str,
        (failed_opens, errno): (usize, i32),
        recovery: ReferenceRecovery,
        stop_words: Source,
        record: bool,
        wait: bool,
    ) -> Self {
        // A test that failed while holding it leaves it poisoned; the next
        // one still runs alone.
        let alone = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = SessionDirectory::create().unwrap();
        let recording = record.then(|| directory.path.join("capture-audio"));
        let diagnostics = recording.clone();
        let (parent_boot, worker_boot, speaker_boot) = (
            new_boot().unwrap(),
            new_boot().unwrap(),
            new_boot().unwrap(),
        );
        let mut parent =
            WorkerChannels::bind(&directory.path, "capture", parent_boot, worker_boot, false)
                .unwrap();
        let mut channels =
            WorkerChannels::bind(&directory.path, "capture", parent_boot, worker_boot, true)
                .unwrap();
        parent.connect(&directory.path, "capture", false).unwrap();
        channels.connect(&directory.path, "capture", true).unwrap();
        let speaker =
            ReferenceChannels::bind(&directory.path, "speaker", speaker_boot, worker_boot).unwrap();
        let device = CaptureControl::register(device_name);
        for _ in 0..failed_opens {
            device.fail_next(lamp_audio::device::OPEN_CALL, errno);
        }
        let path = directory.path.clone();
        // Named, for `capture_wait_cost` to find its counts in /proc.
        let worker = std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                capture_on::<ScriptedCapture>(
                    channels,
                    parent_boot,
                    worker_boot,
                    &path,
                    device_name,
                    diagnostics.as_deref(),
                    NoiseSuppression::Off,
                    ReferenceClock::Capture,
                    recovery,
                    lamp_audio::Reference::AsGiven,
                    stop_words,
                    wait,
                )
                .map_err(|error| error.to_string())
            })
            .unwrap();
        let mut controller = Controller::new(parent_boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let mut rig = Self {
            _directory: directory,
            controller,
            parent,
            speaker,
            device,
            worker: Some(worker),
            recording,
            owner: owner(parent_boot),
            privacy: 0,
            origin: Instant::now(),
            authority_at: None,
            pushed_at: Vec::new(),
            references: 0,
            frames: Vec::new(),
            ready: false,
            prepared: false,
            started: false,
            stopped: false,
            epochs: Vec::new(),
            gaps: 0,
            tolerate_fault: false,
            worker_fault: None,
            fault: None,
            refused: None,
            ending: false,
            _alone: alone,
        };
        rig.service();
        rig.parent
            .control
            .send(Control::ConnectReference { peer: speaker_boot })
            .unwrap();
        rig.parent.control.send(Control::StartCapture).unwrap();
        rig
    }

    fn handshake(&mut self) {
        let rig = self;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "no handshake: ready {} prepared {}",
                rig.ready,
                rig.prepared
            );
            rig.service();
            if rig.speaker.connect_step().unwrap() {
                while let Some(control) = rig.speaker.control.receive().unwrap() {
                    match control {
                        ReferenceControl::CapturePrepared { privacy_generation } => {
                            rig.privacy = privacy_generation;
                            let at = monotonic_us();
                            let prime = ClockPrimed {
                                playback_epoch: 1,
                                privacy_generation,
                                accepted_zero_frames: PRIME_FRAMES,
                                accepted_at_us: at,
                                queue_observed_at_us: at,
                                queued_frames: PRIME_FRAMES,
                                discarded: None,
                            };
                            rig.speaker
                                .control
                                .send(ReferenceControl::ClockPrimed { prime })
                                .unwrap();
                        }
                        ReferenceControl::ReferencePrimed { prepared } => {
                            assert_eq!(prepared.playback_epoch, 1);
                            assert_eq!(prepared.privacy_generation, rig.privacy);
                            let at = monotonic_us();
                            rig.speaker
                                .control
                                .send(ReferenceControl::ClockStarted {
                                    playback_epoch: 1,
                                    start_requested_at_us: at,
                                    started_at_us: at,
                                })
                                .unwrap();
                            assert!(rig.ready && rig.prepared);
                            rig.origin = Instant::now();
                            return;
                        }
                        _ => panic!("the capture worker sent the speaker something else"),
                    }
                }
            }
            sleep(Duration::from_micros(500));
        }
    }

    /// Renew authority as the coordinator does, until the worker has been
    /// sent what ends it, and take whatever the worker has sent.
    fn service(&mut self) {
        if !self.ending
            && self
                .authority_at
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(40))
        {
            let snapshot = self.controller.snapshot(now()).unwrap();
            let sent = self.parent.control.send(Control::Authority { snapshot });
            match sent {
                Err(error) if self.tolerate_fault => {
                    self.refused.get_or_insert(error);
                }
                sent => sent.unwrap(),
            }
            self.authority_at = Some(Instant::now());
        }
        loop {
            let event = match self.parent.control.receive::<WorkerEvent>() {
                // Once the worker has gone, the receive itself can be
                // refused (ECONNREFUSED on Linux): nothing more is read. It
                // isn't kept in `refused`, which holds the send's (#1351).
                Err(_) if self.tolerate_fault => break,
                received => match received.unwrap() {
                    Some(event) => event,
                    None => break,
                },
            };
            match event {
                WorkerEvent::Ready => self.ready = true,
                WorkerEvent::CapturePrepared { .. } => self.prepared = true,
                WorkerEvent::CaptureStarted { epoch, .. } => {
                    self.started = true;
                    self.epochs.push(epoch);
                }
                WorkerEvent::CaptureGap { .. } => self.gaps += 1,
                WorkerEvent::CaptureStopped => self.stopped = true,
                WorkerEvent::ReferenceFault { fault } if self.tolerate_fault => {
                    self.fault.get_or_insert(fault.reason);
                }
                WorkerEvent::ReferenceFault { fault } => {
                    panic!(
                        "reference fault {:?} after {} frames: at {} us, last render accepted at {} us, \
                         {} capture blocks processed, {} microphone samples unread",
                        fault.reason,
                        self.frames.len(),
                        fault.at_us,
                        fault.timing.accepted_at_us,
                        fault.timing.capture_blocks_processed,
                        self.device.queued()
                    )
                }
                WorkerEvent::Fault { code } if self.tolerate_fault => {
                    self.worker_fault.get_or_insert(code);
                }
                WorkerEvent::Fault { code } => {
                    panic!("worker fault {code} after {} frames", self.frames.len())
                }
                _ => {}
            }
        }
        loop {
            let frame = match self.parent.data.receive::<MicrophoneFrame>() {
                Err(_) if self.tolerate_fault => break,
                received => match received.unwrap() {
                    Some(frame) => frame,
                    None => break,
                },
            };
            assert_eq!(frame.samples.len(), CAPTURE_SAMPLES);
            assert_eq!(frame.privacy_generation, self.privacy);
            self.frames.push(Heard {
                sequence: frame.frame_sequence,
                read_at: frame.read_completed_at_us,
                processed_at: frame.timing.processing_completed_at_us,
                power: frame
                    .samples
                    .iter()
                    .map(|&s| f64::from(s).powi(2))
                    .sum::<f64>()
                    / CAPTURE_SAMPLES as f64,
                explained: frame
                    .echo
                    .and_then(|evidence| evidence.echo)
                    .map(|matched| matched.capture_explained),
                raw_power: frame.raw.as_ref().map(|raw| {
                    assert_eq!(raw.len(), CAPTURE_SAMPLES);
                    raw.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / CAPTURE_SAMPLES as f64
                }),
                cancelled: frame.reference != ReferenceTrust::Untrusted,
                linear_power: frame.linear.as_ref().map(|linear| {
                    assert_eq!(linear.len(), CAPTURE_SAMPLES);
                    linear.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>()
                        / CAPTURE_SAMPLES as f64
                }),
            });
        }
    }

    /// The speaker accepts a block and the microphone holds a block, once
    /// every 10 ms of this host's clock.
    fn stream(&mut self, signal: &Signal, ticks: usize) {
        let from = self.pushed_at.len();
        for tick in from..from + ticks {
            let due = self.origin + TICK * tick as u32;
            while Instant::now() < due {
                self.service();
                sleep(Duration::from_micros(300));
            }
            self.accept(signal.render(tick));
            // Stamped before the push: a worker waiting on the microphone
            // (LAMP_CAPTURE_WAIT) may read the block before the push returns.
            let at = monotonic_us();
            self.device.push(&signal.microphone(tick));
            self.pushed_at.push(at);
            self.service();
        }
    }

    /// The microphone holds a block every 10 ms for `ticks`, and the speaker
    /// accepts each block's render `late` after it: a lamp's speaker sends
    /// on its own clock, not beside each block.
    fn stream_render(&mut self, signal: &Signal, ticks: usize, late: Duration) {
        let from = self.pushed_at.len();
        for tick in from..from + ticks {
            let due = self.origin + TICK * tick as u32;
            while Instant::now() < due {
                self.service();
                sleep(Duration::from_micros(300));
            }
            let at = monotonic_us();
            self.device.push(&signal.microphone(tick));
            self.pushed_at.push(at);
            while Instant::now() < due + late {
                self.service();
                sleep(Duration::from_micros(300));
            }
            self.accept(signal.render(tick));
            self.service();
        }
    }

    /// The speaker accepts one more block.
    fn accept(&mut self, samples: Vec<i16>) {
        self.try_accept(samples).unwrap();
    }

    /// The same, saying whether the reference reached the worker's socket.
    fn try_accept(&mut self, samples: Vec<i16>) -> std::io::Result<()> {
        let at = monotonic_us();
        self.speaker.data.send(RenderReference {
            playback_epoch: 1,
            privacy_generation: self.privacy,
            sequence: self.references + 1,
            accepted_at_us: at,
            queue_observed_at_us: at,
            first_sample: PRIME_FRAMES as u64 + self.references * RENDER_SAMPLES as u64,
            queued_frames: 960,
            payload: RenderPayload::Speech {
                owner: self.owner,
                samples,
            },
        })?;
        self.references += 1;
        Ok(())
    }

    /// The microphone holds a block every 10 ms for `ticks`, and the speaker
    /// accepts nothing, as a speaker held up does (#1178): its last packet,
    /// and the 40 ms its queue held, grow old.
    fn speaker_quiet(&mut self, signal: &Signal, ticks: usize) {
        let from = self.pushed_at.len();
        for tick in from..from + ticks {
            let due = self.origin + TICK * tick as u32;
            while Instant::now() < due {
                self.service();
                sleep(Duration::from_micros(300));
            }
            let at = monotonic_us();
            self.device.push(&signal.microphone(tick));
            self.pushed_at.push(at);
            self.service();
        }
    }

    /// No authority from the controller and nothing in the microphone for
    /// `ms`, as a worker held up by the kernel finds it when it runs again:
    /// its authority lapsed. The speaker goes on accepting a block every
    /// tick through it, so reference reaches the worker while its
    /// microphone is closed.
    fn hold_up(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            self.accept(vec![0; RENDER_SAMPLES]);
            sleep(TICK);
        }
        self.origin += Duration::from_millis(ms);
    }

    /// Wait until the worker has read every block the microphone held and
    /// handed on all it is going to.
    fn settle(&mut self, hold: usize) {
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.device.queued() > 0 || self.frames.len() + hold < self.pushed_at.len() {
            assert!(
                Instant::now() < deadline,
                "{} frames of {} blocks, {} samples unread",
                self.frames.len(),
                self.pushed_at.len(),
                self.device.queued()
            );
            self.service();
            sleep(Duration::from_micros(500));
        }
    }

    /// Send the worker what ends it (`Stop`, or authority that closes the
    /// microphone), and nothing after it.
    fn end_with(&mut self, control: Control) {
        self.parent.control.send(control).unwrap();
        self.ending = true;
    }

    /// The worker has ended: nothing more arrives, and its loop returned
    /// without a fault.
    fn finish(mut self) -> Vec<Heard> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.stopped {
            assert!(Instant::now() < deadline, "the worker did not stop");
            self.service();
            sleep(Duration::from_micros(500));
        }
        let result = self.worker.take().unwrap().join().unwrap();
        assert_eq!(result, Ok(()));
        let before = self.frames.len();
        for _ in 0..50 {
            if let Some(frame) = self.parent.data.receive::<MicrophoneFrame>().unwrap() {
                panic!("frame {} after the stop", frame.frame_sequence);
            }
            sleep(Duration::from_millis(1));
        }
        assert_eq!(self.frames.len(), before);
        self.frames
    }
}

/// One session run and thrown away before the first one that counts.
///
/// The worker ends a session when the microphone has more than 40 ms unread
/// or a render packet is more than 40 ms old, as it should. The first time a
/// process runs the canceller under emulation, its first blocks take longer
/// than that, and the session ends on the first frame. What a session does
/// once the process is running is what these tests are about, so up to three
/// are spent first, until one runs through. How many it took is printed.
fn warm_up() {
    static WARM: Once = Once::new();
    WARM.call_once(|| {
        let signal = Signal::new(100);
        for attempt in 1..=3 {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                let mut rig = Rig::start("scripted warm-up");
                rig.stream(&signal, 100);
                rig.settle(0);
                rig.end_with(Control::Stop);
                rig.finish();
            }));
            if ran.is_ok() {
                eprintln!("warm-up: session {attempt} ran through");
                return;
            }
            eprintln!("warm-up: session {attempt} did not run through");
        }
    });
}

fn owner(boot: BootId) -> TurnOwner {
    serde_json::from_value(serde_json::json!({
        "boot": serde_json::to_value(boot).unwrap(),
        "turn": 1,
        "generation": 1,
    }))
    .unwrap()
}

/// Order, capture times and the wait of every frame handed on.
fn check(frames: &[Heard], pushed_at: &[u64], hold: usize) -> (u64, u64) {
    assert_eq!(frames.len() + hold, pushed_at.len());
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!(frame.sequence, index as u64 + 1);
        // Read after the microphone held the block, and never after it was
        // processed: the time is the read's own.
        assert!(frame.read_at >= pushed_at[index], "{index}");
        assert!(frame.read_at <= frame.processed_at, "{index}");
        if index > 0 {
            assert!(frame.read_at > frames[index - 1].read_at, "{index}");
        }
    }
    let read_after_push = median(
        frames
            .iter()
            .zip(pushed_at)
            .map(|(frame, pushed)| frame.read_at - pushed)
            .collect(),
    );
    let waited = median(
        frames
            .iter()
            .map(|frame| frame.processed_at - frame.read_at)
            .collect(),
    );
    (read_after_push, waited)
}

/// Level of the microphone signal and of what was handed on over the last
/// second, and how much of the microphone the render explained there.
fn cancellation(frames: &[Heard], signal: &Signal) -> (f64, f64, Option<u8>) {
    let last = &frames[frames.len() - 100..];
    let after = last.iter().map(|frame| frame.power).sum::<f64>() / 100.0;
    let before = last
        .iter()
        .map(|frame| {
            signal
                .microphone(frame.sequence as usize - 1)
                .iter()
                .map(|&s| f64::from(s).powi(2))
                .sum::<f64>()
                / CAPTURE_SAMPLES as f64
        })
        .sum::<f64>()
        / 100.0;
    let mut explained: Vec<u8> = last.iter().filter_map(|frame| frame.explained).collect();
    explained.sort_unstable();
    (
        level(before),
        level(after),
        explained.get(explained.len() / 2).copied(),
    )
}

#[test]
fn mic1_frames_wait_four_blocks_with_their_own_times_and_a_stop_drops_what_is_held() {
    warm_up();
    let name = Microphone::Mic1.device();
    let blocks = hold::blocks_for(name, ReferenceClock::Capture);
    assert_eq!(blocks, 4);
    let signal = Signal::new(400);
    let mut rig = Rig::start(name);
    rig.stream(&signal, 400);
    rig.settle(blocks);
    assert!(rig.started);
    // Four blocks have been read and not handed on. Stop now.
    assert_eq!(rig.frames.len() + 4, rig.pushed_at.len());
    rig.end_with(Control::Stop);
    let pushed_at = rig.pushed_at.clone();
    let frames = rig.finish();
    let (read_after_push, waited) = check(&frames, &pushed_at, blocks);
    let (before, after, explained) = cancellation(&frames, &signal);
    eprintln!(
        "mic1, hold 4: {} frames of {} blocks; read {read_after_push} us after the block was there, \
         handed on {waited} us after the read (medians); last second {before:.1} dBFS before, \
         {after:.1} dBFS after, render explains {explained:?} percent",
        frames.len(),
        pushed_at.len()
    );
    // Each frame carries the time of its own read, which is when the
    // microphone held it, and is handed on four blocks later.
    assert!(read_after_push < 10_000, "{read_after_push}");
    assert!((35_000..60_000).contains(&waited), "{waited}");
    // The canceller had the render before the sound and accounts for it.
    // The level alone does not show that: on this noise the canceller's
    // suppression lowers the output as far with no hold.
    assert!(
        explained.is_some_and(|percent| percent > Hearing::EVALUATED.explained_percent),
        "{explained:?}"
    );
}

/// The reviewer on #1430: the recorder's held steps from the real loop. With
/// `mic1` and a recording, the four steps before the first block is handed
/// on are kept as `reference_step` records ahead of capture 1, naming blocks
/// 1 to 4 with counts 1 to 4; every capture record after carries a count four
/// ahead of its block; the recording is complete; and the blocks handed on
/// are the ones a run without a recording hands on.
#[test]
fn mic1_with_a_recording_keeps_the_held_steps_before_the_first_block() {
    warm_up();
    let name = Microphone::Mic1.device();
    let signal = Signal::new(200);
    let mut rig = Rig::start_recording(name);
    rig.stream(&signal, 200);
    rig.settle(4);
    rig.end_with(Control::Stop);
    let directory = rig.recording.clone().unwrap();
    let pushed_at = rig.pushed_at.clone();
    let frames = rig.finish();
    check(&frames, &pushed_at, 4);
    assert!(directory.join("complete.json").exists(), "not complete");
    let events = std::fs::read_to_string(directory.join("events.jsonl")).unwrap();
    let rows: Vec<serde_json::Value> = events
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let steps: Vec<_> = rows
        .iter()
        .filter(|row| row["kind"] == "reference_step" || row["kind"] == "capture")
        .collect();
    for (index, row) in steps.iter().take(4).enumerate() {
        assert_eq!(row["kind"], "reference_step", "{index}");
        assert_eq!(row["frames"], 0);
        assert_eq!(row["meta"]["frame_sequence"], index as u64 + 1);
        assert_eq!(
            row["meta"]["reference"]["capture_blocks_processed"],
            index as u64 + 1
        );
    }
    let captures: Vec<_> = steps.iter().skip(4).collect();
    assert!(captures.iter().all(|row| row["kind"] == "capture"));
    assert_eq!(captures.len(), frames.len());
    for (index, row) in captures.iter().enumerate() {
        assert_eq!(row["meta"]["frame_sequence"], index as u64 + 1);
        assert_eq!(
            row["meta"]["reference"]["capture_blocks_processed"],
            index as u64 + 5
        );
    }
    // What the microphone held, recorded before the canceller, block for
    // block: the hold changed what is handed on, not which blocks.
    let pre = std::fs::read(directory.join("pre_aec.pcm16le")).unwrap();
    for (index, block) in pre.chunks_exact(CAPTURE_SAMPLES * 2).enumerate() {
        let samples: Vec<i16> = block
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(samples, signal.microphone(index), "{index}");
    }
}

#[test]
fn mic1_muted_while_frames_are_held_hands_none_of_them_on() {
    warm_up();
    let name = Microphone::Mic1.device();
    let signal = Signal::new(100);
    let mut rig = Rig::start(name);
    rig.stream(&signal, 100);
    rig.settle(4);
    assert_eq!(rig.frames.len() + 4, rig.pushed_at.len());
    rig.controller
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    let snapshot = rig.controller.snapshot(now()).unwrap();
    rig.end_with(Control::Authority { snapshot });
    let pushed_at = rig.pushed_at.clone();
    let frames = rig.finish();
    check(&frames, &pushed_at, 4);
    assert_eq!(frames.last().map(|frame| frame.sequence), Some(96));
}

#[test]
fn another_microphone_is_handed_on_at_once_and_its_early_sound_is_not_accounted_for() {
    warm_up();
    let name = Microphone::Mic2.device();
    assert_eq!(hold::blocks_for(name, ReferenceClock::Capture), 0);
    let signal = Signal::new(400);
    let mut rig = Rig::start(name);
    rig.stream(&signal, 400);
    rig.settle(0);
    assert_eq!(rig.frames.len(), rig.pushed_at.len());
    rig.end_with(Control::Stop);
    let pushed_at = rig.pushed_at.clone();
    let frames = rig.finish();
    let (read_after_push, waited) = check(&frames, &pushed_at, 0);
    let (before, after, explained) = cancellation(&frames, &signal);
    eprintln!(
        "no hold: {} frames of {} blocks; read {read_after_push} us after the block was there, \
         handed on {waited} us after the read (medians); last second {before:.1} dBFS before, \
         {after:.1} dBFS after, render explains {explained:?} percent",
        frames.len(),
        pushed_at.len()
    );
    assert!(read_after_push < 10_000, "{read_after_push}");
    assert!(waited < 10_000, "{waited}");
    // The same sound two blocks ahead of its render: the canceller has
    // nothing that accounts for it.
    assert!(
        explained.is_none_or(|percent| percent <= Hearing::EVALUATED.explained_percent),
        "{explained:?}"
    );
}

/// #1047 and #828: every frame also carries what the stop words were asked
/// for, and only that (mic1, which holds the echo four blocks behind its
/// render, as the first test here).
/// - Raw blocks are the block as the microphone recorded it, before the
///   canceller: louder than what the canceller lets through of the lamp's
///   echo.
/// - Linear blocks are the canceller's linear output, on every block it ran
///   for: most of the echo is gone, but more is left than in the
///   canceller's output, which its suppressor takes on from there. On this
///   rig (x86_64 Linux, 19 October), over the last 100 blocks: raw 2.2e8,
///   linear 0.9e6 to 1.1e6, the canceller's output 0.2e6 to 0.3e6.
/// - With the canceller's output, nothing more.
#[test]
fn stop_words_blocks_carry_what_was_asked_only_when_asked() {
    warm_up();
    let name = Microphone::Mic1.device();
    let blocks = hold::blocks_for(name, ReferenceClock::Capture);
    // The raw blocks' power, from the first run, for the linear run's.
    let mut raw_heard = None;
    for source in [Source::Raw, Source::Linear, Source::Processed] {
        let signal = Signal::new(400);
        let mut rig = Rig::start_asking(name, 0, ReferenceRecovery::Off, source);
        rig.stream(&signal, 400);
        rig.settle(blocks);
        rig.end_with(Control::Stop);
        let frames = rig.finish();
        assert!(frames.len() >= 200, "{source:?}: {} frames", frames.len());
        for frame in &frames {
            assert_eq!(
                frame.raw_power.is_some(),
                source == Source::Raw,
                "{source:?}"
            );
            assert_eq!(
                frame.linear_power.is_some(),
                source == Source::Linear && frame.cancelled,
                "{source:?}"
            );
        }
        let last = &frames[frames.len() - 100..];
        let clean: f64 = last.iter().map(|frame| frame.power).sum();
        match source {
            Source::Raw => {
                let raw: f64 = last.iter().filter_map(|frame| frame.raw_power).sum();
                assert!(raw > clean, "raw {raw:.0}, after the canceller {clean:.0}");
                raw_heard = Some(raw);
            }
            Source::Linear => {
                let linear: Vec<f64> = last.iter().filter_map(|frame| frame.linear_power).collect();
                assert!(linear.len() >= 90, "{} of the last 100", linear.len());
                let linear: f64 = linear.iter().sum();
                let raw = raw_heard.unwrap();
                assert!(
                    linear * 10.0 < raw,
                    "linear {linear:.0}, the raw blocks {raw:.0}"
                );
                assert!(
                    linear > clean,
                    "linear {linear:.0}, after the canceller {clean:.0}"
                );
            }
            Source::Processed => {}
        }
    }
}

/// A microphone still busy for a moment when the worker starts (left so by
/// the voice that ran before) is opened once it is free: the worker prepares
/// it and the handshake completes, instead of the worker ending.
#[test]
fn a_microphone_busy_for_a_moment_at_start_is_opened_once_it_is_free() {
    warm_up();
    let signal = Signal::new(50);
    let mut rig = Rig::start_busy("scripted busy at start", 3);
    assert!(rig.ready && rig.prepared);
    rig.stream(&signal, 50);
    rig.end_with(Control::Stop);
    rig.finish();
}

/// A microphone off USB for a moment when the worker starts (lamp-4ace,
/// 2026-10-17, #1178: 4-5 s, twice) is opened once it is back: the worker
/// tries every 250 ms, prepares it, and the handshake completes, instead of
/// the run ending and the lamp muting.
#[test]
fn a_microphone_gone_for_a_moment_at_start_is_opened_once_it_is_back() {
    warm_up();
    let signal = Signal::new(50);
    let started = Instant::now();
    let mut rig = Rig::start_gone("scripted gone at start", 3);
    assert!(rig.ready && rig.prepared);
    assert!(
        started.elapsed() >= Duration::from_millis(750),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(rig.device.counts().0, 1, "opened once it was back");
    rig.stream(&signal, 50);
    rig.end_with(Control::Stop);
    rig.finish();
}

/// A mute while the worker waits for a microphone off USB opens nothing: the
/// next try is refused for want of permission, the worker ends, and the
/// device was never opened.
#[test]
fn a_mute_while_the_microphone_is_waited_for_opens_nothing() {
    mute_during_the_wait("scripted gone then muted", lamp_audio::device::errno::NODEV);
}

/// The same for a microphone busy at start: its wait (8 s) is the longer
/// window on the privacy path, and a mute in it opens nothing either.
#[test]
fn a_mute_while_a_busy_microphone_is_waited_for_opens_nothing() {
    mute_during_the_wait("scripted busy then muted", lamp_audio::device::errno::BUSY);
}

fn mute_during_the_wait(device_name: &'static str, errno: i32) {
    warm_up();
    let mut rig = Rig::spawn(
        device_name,
        (40, errno),
        ReferenceRecovery::Off,
        Source::Processed,
        false,
        false,
    );
    // Long enough for the first tries to fail: it is waiting.
    let until = Instant::now() + Duration::from_millis(600);
    while Instant::now() < until {
        rig.service();
        let _ = rig.speaker.connect_step().unwrap();
        sleep(Duration::from_millis(1));
    }
    assert!(rig.ready && !rig.prepared);
    rig.controller
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    // The worker ends on the refusal, saying so: its fault may be read
    // before the thread is seen to finish, and a refused receive ends the
    // drain. Both are recorded, not a failure. Nothing is sent after the
    // denied authority (#1359).
    rig.tolerate_fault = true;
    let snapshot = rig.controller.snapshot(now()).unwrap();
    rig.end_with(Control::Authority { snapshot });
    let worker = rig.worker.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "the worker went on waiting");
        rig.service();
        sleep(Duration::from_millis(1));
    }
    let ended = worker.join().unwrap();
    let reason = ended.expect_err("the worker ended on the refusal");
    assert!(reason.contains("NotPermitted"), "{reason}");
    if let Some(code) = &rig.worker_fault {
        assert!(code.contains("NotPermitted"), "{code}");
    }
    assert!(!rig.prepared);
    assert_eq!(rig.device.counts().0, 0, "the microphone was never opened");
}

/// A worker held up again and again (#784): each stall lapses its authority
/// and closes the microphone, and each time it opens again under a new
/// epoch, while the speaker goes on accepting blocks through every gap. On
/// lamp-4ace the second stall of one worker ended the voice; on lamp-52e6 the
/// third did, when reference came in while the microphone was closed. Three
/// stalls inside a minute are what bounded recovery may reseed for (three a
/// minute); a fourth within the same minute spends past that budget, which
/// is the policy's own limit, not a capture fault.
#[test]
fn stalls_in_a_row_each_reopen_the_microphone_while_the_reference_goes_on() {
    warm_up();
    let signal = Signal::new(400);
    let mut rig = Rig::start_with("scripted stalls", 0, ReferenceRecovery::Bounded);
    rig.stream(&signal, 50);
    for stall in 1..=3 {
        rig.hold_up(400);
        rig.stream(&signal, 50);
        let deadline = Instant::now() + Duration::from_secs(2);
        while rig.epochs.len() < stall + 1 {
            assert!(
                Instant::now() < deadline,
                "stall {stall}: starts {:?}, gaps {}",
                rig.epochs,
                rig.gaps
            );
            rig.service();
            sleep(Duration::from_micros(500));
        }
    }
    assert_eq!(rig.epochs, [1, 2, 3, 4]);
    assert_eq!(rig.gaps, 3);
    rig.end_with(Control::Stop);
    rig.finish();
}

/// #1178, lamp-cb94 at 04:53:53 UTC on 17 October: the speaker accepts
/// nothing for a while as the microphone goes on, as a speaker held up does.
/// With recovery off, as every unit runs (`--reference-recovery off`), the
/// capture worker ends on `ReferenceLate`. Its sockets close. The speaker's
/// next reference to it, and the controller's next authority, are refused:
/// "Connection refused (os error 111)". That is cb94's chain: the
/// speaker's bare refusal, the capture worker's `reference ReferenceLate`,
/// then the voice's own refusal and "capture worker exited abnormally". Each
/// refusal now names its socket. With bounded recovery the worker goes on:
/// its frames are handed on through the pause and after it, and it stops
/// cleanly.
#[test]
fn a_speaker_held_up_ends_the_capture_worker_unless_recovery_is_bounded() {
    warm_up();
    let signal = Signal::new(400);
    let mut rig = Rig::start_with("scripted held-up speaker, off", 0, ReferenceRecovery::Off);
    rig.tolerate_fault = true;
    rig.stream(&signal, 100);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !rig.worker.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "the worker did not end: {} frames",
            rig.frames.len()
        );
        rig.speaker_quiet(&signal, 1);
    }
    // Its fault, if the controller read it before a send of its own found
    // the worker gone: on Linux that send drops what was waiting
    // (`a_refused_send_drops_what_the_gone_peer_had_sent`).
    assert!(
        rig.fault
            .is_none_or(|kind| kind == ReferenceFaultKind::ReferenceLate),
        "{:?}",
        rig.fault
    );
    // Not before the 40 ms queued that the speaker's last packet reported
    // have run out (on cb94 it was 19 ms); the worker ends a block or two
    // after the block it found that on. And within the 10 quiet blocks the
    // bounded half below goes through (the reviewer on #1298).
    let quiet_for = rig.pushed_at.len() - 100;
    eprintln!("off: ended after {quiet_for} quiet blocks");
    assert!(
        (4..=10).contains(&quiet_for),
        "ended after {quiet_for} quiet blocks"
    );
    let ended = rig.worker.take().unwrap().join().unwrap();
    assert!(
        ended.as_ref().unwrap_err().contains("ReferenceLate"),
        "{ended:?}"
    );
    let refused = rig.try_accept(vec![0; RENDER_SAMPLES]).unwrap_err();
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::ConnectionRefused,
        "{refused}"
    );
    assert!(
        refused.to_string().starts_with("ref.d.speaker: "),
        "{refused}"
    );
    rig.authority_at = None;
    rig.service();
    let refused = rig
        .refused
        .take()
        .expect("the controller's send is refused");
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::ConnectionRefused,
        "{refused}"
    );
    assert!(
        refused.to_string().starts_with("capture.c.p: "),
        "{refused}"
    );

    // One rig at a time: the first goes before the second starts.
    drop(rig);
    let mut rig = Rig::start_with(
        "scripted held-up speaker, bounded",
        0,
        ReferenceRecovery::Bounded,
    );
    rig.stream(&signal, 100);
    rig.speaker_quiet(&signal, 10);
    rig.stream(&signal, 200);
    rig.settle(0);
    assert_eq!(rig.frames.len(), rig.pushed_at.len());
    rig.end_with(Control::Stop);
    let frames = rig.finish();
    assert_eq!(frames.len(), 310);
}

/// #1359: the worker ends on `Stop` and is gone before the controller looks
/// again, with a renewal of its authority due. Its `CaptureStopped` is still
/// read, because nothing is sent after what ends the worker. A renewal sent
/// then is refused, and on Linux the refusal drops the `CaptureStopped`
/// waiting unread: the held-up test's bounded half failed that way about one
/// run in six to ten.
#[test]
fn a_stop_is_read_though_the_worker_has_gone_when_authority_falls_due() {
    warm_up();
    let signal = Signal::new(100);
    let mut rig = Rig::start("scripted gone before it is read");
    rig.stream(&signal, 50);
    rig.settle(0);
    rig.end_with(Control::Stop);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !rig.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline, "the worker did not end");
        sleep(Duration::from_micros(500));
    }
    rig.authority_at = None;
    let frames = rig.finish();
    assert_eq!(frames.len(), 50);
}

/// With `LAMP_CAPTURE_WAIT=on` the running loop waits on the microphone
/// instead of ticking, and hands on every block in order, at once; without
/// it the microphone is never waited on: the old loop. One run, both states.
#[test]
fn the_loop_waits_on_the_microphone_only_with_the_switch() {
    warm_up();
    for wait in [false, true] {
        let name = Microphone::Mic2.device();
        let signal = Signal::new(200);
        let mut rig = Rig::start_waiting(name, wait);
        rig.stream(&signal, 100);
        // Two blocks with no render beside them: only the microphone wakes
        // the loop, so it is waited on (with render, the speaker's socket
        // mostly wakes it first).
        rig.speaker_quiet(&signal, 2);
        rig.stream(&signal, 98);
        rig.settle(0);
        let waits = rig.device.waits();
        rig.end_with(Control::Stop);
        let pushed_at = rig.pushed_at.clone();
        let frames = rig.finish();
        let (read_after_push, waited) = check(&frames, &pushed_at, 0);
        eprintln!(
            "LAMP_CAPTURE_WAIT {wait}: {waits} waits on the microphone, {} frames, read \
             {read_after_push} us after the block was there, handed on {waited} us after (medians)",
            frames.len()
        );
        if wait {
            assert!(waits > 0, "{waits}");
        } else {
            assert_eq!(waits, 0);
        }
        assert!(read_after_push < 10_000, "{read_after_push}");
    }
}

/// With the switch, a block is read as it arrives, not when the render comes
/// (#1498's review): here the render comes 5 ms after each block. The
/// scripted microphone hands over whole blocks, as the lamp's
/// `plug:device_micro2` does. Of the 200 blocks after the first 20 (render
/// beside them, so the prime has started), all but a few are read within
/// 1 ms of being there.
#[test]
fn with_the_render_5_ms_after_each_block_all_but_a_few_blocks_are_read_within_a_ms() {
    let late = Duration::from_millis(5);
    warm_up();
    let ticks = 200;
    let signal = Signal::new(20 + ticks);
    let mut rig = Rig::start_waiting(Microphone::Mic2.device(), true);
    rig.stream(&signal, 20);
    rig.stream_render(&signal, ticks, late);
    rig.settle(0);
    rig.end_with(Control::Stop);
    let pushed_at = rig.pushed_at.clone();
    let frames = rig.finish();
    check(&frames, &pushed_at, 0);
    let mut reads: Vec<u64> = frames[20..]
        .iter()
        .zip(&pushed_at[20..])
        .map(|(frame, pushed)| frame.read_at - pushed)
        .collect();
    reads.sort_unstable();
    eprintln!(
        "render {late:?} after each block: {} blocks read {} us (median), {} us (p90), {} us (most) after they were there",
        reads.len(),
        reads[reads.len() / 2],
        reads[reads.len() * 9 / 10],
        reads[reads.len() - 1],
    );
    assert_eq!(reads.len(), ticks);
    // Without the switch the loop passes every 2 ms, so a block waits for the
    // next pass: on ARM64, 105 of 200 more than 1 ms, the median 1,016 us
    // (#1569 row 41). With it, a block is read as it arrives, but the runner
    // can still take the capture thread off its core once or twice beside
    // the rest of a pull request's tests: one or two blocks of 200 read 2.2
    // to 3.4 ms late, the median 23 us. A few slow, never more.
    let slow = reads.iter().filter(|read| **read > 1_000).count();
    assert!(
        slow <= ticks / 40,
        "{slow} of {ticks} read more than 1 ms after they were there: {reads:?}"
    );
}

/// The capture thread's time on a core (ns) and its voluntary and forced
/// context switches, from /proc.
fn capture_thread() -> (u64, u64, u64) {
    for task in std::fs::read_dir("/proc/self/task").unwrap() {
        let path = task.unwrap().path();
        if std::fs::read_to_string(path.join("comm"))
            .unwrap_or_default()
            .trim()
            != "capture"
        {
            continue;
        }
        let on_core = std::fs::read_to_string(path.join("schedstat")).unwrap();
        let status = std::fs::read_to_string(path.join("status")).unwrap();
        let count = |key: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        return (
            on_core.split_whitespace().next().unwrap().parse().unwrap(),
            count("voluntary_ctxt_switches:"),
            count("nonvoluntary_ctxt_switches:"),
        );
    }
    panic!("no capture thread");
}

/// The numbers for #705 (`--ignored --nocapture`, Linux), ticking and with
/// `LAMP_CAPTURE_WAIT=on` in one run: the capture thread's wakeups and share
/// of a core over 10 s quiet (silence from both) and 10 s talking (the test
/// noise), and how long from a block being in the microphone to its frame
/// being sent. The rig sends the render beside each block, so the numbers
/// hold only when it comes with each block; how a block is read when it
/// does not is
/// `with_the_render_5_ms_after_each_block_all_but_a_few_blocks_are_read_within_a_ms`.
#[test]
#[ignore = "a measurement, 20 s"]
fn capture_wait_cost() {
    warm_up();
    let ticks = 1_000;
    let quiet = Signal {
        base: vec![0.0; (2 * ticks + 8) * 480 + 24],
    };
    let talking = Signal::new(2 * ticks);
    for wait in [false, true] {
        let mut rig = Rig::start_waiting(Microphone::Mic2.device(), wait);
        for (scene, signal) in [("quiet", &quiet), ("talking", &talking)] {
            let from = rig.pushed_at.len();
            let heard = rig.frames.len();
            let (core_before, woke_before, forced_before) = capture_thread();
            let started = Instant::now();
            rig.stream(signal, ticks);
            let seconds = started.elapsed().as_secs_f64();
            let (core_after, woke_after, forced_after) = capture_thread();
            rig.settle(0);
            let sent = median(
                rig.frames[heard..]
                    .iter()
                    .zip(&rig.pushed_at[from..])
                    .map(|(frame, pushed)| frame.processed_at - pushed)
                    .collect(),
            );
            let read = median(
                rig.frames[heard..]
                    .iter()
                    .zip(&rig.pushed_at[from..])
                    .map(|(frame, pushed)| frame.read_at - pushed)
                    .collect(),
            );
            eprintln!(
                "LAMP_CAPTURE_WAIT {wait} {scene}: {:.0} wakeups/s ({:.0} forced), {:.2} % of a core; \
             block read {read} us and sent {sent} us after it was there (medians)",
                (woke_after - woke_before) as f64 / seconds,
                (forced_after - forced_before) as f64 / seconds,
                (core_after - core_before) as f64 / seconds / 1e7,
            );
        }
        rig.end_with(Control::Stop);
        rig.finish();
    }
}
