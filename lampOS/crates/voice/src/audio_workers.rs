//! Dedicated Linux audio loops with actual accepted PCM and a direct reference link.
use crate::{
    activity::SpeechProbability,
    diagnostic_control::{DiagnosticFinished, DiagnosticStarted},
    diagnostics::{self, EndReason, Recorder, ResetMeta, ResetReason, StreamKind},
    open_at_start::{BUSY_AT_START, OpenAtStart, SPEAKER_BUSY_AT_START, open_waiting_out_busy},
    options::{
        AudioWorkerOptions, NoiseSuppression, ReferenceClock, ReferenceRecovery, SoftwareProcessing,
    },
    playback::SpeechPlayback,
    reference::{
        PRIME_FRAMES, PrimeBarrier, RecoveryPolicy, ReferenceAssembly, accepted_write_survived,
        hold::{self, CaptureHold},
    },
    stage_cost::Stage,
    stop_words::Source,
    transport::{ReferenceChannels, WorkerChannels},
    wire::{
        CaptureTiming, ClockPrimed, Control, MicrophoneFrame, PlaybackDiscardReason,
        ReferenceControl, ReferenceFault, ReferenceFaultKind, ReferenceTiming, RenderPayload,
        RenderReference, SpeakerChunk, WorkerEvent,
    },
};
use lamp_audio::{
    EchoProcessor, RENDER_SAMPLES,
    capture::{
        CaptureConfig, CaptureError, Discontinuity, ErrorKind as CaptureErrorKind, MicrophoneOn,
        StartReport,
    },
    device::{AlsaCapture, CaptureDevice},
    pcm::{Speaker, Underrun, Written},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{
    BootId, Error as InteractionError, MonoTime, Permission, Snapshot, TurnOwner,
};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const TICK: Duration = Duration::from_millis(2);
/// With `LAMP_CAPTURE_WAIT=on` (`quiet_pace::CAPTURE_WAIT_ENVIRONMENT`, #705,
/// the lamp's heat) the loop waits in `poll` on the microphone and its
/// sockets between passes, instead of passing every [`TICK`]. Off unless
/// set; off, the loop is the ticking one. With the switch, the longest one wait lasts from its pass's start: a wake
/// that was missed costs at most this, and the authority, the partial
/// block's deadline and the channels' ages are still looked at this often.
const WAIT_AT_MOST: Duration = Duration::from_millis(20);
const MAX_CONTROL: usize = 8;
const MAX_REFERENCE: usize = 16;
const BARRIER_US: u64 = 100_000;
/// The reference's cursor as a recording keeps it, for a capture block and
/// for a step whose block is held back.
fn reference_meta(timing: &ReferenceTiming) -> diagnostics::ReferenceMeta {
    diagnostics::ReferenceMeta {
        playback_epoch: timing.playback_epoch,
        accepted_through: timing.accepted_through,
        analysed_through: timing.analysed_through,
        capture_blocks_processed: timing.capture_blocks_processed,
        capture_blocks_before_clock_start: timing.capture_blocks_before_clock_start,
        accepted_at_us: timing.accepted_at_us,
        analysed_at_us: timing.analysed_at_us,
        queue_observed_at_us: timing.queue_observed_at_us,
        queued_frames: timing.queued_frames as u64,
        clock_start_requested_at_us: timing.clock_start_requested_at_us,
        clock_started_at_us: timing.clock_started_at_us,
        ignored_old_epoch_packets: timing.ignored_old_epoch_packets,
    }
}

/// With `LAMP_QUIET_EVIDENCE=on`, how often capture says in the journal how
/// many blocks the echo evidence's search was skipped on (#705), so a run
/// with the switch can say how often it applied.
const EVIDENCE_SAID_EVERY: Duration = Duration::from_secs(60);

/// Whether capture says its line now: only with the switch, once a minute.
fn evidence_due(on: bool, since: Duration) -> bool {
    on && since >= EVIDENCE_SAID_EVERY
}

/// The journal's line for [`EVIDENCE_SAID_EVERY`]: since the last line, the
/// blocks processed, skipped, and of those the ones that saved work (a loud
/// room searched or an alignment held fitted, [`lamp_audio::evidence::Skipped`]).
fn evidence_line(
    now: lamp_audio::evidence::Skipped,
    then: lamp_audio::evidence::Skipped,
    over: Duration,
) -> String {
    let blocks = now.observed.saturating_sub(then.observed);
    let skipped = now.skipped.saturating_sub(then.skipped);
    let saved = now.saved.saturating_sub(then.saved);
    format!(
        "lamp-voice: capture: the echo search was skipped on {skipped} of {blocks} blocks in the last {} s, {saved} of them loud or with an alignment held (LAMP_QUIET_EVIDENCE=on)",
        over.as_secs()
    )
}

/// How long a microphone closed by a stall may stay closed before the
/// worker gives up: the coordinator's own bound on a capture gap (#784).
const REOPEN_US: u64 = 1_500_000;
/// The longest the speaker stays up on a lapsed authority (#1178): the
/// same bound, after which the voice has ended anyway (the reviewer on #1214).
const LAPSE_MOST_US: u64 = REOPEN_US;
/// A tick that comes this long after the last was held up.
const HELD_UP: Duration = Duration::from_millis(50);
/// What a held-up capture worker may read in one tick: snapshots come no
/// faster than one a capture block, so 1.5 s leaves at most 150.
const HELD_UP_CONTROL: usize = 256;

/// A discontinuity a short stall of this worker causes: the authority it
/// held lapsed while it was held up, or its 40 ms buffer overran. Anything
/// else (privacy, the device, the clock, control) still ends the worker.
fn stall(error: &CaptureError) -> Option<Discontinuity> {
    match error.kind {
        CaptureErrorKind::Discontinuity(
            reason @ (Discontinuity::AuthorityExpired
            | Discontinuity::XRun
            | Discontinuity::PartialTimeout
            | Discontinuity::ReadBudgetExceeded),
        ) => Some(reason),
        _ => None,
    }
}

/// The microphone was closed by a stall: say so, and drop the blocks held
/// for the canceller, which belong to the closed epoch. Returns when.
fn begin_gap<T>(
    channels: &mut WorkerChannels,
    epoch: u64,
    error: &CaptureError,
    hold: &mut CaptureHold<T>,
) -> Result<u64> {
    let reason = stall(error).ok_or_else(|| io::Error::other("not a stall"))?;
    let closed_at_us = monotonic_us();
    channels.control.send(WorkerEvent::CaptureGap {
        epoch,
        reason: format!("{reason:?}"),
        closed_at_us,
    })?;
    *hold = CaptureHold::new(hold.blocks())
        .ok_or_else(|| io::Error::other("capture hold beyond its bound"))?;
    Ok(closed_at_us)
}

/// A stall in any epoch: open a gap, or stay in the one already open (a
/// second stall of the same worker, or one while it reopens).
fn into_gap<T>(
    gap: &mut Option<u64>,
    channels: &mut WorkerChannels,
    epoch: u64,
    error: &CaptureError,
    hold: &mut CaptureHold<T>,
) -> Result<()> {
    if gap.is_none() {
        *gap = Some(begin_gap(channels, epoch, error, hold)?);
    }
    Ok(())
}

fn capture_started(
    report: &StartReport,
    dsp_epoch: u64,
    noise_suppression: NoiseSuppression,
    privacy: u64,
) -> WorkerEvent {
    WorkerEvent::CaptureStarted {
        epoch: report.epoch,
        dsp_epoch,
        noise_suppression,
        privacy_generation: privacy,
        rate: report.format.rate,
        channels: report.format.channels,
        period_frames: report.format.period_frames,
        buffer_frames: report.format.buffer_frames,
        monotonic_status_timestamps: report.format.monotonic_status_timestamps,
        open_started_at_us: report.open_started_at_us,
        prepared_at_us: report.prepared_at_us,
        capture_started_at_us: report.capture_started_at_us,
        start_completed_at_us: report.start_completed_at_us,
    }
}

struct Recording {
    recorder: Recorder,
    started: DiagnosticStarted,
    privacy: Option<(u64, bool)>,
}
impl Recording {
    fn start(
        path: Option<&Path>,
        boot: BootId,
        stream: StreamKind,
        software_processing: Option<SoftwareProcessing>,
        channels: &mut WorkerChannels,
    ) -> Result<Option<Self>> {
        let Some(path) = path else {
            return Ok(None);
        };
        let mut config = diagnostics::Config::new(path.to_owned(), boot, stream);
        config.software_processing = software_processing;
        let recorder = Recorder::start(config)?;
        let started = DiagnosticStarted {
            boot,
            stream: stream.into(),
            started_at_us: recorder.started_at_us(),
        };
        channels
            .control
            .send(WorkerEvent::DiagnosticsStarted { started })?;
        Ok(Some(Self {
            recorder,
            started,
            privacy: None,
        }))
    }
    fn observe_privacy(&mut self, snapshot: Snapshot) {
        let observed = (
            snapshot.microphone_generation(),
            snapshot.microphone_permission() == Permission::Allowed,
        );
        if self.privacy != Some(observed) {
            let _ = self.recorder.try_privacy(diagnostics::PrivacyMeta {
                at_us: monotonic_us(),
                generation: observed.0,
                open: observed.1,
            });
            self.privacy = Some(observed);
        }
    }
    fn render(
        &mut self,
        written: Written,
        owner: Option<TurnOwner>,
        sequence: Option<u64>,
    ) -> diagnostics::RenderMeta {
        diagnostics::RenderMeta {
            playback_epoch: written.first_sample.epoch,
            privacy_generation: self.privacy.map_or(0, |p| p.0),
            first_sample: written.first_sample.frame,
            end_sample: written.end_sample.frame,
            accepted_at_us: written.completed_at_us,
            queue_observed_at_us: written.queue_observed_at_us,
            queued_frames: written.queued_frames,
            owner,
            chunk_sequence: sequence,
        }
    }
    fn finish(self, channels: &mut WorkerChannels, reason: EndReason) -> Result<()> {
        let report = self.recorder.finish(
            diagnostics::EndMeta {
                at_us: monotonic_us(),
                reason,
            },
            diagnostics::DEFAULT_FINISH_WAIT,
        );
        let valid = report.valid;
        channels.control.send(WorkerEvent::DiagnosticsFinished {
            receipt: DiagnosticFinished::from_report(self.started, report),
        })?;
        if !valid {
            return Err(
                io::Error::other("audio diagnostic recording did not finish validly").into(),
            );
        }
        Ok(())
    }
}

fn discarded_range(range: lamp_audio::ownership::QueuedRange) -> diagnostics::DiscardedRange {
    diagnostics::DiscardedRange {
        playback_epoch: range.epoch,
        retired_through: range.retired_through,
        accepted_through: range.accepted_through,
    }
}

fn diagnostic_reset(
    recording: &mut Option<Recording>,
    reason: ResetReason,
    capture_epoch: u64,
    dsp_epoch: u64,
    playback_epoch: u64,
    privacy_generation: u64,
    discarded: Option<lamp_audio::ownership::QueuedRange>,
) {
    if let Some(recording) = recording {
        let _ = recording.recorder.try_reset(ResetMeta {
            at_us: monotonic_us(),
            reason,
            capture_epoch,
            dsp_epoch,
            playback_epoch,
            privacy_generation,
            discarded: discarded.map(discarded_range),
        });
    }
}

pub fn capture(
    channels: WorkerChannels,
    controller: BootId,
    worker: BootId,
    directory: &Path,
    device: &str,
    diagnostic_directory: Option<&Path>,
) -> Result<()> {
    capture_with_noise_suppression(
        channels,
        controller,
        worker,
        directory,
        device,
        diagnostic_directory,
        NoiseSuppression::On,
    )
}

pub fn capture_with_noise_suppression(
    channels: WorkerChannels,
    controller: BootId,
    worker: BootId,
    directory: &Path,
    device: &str,
    diagnostic_directory: Option<&Path>,
    noise_suppression: NoiseSuppression,
) -> Result<()> {
    capture_on::<AlsaCapture>(
        channels,
        controller,
        worker,
        directory,
        device,
        diagnostic_directory,
        noise_suppression,
        ReferenceClock::Fixed,
        ReferenceRecovery::Off,
        lamp_audio::Reference::AsGiven,
        Source::Processed,
        capture_wait_from_environment(),
    )
}

/// The capture worker as the supervisor configured it: the noise suppression
/// and the way the echo reference is kept beside the microphone.
pub fn capture_with_options(
    channels: WorkerChannels,
    controller: BootId,
    worker: BootId,
    directory: &Path,
    device: &str,
    options: &AudioWorkerOptions,
) -> Result<()> {
    capture_on::<AlsaCapture>(
        channels,
        controller,
        worker,
        directory,
        device,
        options.diagnostic_directory.as_deref(),
        options.noise_suppression,
        options.reference_clock,
        options.reference_recovery,
        options.echo_reference.reference(),
        options.stop_words,
        capture_wait_from_environment(),
    )
}

/// The capture worker over any capture device. The entry points above are the
/// board's and use ALSA. A process-level test names a scripted device instead
/// and runs this same loop: nothing in it knows which one it has.
#[allow(clippy::too_many_arguments)]
pub fn capture_on<D: CaptureDevice>(
    mut channels: WorkerChannels,
    controller: BootId,
    worker: BootId,
    directory: &Path,
    device: &str,
    diagnostic_directory: Option<&Path>,
    noise_suppression: NoiseSuppression,
    reference_clock: ReferenceClock,
    reference_recovery: ReferenceRecovery,
    echo_reference: lamp_audio::Reference,
    stop_words: Source,
    // `LAMP_CAPTURE_WAIT=on` (#705): wait on the microphone between passes.
    capture_wait: bool,
) -> Result<()> {
    // The worker inherits the unit's RR 20 and all cores; with
    // LAMP_CAPTURE_CORES its thread (and the ones it starts) keep to those
    // cores. Unset, nothing; a bad value logs and keeps the default (#705).
    crate::capture_cores::apply_from_environment();
    let mut recording = Recording::start(
        diagnostic_directory,
        controller,
        StreamKind::Capture,
        Some(noise_suppression.software_processing()),
        &mut channels,
    )?;
    let mut microphone = MicrophoneOn::<D>::new(device, controller, CaptureConfig::default())?;
    let result = capture_loop(
        &mut channels,
        &mut microphone,
        worker,
        directory,
        &mut recording,
        noise_suppression,
        reference_clock,
        reference_recovery,
        echo_reference,
        hold::blocks_for(device, reference_clock),
        stop_words,
        device,
        capture_wait,
    );
    // Invalidate input authority and close hardware before any filesystem acknowledgement wait.
    let close = microphone.control_lost();
    drop(microphone);
    if let Err(error) = &result {
        report_fault(&mut channels, error.as_ref());
    }
    if let Err(error) = &close {
        report_fault(&mut channels, error);
    }
    let reason = if close.is_err() {
        EndReason::Fault
    } else {
        result.as_ref().copied().unwrap_or(EndReason::Fault)
    };
    let finish = recording.map_or(Ok(()), |r| r.finish(&mut channels, reason));
    result
        .map(|_| ())
        .and_then(|()| close.map_err(Into::into))
        .and(finish)
}

#[allow(clippy::too_many_arguments)]
fn capture_loop<D: CaptureDevice>(
    channels: &mut WorkerChannels,
    microphone: &mut MicrophoneOn<D>,
    worker: BootId,
    directory: &Path,
    recording: &mut Option<Recording>,
    noise_suppression: NoiseSuppression,
    reference_clock: ReferenceClock,
    reference_recovery: ReferenceRecovery,
    echo_reference: lamp_audio::Reference,
    hold_blocks: usize,
    // What each frame also carries for the stop words: the block before
    // the canceller (#1047) or the canceller's linear output (#828).
    stop_words: Source,
    // The ALSA name opened: whose it is when busy at start (#1567).
    device: &str,
    wait: bool,
) -> Result<EndReason> {
    // A frame waits this many capture blocks before it is cancelled, so that
    // the canceller has its render first. Whatever still waits when this
    // loop returns is dropped with it.
    let mut hold = CaptureHold::new(hold_blocks)
        .ok_or_else(|| io::Error::other("capture hold beyond its bound"))?;
    let mut link: Option<ReferenceChannels> = None;
    let mut prepare_requested = false;
    let mut capture_prepared = false;
    let mut at_start = OpenAtStart::default();
    let mut privacy = 0;
    // The fixed margin is the default and the fallback. The capture-clocked
    // reference follows the difference between the two USB clocks; loss,
    // reordering and staleness fault exactly as they do in the fixed mode.
    // With bounded recovery a doubt that can be resolved is: the fixed margin
    // is re-centred when the clocks use it up, and the capture-clocked
    // reference stops cancelling until fresh reference returns. Either way
    // within a budget, with a status for every change.
    let mut reference = match (reference_clock, reference_recovery) {
        (ReferenceClock::Fixed, ReferenceRecovery::Off) => ReferenceAssembly::default(),
        (ReferenceClock::Capture, ReferenceRecovery::Off) => ReferenceAssembly::capture_clocked(),
        (ReferenceClock::Fixed, ReferenceRecovery::Bounded) => {
            ReferenceAssembly::recentring(RecoveryPolicy::BOUNDED)
        }
        (ReferenceClock::Capture, ReferenceRecovery::Bounded) => {
            ReferenceAssembly::recovering(RecoveryPolicy::BOUNDED)
        }
    };
    let mut echo = EchoProcessor::with_reference(noise_suppression.enabled(), echo_reference);
    // The detector may read the canceller's linear output (#828); the
    // canceller keeps it only then.
    let vad_input = crate::activity::DetectorInput::from_environment();
    let vad_linear = vad_input.reads_linear();
    // So may the stop words (`--linear-blocks`, #828).
    let words_linear = stop_words == Source::Linear;
    if vad_input.value().is_some() || words_linear {
        echo = echo.with_linear_output();
    }
    if words_linear {
        eprintln!(
            "lamp-voice: capture: the stop words are sent the canceller's linear output (LAMP_STOP_WORDS_SOURCE=linear)"
        );
    }
    if let Some(value) = vad_input.value() {
        eprintln!(
            "lamp-voice: capture: the speech detector reads the canceller's linear output{} (LAMP_VAD_INPUT={value})",
            if value == "linear-gated" {
                " where the duplex rules would take speech for a person"
            } else {
                ""
            }
        );
    }
    // Blocks the detector scored, and of those on the linear output, since
    // the last minute's line: a run with the switch shows it took.
    let mut vad_said = (Instant::now(), 0u64, 0u64);
    // Blocks sent, and of those with the linear output for the stop words,
    // since the last minute's line: the rest went without it.
    let mut words_said = (Instant::now(), 0u64, 0u64);
    let evidence_skip = crate::quiet_pace::evidence_skipped();
    echo.skip_evidence_while_render_silent(evidence_skip);
    let mut evidence_said = (Instant::now(), echo.evidence_skipped());
    let mut speech = SpeechProbability::default();
    let mut dsp_epoch = 1u64;
    let mut last_diagnostic = 0;
    let mut primed_at = None;
    // Since when a stall has kept the microphone closed. Only a microphone
    // that has started is reopened: before that, the start is the
    // reference's barrier to keep, and a stall in it still ends the worker.
    let mut gap: Option<u64> = None;
    let mut started = false;
    let mut last_tick = Instant::now();
    channels.control.send(WorkerEvent::Ready)?;
    // With LAMP_STALLS=on, a pass or an overrun wait of 50 ms or more is
    // recorded with what held it (#1178).
    let mut stalls = crate::stalls::Stalls::from_environment("capture");
    // With LAMP_CAPTURE_STAGES=on, each stage's thread CPU time, a line a
    // minute.
    let mut cost = crate::stage_cost::StageCost::new(crate::stage_cost::on());
    if wait {
        eprintln!(
            "lamp-voice: capture: waits on the microphone and its sockets between blocks (LAMP_CAPTURE_WAIT=on)"
        );
    }
    loop {
        let tick = Instant::now();
        let stall_pass = stalls.begin();
        cost.begin();
        // A worker held up (#784) finds the snapshots sent meanwhile queued:
        // they are read in this tick, up to what the longest gap can leave.
        let bound = if tick.duration_since(last_tick) > HELD_UP {
            HELD_UP_CONTROL
        } else {
            MAX_CONTROL
        };
        last_tick = tick;
        for index in 0..=bound {
            let Some(control) = channels.control.receive_renewed(Control::is_renewal)? else {
                break;
            };
            if index == bound {
                return Err(io::Error::other("capture control tick bound").into());
            }
            match control {
                Control::Authority { snapshot } => match microphone.install(snapshot) {
                    Ok(update) => {
                        if let Some(recording) = recording {
                            recording.observe_privacy(snapshot);
                        }
                        if update.closed {
                            channels.control.send(WorkerEvent::CaptureStopped)?;
                            return Ok(EndReason::PrivacyClosed);
                        }
                    }
                    Err(error) if matches!(error.kind, CaptureErrorKind::Authority(reason) if rejected_old_state(reason)) =>
                        {}
                    Err(error) if started && stall(&error).is_some() => {
                        into_gap(&mut gap, channels, microphone.epoch(), &error, &mut hold)?;
                    }
                    Err(error) => return Err(error.into()),
                },
                Control::ConnectReference { peer } => {
                    if link.is_some() {
                        return Err(io::Error::other("reference peer already bound").into());
                    }
                    link = Some(ReferenceChannels::bind(directory, "capture", worker, peer)?);
                }
                Control::StartCapture => {
                    if prepare_requested {
                        return Err(io::Error::other("capture start already requested").into());
                    }
                    prepare_requested = true;
                }
                Control::ProviderOutputCapacity { .. } => {
                    return Err(io::Error::other("provider capacity sent to capture").into());
                }
                Control::Stop => {
                    microphone.revoke()?;
                    channels.control.send(WorkerEvent::CaptureStopped)?;
                    return Ok(EndReason::Stopped);
                }
            }
        }
        match microphone.maintain() {
            Ok(()) => {}
            Err(error) if started && stall(&error).is_some() => {
                into_gap(&mut gap, channels, microphone.epoch(), &error, &mut hold)?;
            }
            Err(error) => return Err(error.into()),
        }
        // A stall closed the microphone: it opens again only under a fresh
        // authority. `prepare` checks it, the privacy switch and the mute
        // before the device is touched; until the coordinator's next
        // snapshot arrives it is not permitted, and the worker waits.
        if let Some(since) = gap {
            if monotonic_us().saturating_sub(since) > REOPEN_US {
                return Err(io::Error::other("microphone did not reopen after a stall").into());
            }
            if microphone.reset_required().is_some() {
                microphone.reset()?;
            }
            match microphone.prepare() {
                Ok(report) if report.microphone_generation == privacy => {
                    match microphone.start_prepared() {
                        Ok(report) => {
                            channels.control.send(capture_started(
                                &report,
                                dsp_epoch,
                                noise_suppression,
                                privacy,
                            ))?;
                            gap = None;
                        }
                        // The fresh authority lapsed between open and start:
                        // the microphone closed again; the next one reopens it.
                        Err(error) if stall(&error).is_some() => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                // The privacy switch moved during the gap: not a stall.
                Ok(_) => {
                    return Err(io::Error::other("privacy changed during a capture gap").into());
                }
                Err(error)
                    if matches!(error.kind, CaptureErrorKind::NotPermitted)
                        || stall(&error).is_some() => {}
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(link) = link.as_mut()
            && link.connect_step()?
        {
            if prepare_requested && !capture_prepared && at_start.due(Instant::now()) {
                microphone.reset()?;
                let report = match microphone.prepare() {
                    Ok(report) => Some(report),
                    Err(error) if at_start.waits(&error, Instant::now()) => {
                        if at_start.busy_first() {
                            let looked = Instant::now();
                            let holder = crate::mic_holder::said(Path::new("/proc"), device);
                            eprintln!(
                                "lamp-voice: capture: microphone busy at start ({holder}, looked up in {} ms); waiting up to {} s",
                                looked.elapsed().as_millis(),
                                BUSY_AT_START.as_secs()
                            );
                        }
                        None
                    }
                    Err(error) => return Err(error.into()),
                };
                if let Some(report) = report {
                    let waited = at_start.opened(Instant::now());
                    if let Some(waited) = &waited {
                        eprintln!("lamp-voice: capture: {}", waited.plain());
                    }
                    privacy = report.microphone_generation;
                    capture_prepared = true;
                    channels.control.send(WorkerEvent::CapturePrepared {
                        epoch: report.epoch,
                        privacy_generation: privacy,
                        prepared_at_us: report.prepared_at_us,
                        waited,
                    })?;
                    link.control.send(ReferenceControl::CapturePrepared {
                        privacy_generation: privacy,
                    })?;
                }
            }
            let mut control_count = 0;
            // Receive data first, then recheck priority lifecycle before using
            // it: two sockets can become readable between separate drains.
            // Held up, the speaker's reference queued as well: read it all.
            let reference_bound = if bound == MAX_CONTROL {
                MAX_REFERENCE
            } else {
                HELD_UP_CONTROL
            };
            for data_index in 0..=reference_bound {
                let packet = link.data.receive::<RenderReference>()?;
                for _ in 0..=MAX_CONTROL {
                    let Some(control) = link.control.receive::<ReferenceControl>()? else {
                        break;
                    };
                    if control_count == MAX_CONTROL {
                        return Err(io::Error::other("reference control tick bound").into());
                    }
                    control_count += 1;
                    match control {
                        ReferenceControl::ClockPrimed { prime } => {
                            if !capture_prepared {
                                return Err(reference
                                    .fault(ReferenceFaultKind::InvalidPrime, monotonic_us())
                                    .into());
                            }
                            let previous = reference.timing().playback_epoch;
                            let prepared =
                                reference.prime(prime, monotonic_us(), privacy, &mut echo)?;
                            speech.reset();
                            if previous != 0 {
                                dsp_epoch = dsp_epoch
                                    .checked_add(1)
                                    .ok_or_else(|| io::Error::other("DSP epoch exhausted"))?;
                                channels.control.send(WorkerEvent::CaptureDspReset {
                                    epoch: microphone.epoch(),
                                    dsp_epoch,
                                    observed_at_us: monotonic_us(),
                                })?;
                            }
                            diagnostic_reset(
                                recording,
                                if previous == 0 {
                                    ResetReason::Start
                                } else {
                                    ResetReason::DspReset
                                },
                                microphone.epoch(),
                                dsp_epoch,
                                prime.playback_epoch,
                                privacy,
                                prime.discarded,
                            );
                            // No microphone sequence or retained upstream prefix is reset.
                            primed_at = Some(monotonic_us());
                            link.control
                                .send(ReferenceControl::ReferencePrimed { prepared })?;
                        }
                        ReferenceControl::ClockStarted {
                            playback_epoch,
                            start_requested_at_us,
                            started_at_us,
                        } => {
                            reference.started(
                                playback_epoch,
                                start_requested_at_us,
                                started_at_us,
                                monotonic_us(),
                            )?;
                            primed_at = None;
                            if !microphone.is_running() && gap.is_none() {
                                let report = microphone.start_prepared()?;
                                started = true;
                                channels.control.send(capture_started(
                                    &report,
                                    dsp_epoch,
                                    noise_suppression,
                                    privacy,
                                ))?;
                            }
                        }
                        ReferenceControl::Stopped { .. } => {
                            microphone.revoke()?;
                            channels.control.send(WorkerEvent::CaptureStopped)?;
                            return Ok(EndReason::Stopped);
                        }
                        _ => {
                            return Err(io::Error::other(
                                "wrong direct reference command for capture",
                            )
                            .into());
                        }
                    }
                }
                let Some(packet) = packet else {
                    break;
                };
                if data_index == reference_bound {
                    return Err(reference
                        .fault(ReferenceFaultKind::ReferenceBacklog, monotonic_us())
                        .into());
                }
                // A closed microphone is a privacy mismatch, unless a stall
                // closed it: the speaker's reference goes on through the gap
                // (#784, the third stall on lamp-52e6 ended on this), and the
                // privacy generation is still checked on every packet.
                if !microphone.is_open() && gap.is_none() {
                    return Err(reference
                        .fault(ReferenceFaultKind::PrivacyMismatch, monotonic_us())
                        .into());
                }
                let from = cost.now();
                reference.accept(packet, monotonic_us(), privacy, &mut echo)?;
                cost.stage(Stage::Reference, from);
            }
        }
        if primed_at.is_some_and(|at| monotonic_us().saturating_sub(at) >= BARRIER_US) {
            return Err(reference
                .fault(ReferenceFaultKind::BarrierTimeout, monotonic_us())
                .into());
        }
        let read = if microphone.is_running() {
            let from = cost.now();
            let read = microphone.try_read();
            cost.stage(Stage::Read, from);
            match read {
                Ok(frame) => frame,
                Err(error) if started && stall(&error).is_some() => {
                    into_gap(&mut gap, channels, microphone.epoch(), &error, &mut hold)?;
                    None
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            None
        };
        let available = read.as_ref().map(|frame| frame.timing.available_frames);
        if let Some(frame) = read {
            let from = cost.now();
            let step = reference.capture_step(
                frame.timing.delayed_frames,
                frame.timing.read_completed_at_us,
                monotonic_us(),
                &mut echo,
            )?;
            cost.stage(Stage::Reference, from);
            // A reseed is a new DSP epoch from this block on.
            report_reference(
                &mut reference,
                channels,
                recording,
                &mut speech,
                &mut dsp_epoch,
                microphone.epoch(),
                privacy,
            )?;
            // The reference has moved on by this block; the block cancelled
            // against it is the one that has waited its turn. Until the
            // first has, there is nothing to hand on: the reference was
            // reported just above, so the rest of this pass has nothing to do.
            let held = (frame.epoch, frame.microphone_generation, frame.sequence);
            let Some(frame) = hold.push(frame) else {
                // A recording keeps the step, so its replay can take it as
                // the lamp did before the block it waits for (#828).
                if let Some(recording) = recording {
                    let _ = recording
                        .recorder
                        .try_reference_step(diagnostics::ReferenceStepMeta {
                            epoch: held.0,
                            dsp_epoch,
                            privacy_generation: held.1,
                            frame_sequence: held.2,
                            at_us: monotonic_us(),
                            reference: reference_meta(&reference.timing()),
                        });
                }
                said(cost.done());
                stall_pass.done();
                pace(
                    wait,
                    microphone,
                    channels,
                    link.as_ref(),
                    gap,
                    primed_at,
                    available,
                    tick,
                );
                continue;
            };
            // As the microphone recorded it, for the stop words, when asked.
            let raw = (stop_words == Source::Raw).then(|| frame.samples.to_vec());
            // The canceller has this block's reference, or the block goes on
            // as the microphone recorded it. Nothing is put in its place.
            let from = cost.now();
            let (clean, delay) = match step {
                Some(delay) => (echo.capture(&frame.samples, delay)?, delay),
                None => (frame.samples, 0),
            };
            cost.stage(Stage::Canceller, from);
            // The linear output is this block's only when the canceller ran.
            // With the switch the score is 4 ms (one canceller block) ahead
            // of the block handed on; the letters, `explained` and `heard`
            // still come from `clean`.
            let linear = step.and(echo.linear());
            // The same, for the stop words, when asked. A block the
            // canceller did not run for goes without, and the coordinator
            // hands on what it sends instead, as the microphone recorded it.
            let words_linear_block = linear
                .filter(|_| words_linear)
                .map(|linear| linear.to_vec());
            words_said.1 += u64::from(words_linear);
            words_said.2 += u64::from(words_linear_block.is_some());
            // With `linear-gated`, the block's own trusted evidence, as the
            // frame would carry it, admits the linear output. Nothing is
            // judged with any other value.
            let admitted = vad_input == crate::activity::DetectorInput::LinearGated
                && crate::activity::linear_admitted(
                    &lamp_audio::duplex::Hearing::EVALUATED,
                    reference
                        .evidence_trusted()
                        .then(|| echo.evidence())
                        .flatten()
                        .as_ref(),
                );
            let input = crate::activity::detector_input(vad_input, admitted, &clean, linear);
            vad_said.1 += 1;
            vad_said.2 += u64::from(linear.is_some_and(|linear| std::ptr::eq(input, linear)));
            let from = cost.now();
            let probability = speech.analyze(input);
            cost.stage(Stage::Detector, from);
            reference.note_score(probability);
            if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
                return Err(io::Error::other("invalid local speech probability").into());
            }
            // The last authority check of the frame: a stall landing after
            // the read (#784, the second stall on lamp-4ace) is a gap like
            // any other, and this frame is not handed on.
            match microphone.maintain() {
                Ok(()) => {}
                Err(error) if started && stall(&error).is_some() => {
                    into_gap(&mut gap, channels, microphone.epoch(), &error, &mut hold)?;
                    said(cost.done());
                    stall_pass.done();
                    finish_tick(tick);
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            let timing = reference.timing();
            let processing_completed_at_us = monotonic_us();
            if let Some(recording) = recording {
                let _ = recording.recorder.try_capture(
                    diagnostics::CaptureMeta {
                        epoch: frame.epoch,
                        dsp_epoch,
                        privacy_generation: frame.microphone_generation,
                        frame_sequence: frame.sequence,
                        first_read_started_at_us: frame.timing.first_read_started_at_us,
                        last_read_started_at_us: frame.timing.last_read_started_at_us,
                        read_completed_at_us: frame.timing.read_completed_at_us,
                        successful_reads: frame.timing.successful_reads,
                        alsa_status_monotonic_us: frame.timing.alsa_status_monotonic_us,
                        status_observed_at_us: frame.timing.status_observed_at_us,
                        available_frames: frame.timing.available_frames,
                        delayed_frames: frame.timing.delayed_frames,
                        processing_completed_at_us,
                        aec_queue_delay_ms: delay,
                        aec_internal_alignment_ms: echo.internal_alignment_ms(),
                        vad_score: probability,
                        reference: reference_meta(&timing),
                    },
                    &frame.samples,
                    &clean,
                );
            }
            let from = cost.now();
            channels.data.send(MicrophoneFrame {
                epoch: frame.epoch,
                dsp_epoch,
                privacy_generation: frame.microphone_generation,
                frame_sequence: frame.sequence,
                read_completed_at_us: frame.timing.read_completed_at_us,
                timing: CaptureTiming {
                    first_read_started_at_us: frame.timing.first_read_started_at_us,
                    last_read_started_at_us: frame.timing.last_read_started_at_us,
                    successful_reads: frame.timing.successful_reads,
                    alsa_status_monotonic_us: frame.timing.alsa_status_monotonic_us,
                    status_observed_at_us: frame.timing.status_observed_at_us,
                    available_frames: frame.timing.available_frames,
                    delayed_frames: frame.timing.delayed_frames,
                    processing_completed_at_us,
                    aec_queue_delay_ms: delay,
                    reference: timing,
                },
                probability,
                // What the render explains of this very block, for a
                // coordinator that listens while the lamp speaks. Withheld
                // while the reference is untrusted or the canceller is cold.
                echo: if reference.evidence_trusted() {
                    echo.evidence()
                } else {
                    None
                },
                reference: reference.trust(),
                samples: clean.to_vec(),
                raw,
                linear: words_linear_block,
            })?;
            cost.stage(Stage::Encode, from);
            cost.block();
            if monotonic_us().saturating_sub(last_diagnostic) >= 1_000_000 {
                channels
                    .control
                    .send(WorkerEvent::ReferenceClock { timing })?;
                last_diagnostic = monotonic_us();
            }
            if evidence_due(evidence_skip, evidence_said.0.elapsed()) {
                let counts = echo.evidence_skipped();
                eprintln!(
                    "{}",
                    evidence_line(counts, evidence_said.1, evidence_said.0.elapsed())
                );
                evidence_said = (Instant::now(), counts);
            }
            if evidence_due(vad_linear, vad_said.0.elapsed()) {
                let (since, blocks, linear) = vad_said;
                eprintln!(
                    "lamp-voice: capture: the speech detector read the linear output on {linear} of {blocks} blocks in the last {} s (LAMP_VAD_INPUT={})",
                    since.elapsed().as_secs(),
                    vad_input.value().unwrap_or("")
                );
                vad_said = (Instant::now(), 0, 0);
            }
            if evidence_due(words_linear, words_said.0.elapsed()) {
                let (since, blocks, linear) = words_said;
                eprintln!(
                    "lamp-voice: capture: the stop words were sent the linear output on {linear} of {blocks} blocks in the last {} s (LAMP_STOP_WORDS_SOURCE=linear)",
                    since.elapsed().as_secs()
                );
                words_said = (Instant::now(), 0, 0);
            }
        }
        // A change of trust found while draining reference is not kept
        // waiting for the next capture block.
        report_reference(
            &mut reference,
            channels,
            recording,
            &mut speech,
            &mut dsp_epoch,
            microphone.epoch(),
            privacy,
        )?;
        said(cost.done());
        stall_pass.done();
        pace(
            wait,
            microphone,
            channels,
            link.as_ref(),
            gap,
            primed_at,
            available,
            tick,
        );
    }
}

/// A minute's line from [`crate::stage_cost`], to the journal.
fn said(line: Option<String>) {
    if let Some(line) = line {
        eprintln!("{line}");
    }
}

/// Send what recovery changed: a new DSP epoch for each reseed, with the
/// detector's state started again beside the canceller's, then each status.
fn report_reference(
    reference: &mut ReferenceAssembly,
    channels: &mut WorkerChannels,
    recording: &mut Option<Recording>,
    speech: &mut SpeechProbability,
    dsp_epoch: &mut u64,
    capture_epoch: u64,
    privacy: u64,
) -> Result<()> {
    if reference.take_reseed() {
        *dsp_epoch = dsp_epoch
            .checked_add(1)
            .ok_or_else(|| io::Error::other("DSP epoch exhausted"))?;
        speech.reset();
        channels.control.send(WorkerEvent::CaptureDspReset {
            epoch: capture_epoch,
            dsp_epoch: *dsp_epoch,
            observed_at_us: monotonic_us(),
        })?;
        diagnostic_reset(
            recording,
            ResetReason::DspReset,
            capture_epoch,
            *dsp_epoch,
            reference.timing().playback_epoch,
            privacy,
            None,
        );
    }
    while let Some(status) = reference.take_status() {
        channels
            .control
            .send(WorkerEvent::ReferenceStatus { status })?;
    }
    Ok(())
}

pub fn speaker(
    mut channels: WorkerChannels,
    controller: BootId,
    worker: BootId,
    directory: &Path,
    device: &str,
    diagnostic_directory: Option<&Path>,
) -> Result<()> {
    if device.is_empty()
        || device.len() > 128
        || device.bytes().any(|b| b == 0 || b.is_ascii_control())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "explicit valid speaker alias required",
        )
        .into());
    }
    let mut recording = Recording::start(
        diagnostic_directory,
        controller,
        StreamKind::Render,
        None,
        &mut channels,
    )?;
    let mut speaker = open_speaker(&mut channels, device, controller)?;
    let result = speaker_loop(
        &mut channels,
        &mut speaker,
        worker,
        directory,
        &mut recording,
    );
    // Physical drop and permanent authority invalidation precede diagnostic finalization.
    let stop = speaker.fault();
    drop(speaker);
    if let Err(error) = &result {
        report_fault(&mut channels, error.as_ref());
    }
    if let Err(error) = &stop {
        report_fault(&mut channels, error.as_ref());
    }
    let reason = if stop.is_err() {
        EndReason::Fault
    } else {
        result.as_ref().copied().unwrap_or(EndReason::Fault)
    };
    let finish = recording.map_or(Ok(()), |r| r.finish(&mut channels, reason));
    result.map(|_| ()).and(stop).and(finish)
}
/// The speaker's clock opened, a busy device waited out first as the
/// microphone's is (#4332): the coordinator is told at the first busy answer,
/// before this worker is ready, so it waits for it that much longer. Nothing
/// is sent to this worker before it is ready, so there is no lease to keep
/// and no control to answer while it waits; a stop ends the coordinator's
/// wait, which kills this process.
fn open_speaker(
    channels: &mut WorkerChannels,
    device: &str,
    controller: BootId,
) -> Result<Speaker> {
    let (speaker, waited) = open_waiting_out_busy(
        || Speaker::open_clock(device, controller),
        lamp_audio::pcm::open_fault,
        || {
            eprintln!(
                "lamp-voice: speaker: the speaker {device} is busy at start; waiting up to {} s for it, as the microphone waits for its own",
                SPEAKER_BUSY_AT_START.as_secs()
            );
            channels.control.send(WorkerEvent::BusyAtStart)?;
            Ok(())
        },
        |until| {
            std::thread::sleep(until.saturating_duration_since(Instant::now()));
            Ok(())
        },
        Instant::now,
    )?;
    if let Some(waited) = waited {
        eprintln!(
            "lamp-voice: speaker: the speaker was busy for {:.1} s at start; opened on try {}",
            waited.waited_ms as f64 / 1000.0,
            waited.opened_on_try
        );
    }
    Ok(speaker)
}
struct PendingChunk {
    chunk: SpeakerChunk,
    offset: usize,
}
#[derive(Default)]
struct ReferenceOutput {
    sequence: u64,
    privacy: Option<u64>,
    primed_at: Option<u64>,
    prime_last_write: Option<Written>,
    prime_barrier: PrimeBarrier,
}
impl ReferenceOutput {
    fn reset(&mut self) {
        self.sequence = 0;
        self.primed_at = None;
        self.prime_last_write = None;
        self.prime_barrier.reset();
    }
    fn fault(&self, speaker: &Speaker, reason: ReferenceFaultKind) -> ReferenceFault {
        let range = speaker.queued_range();
        ReferenceFault {
            reason,
            at_us: monotonic_us(),
            expected_sequence: self.sequence.saturating_add(1),
            received_sequence: None,
            received_first_sample: None,
            timing: ReferenceTiming {
                playback_epoch: range.epoch,
                accepted_through: range.accepted_through,
                queue_observed_at_us: speaker.last_poll().map_or(0, |s| s.observed_at_us),
                queued_frames: speaker.last_poll().map_or(0, |s| s.queued_frames),
                ..ReferenceTiming::default()
            },
        }
    }
    fn prime(
        &mut self,
        speaker: &mut Speaker,
        link: &mut ReferenceChannels,
        recording: &mut Option<Recording>,
    ) -> Result<()> {
        if self.prime_barrier.is_waiting() || speaker.clock_started() {
            return Ok(());
        }
        let token = speaker.zero_authority()?;
        if Some(token.microphone_generation()) != self.privacy {
            return Err(self
                .fault(speaker, ReferenceFaultKind::PrivacyMismatch)
                .into());
        }
        if self.primed_at.is_none() && speaker.queued_range().epoch == 1 {
            diagnostic_reset(
                recording,
                ResetReason::Start,
                0,
                0,
                1,
                token.microphone_generation(),
                None,
            );
        }
        self.primed_at.get_or_insert_with(monotonic_us);
        let remaining =
            PRIME_FRAMES.saturating_sub(speaker.queued_range().accepted_through as usize);
        if remaining > 0 {
            let written = speaker.try_write_silence(token, remaining.min(RENDER_SAMPLES))?;
            if written.frames > 0 {
                if let Some(recording) = recording {
                    let meta = recording.render(written, None, None);
                    let _ = recording.recorder.try_silence(
                        meta,
                        diagnostics::SilenceKind::Prime,
                        written.frames as u16,
                    );
                }
                self.prime_last_write = Some(written);
            }
        }
        if speaker.queued_range().accepted_through == PRIME_FRAMES as u64 {
            let last = self
                .prime_last_write
                .ok_or_else(|| self.fault(speaker, ReferenceFaultKind::InvalidPrime))?;
            if last.queued_frames != PRIME_FRAMES as i64 {
                return Err(self.fault(speaker, ReferenceFaultKind::InvalidPrime).into());
            }
            let prime = ClockPrimed {
                playback_epoch: speaker.queued_range().epoch,
                privacy_generation: token.microphone_generation(),
                accepted_zero_frames: PRIME_FRAMES,
                accepted_at_us: last.completed_at_us,
                queue_observed_at_us: last.queue_observed_at_us,
                queued_frames: last.queued_frames as usize,
                discarded: speaker.last_discard(),
            };
            link.control.send(ReferenceControl::ClockPrimed { prime })?;
            let began_at = self
                .primed_at
                .ok_or_else(|| self.fault(speaker, ReferenceFaultKind::InvalidPrime))?;
            self.prime_barrier
                .sent(prime, began_at)
                .map_err(|reason| self.fault(speaker, reason))?;
        }
        Ok(())
    }
    fn send(
        &mut self,
        link: &mut ReferenceChannels,
        written: Written,
        payload: RenderPayload,
    ) -> Result<()> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("render sequence exhausted"))?;
        link.data.send(RenderReference {
            playback_epoch: written.first_sample.epoch,
            privacy_generation: self
                .privacy
                .ok_or_else(|| io::Error::other("missing reference privacy"))?,
            sequence,
            first_sample: written.first_sample.frame,
            accepted_at_us: written.completed_at_us,
            queue_observed_at_us: written.queue_observed_at_us,
            queued_frames: written.queued_frames as usize,
            payload,
        })?;
        self.sequence = sequence;
        Ok(())
    }
}

fn speaker_loop(
    channels: &mut WorkerChannels,
    speaker: &mut Speaker,
    worker: BootId,
    directory: &Path,
    recording: &mut Option<Recording>,
) -> Result<EndReason> {
    let mut link: Option<ReferenceChannels> = None;
    let mut output = ReferenceOutput::default();
    let mut pending: Option<PendingChunk> = None;
    let mut speech = SpeechPlayback::default();
    let mut last_sequence = 0;
    let mut flush_count = speaker.flush_count();
    // Since when the authority has been lapsed, the speaker silent (#1178).
    let mut lapsed_since: Option<u64> = None;
    channels.control.send(WorkerEvent::Ready)?;
    // With LAMP_STALLS=on, a pass or an overrun wait of 50 ms or more is
    // recorded with what held it (#1178).
    let mut stalls = crate::stalls::Stalls::from_environment("speaker");
    'tick: loop {
        let tick = Instant::now();
        let stall_pass = stalls.begin();
        for index in 0..=MAX_CONTROL {
            let Some(control) = channels.control.receive_renewed(Control::is_renewal)? else {
                break;
            };
            if index == MAX_CONTROL {
                return Err(io::Error::other("speaker control tick bound").into());
            }
            match control {
                Control::Authority { snapshot } => {
                    if let Err(error) = speaker.install(snapshot) {
                        if underran(error.as_ref()) {
                            // Installed before the check underran (#3697).
                            underrun(
                                channels,
                                speaker,
                                &mut output,
                                &mut flush_count,
                                speech.unfinished_owner(),
                                &mut speech,
                                &mut pending,
                                recording,
                            )?;
                        } else if error
                            .downcast_ref::<InteractionError>()
                            .is_some_and(|reason| rejected_old_state(*reason))
                        {
                            continue;
                        } else if snapshot.microphone_permission() == Permission::Allowed
                            || error.downcast_ref::<InteractionError>()
                                != Some(&InteractionError::PrivacyClosed)
                        {
                            return Err(error);
                        }
                    }
                    if let Some(recording) = recording {
                        recording.observe_privacy(snapshot);
                    }
                    if output.privacy.is_some()
                        && snapshot.microphone_permission() != Permission::Allowed
                    {
                        // install may already have dropped queued PCM. Preserve
                        // that original discarded range instead of replacing it
                        // with a second empty drop before its diagnostic receipt.
                        if speaker.flush_count() == flush_count {
                            speaker.stop()?;
                        }
                        report_speech_reset(channels, &mut speech)?;
                        diagnostic_reset(
                            recording,
                            ResetReason::PrivacyChange,
                            0,
                            0,
                            speaker.queued_range().epoch,
                            snapshot.microphone_generation(),
                            speaker.last_discard(),
                        );
                        if let Some(link) = link.as_mut() {
                            link.notify_stopped_after_local_close(speaker.queued_range().epoch);
                        }
                        return Ok(EndReason::PrivacyClosed);
                    }
                    supersede_speech(
                        channels,
                        speaker,
                        &mut speech,
                        snapshot,
                        output.privacy,
                        flush_count,
                    )?;
                }
                Control::ConnectReference { peer } => {
                    if link.is_some() {
                        return Err(io::Error::other("reference peer already bound").into());
                    }
                    link = Some(ReferenceChannels::bind(directory, "speaker", worker, peer)?);
                }
                Control::Stop => {
                    speaker.stop()?;
                    report_speech_reset(channels, &mut speech)?;
                    diagnostic_reset(
                        recording,
                        ResetReason::Stop,
                        0,
                        0,
                        speaker.queued_range().epoch,
                        output.privacy.unwrap_or(0),
                        speaker.last_discard(),
                    );
                    if let Some(link) = link.as_mut() {
                        link.notify_stopped_after_local_close(speaker.queued_range().epoch);
                    }
                    return Ok(EndReason::Stopped);
                }
                Control::StartCapture | Control::ProviderOutputCapacity { .. } => {
                    return Err(io::Error::other("invalid speaker command").into());
                }
            }
        }
        // #1178: held up past its lease (on lamp-4ace at midnight, the log
        // sync loading the eMMC; a turn's lease is 100 ms) with nothing
        // fresher queued, the authority has lapsed. That costs a gap, never
        // the voice, as the provider workers skip a lapsed snapshot: the
        // device stops, its reset is reported as after any discard, and
        // nothing is written or primed until a fresh authority is installed
        // above; then the clock is primed again, as after any reset.
        if speaker.authority_lapsed() {
            lapse(
                channels,
                speaker,
                &mut output,
                &mut flush_count,
                &mut speech,
                &mut pending,
                recording,
                &mut lapsed_since,
            )?;
            stall_pass.done();
            finish_tick(tick);
            continue;
        }
        if let Some(since) = lapsed_since.take() {
            eprintln!(
                "lamp-voice speaker: a fresh authority after {} ms silent; going on",
                monotonic_us().saturating_sub(since) / 1_000
            );
        }
        // It may lapse during the tick too; the device has then stopped.
        match speaker.maintain() {
            Ok(()) => {}
            Err(error) if underran(error.as_ref()) => {
                underrun(
                    channels,
                    speaker,
                    &mut output,
                    &mut flush_count,
                    speech.unfinished_owner(),
                    &mut speech,
                    &mut pending,
                    recording,
                )?;
                stall_pass.done();
                finish_tick(tick);
                continue 'tick;
            }
            Err(error) if lapsed(error.as_ref()) => {
                lapse(
                    channels,
                    speaker,
                    &mut output,
                    &mut flush_count,
                    &mut speech,
                    &mut pending,
                    recording,
                    &mut lapsed_since,
                )?;
                stall_pass.done();
                finish_tick(tick);
                continue 'tick;
            }
            Err(error) => return Err(error),
        }
        if report_discard(channels, speaker, &mut output, &mut flush_count, recording)? {
            report_speech_reset(channels, &mut speech)?;
        }
        if let Some(tail) = speaker.take_retired_tail()? {
            channels.control.send(WorkerEvent::CancelledTailRetired {
                owner: tail.owner,
                superseded_by: tail.superseded_by,
                first_sample: tail.first_sample,
                end_sample: tail.end_sample,
                queued_frames: tail.queued_frames,
                speech_frames: tail.speech_frames,
                started_at_us: tail.started_at_us,
                observed_at_us: speaker
                    .last_poll()
                    .ok_or_else(|| {
                        io::Error::other("tail retirement requires a device observation")
                    })?
                    .observed_at_us,
            })?;
        }
        // A hard deadline can expire while collecting the retirement receipt.
        if report_discard(channels, speaker, &mut output, &mut flush_count, recording)? {
            report_speech_reset(channels, &mut speech)?;
        }
        let Some(link) = link.as_mut() else {
            stall_pass.done();
            finish_tick(tick);
            continue;
        };
        if !link.connect_step()? {
            stall_pass.done();
            finish_tick(tick);
            continue;
        }
        for index in 0..=MAX_CONTROL {
            let Some(control) = link.receive_control()? else {
                break;
            };
            if index == MAX_CONTROL {
                return Err(io::Error::other("speaker reference control tick bound").into());
            }
            match control {
                ReferenceControl::CapturePrepared { privacy_generation }
                    if output.privacy.is_none() =>
                {
                    output.privacy = Some(privacy_generation);
                }
                ReferenceControl::ReferencePrimed { prepared } => {
                    // The capture side's answer to a prime the speaker reset
                    // before it came (a lapse during the handshake, #1178):
                    // that epoch is gone, and its late answer starts nothing.
                    // Taken, it ended the worker (`InvalidStart`; Ten on #1214).
                    if prepared.playback_epoch < speaker.queued_range().epoch {
                        eprintln!(
                            "lamp-voice speaker: a late prime of epoch {} from before a reset is dropped",
                            prepared.playback_epoch
                        );
                        continue;
                    }
                    let token = match speaker.zero_authority() {
                        Ok(token) => token,
                        Err(error) if lapsed(error.as_ref()) => {
                            lapse(
                                channels,
                                speaker,
                                &mut output,
                                &mut flush_count,
                                &mut speech,
                                &mut pending,
                                recording,
                                &mut lapsed_since,
                            )?;
                            stall_pass.done();
                            finish_tick(tick);
                            continue 'tick;
                        }
                        Err(error) => return Err(error),
                    };
                    let playback_epoch = output
                        .prime_barrier
                        .acknowledge(
                            prepared,
                            speaker.queued_range(),
                            token.microphone_generation(),
                            monotonic_us(),
                        )
                        .map_err(|reason| output.fault(speaker, reason))?;
                    // A host bound on the method call, not exact DAC start.
                    // Failed/expired starts return before any success receipt.
                    let start_requested_at_us = monotonic_us();
                    let started_at_us = match speaker.start_clock(token) {
                        Ok(at) => at,
                        Err(error) if lapsed(error.as_ref()) => {
                            lapse(
                                channels,
                                speaker,
                                &mut output,
                                &mut flush_count,
                                &mut speech,
                                &mut pending,
                                recording,
                                &mut lapsed_since,
                            )?;
                            stall_pass.done();
                            finish_tick(tick);
                            continue 'tick;
                        }
                        Err(error) => return Err(error),
                    };
                    output.primed_at = None;
                    link.control.send(ReferenceControl::ClockStarted {
                        playback_epoch,
                        start_requested_at_us,
                        started_at_us,
                    })?;
                }
                _ => {
                    return Err(output
                        .fault(speaker, ReferenceFaultKind::InvalidStart)
                        .into());
                }
            }
        }
        if output
            .primed_at
            .is_some_and(|at| monotonic_us().saturating_sub(at) >= BARRIER_US)
        {
            return Err(output
                .fault(speaker, ReferenceFaultKind::BarrierTimeout)
                .into());
        }
        if output.privacy.is_some() && !speaker.clock_started() {
            match output.prime(speaker, link, recording) {
                Ok(()) => {}
                Err(error) if underran(error.as_ref()) => {
                    underrun(
                        channels,
                        speaker,
                        &mut output,
                        &mut flush_count,
                        speech.unfinished_owner(),
                        &mut speech,
                        &mut pending,
                        recording,
                    )?;
                    stall_pass.done();
                    finish_tick(tick);
                    continue 'tick;
                }
                Err(error) if lapsed(error.as_ref()) => {
                    lapse(
                        channels,
                        speaker,
                        &mut output,
                        &mut flush_count,
                        &mut speech,
                        &mut pending,
                        recording,
                        &mut lapsed_since,
                    )?;
                    stall_pass.done();
                    finish_tick(tick);
                    continue 'tick;
                }
                Err(error) => return Err(error),
            }
        }
        if !speaker.clock_started() {
            stall_pass.done();
            finish_tick(tick);
            continue;
        }
        report_speech_drained(channels, speaker, &mut speech)?;
        if let Some(final_write) = speech.take_retired(speaker.queued_range()) {
            channels.control.send(WorkerEvent::SpeechRetired {
                owner: final_write.owner,
                sequence: final_write.sequence,
                final_sample: final_write.cursor,
                observed_at_us: speaker
                    .last_poll()
                    .map_or_else(monotonic_us, |s| s.observed_at_us),
            })?;
        }
        if pending.is_none()
            && let Some(chunk) = channels.data.receive::<SpeakerChunk>()?
        {
            if chunk.samples.is_empty()
                || chunk.samples.len() > RENDER_SAMPLES
                || chunk.chunk_sequence <= last_sequence
                || (chunk.end_of_speech && chunk.samples.len() != RENDER_SAMPLES)
            {
                return Err(
                    io::Error::other("invalid speaker chunk size/sequence/final block").into(),
                );
            }
            last_sequence = chunk.chunk_sequence;
            match speaker.install(chunk.snapshot) {
                Ok(()) => supersede_speech(
                    channels,
                    speaker,
                    &mut speech,
                    chunk.snapshot,
                    output.privacy,
                    flush_count,
                )?,
                Err(error) if old_interaction_error(error.as_ref()) => {}
                // Installed before the check underran (#3697); reset below.
                Err(error) if underran(error.as_ref()) => underrun(
                    channels,
                    speaker,
                    &mut output,
                    &mut flush_count,
                    speech.unfinished_owner(),
                    &mut speech,
                    &mut pending,
                    recording,
                )?,
                Err(error) => return Err(error),
            }
            if report_discard(channels, speaker, &mut output, &mut flush_count, recording)? {
                report_speech_reset(channels, &mut speech)?;
            }
            pending = Some(PendingChunk { chunk, offset: 0 });
        }
        // Aim for 10..20 ms. Provider pacing must not stop the physical clock:
        // accept real zero-only PCM if speech is unavailable, and report that
        // delivery gap without consuming or completing any missing speech.
        let writable_block = speaker
            .last_poll()
            .is_some_and(|status| status.tracked_authorized_frames <= RENDER_SAMPLES);
        if speaker.clock_started() && writable_block {
            let mut wrote_speech = false;
            if let Some(partial) = pending.as_mut() {
                // Read before this write's reset report clears it (#3697).
                let speaking = speech
                    .unfinished_owner()
                    .or((partial.offset > 0).then(|| partial.chunk.permit.owner()));
                let result = speaker.try_write(
                    partial.chunk.permit,
                    &partial.chunk.samples[partial.offset..],
                );
                // Record actual acceptance under its original epoch even if the
                // post-write permission check already dropped it. The following
                // reset receipt marks that tail; it never reaches new-epoch AEC.
                if let Ok(written) = &result
                    && written.frames > 0
                    && let Some(recording) = recording
                {
                    let end = partial
                        .offset
                        .checked_add(written.frames)
                        .filter(|end| *end <= partial.chunk.samples.len())
                        .ok_or_else(|| {
                            io::Error::other("invalid diagnostic accepted speaker count")
                        })?;
                    let meta = recording.render(
                        *written,
                        Some(partial.chunk.permit.owner()),
                        Some(partial.chunk.chunk_sequence),
                    );
                    let _ = recording
                        .recorder
                        .try_render(meta, &partial.chunk.samples[partial.offset..end]);
                }
                if report_discard(channels, speaker, &mut output, &mut flush_count, recording)? {
                    report_speech_reset(channels, &mut speech)?;
                }
                match result {
                    Ok(written) => {
                        if !accepted_write_survived(
                            written.first_sample,
                            written.end_sample,
                            speaker.queued_range(),
                            written.frames,
                        )? {
                            // Receipt first; the dropped tail is never new-epoch render.
                            pending = None;
                            stall_pass.done();
                            finish_tick(tick);
                            continue;
                        }
                        if written.frames > 0 {
                            wrote_speech = true;
                            let end = partial
                                .offset
                                .checked_add(written.frames)
                                .filter(|end| *end <= partial.chunk.samples.len())
                                .ok_or_else(|| {
                                    io::Error::other("invalid accepted speaker count")
                                })?;
                            let owner = partial.chunk.permit.owner();
                            output.send(
                                link,
                                written,
                                RenderPayload::Speech {
                                    owner,
                                    samples: partial.chunk.samples[partial.offset..end].to_vec(),
                                },
                            )?;
                            partial.offset = end;
                            if let Some(gap) = speech.speech_accepted(
                                owner,
                                partial.chunk.chunk_sequence,
                                written.first_sample,
                                written.end_sample,
                                written.completed_at_us,
                                partial.chunk.end_of_speech
                                    && partial.offset == partial.chunk.samples.len(),
                            )? {
                                channels.control.send(gap)?;
                            }
                            channels.control.send(WorkerEvent::Playback {
                                owner,
                                first_sample: written.first_sample,
                                end_sample: written.end_sample,
                                queue_observed_at_us: written.queue_observed_at_us,
                                turn: owner.turn(),
                                sequence: partial.chunk.chunk_sequence,
                                queued_frames: written.queued_frames as usize,
                                accepted_frames: written.frames,
                                observed_at_us: written.completed_at_us,
                            })?;
                            if partial.offset == partial.chunk.samples.len() {
                                pending = None;
                            }
                        }
                    }
                    Err(error) if old_interaction_error(error.as_ref()) => {
                        channels.control.send(WorkerEvent::PlaybackRejected {
                            turn: partial.chunk.permit.owner().turn(),
                            sequence: partial.chunk.chunk_sequence,
                            reason: error.to_string(),
                            lapsed: lapsed(error.as_ref()),
                        })?;
                        pending = None;
                    }
                    Err(error) if underran(error.as_ref()) => {
                        underrun(
                            channels,
                            speaker,
                            &mut output,
                            &mut flush_count,
                            speaking,
                            &mut speech,
                            &mut pending,
                            recording,
                        )?;
                        stall_pass.done();
                        finish_tick(tick);
                        continue 'tick;
                    }
                    Err(error) => return Err(error),
                }
            }
            // A stale independently queued packet is rejected above without
            // sacrificing this tick's zero reference. A valid successor waits
            // while the immutable old tail retires; it cannot extend that tail.
            if speaker.clock_started()
                && !wrote_speech
                && (pending.is_none() || speaker.is_retiring_tail())
            {
                let written = match speaker
                    .zero_authority()
                    .and_then(|token| speaker.try_write_silence(token, RENDER_SAMPLES))
                {
                    Ok(written) => written,
                    Err(error) if underran(error.as_ref()) => {
                        underrun(
                            channels,
                            speaker,
                            &mut output,
                            &mut flush_count,
                            speech.unfinished_owner(),
                            &mut speech,
                            &mut pending,
                            recording,
                        )?;
                        stall_pass.done();
                        finish_tick(tick);
                        continue 'tick;
                    }
                    Err(error) if lapsed(error.as_ref()) => {
                        lapse(
                            channels,
                            speaker,
                            &mut output,
                            &mut flush_count,
                            &mut speech,
                            &mut pending,
                            recording,
                            &mut lapsed_since,
                        )?;
                        stall_pass.done();
                        finish_tick(tick);
                        continue 'tick;
                    }
                    Err(error) => return Err(error),
                };
                if written.frames > 0 {
                    if let Some(recording) = recording {
                        let meta = recording.render(written, None, None);
                        let _ = recording.recorder.try_silence(
                            meta,
                            if speech.unfinished_owner().is_some() {
                                diagnostics::SilenceKind::SpeechGap
                            } else {
                                diagnostics::SilenceKind::Idle
                            },
                            written.frames as u16,
                        );
                    }
                    if !accepted_write_survived(
                        written.first_sample,
                        written.end_sample,
                        speaker.queued_range(),
                        written.frames,
                    )? {
                        return Err(output
                            .fault(speaker, ReferenceFaultKind::ClockStopped)
                            .into());
                    }
                    output.send(
                        link,
                        written,
                        RenderPayload::Silence {
                            frames: written.frames as u16,
                        },
                    )?;
                    if let Some(gap) = speech.zeros_accepted(
                        written.first_sample,
                        written.end_sample,
                        written.completed_at_us,
                    )? {
                        channels.control.send(gap)?;
                    }
                }
            }
        }
        stall_pass.done();
        finish_tick(tick);
    }
}

fn report_speech_drained(
    channels: &mut WorkerChannels,
    speaker: &Speaker,
    speech: &mut SpeechPlayback,
) -> Result<()> {
    if let Some(drained) = speech.take_drained(speaker.queued_range()) {
        channels.control.send(WorkerEvent::SpeechDrained {
            owner: drained.owner,
            sequence: drained.sequence,
            last_sample: drained.cursor,
            observed_at_us: speaker
                .last_poll()
                .ok_or_else(|| io::Error::other("speech retirement requires a device observation"))?
                .observed_at_us,
        })?;
    }
    Ok(())
}

fn report_speech_reset(channels: &mut WorkerChannels, speech: &mut SpeechPlayback) -> Result<()> {
    if let Some(gap) = speech.reset(monotonic_us()) {
        channels.control.send(gap)?;
    }
    Ok(())
}

fn supersede_speech(
    channels: &mut WorkerChannels,
    speaker: &mut Speaker,
    speech: &mut SpeechPlayback,
    snapshot: lamp_turn_owner::Snapshot,
    microphone_generation: Option<u64>,
    previous_flush_count: u64,
) -> Result<()> {
    if speech
        .owner()
        .is_some_and(|owner| snapshot.owner() != Some(owner))
        && speaker.flush_count() == previous_flush_count
    {
        // Retire from the checked old queue before any explicit drop clears
        // that observation. A drop itself is reported separately, never drained.
        report_speech_drained(channels, speaker, speech)?;
        if !speech.can_supersede(
            snapshot,
            microphone_generation,
            MonoTime::from_micros(monotonic_us()),
        ) {
            speaker.stop()?;
        }
        report_speech_reset(channels, speech)?;
    }
    Ok(())
}

fn report_discard(
    channels: &mut WorkerChannels,
    speaker: &mut Speaker,
    output: &mut ReferenceOutput,
    flush_count: &mut u64,
    recording: &mut Option<Recording>,
) -> Result<bool> {
    if speaker.flush_count() == *flush_count {
        return Ok(false);
    }
    *flush_count = speaker.flush_count();
    channels.control.send(WorkerEvent::PlaybackClockReset {
        previous: speaker
            .last_discard()
            .ok_or_else(|| io::Error::other("physical reset lost its original cursor range"))?,
        next_epoch: speaker.queued_range().epoch,
        observed_at_us: monotonic_us(),
    })?;
    diagnostic_reset(
        recording,
        ResetReason::PlaybackReset,
        0,
        0,
        speaker.queued_range().epoch,
        output.privacy.unwrap_or(0),
        speaker.last_discard(),
    );
    if let Some(discarded) = speaker.take_discard() {
        channels.control.send(WorkerEvent::PlaybackDiscarded {
            owner: discarded.owner,
            reason: discarded.reason.into(),
            queued_frames: discarded.queued_frames,
            other_owners: discarded.other_owners,
            observed_at_us: monotonic_us(),
        })?;
    }
    // Speech an underrun dropped (#3697), named as a discard names it.
    if let Some(underran) = speaker.take_underran() {
        channels.control.send(WorkerEvent::PlaybackDiscarded {
            owner: underran.owner,
            reason: PlaybackDiscardReason::Underran,
            queued_frames: underran.queued_frames,
            other_owners: underran.other_owners,
            observed_at_us: monotonic_us(),
        })?;
    }
    output.reset();
    Ok(true)
}
fn report_fault(
    channels: &mut WorkerChannels,
    error: &(dyn std::error::Error + Send + Sync + 'static),
) {
    if let Some(fault) = error.downcast_ref::<ReferenceFault>() {
        let _ = channels
            .control
            .send(WorkerEvent::ReferenceFault { fault: *fault });
    } else {
        let _ = channels.control.send(WorkerEvent::Fault {
            code: error.to_string(),
        });
    }
}
/// Whether an error is the speaker's authority lapsing (#1178).
fn lapsed(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<InteractionError>() == Some(&InteractionError::ExpiredState)
}

/// The speaker's authority lapsed (#1178): once a lapse, stop the device
/// unless a check already has, report the reset as after any discard, and
/// say so. Nothing is written until a fresh authority is installed.
///
/// What it was saying is lost, and the coordinator is told so in the forms
/// it takes as a cut answer, never as a failed voice (the reviewer on
/// #1214): queued speech is dropped by its own ownership check, which names
/// its owner and the lapse (`PlaybackDiscarded`, `ExpiredState`), and the
/// chunk not yet written in full is refused as lapsed (`PlaybackRejected`),
/// so it is never written under its old permit after the fresh authority.
#[allow(clippy::too_many_arguments)]
fn lapse(
    channels: &mut WorkerChannels,
    speaker: &mut Speaker,
    output: &mut ReferenceOutput,
    flush_count: &mut u64,
    speech: &mut SpeechPlayback,
    pending: &mut Option<PendingChunk>,
    recording: &mut Option<Recording>,
    lapsed_since: &mut Option<u64>,
) -> Result<()> {
    if let Some(since) = *lapsed_since {
        // Past the capture worker's own bound the voice has ended anyway,
        // and a speaker whose coordinator is gone (its datagrams give no
        // end) must not keep the device until systemd stops it.
        if monotonic_us().saturating_sub(since) >= LAPSE_MOST_US {
            return Err(InteractionError::ExpiredState.into());
        }
        return Ok(());
    }
    *lapsed_since = Some(monotonic_us());
    match speaker.maintain() {
        Ok(()) => {}
        // Either way the device has stopped; its discard is reported below.
        Err(error) if lapsed(error.as_ref()) || underran(error.as_ref()) => {}
        Err(error) => return Err(error),
    }
    if speaker.flush_count() == *flush_count {
        speaker.stop()?;
    }
    if report_discard(channels, speaker, output, flush_count, recording)? {
        report_speech_reset(channels, speech)?;
    }
    if let Some(partial) = pending.take() {
        channels.control.send(WorkerEvent::PlaybackRejected {
            turn: partial.chunk.permit.owner().turn(),
            sequence: partial.chunk.chunk_sequence,
            reason: InteractionError::ExpiredState.to_string(),
            lapsed: true,
        })?;
    }
    eprintln!(
        "lamp-voice speaker: its authority lapsed while it was held up; silent until a fresh one"
    );
    Ok(())
}

/// Whether an error is the speaker underrunning with only zeros queued (#3697).
fn underran(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<Underrun>().is_some()
}

/// The speaker underran while it was held up (#3697): on lamp-52e6 0.39 s
/// into a start, as the models loaded, it ended the voice, and the restart
/// guard muted the next start. `pcm` stopped the device and kept the guard,
/// so, as after any reset, the discard is reported and the clock primed again
/// through the barrier: a gap, never the voice.
///
/// An answer under way (`speaking`, read before any reset this tick
/// reported clears it) loses what it was saying mid-word, as after a lapse
/// (#1178): its queued speech is reported dropped (`Underran`), and its chunk
/// waiting or part-written is refused as lapsed, so the coordinator cuts the
/// answer and nothing is written again under its old permit. Another
/// owner's waiting chunk, never begun, is kept for after the prime.
#[allow(clippy::too_many_arguments)]
fn underrun(
    channels: &mut WorkerChannels,
    speaker: &mut Speaker,
    output: &mut ReferenceOutput,
    flush_count: &mut u64,
    speaking: Option<TurnOwner>,
    speech: &mut SpeechPlayback,
    pending: &mut Option<PendingChunk>,
    recording: &mut Option<Recording>,
) -> Result<()> {
    let cut = speaking
        .or(speaker.underran().map(|dropped| dropped.owner))
        .or(pending
            .as_ref()
            .filter(|partial| partial.offset > 0)
            .map(|partial| partial.chunk.permit.owner()));
    if report_discard(channels, speaker, output, flush_count, recording)? {
        report_speech_reset(channels, speech)?;
    }
    let Some(cut) = cut else {
        eprintln!(
            "lamp-voice speaker: it underran while held up with only zeros queued; priming again"
        );
        return Ok(());
    };
    if pending
        .as_ref()
        .is_some_and(|partial| partial.offset > 0 || partial.chunk.permit.owner() == cut)
        && let Some(partial) = pending.take()
    {
        channels.control.send(WorkerEvent::PlaybackRejected {
            turn: partial.chunk.permit.owner().turn(),
            sequence: partial.chunk.chunk_sequence,
            reason: Underrun.to_string(),
            lapsed: true,
        })?;
    }
    eprintln!(
        "lamp-voice speaker: it underran while held up mid-answer; the answer is cut, priming again"
    );
    Ok(())
}

fn rejected_old_state(error: InteractionError) -> bool {
    matches!(
        error,
        InteractionError::StaleSnapshot
            | InteractionError::WrongBoot
            | InteractionError::FutureState
            | InteractionError::ExpiredState
    )
}
fn old_interaction_error(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error
        .downcast_ref::<InteractionError>()
        .is_some_and(|error| {
            rejected_old_state(*error)
                || matches!(
                    error,
                    InteractionError::PrivacyClosed
                        | InteractionError::NoAuthority
                        | InteractionError::ExpiredPermit
                        | InteractionError::FuturePermit
                        | InteractionError::StaleOwner
                        | InteractionError::InputStillActive
                        | InteractionError::StaleCameraGrant
                        | InteractionError::StalePresentation
                        | InteractionError::StateTooOld
                )
        })
}
/// Whether `LAMP_CAPTURE_WAIT` says `on`.
fn wait_switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch as the board's capture worker reads it.
fn capture_wait_from_environment() -> bool {
    wait_switched_on(
        std::env::var(crate::quiet_pace::CAPTURE_WAIT_ENVIRONMENT)
            .ok()
            .as_deref(),
    )
}

/// How a wait between passes ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waited {
    /// A block was already there: no wait.
    AtOnce,
    Samples,
    Sockets,
    Timeout,
    /// The device cannot be waited on, the wait failed, or it woke with
    /// neither before its bound: a tick instead.
    Ticked,
}

/// One wait between passes with `LAMP_CAPTURE_WAIT=on`: the microphone
/// (`device`, woken once the block being assembled can be read whole, its
/// `avail_min`) and the sockets in one `poll`, for the whole wait, until
/// [`WAIT_AT_MOST`] from the pass's start, `tick`. So a block is read as it
/// arrives, whenever the render comes. `block_there`: the device already
/// held a block after the last read, so there is no wait. Nothing is read
/// here: the next pass reads it in its old order.
fn wait_between_passes(
    sockets: &[std::os::fd::BorrowedFd<'_>],
    block_there: bool,
    tick: Instant,
    device: impl FnOnce(
        &[std::os::fd::BorrowedFd<'_>],
        Duration,
    ) -> Option<
        std::result::Result<lamp_audio::device::Woken, lamp_audio::device::DeviceFault>,
    >,
) -> Waited {
    if block_there {
        return Waited::AtOnce;
    }
    let deadline = tick + WAIT_AT_MOST;
    match device(sockets, deadline.saturating_duration_since(Instant::now())) {
        Some(Ok(woken)) if woken.samples => Waited::Samples,
        Some(Ok(woken)) if woken.others => Waited::Sockets,
        Some(Ok(_)) if Instant::now() >= deadline => Waited::Timeout,
        // A descriptor that woke with neither (a plugin's) may still be
        // readable: a tick before it is polled again, never a busy loop.
        Some(Ok(_)) => {
            std::thread::sleep(TICK);
            Waited::Ticked
        }
        // A failed wait is never a busy loop: it waits as the tick does,
        // and the read finds what failed.
        None | Some(Err(_)) => {
            finish_tick(tick);
            Waited::Ticked
        }
    }
}

/// The end of a pass: the old tick, or with the switch a wait on the
/// microphone and the sockets (the coordinator's control, which carries the
/// stop, and the speaker's reference control and render). It waits only
/// while the microphone runs with nothing pending on time alone (no gap, no
/// prime awaiting its start); otherwise it ticks as before. `available`: the
/// samples the device still held after the block this pass read.
#[allow(clippy::too_many_arguments)]
fn pace<D: CaptureDevice>(
    wait: bool,
    microphone: &mut MicrophoneOn<D>,
    channels: &WorkerChannels,
    link: Option<&ReferenceChannels>,
    gap: Option<u64>,
    primed_at: Option<u64>,
    available: Option<i64>,
    tick: Instant,
) {
    use std::os::fd::AsFd;
    let Some(link) =
        link.filter(|_| wait && microphone.is_running() && gap.is_none() && primed_at.is_none())
    else {
        finish_tick(tick);
        return;
    };
    let block_there =
        available.is_some_and(|available| available >= lamp_audio::CAPTURE_SAMPLES as i64);
    let sockets = [
        channels.control.as_fd(),
        link.control.as_fd(),
        link.data.as_fd(),
    ];
    wait_between_passes(&sockets, block_there, tick, |sockets, timeout| {
        microphone.wait(sockets, timeout)
    });
}

fn finish_tick(started: Instant) {
    if let Some(remaining) = TICK.checked_sub(started.elapsed()) {
        std::thread::sleep(remaining);
    }
}

#[cfg(test)]
mod evidence_line_tests {
    use super::*;

    #[test]
    fn the_journal_says_the_blocks_skipped_since_its_last_line() {
        use lamp_audio::evidence::Skipped;
        let now = Skipped {
            observed: 7_000,
            skipped: 6_100,
            saved: 4_500,
        };
        let then = Skipped {
            observed: 1_000,
            skipped: 400,
            saved: 400,
        };
        assert_eq!(
            evidence_line(now, then, Duration::from_millis(60_400)),
            "lamp-voice: capture: the echo search was skipped on 5700 of 6000 blocks in the last 60 s, 4100 of them loud or with an alignment held (LAMP_QUIET_EVIDENCE=on)"
        );
    }

    /// Said only with the switch, and then once a minute.
    #[test]
    fn the_line_is_said_only_with_the_switch_once_a_minute() {
        let minute = EVIDENCE_SAID_EVERY;
        assert!(!evidence_due(false, minute * 10));
        assert!(!evidence_due(true, minute - Duration::from_millis(1)));
        assert!(evidence_due(true, minute));
    }
}

#[cfg(test)]
mod capture_wait_tests {
    use super::*;
    use lamp_audio::device::{DeviceFault, Woken};
    use rustix::event::{PollFd, PollFlags, poll};
    use std::{
        cell::Cell,
        os::{
            fd::{AsFd, BorrowedFd, OwnedFd},
            unix::net::UnixDatagram,
        },
    };

    type Answer = Option<std::result::Result<Woken, DeviceFault>>;

    /// A microphone's descriptor: a pipe, readable once written.
    fn pcm() -> (OwnedFd, OwnedFd) {
        rustix::pipe::pipe().unwrap()
    }

    /// Wait as a device does: its descriptor and the others in one poll.
    fn device<'a>(
        pcm: &'a OwnedFd,
        calls: &'a Cell<u32>,
    ) -> impl FnOnce(&[BorrowedFd<'_>], Duration) -> Answer + 'a {
        move |others, timeout| {
            calls.set(calls.get() + 1);
            let mut fds = vec![PollFd::new(pcm, PollFlags::IN)];
            fds.extend(
                others
                    .iter()
                    .map(|fd| PollFd::from_borrowed_fd(*fd, PollFlags::IN)),
            );
            let timeout = rustix::time::Timespec {
                tv_sec: 0,
                tv_nsec: timeout.as_nanos() as _,
            };
            poll(&mut fds, Some(&timeout)).unwrap();
            Some(Ok(Woken {
                samples: !fds[0].revents().is_empty(),
                others: fds[1..].iter().any(|fd| !fd.revents().is_empty()),
            }))
        }
    }

    #[test]
    fn a_block_arriving_wakes_the_wait() {
        let (read, write) = pcm();
        let (socket, _peer) = UnixDatagram::pair().unwrap();
        rustix::io::write(&write, &[1]).unwrap();
        let calls = Cell::new(0);
        let started = Instant::now();
        let waited = wait_between_passes(&[socket.as_fd()], false, started, device(&read, &calls));
        assert_eq!(waited, Waited::Samples);
        assert_eq!(calls.get(), 1);
        assert!(
            started.elapsed() < Duration::from_millis(5),
            "{:?}",
            started.elapsed()
        );
    }

    /// The stop comes on the coordinator's control socket: it ends the wait
    /// at once, the microphone polled beside it.
    #[test]
    fn a_stop_on_the_control_socket_ends_the_wait_at_once() {
        let (read, _write) = pcm();
        let (socket, peer) = UnixDatagram::pair().unwrap();
        peer.send(b"stop").unwrap();
        let calls = Cell::new(0);
        let started = Instant::now();
        let waited = wait_between_passes(&[socket.as_fd()], false, started, device(&read, &calls));
        assert_eq!(waited, Waited::Sockets);
        assert_eq!(calls.get(), 1);
        assert!(
            started.elapsed() < Duration::from_millis(5),
            "{:?}",
            started.elapsed()
        );
    }

    /// A block the device already holds (read ahead of the wait, as its
    /// status said) is handled without waiting or polling at all.
    #[test]
    fn a_block_already_there_is_handled_without_waiting() {
        let (read, _write) = pcm();
        let (socket, _peer) = UnixDatagram::pair().unwrap();
        let calls = Cell::new(0);
        let waited = wait_between_passes(
            &[socket.as_fd()],
            true,
            Instant::now(),
            device(&read, &calls),
        );
        assert_eq!(waited, Waited::AtOnce);
        assert_eq!(calls.get(), 0);
    }

    /// Nothing comes: the wait ends at its bound from the pass's start.
    #[test]
    fn with_nothing_the_wait_ends_at_its_bound() {
        let (read, _write) = pcm();
        let (socket, _peer) = UnixDatagram::pair().unwrap();
        let calls = Cell::new(0);
        let started = Instant::now();
        let waited = wait_between_passes(&[socket.as_fd()], false, started, device(&read, &calls));
        assert_eq!(waited, Waited::Timeout);
        let took = started.elapsed();
        assert!(took >= WAIT_AT_MOST - Duration::from_millis(1), "{took:?}");
        assert!(took < WAIT_AT_MOST * 3, "{took:?}");
    }

    /// A device that cannot be waited on, whose wait failed, or that woke
    /// with neither before the bound is paced by the old tick, not spun on.
    #[test]
    fn a_device_that_cannot_wait_is_ticked() {
        let (socket, _peer) = UnixDatagram::pair().unwrap();
        for answer in [
            None,
            Some(Err(DeviceFault::new("poll", 4))),
            Some(Ok(Woken::default())),
        ] {
            let started = Instant::now();
            let waited = wait_between_passes(&[socket.as_fd()], false, started, |_, _| answer);
            assert_eq!(waited, Waited::Ticked);
            assert!(started.elapsed() >= TICK, "{:?}", started.elapsed());
        }
    }

    /// Only `on` switches it on; anything else is the ticking loop.
    #[test]
    fn the_switch_is_on_only_when_it_says_on() {
        assert!(wait_switched_on(Some("on")));
        for other in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!wait_switched_on(other), "{other:?}");
        }
    }
}
