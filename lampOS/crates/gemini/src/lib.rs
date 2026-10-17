//! Bounded Gemini Live transport for a separately supervised provider process.
//!
//! This crate does not admit speech, mint actuator permits, or decide addressees.
//! The caller maps each immutable [`Lineage`] to the original interaction owner.
//! Input transcription and provider VAD are session-scoped observations, never
//! authorization. Local playback cancellation must not wait for this transport.
#![forbid(unsafe_code)]

mod config;
mod recovery;
mod session;
#[doc(hidden)]
pub mod testing;
pub mod wire;

pub use config::{
    BarrierPolicy, BarrierWait, Credential, GOOGLE_ENDPOINT, MAX_CONTEXT_TOKENS,
    MAX_OPEN_ACTIVITY_REQUESTS, MAX_RETAINED_SAMPLES, MAX_SUCCESSOR_PATIENCE, MAX_THINKING_BUDGET,
    Resumption, ResumptionHandle, SessionConfig, SuccessorStart, ThinkingLevel, Timeouts,
    valid_language_code,
};
pub use recovery::{
    Attempt, Connect, Context, Dialer, Failure, FailureReason, LinkUpdate, LinkUpdateKind,
    MAX_COUNTED_ATTEMPTS, Notice, OutputNotice, RecoveryPolicy, Stage, StartRefusal, Supervisor,
};
use serde::{Deserialize, Serialize};
pub use session::{Connection, InputSender, ResumptionPoint, connect};
use std::{fmt, num::NonZeroU64, time::Duration};

