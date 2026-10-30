//! Explicit, size-bounded local process envelopes. Controller and worker boots
//! are checked by the receiver, never learned from unsolicited messages.
use crate::{
    diagnostic_control::{DiagnosticFinished, DiagnosticStarted},
    options::NoiseSuppression,
};
use lamp_audio::ownership::{PlaybackCursor, QueuedRange};
use lamp_ipc::MAX_DATAGRAM_BYTES;
use lamp_turn_owner::{BootId, Error as InteractionError, OutputPermit, Snapshot, TurnOwner};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope<T> {
    pub boot: BootId,
    pub sequence: u64,
    pub sent_at_us: u64,
    pub payload: T,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    Authority {
        snapshot: Snapshot,
    },
    ConnectReference {
        peer: BootId,
    },
    StartCapture,
    /// Boot-scoped provider packet capacity, independent of interaction authority.
    ProviderOutputCapacity {
        through: u64,
    },
    Stop,
}

impl Control {
    /// An authority renewal: a stale one with the next renewal behind it is
    /// passed over (`Channel::receive_renewed`, #1569 row 42).
    pub fn is_renewal(&self) -> bool {
        matches!(self, Self::Authority { .. })
    }
}

impl<'de> Deserialize<'de> for Control {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Serde's internally tagged unit variants otherwise accept extra keys.
        // Empty struct variants enforce the same strict shape as payload variants.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum StrictControl {
            Authority { snapshot: Snapshot },
            ConnectReference { peer: BootId },
            StartCapture {},
            ProviderOutputCapacity { through: u64 },
            Stop {},
        }
        Ok(match StrictControl::deserialize(deserializer)? {
            StrictControl::Authority { snapshot } => Self::Authority { snapshot },
            StrictControl::ConnectReference { peer } => Self::ConnectReference { peer },
            StrictControl::StartCapture {} => Self::StartCapture,
            StrictControl::ProviderOutputCapacity { through } => {
                Self::ProviderOutputCapacity { through }
            }
            StrictControl::Stop {} => Self::Stop,
        })
    }
}

/// A device that failed to open at start, and opened while it was waited
/// for: off USB (`gone`) or still held by another (busy), for how long since
/// the first failed open, and on which try it opened. The microphone and the
/// speaker (#4332) both report their open wait with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenWaited {
    pub gone: bool,
    pub waited_ms: u64,
    pub opened_on_try: u32,
}

impl OpenWaited {
    /// For a person: the journal, the session's events and the lamp's health.
    pub fn plain(&self) -> String {
        let seconds = self.waited_ms as f64 / 1000.0;
        let what = if self.gone { "was off USB" } else { "was busy" };
        format!(
            "the voice mic {what} for {seconds:.1} s at start; opened on try {}",
            self.opened_on_try
        )
    }
}

/// Physical source evidence already accepted by the Controls switch fence.
/// This is metadata, never a microphone permission or software-mute report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrivacyReceipt {
    pub controls_session: String,
    pub physical: lamp_msgs::PrivacySwitch,
}

impl PrivacyReceipt {
    pub fn check(&self) -> io::Result<()> {
        if self.controls_session.len() != 16
            || !self
                .controls_session
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.physical.check().is_err()
            || self.physical.muted.is_none()
            || self.physical.fault.is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid physical privacy receipt",
            ));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for PrivacyReceipt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            controls_session: String,
            physical: lamp_msgs::PrivacySwitch,
        }
        let fields = Fields::deserialize(deserializer)?;
        let value = Self {
            controls_session: fields.controls_session,
            physical: fields.physical,
        };
        value.check().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerEvent {
    Ready,
    /// Before `Ready`, from the speaker only: its device answered busy at
    /// start and is tried again for up to `open_at_start::SPEAKER_BUSY_AT_START`, so
    /// the coordinator waits for it that much longer (#4332).
    BusyAtStart,
    /// Cloud submission state; never grants microphone or playback authority.
    /// This travels independently of credited provider answer packets.
    ProviderLink {
        link: crate::provider_link::ProviderLink,
    },
    VisualContext {
        receipt: crate::visual_context::Receipt,
    },
    Ring {
        report: crate::ring_wire::RingFeedback,
    },
    DiagnosticsStarted {
        started: DiagnosticStarted,
    },
    DiagnosticsFinished {
        receipt: DiagnosticFinished,
    },
    Privacy {
        acquired_at_us: u64,
        muted: bool,
        /// Legacy direct GPIO has no Controls source identity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receipt: Option<PrivacyReceipt>,
    },
    /// The selected worker's acquisition expired. This is not a new physical
    /// reading; the coordinator closes its gate and may ride the bounded gap.
    PrivacyUnavailable {
        since_us: u64,
    },
    CapturePrepared {
        epoch: u64,
        privacy_generation: u64,
        prepared_at_us: u64,
        /// The microphone failed to open at first and was waited for (#1178).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        waited: Option<OpenWaited>,
    },
    CaptureStarted {
        epoch: u64,
        dsp_epoch: u64,
        noise_suppression: NoiseSuppression,
        privacy_generation: u64,
        rate: u32,
        channels: u32,
        period_frames: i64,
        buffer_frames: i64,
        monotonic_status_timestamps: bool,
        open_started_at_us: u64,
        prepared_at_us: u64,
        capture_started_at_us: u64,
        start_completed_at_us: u64,
    },
    CaptureStopped,
    /// A stall closed the microphone (its authority lapsed, or its buffer
    /// overran while the worker was held up). It stays closed until the next
    /// fresh authority opens it again under a new epoch, or the worker gives
    /// up. Not a fault: a gap.
    CaptureGap {
        epoch: u64,
        reason: String,
        closed_at_us: u64,
    },
    /// The capture worker's echo reference changed trust. Not a fault:
    /// capture goes on, and each frame says whether it was cancelled.
    ReferenceStatus {
        status: ReferenceStatus,
    },
    /// A planned speaker flush resets echo reference, not the user's active turn.
    CaptureDspReset {
        epoch: u64,
        dsp_epoch: u64,
        observed_at_us: u64,
    },
    Playback {
        owner: TurnOwner,
        first_sample: PlaybackCursor,
        end_sample: PlaybackCursor,
        queue_observed_at_us: u64,
        turn: u64,
        sequence: u64,
        queued_frames: usize,
        accepted_frames: usize,
        observed_at_us: u64,
    },
    /// Successful physical drop/reset, including an empty old speech queue.
    /// This never certifies playback completion or grants output authority.
    PlaybackClockReset {
        previous: QueuedRange,
        next_epoch: u64,
        observed_at_us: u64,
    },
    /// Actual last accepted speech cursor drained, including a nonfinal pause.
    /// Does not retire a playback token or complete a conversational reply.
    SpeechDrained {
        owner: TurnOwner,
        sequence: u64,
        last_sample: PlaybackCursor,
        observed_at_us: u64,
    },
    SpeechRetired {
        owner: TurnOwner,
        sequence: u64,
        final_sample: PlaybackCursor,
        observed_at_us: u64,
    },
    /// Accepted old-owner PCM retired after ordinary admitted supersession.
    /// This preserves cancellation; it is never a completed-answer receipt.
    CancelledTailRetired {
        owner: TurnOwner,
        superseded_by: TurnOwner,
        first_sample: PlaybackCursor,
        end_sample: PlaybackCursor,
        queued_frames: usize,
        speech_frames: usize,
        started_at_us: u64,
        observed_at_us: u64,
    },
    /// Real zero PCM accepted while a reply still owes speech. These receipts
    /// describe a delivery gap, never inferred room silence or completed speech.
    PlaybackGap {
        phase: PlaybackGapPhase,
        gap: PlaybackGap,
        observed_at_us: u64,
    },
    ReferenceClock {
        timing: ReferenceTiming,
    },
    ReferenceFault {
        fault: ReferenceFault,
    },
    PlaybackRejected {
        turn: u64,
        sequence: u64,
        reason: String,
        /// Refused because the speaker's authority lapsed while it was
        /// held up (#1178), or it underran mid-answer (#3697): the answer is
        /// cut, not the voice.
        lapsed: bool,
    },
    PlaybackDiscarded {
        owner: TurnOwner,
        reason: PlaybackDiscardReason,
        queued_frames: usize,
        other_owners: bool,
        observed_at_us: u64,
    },
    Fault {
        code: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackGapPhase {
    Started,
    Resumed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackGap {
    /// The affected reply; this is not permission to write either speech or zeros.
    pub owner: TurnOwner,
    pub first_sample: PlaybackCursor,
    pub end_sample: PlaybackCursor,
    pub started_at_us: u64,
    pub last_zero_accepted_at_us: u64,
    pub zero_frames: u64,
}

impl PlaybackGap {
    /// Validate an accepted-zero receipt at its worker observation time.
    /// A start carries at most the first 240-frame render block; a terminal
    /// receipt carries all accepted zeros. Origin, listening policy and current
    /// reply ownership remain the caller's separate concerns.
    pub fn is_valid_receipt(&self, phase: PlaybackGapPhase, observed_at_us: u64) -> bool {
        self.first_sample.epoch != 0
            && self.first_sample.epoch == self.end_sample.epoch
            && self.zero_frames != 0
            && self.end_sample.frame.checked_sub(self.first_sample.frame) == Some(self.zero_frames)
            && self.started_at_us != 0
            && self.last_zero_accepted_at_us >= self.started_at_us
            && observed_at_us >= self.last_zero_accepted_at_us
            && (phase != PlaybackGapPhase::Started || self.zero_frames <= 240)
    }
}

/// Stable typed reasons for every failure reachable through BoundaryGuard::check.
/// Other authority failures still fail closed; their text is never interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackDiscardReason {
    StaleOwner,
    ExpiredPermit,
    ExpiredState,
    PrivacyClosed,
    InputUnavailable,
    StaleCameraGrant,
    InputStillActive,
    StalePresentation,
    StateTooOld,
    FuturePermit,
    FutureState,
    WrongBoot,
    WrongOutputKind,
    NoAuthority,
    ClockRegression,
    Faulted,
    OtherAuthorityFailure,
    /// The speaker underran mid-answer (#3697): what it had queued is gone,
    /// as after a lapse (#1178). The answer is cut, never the voice.
    Underran,
}

impl From<InteractionError> for PlaybackDiscardReason {
    fn from(error: InteractionError) -> Self {
        match error {
            InteractionError::StaleOwner => Self::StaleOwner,
            InteractionError::ExpiredPermit => Self::ExpiredPermit,
            InteractionError::ExpiredState => Self::ExpiredState,
            InteractionError::PrivacyClosed => Self::PrivacyClosed,
            InteractionError::InputUnavailable => Self::InputUnavailable,
            InteractionError::StaleCameraGrant => Self::StaleCameraGrant,
            InteractionError::InputStillActive => Self::InputStillActive,
            InteractionError::StalePresentation => Self::StalePresentation,
            InteractionError::StateTooOld => Self::StateTooOld,
            InteractionError::FuturePermit => Self::FuturePermit,
            InteractionError::FutureState => Self::FutureState,
            InteractionError::WrongBoot => Self::WrongBoot,
            InteractionError::WrongOutputKind => Self::WrongOutputKind,
            InteractionError::NoAuthority => Self::NoAuthority,
            InteractionError::ClockRegression => Self::ClockRegression,
            InteractionError::Faulted => Self::Faulted,
            _ => Self::OtherAuthorityFailure,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrophoneFrame {
    pub epoch: u64,
    /// Changes on AEC reset; planned playback cancellation preserves frame sequence.
    pub dsp_epoch: u64,
    pub privacy_generation: u64,
    pub frame_sequence: u64,
    /// Host read completion, not a measured acoustic microphone timestamp.
    pub read_completed_at_us: u64,
    pub timing: CaptureTiming,
    pub probability: f32,
    /// What the accepted render explains of this block, before and after
    /// cancellation. Absent from a worker that does not report it, and
    /// while the reference is not trusted.
    #[serde(default)]
    pub echo: Option<lamp_audio::evidence::EchoEvidence>,
    /// How far the echo reference for this block was known. Anything but
    /// `trusted` and `warming` means `samples` did not pass the canceller.
    #[serde(default)]
    pub reference: ReferenceTrust,
    pub samples: Vec<i16>,
    /// The same block as the microphone recorded it, before the canceller,
    /// only from a capture worker asked for it (`--raw-blocks`): for the
    /// stop words' speech-to-text, which the canceller leaves nothing of a
    /// word said over the lamp (#1047, `LAMP_STOP_WORDS_SOURCE=raw`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Vec<i16>>,
    /// The canceller's linear output for this block, before its
    /// residual-echo suppressor, only from a capture worker asked for it
    /// (`--linear-blocks`) and only when the canceller ran for the block:
    /// for the stop words' speech-to-text, which the raw block gives mostly
    /// the lamp's own voice (#828, `LAMP_STOP_WORDS_SOURCE=linear`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear: Option<Vec<i16>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerChunk {
    pub snapshot: Snapshot,
    pub permit: OutputPermit,
    pub chunk_sequence: u64,
    /// The provider has completed; this is its final actual PCM block, including
    /// any trailing silence deliberately written to complete a 10 ms block.
    pub end_of_speech: bool,
    pub samples: Vec<i16>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderReference {
    pub playback_epoch: u64,
    pub privacy_generation: u64,
    pub sequence: u64,
    pub accepted_at_us: u64,
    pub queue_observed_at_us: u64,
    pub first_sample: u64,
    pub queued_frames: usize,
    /// Only samples actually accepted by the speaker; may be a partial block.
    pub payload: RenderPayload,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderPayload {
    Silence { frames: u16 },
    Speech { owner: TurnOwner, samples: Vec<i16> },
}

impl RenderPayload {
    pub fn frames(&self) -> usize {
        match self {
            Self::Silence { frames } => usize::from(*frames),
            Self::Speech { samples, .. } => samples.len(),
        }
    }
}

/// Host/driver diagnostics. No field is a measured acoustic acquisition time.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTiming {
    pub first_read_started_at_us: u64,
    pub last_read_started_at_us: u64,
    pub successful_reads: u16,
    pub alsa_status_monotonic_us: Option<u64>,
    pub status_observed_at_us: u64,
    pub available_frames: i64,
    pub delayed_frames: i64,
    pub processing_completed_at_us: u64,
    /// ALSA queue-derived estimate, not an acoustic delay measurement.
    pub aec_queue_delay_ms: u16,
    pub reference: ReferenceTiming,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockPrimed {
    pub playback_epoch: u64,
    pub privacy_generation: u64,
    pub accepted_zero_frames: usize,
    pub accepted_at_us: u64,
    pub queue_observed_at_us: u64,
    pub queued_frames: usize,
    pub discarded: Option<QueuedRange>,
}

/// Where capture has prepared the speaker's certified prime. Buffered samples
/// have not yet reached the DSP and must not advance its analysis cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimeDisposition {
    Analysed,
    BufferedForCapture,
}

/// Capture's acknowledgement of one validated, actually accepted speaker prime.
/// This is preparation evidence, not microphone readiness or acoustic training.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedPrime {
    pub playback_epoch: u64,
    pub privacy_generation: u64,
    pub prepared_through: u64,
    pub analysed_through: u64,
    pub disposition: PrimeDisposition,
}

/// Private speaker/capture priority channel; no provider or parent PCM relay.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceControl {
    CapturePrepared {
        privacy_generation: u64,
    },
    ClockPrimed {
        prime: ClockPrimed,
    },
    ReferencePrimed {
        prepared: PreparedPrime,
    },
    ClockStarted {
        playback_epoch: u64,
        start_requested_at_us: u64,
        started_at_us: u64,
    },
    Stopped {
        playback_epoch: u64,
        observed_at_us: u64,
    },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceTiming {
    pub playback_epoch: u64,
    pub accepted_through: u64,
    pub analysed_through: u64,
    /// Software DSP call accounting only; capture/render use separate USB clocks.
    pub capture_blocks_processed: u64,
    /// Retained host reads strictly before the speaker start-call request;
    /// fixed on ClockStarted. Total processed calls are never rebased.
    pub capture_blocks_before_clock_start: u64,
    pub accepted_at_us: u64,
    pub analysed_at_us: u64,
    pub queue_observed_at_us: u64,
    pub queued_frames: usize,
    /// Host timestamp before the start_clock method call; conservative lower
    /// bound, not the physical DAC start. Reads during the call get no exemption.
    pub clock_start_requested_at_us: Option<u64>,
    /// Host timestamp after pcm.start() succeeds, not physical DAC onset.
    pub clock_started_at_us: Option<u64>,
    pub ignored_old_epoch_packets: u64,
    /// Present only while the reference follows the capture clock, so the
    /// fixed mode's report is what it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift: Option<ReferenceDrift>,
    /// Present only with bounded reference recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryCounts>,
}

/// Whether the canceller has the reference for the capture it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceTrust {
    /// The reference for each capture block is known by count.
    #[default]
    Trusted,
    /// It is not. The canceller is not run and the samples are the
    /// microphone's own.
    Untrusted,
    /// Known again after a reseed, with a canceller that has not yet heard
    /// enough of the lamp to be believed. Cancelled, without evidence.
    Warming,
}

/// Why the reference stopped being trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceCause {
    /// A capture block had no reference waiting for it.
    Absent,
    /// By the speaker's own timestamps it accepted nothing for longer than
    /// its queue can cover: what it played and what was counted have parted.
    SenderPaused,
    /// A packet did not arrive.
    SequenceGap,
    /// The next packet does not start where the last one ended.
    CursorGap,
    /// More arrived at once than may wait.
    Overflow,
    /// The microphone had more waiting than a capture block may be late.
    CaptureBacklog,
    /// The speaker restarted its clock.
    ClockRestart,
    /// Fixed margin only: both sides were alive and on time, and the count
    /// between them reached its bound. A difference between the two clocks
    /// does that in minutes.
    MarginExhausted,
}

/// One change of trust, as it happened. Counts are since the worker started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceStatus {
    pub trust: ReferenceTrust,
    /// Why trust was lost, on the change that lost it and on the reseed
    /// that followed.
    pub cause: Option<ReferenceCause>,
    pub at_us: u64,
    pub playback_epoch: u64,
    pub capture_blocks_processed: u64,
    pub expected_sequence: u64,
    pub received_sequence: Option<u64>,
    pub accepted_through: u64,
    pub analysed_through: u64,
    /// On a reseed: how long the reference had been untrusted.
    pub untrusted_us: Option<u64>,
    pub counts: RecoveryCounts,
}

/// What recovery has cost so far. Nothing here is renumbered or forgotten.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCounts {
    pub trust: ReferenceTrust,
    /// Times trust was lost.
    pub degradations: u32,
    /// Times the canceller was reset onto a fresh run of reference.
    pub reseeds: u32,
    /// Capture blocks that did not pass the canceller.
    pub bypassed_capture_blocks: u64,
    /// Accepted reference frames that were never given to the canceller.
    pub skipped_frames: u64,
    /// Packets that arrived older than 40 ms and were used, being complete.
    pub late_packets: u64,
    /// Packets at or before one already seen, which were not used.
    pub ignored_packets: u64,
}

/// How the capture-clocked reference stands. The state of this software,
/// not a measurement of either device's clock.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceDrift {
    /// Read rate in force, parts per million above one frame per frame.
    pub rate_ppm: f32,
    /// The settled part of it: how much faster the speaker accepts frames
    /// than the microphone delivers them, as far as it has been learned.
    pub learned_ppm: f32,
    /// Accepted frames waiting to be read.
    pub unread_frames: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceFaultKind {
    InvalidPrime,
    InvalidStart,
    InvalidReference,
    SequenceGap,
    CursorGap,
    PrivacyMismatch,
    ReferenceLate,
    ReferenceBacklog,
    PartialExpired,
    QueueTiming,
    CaptureTiming,
    ClockStopped,
    BarrierTimeout,
    SpeechStarved,
    /// More reseeds than the recovery budget allows in its window.
    RecoveryExhausted,
    /// The reference stayed untrusted for longer than recovery may take.
    RecoveryTimedOut,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceFault {
    pub reason: ReferenceFaultKind,
    pub at_us: u64,
    pub expected_sequence: u64,
    pub received_sequence: Option<u64>,
    pub received_first_sample: Option<u64>,
    pub timing: ReferenceTiming,
}

impl std::fmt::Display for ReferenceFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "reference {:?}: epoch={} accepted={} analysed={} capture_blocks={} before_clock_start={} at_us={}",
            self.reason,
            self.timing.playback_epoch,
            self.timing.accepted_through,
            self.timing.analysed_through,
            self.timing.capture_blocks_processed,
            self.timing.capture_blocks_before_clock_start,
            self.at_us
        )
    }
}
impl std::error::Error for ReferenceFault {}