pub const INPUT_RATE: u32 = 16_000;
pub const OUTPUT_RATE: u32 = 24_000;
pub const MAX_INPUT_SAMPLES: usize = 320;
/// The oldest audio a request may bring. A person who speaks over the lamp
/// is held while the lamp falls silent and listens for more: up to 1.6 s of
/// that person's speech precedes the moment their request is accepted.
pub const MAX_INPUT_AGE: Duration = Duration::from_millis(2_500);
pub const INPUT_QUEUE_CAPACITY: usize = 32;
/// The most an input queue may hold ([`SessionConfig::writes`]): 128 blocks of
/// audio, 2.56 s, about the audio's own [`MAX_INPUT_AGE`]. A block's age is
/// checked only when the session takes it, which it cannot while a write
/// waits, so speech behind a write that waits longer than that still fills
/// the queue and ends the connection as [`Error::Backpressure`]. Only the
/// look's picture under patient writes can wait so long (the reviewer on
/// lampOS#1397).
pub const MAX_INPUT_QUEUE: usize = 128;
pub const EVENT_QUEUE_CAPACITY: usize = 16;
/// Decoded output held for a consumer slower than the network, as real-time
/// playback always is: 300 s of 24 kHz PCM. Past this the socket is not read.
pub const OUTPUT_BUFFER_BYTES: usize = 300 * 2 * OUTPUT_RATE as usize;
pub const MAX_HANDLE_BYTES: usize = 8_192;
pub const MAX_OUTPUT_SAMPLES: usize = 48_000;
pub const MAX_WIRE_BYTES: usize = 262_144;
/// Largest JPEG accepted as visual context. Audio and images share one
/// connection in order, so an image delays the audio behind it by its own
/// transmission time; this bound keeps that delay small on an ordinary link.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024;
/// Largest JPEG a written turn may carry: the still of a look asked for at
/// a side of its own (`lamp-eyes`, `full_look::LARGE_MOST_BYTES`), for
/// writing on paper (#941). A look is a turn of its own, with no audio
/// behind it. 184 KiB is 245 KiB as base64, so the turn with its note
/// ([`wire::MAX_NOTE_BYTES`]) and JSON stays one message under
/// [`MAX_WIRE_BYTES`]. Pictures with speech keep [`MAX_IMAGE_BYTES`].
pub const MAX_LOOK_IMAGE_BYTES: usize = 184 * 1024;
/// Images one request may carry. A newer image replacing one not yet sent
/// does not count twice.
pub const MAX_IMAGES_PER_REQUEST: u8 = 2;
/// Oldest image accepted, from capture to offer and again to sending.
pub const MAX_IMAGE_AGE: Duration = Duration::from_secs(2);
pub const MAX_TEXT_BYTES: usize = 8_192;
/// Functions one session may be told of.
pub const MAX_TOOLS: usize = 32;
/// Calls the service may make in one message, and that one request may have
/// unanswered at once.
pub const MAX_TOOL_CALLS: usize = 8;
/// The answer to a call that arrives while nothing can own it: most often
/// while the person is still speaking ("Yes." … "please start it."). The
/// model is told why it was not done and that it may ask again. On
/// lamp-4ace on 2026-10-15 a bare `cancelled` read to it as the person
/// changing their mind: in three of five two-breath yeses it never asked
/// again and the task was not started. Sent under `not_done`, not `error`,
/// and saying that nothing went wrong: at 20:07 the same evening, a request
/// to hand work over, cut at its comma, was asked for again and prepared,
/// and the lamp still said "the work couldn't be prepared right now".
pub const NOT_DONE_STILL_SPEAKING: &str = "Not done yet, and nothing went wrong: the person was still speaking when this was asked. Say nothing about this call. Listen to the rest of what they say; if it still asks for this, call it again.";
/// The most a call's identifier may be.
pub const MAX_TOOL_ID_BYTES: usize = 128;
/// The most a function's declared parameters, a call's arguments or an
/// answer to a call may be, as JSON text.
pub const MAX_TOOL_JSON_BYTES: usize = 4_096;
/// The most a session's instruction may be: a character file, and what the
/// runtime adds to it for one session.
pub const MAX_INSTRUCTION_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(NonZeroU64);
impl SessionId {
    /// Must be fresh after every reconnect, supplied by the trusted supervisor.
    pub fn new(value: u64) -> Result<Self> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(Error::InvalidConfiguration)
    }
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(NonZeroU64);
impl RequestId {
    /// Strictly increasing within a session; old identifiers are never reused.
    pub fn new(value: u64) -> Result<Self> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(Error::InvalidConfiguration)
    }
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Lineage {
    pub session: SessionId,
    pub request: RequestId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Ready,
    Disconnected(Error),
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceActivity {
    Start,
    End,
    Unspecified,
}

/// A function the model may ask this side to run. The model names it and
/// gives arguments that fit `parameters`; what is run, and whether, is
/// decided by whoever receives [`Event::ToolCall`], never by the model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// A lower-case letter, then up to 47 of `a-z`, `0-9` and `_`.
    pub name: String,
    /// What it does and when to call it, for the model. At most 1,024 bytes.
    pub description: String,
    /// A JSON Schema object for the arguments. Absent: it takes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Nothing is to be said on account of this function. It is declared
    /// as one the service need not wait for, and its answer is sent as one
    /// to take note of without generating anything. A request whose calls
    /// were all of this kind ends when the service completes the turn it
    /// made them in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub silent: bool,
}
impl Tool {
    pub fn validate(&self) -> Result<()> {
        let mut name = self.name.bytes();
        let named = name.next().is_some_and(|first| first.is_ascii_lowercase())
            && self.name.len() <= 48
            && name.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        let described = !self.description.trim().is_empty()
            && self.description.len() <= 1_024
            && !self.description.contains('\0');
        let shaped = self.parameters.as_ref().is_none_or(|parameters| {
            parameters.is_object() && parameters.to_string().len() <= MAX_TOOL_JSON_BYTES
        });
        if named && described && shaped {
            Ok(())
        } else {
            Err(Error::InvalidConfiguration)
        }
    }
}