pub fn encode<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    if data.len() > MAX_DATAGRAM_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runtime message exceeds IPC bound",
        ));
    }
    Ok(data)
}

pub fn decode<T: DeserializeOwned>(data: &[u8]) -> io::Result<T> {
    if data.len() > MAX_DATAGRAM_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "runtime message exceeds IPC bound",
        ));
    }
    serde_json::from_slice(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed runtime message"))
}

#[derive(Debug)]
pub struct ReceiveOrder {
    boot: BootId,
    sequence: u64,
    sent_at_us: u64,
}

impl ReceiveOrder {
    /// The next message in order from the same boot, after the first.
    pub fn is_next<T>(&self, packet: &Envelope<T>) -> bool {
        self.sequence > 0
            && packet.boot == self.boot
            && packet.sequence == self.sequence + 1
            && packet.sent_at_us >= self.sent_at_us
    }

    pub fn new(boot: BootId) -> Self {
        Self {
            boot,
            sequence: 0,
            sent_at_us: 0,
        }
    }

    /// The next message in order from the same boot, the first included.
    pub fn is_in_order<T>(&self, packet: &Envelope<T>) -> bool {
        packet.boot == self.boot
            && self.sequence.checked_add(1) == Some(packet.sequence)
            && packet.sent_at_us >= self.sent_at_us
    }