/// One call the service made. `id` is the service's own and is empty when it
/// sent none; `args` is whatever JSON it sent, an empty object when it sent
/// nothing, and is not checked against the declared parameters here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug)]
pub enum Event {
    InputStarted {
        lineage: Lineage,
        waiting_for_barrier: bool,
    },
    Audio {
        lineage: Lineage,
        sequence: u64,
        pcm: Vec<i16>,
    },
    OutputTranscript {
        lineage: Lineage,
        text: String,
        finished: bool,
    },
    ModelText {
        lineage: Lineage,
        text: String,
    },
    GenerationComplete {
        lineage: Lineage,
    },
    /// The service asked for a function to be run for this request. It
    /// generates nothing more for the request until the call is answered
    /// with [`InputSender::try_tool_response`], so whoever takes this event
    /// answers every call, promptly, also when it will not run it. The wait
    /// counts against the request's `stall` bound like any other silence.
    ToolCall {
        lineage: Lineage,
        call: ToolCall,
    },
    /// The service withdrew calls it had made: the person spoke over the
    /// answer they were part of. An answer offered for one of them
    /// afterwards is dropped.
    ToolCallsCancelled {
        session: SessionId,
        ids: Vec<String>,
    },
    Interrupted {
        lineage: Lineage,
    },
    /// `idle=false` is not an ownership-retirement barrier.
    TurnComplete {
        lineage: Lineage,
        idle: bool,
    },
    /// The request was committed and the service sent nothing for
    /// [`Timeouts::unanswered`]. It is closed here as one the service chose
    /// not to answer. The service never said so: output that still arrives
    /// for it is dropped as unowned, and holds the next commit until it ends.
    Unanswered {
        lineage: Lineage,
        waited: Duration,
    },
    /// The service said it is not generating because it is waiting for more
    /// input. `awaiting` is the committed request that has produced nothing,
    /// if there is one, and how long ago it was committed. An observation:
    /// nothing is decided on it.
    WaitingForInput {
        session: SessionId,
        awaiting: Option<(Lineage, Duration)>,
    },
    /// A server message this codec refused, just before the connection ends
    /// with `error`. `shape` is its field names, nested, and the value of a
    /// few schema enumerations; no audio, no text and no handle.
    Refused {
        session: SessionId,
        error: Error,
        shape: String,
    },
    /// The service's own token counts, as it sent them. Counts only.
    Usage {
        session: SessionId,
        usage: Usage,
    },
    /// Google gives input transcripts no reliable per-turn ordering.
    UncorrelatedInputTranscript {
        session: SessionId,
        text: String,
        finished: bool,
    },
    VoiceActivity {
        session: SessionId,
        kind: VoiceActivity,
        audio_offset: Option<Duration>,
    },
    /// Advance notice that the service will close this connection. Not a
    /// failure: an unfinished request continues until its idle barrier.
    GoAway {
        session: SessionId,
        time_left: Option<Duration>,
    },
    /// A server message that no current request can own was dropped. It never
    /// cancels, completes or supplies output for a request.
    Discarded {
        session: SessionId,
        reason: Discard,
    },
    /// [`BarrierPolicy::AssumeAfterQuiet`] treated an earlier response as over
    /// because the service, asked to interrupt it, stayed silent for
    /// `Timeouts::barrier` without confirming. The service never said so:
    /// ownership of `successor`'s answer rests on that assumption and is not
    /// qualified evidence.
    BarrierAssumed {
        session: SessionId,
        superseded: Option<RequestId>,
        successor: Option<RequestId>,
    },
    /// A barrier ended: the earlier response settled, or the wait for it ran
    /// out. Counts only; no audio and no text.
    Barrier {
        session: SessionId,
        outcome: BarrierOutcome,
        report: BarrierReport,
    },
    /// Input up to `through` was abandoned, not continued, while some of it
    /// was still kept here or already written but not committed. Only with
    /// [`SuccessorStart::AfterFirstOutput`]. Counts only.
    InputDiscarded {
        session: SessionId,
        /// Every request up to this one.
        through: RequestId,
        /// Kept and never handed to the socket: dropped, and never sent.
        dropped_samples: u64,
        /// Already handed to the socket in an activity that has no End yet.
        /// **Not discarded and not recallable:** it stays in that activity,
        /// and the next request that continues it is heard after it.
        sent_samples: u64,
    },
}

/// Token counts from one `usageMetadata` message. A count the service left
/// out, or sent in a form this codec does not read, is absent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    pub total: Option<u64>,
    pub prompt: Option<u64>,
    pub response: Option<u64>,
    pub thoughts: Option<u64>,
    pub cached: Option<u64>,
    /// The audio share of `prompt` and of `response`.
    pub prompt_audio: Option<u64>,
    pub response_audio: Option<u64>,
}

/// How a barrier ended. An assumption is reported as
/// [`Event::BarrierAssumed`], as before.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierOutcome {
    /// The earlier response's idle completion arrived.
    Settled,
    /// It did not within the bound, and the connection is about to fail
    /// with [`Error::BarrierTimeout`].
    TimedOut,
}

/// What stood behind a barrier while it stood: enough to tell a terminal
/// that never came from one that came and was dropped. Everything counted
/// here was received for the earlier response and withheld from the caller.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BarrierReport {
    /// The response asked to stop. Unknown when the barrier was raised by
    /// stray output with no response on record.
    pub superseded: Option<RequestId>,
    /// The request waiting behind it, if any.
    pub successor: Option<RequestId>,
    /// From the interruption to this report.
    pub waited: Duration,
    /// From the earlier response's last event, or the interruption, to this
    /// report.
    pub quiet: Duration,
    /// Whether the successor was still being spoken when the barrier ended.
    pub successor_input_open: bool,
    /// How long the successor's `activityEnd` had been withheld.
    pub end_withheld: Option<Duration>,
    pub audio_frames: u32,
    pub audio_samples: u64,
    pub text_frames: u32,
    pub generation_complete: u32,
    pub interrupted: u32,
    /// Completions that did not settle it: `turnComplete` without idle.
    pub completions_not_idle: u32,
    /// How long the next request's `activityStart` was kept back, when
    /// [`SuccessorStart::AfterFirstOutput`] kept it back at all.
    pub start_held: Option<Duration>,
    /// What ended that.
    pub start_released_by: Option<StartRelease>,
    /// Samples of the next request that were being kept when it ended.
    pub retained_samples: u64,
    /// Messages carrying the documented `waitingForInput` while the barrier
    /// stood. An observation: nothing is decided on it.
    pub waiting_for_input: u32,
    /// Session resumption updates while the barrier stood, by whether they
    /// said the session could be resumed at that point. The documentation
    /// says it cannot be while the model is generating. An observation.
    pub resumable_updates: u32,
    pub unresumable_updates: u32,
    /// Kept samples dropped unsent while the barrier stood, because the
    /// request they belonged to was abandoned and not continued.
    pub discarded_samples: u64,
}

/// What ended the keeping back of a next request's `activityStart`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartRelease {
    /// The earlier response produced its first output and was interrupted.
    FirstOutput,
    /// The earlier response completed idle; nothing was interrupted.
    Completion,
    /// Neither came within the patience. Written anyway: from there the
    /// default behavior, including its strict wait and its failure.
    Patience,
    /// More audio than may be kept. Written anyway, as for patience.
    Overflow,
    /// The earlier response had produced nothing for `Timeouts::unanswered`
    /// since its commit and was taken as not answered. Nothing was
    /// interrupted and no barrier is left.
    Unanswered,
}

/// Why an image was not accepted. None of these affects the connection or
/// the request's audio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRefusal {
    NotConnected,
    /// Not the request now speaking: unknown, superseded, cancelled, or its
    /// input has already ended.
    NotCurrent,
    TooLarge,
    /// Not a complete JPEG by its markers. Contents are not decoded.
    NotJpeg,
    TooOld,
    /// The request already carries [`MAX_IMAGES_PER_REQUEST`].
    LimitReached,
    /// Offered under a camera grant that has since been revoked.
    Revoked,
}

/// What a revocation found. See `InputSender::revoke_images`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRevocation {
    /// No image of a revoked grant was waiting or being written.
    Nothing,
    /// One was waiting and has been dropped unsent.
    DroppedUnsent,
    /// One was being written. The write is abandoned and with it the
    /// connection, because part of the image may already be on the wire.
    AbandonedInFlight,
}

/// An accepted image waits in one slot until the connection can take it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageOffer {
    Queued,
    /// It took the place of an image of the same request not yet sent.
    ReplacedUnsent,
}

/// The last image that reached the socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageReceipt {
    pub request: RequestId,
    /// The caller's own token for the frame, returned unchanged.
    pub frame: u64,
    pub bytes: u32,
    /// From the offer until the write completed.
    pub waited: Duration,
    /// The write alone: how long audio behind it was held up locally.
    pub written_in: Duration,
}

/// What became of the images offered on one connection. Counters only grow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageLedger {
    pub accepted: u64,
    pub sent: u64,
    /// Replaced by a newer image before being sent.
    pub replaced: u64,
    /// Accepted, then dropped unsent: its request ended, was cancelled or
    /// superseded, or the image grew too old while waiting.
    pub dropped: u64,
    /// Accepted, then withdrawn because its camera grant was revoked, before
    /// or during its write.
    pub revoked: u64,
    pub last_sent: Option<ImageReceipt>,
}

/// Why a server message was dropped instead of being attached to a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Discard {
    /// `interrupted` for a request this client never asked to interrupt. With
    /// provider activity detection disabled only an explicit activityStart can
    /// interrupt, so the event belongs to an earlier request.
    LateInterruption,
    /// Completion that arrived before the current request could have one.
    LateTerminal,
    /// Audio or text that arrived while the current request was still input.
    LateOutput,
    /// Output or lifecycle with no request in progress.
    UnownedOutput,
    /// A function call with no committed request to own it. It was answered
    /// on the wire as cancelled, so that the service is not left waiting,
    /// and was not reported as a call.
    UnownedToolCall,
    /// An answer to a call that is not waiting for one: it was withdrawn,
    /// already answered, or its request has ended.
    StaleToolResponse,
}

/// What a caller may do about a connection that ended with this error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recovery {
    /// A fresh connection may succeed now.
    Reconnect,
    /// The service refused for a reason that will not clear on its own soon
    /// (quota, rate or usage limit). Retrying immediately only adds load.
    Unavailable,
    /// Configuration, authentication, protocol or local fault.
    Fatal,
}