    pub fn accept<T>(
        &mut self,
        packet: &Envelope<T>,
        now_us: u64,
        max_age_us: u64,
    ) -> io::Result<()> {
        if packet.boot != self.boot
            || packet.sequence <= self.sequence
            || packet.sent_at_us < self.sent_at_us
            || now_us
                .checked_sub(packet.sent_at_us)
                .is_none_or(|age| age >= max_age_us)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                // Which test failed, for the journal: a message read before
                // the clock was (sent "in the future") once looked stale.
                format!(
                    "stale, reordered or wrong-boot runtime message (sequence {} after {}, sent {} us before now, bound {} us, same boot {})",
                    packet.sequence,
                    self.sequence,
                    i128::from(now_us) - i128::from(packet.sent_at_us),
                    max_age_us,
                    packet.boot == self.boot,
                ),
            ));
        }
        self.sequence = packet.sequence;
        self.sent_at_us = packet.sent_at_us;
        Ok(())
    }
}

#[cfg(test)]
mod playback_gap_receipt_tests {
    use super::*;

    fn receipt(frames: u64) -> PlaybackGap {
        PlaybackGap {
            owner: serde_json::from_value(serde_json::json!({
                "boot": vec![1; 16], "turn": 1, "generation": 1
            }))
            .unwrap(),
            first_sample: PlaybackCursor { epoch: 1, frame: 0 },
            end_sample: PlaybackCursor {
                epoch: 1,
                frame: frames,
            },
            started_at_us: 100,
            last_zero_accepted_at_us: 100,
            zero_frames: frames,
        }
    }