/// Which write to the service did not finish within its deadline
/// ([`Error::WriteTimeout`]), so a connection lost that way says what it
/// was writing (the first builder on lampOS#705, `write_timeout`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Write {
    /// The session's setup, its first message.
    Setup,
    /// A block of the person's audio.
    Audio,
    /// A camera frame sent with speech.
    Image,
    /// A written turn with its picture: the lamp's look.
    Picture,
    /// A written turn without one.
    Written,
    /// An activity's start or end.
    Activity,
    /// The note that goes beside a request.
    Note,
    /// Context, between requests.
    Context,
    /// A tool's response.
    Tool,
    /// A ping or a pong.
    Keepalive,
}

/// Fixed diagnostic categories. Never retain raw HTTP/WebSocket/server errors,
/// close reasons, URLs, headers, response bodies, or authentication values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidConfiguration,
    ConnectTimeout,
    SetupTimeout,
    ReadTimeout,
    /// A write did not finish within its deadline: the transport's buffer
    /// stayed full, so the link is behind. `kib` is the message's size,
    /// rounded up, and `deadline_ms` the time it had.
    WriteTimeout {
        write: Write,
        kib: u32,
        deadline_ms: u32,
    },
    BarrierTimeout,
    /// No output arrived for an ended input within `Timeouts::response`.
    ResponseTimeout,
    /// Output began, then stopped before generation completed.
    StalledResponse,
    /// Generation completed but the idle barrier never followed.
    CompletionTimeout,
    Authentication,
    RateLimited,
    QuotaExceeded,
    ServerUnavailable,
    ServerRejected,
    ServerGoAway,
    Transport,
    PeerClosed {
        code: Option<u16>,
    },
    MalformedMessage,
    UnsupportedMessage,
    MessageTooLarge,
    InvalidAudio,
    Backpressure,
    /// One request more than [`MAX_OPEN_ACTIVITY_REQUESTS`] offered audio to
    /// an activity that none of them ended and no abandonment had reported.
    /// That audio could not be accounted for and was not sent. Terminal.
    OpenActivityOverrun,
    StaleInput,
    InputSequence,
    StaleRequest,
    OverlappingInput,
    UnexpectedResponse,
    /// Input that was being sent was withdrawn locally and its connection
    /// abandoned: an image whose camera grant was revoked mid-write.
    InputCancelled,
    /// Input was offered while no provider connection was established.
    NotConnected,
    Closed,
}
impl Error {
    pub fn recovery(self) -> Recovery {
        match self {
            Self::ConnectTimeout
            | Self::SetupTimeout
            | Self::ReadTimeout
            | Self::WriteTimeout { .. }
            | Self::BarrierTimeout
            | Self::ResponseTimeout
            | Self::StalledResponse
            | Self::CompletionTimeout
            | Self::ServerUnavailable
            | Self::ServerGoAway
            | Self::Transport
            // A consumer that took nothing for the delivery bound, or input
            // that overran its queue. The connection is abandoned with what
            // it still held, the request in flight is reported lost once,
            // and a new connection serves the next request. The connection
            // itself was healthy; reconnecting is how its stuck work is shed.
            | Self::Backpressure
            // Input already partly on the wire was withdrawn locally, which
            // can only be done by abandoning the connection that carries it.
            | Self::InputCancelled => Recovery::Reconnect,
            // Application close codes observed on the deployment relay for a
            // missing key, an unconfigured route and an exhausted usage limit.
            Self::PeerClosed {
                code: Some(4001 | 4002 | 4029),
            } => Recovery::Unavailable,
            Self::PeerClosed { .. } => Recovery::Reconnect,
            Self::RateLimited | Self::QuotaExceeded => Recovery::Unavailable,
            Self::InvalidConfiguration
            | Self::Authentication
            | Self::ServerRejected
            | Self::MalformedMessage
            | Self::UnsupportedMessage
            | Self::MessageTooLarge
            | Self::InvalidAudio
            // This side's own bound on an open activity, exhausted. Not a
            // fault of the connection, and no evidence that a new one, or
            // anything sent again on one, would be right for what that
            // activity held: it is not retried.
            | Self::OpenActivityOverrun
            | Self::StaleInput
            | Self::InputSequence
            | Self::StaleRequest
            | Self::OverlappingInput
            | Self::UnexpectedResponse
            | Self::NotConnected
            | Self::Closed => Recovery::Fatal,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // All variants contain only fixed labels and numbers: an optional
        // close code, and a write's kind, size and bound.
        write!(f, "Gemini Live {self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