    #[test]
    fn the_start_is_at_most_one_block_but_terminal_receipts_can_grow() {
        for phase in [
            PlaybackGapPhase::Started,
            PlaybackGapPhase::Resumed,
            PlaybackGapPhase::Cancelled,
        ] {
            for frames in [0, 1, 239, 240, 241, 7200, u64::MAX] {
                assert_eq!(
                    receipt(frames).is_valid_receipt(phase, 100),
                    frames != 0 && (phase != PlaybackGapPhase::Started || frames <= 240),
                    "phase {phase:?}, frames {frames}"
                );
            }
        }
    }

    #[test]
    fn epoch_and_cursor_length_must_identify_the_accepted_frames() {
        let valid = receipt(240);
        assert!(valid.is_valid_receipt(PlaybackGapPhase::Started, 100));
        for case in 0..5 {
            let mut gap = valid;
            match case {
                0 => {
                    gap.first_sample.epoch = 0;
                    gap.end_sample.epoch = 0;
                }
                1 => gap.end_sample.epoch = 2,
                2 => gap.end_sample.frame = 239,
                3 => gap.end_sample.frame = 241,
                _ => {
                    gap.first_sample.frame = u64::MAX;
                    gap.end_sample.frame = 239;
                }
            }
            for phase in [
                PlaybackGapPhase::Started,
                PlaybackGapPhase::Resumed,
                PlaybackGapPhase::Cancelled,
            ] {
                assert!(
                    !gap.is_valid_receipt(phase, 100),
                    "case {case}, phase {phase:?}"
                );
            }
        }
        let mut at_cursor_end = valid;
        at_cursor_end.first_sample.frame = u64::MAX - 240;
        at_cursor_end.end_sample.frame = u64::MAX;
        assert!(at_cursor_end.is_valid_receipt(PlaybackGapPhase::Started, 100));
    }

    #[test]
    fn timestamps_allow_equality_but_not_zero_start_or_regression() {
        for phase in [
            PlaybackGapPhase::Started,
            PlaybackGapPhase::Resumed,
            PlaybackGapPhase::Cancelled,
        ] {
            let mut gap = receipt(240);
            assert!(gap.is_valid_receipt(phase, 100));
            assert!(!gap.is_valid_receipt(phase, 99));
            gap.started_at_us = 0;
            assert!(!gap.is_valid_receipt(phase, 100));
            gap.started_at_us = 101;
            assert!(!gap.is_valid_receipt(phase, 101));
            gap.last_zero_accepted_at_us = 101;
            assert!(gap.is_valid_receipt(phase, 101));
            assert!(gap.is_valid_receipt(phase, u64::MAX));
        }
    }
}

#[cfg(test)]
mod privacy_receipt_tests {
    use super::*;
    use lamp_msgs::{ClockStamp, PrivacySwitch};

    fn receipt() -> PrivacyReceipt {
        PrivacyReceipt {
            controls_session: "0123456789abcdef".into(),
            physical: PrivacySwitch {
                stamp: ClockStamp {
                    clock_id: "test-clock".into(),
                    at_us: 100,
                    sequence: 7,
                },
                reader_session_id: "reader".into(),
                muted: Some(true),
                fault: None,
            },
        }
    }

    #[test]
    fn legacy_privacy_event_remains_receipt_free() {
        let value: WorkerEvent =
            serde_json::from_str(r#"{"kind":"privacy","acquired_at_us":100,"muted":true}"#)
                .unwrap();
        assert!(matches!(value, WorkerEvent::Privacy { receipt: None, .. }));
        assert!(
            !serde_json::to_value(value)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("receipt")
        );
    }

    #[test]
    fn controls_receipt_retains_original_source_and_acquisition() {
        let original = receipt();
        let event = WorkerEvent::Privacy {
            acquired_at_us: 100,
            muted: true,
            receipt: Some(original.clone()),
        };
        let decoded: WorkerEvent =
            serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
        let WorkerEvent::Privacy {
            acquired_at_us,
            muted,
            receipt: Some(decoded),
        } = decoded
        else {
            panic!("missing source evidence")
        };
        assert_eq!(decoded, original);
        assert_eq!(decoded.physical.stamp.at_us, acquired_at_us);
        assert_eq!(decoded.physical.muted, Some(muted));
    }

    #[test]
    fn receipt_decode_rejects_unbounded_identity_and_unknown_physical_state() {
        let mut invalid = receipt();
        invalid.controls_session = "x".repeat(129);
        assert!(
            serde_json::from_value::<PrivacyReceipt>(serde_json::to_value(invalid).unwrap())
                .is_err()
        );
        let mut invalid = receipt();
        invalid.physical.reader_session_id = "x".repeat(129);
        assert!(
            serde_json::from_value::<PrivacyReceipt>(serde_json::to_value(invalid).unwrap())
                .is_err()
        );
        let mut invalid = receipt();
        invalid.physical.muted = None;
        invalid.physical.fault = Some(lamp_msgs::ControlFault::Missing);
        assert!(
            serde_json::from_value::<PrivacyReceipt>(serde_json::to_value(invalid).unwrap())
                .is_err()
        );
    }
}
