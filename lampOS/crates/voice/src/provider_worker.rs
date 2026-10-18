//! Cloud I/O stays in a separate process. Input is private, bounded and tied to
//! its capture privacy generation; response lineage never follows a new turn.
//!
//! Production Legacy mode ends on a lost unfinished request. Explicit Typed
//! and Coordinated modes retain its terminal outcome while recovering without
//! replay. Coordinated availability bypasses PCM credit, not ownership or the
//! ordered answer stream. See `docs/provider-link-channel.md`.
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use crate::{
    config::ProviderConfig,
    provider_flow::{OutputSender, PACKET_SAMPLES},
    provider_link::{LinkClock, LinkFailure, PendingLink, ProviderLink},
    transport::{Channel, WorkerChannels},
    visual_context::{ContextDelivery, Source as VisualSource, Turn as VisualTurn},
    wire::{Control, WorkerEvent},
};
use lamp_gemini::{
    BarrierWait, Connect, Context, Discard, Error, Event, Failure, FailureReason, Notice,
    RequestId, SessionConfig, SessionId, Stage, StartRefusal, State, SuccessorStart, Supervisor,
};
#[cfg(not(test))]
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Permission, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io,
    path::Path,
    str::FromStr,
    time::{Duration, Instant},
};

mod reminder_policy;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const MAX_QUEUED_OUTPUT: usize = 512;
/// Above this backlog no further provider event is taken. The transport then
/// stops reading its socket, so a fast or bursty service is held back by flow
/// control instead of ending the worker. One event adds at most 50 packets.
const OUTPUT_HIGH_WATER: usize = 384;
/// Initial setup is unchanged. A handle the service volunteers is kept in this
/// process's memory only, so a reconnect can restore the conversation.
// What the tests below set. In production the private configuration decides
// (`resumption` in the provider configuration; retained unless it says off).
#[cfg(test)]
const RESUMPTION: lamp_gemini::Resumption = lamp_gemini::Resumption::Retain;
/// Until live service probes qualify cancellation ordering, an old response
/// must settle before a successor is committed. Quiet alone cannot attribute
/// a late answer. The assumption policy remains explicit in transport tests.
#[cfg(test)]
use lamp_gemini::BarrierPolicy;
// In production the private configuration decides this too.
#[cfg(test)]
use lamp_gemini::RecoveryPolicy;
#[cfg(test)]
const BARRIER: BarrierPolicy = BarrierPolicy::Require;
/// Requests in a row the service left unanswered after which the session is
/// renewed at its next idle moment, and the least time between two such
/// renewals. A session can stop being of use without failing: on lamp-4ace
/// on 2026-10-13 one about thirteen minutes old, in a room talking another
/// language, began to hear everything said to it as Spanish, and with the
/// character file of that hour answered nothing at all for five minutes. A
/// new session heard the same question correctly. Most runs of eight
/// unanswered requests are simply a room not talking to the lamp, and for
/// those a renewal costs two seconds in which it does not hear and gives a
/// shorter history to pay for. It was at most once in three minutes until
/// that afternoon, when the lamp sat in a talking office with a character
/// that answers only when called: it renewed every three minutes, and a new
/// session is where the lamp is likeliest to answer what was not for it
/// (see `thread::GOING_ON_FOR`).
const RENEW_AFTER_UNANSWERED: u32 = 8;
const RENEW_AT_MOST_EVERY: Duration = Duration::from_secs(900);
worker_global! {
    /// Requests the service has left unanswered since it last answered one.
    fn unanswered_requests() -> std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
}
/// Where the eyes keep their newest frame, when the lamp may take a look.
static LOOK_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
/// A look is a full look (`--eyes-look-full`): its still may be older.
static LOOK_FULL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Where the conversation's thread is kept across a restart, if anywhere.
static THREAD_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
/// Whether a new session is told how things stand.
static SESSION_FACT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the session is told how busy the room is.
static ROOM_FACTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether a stuffy room is told beside what a person says.
static AIR_FACTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the model is told, while a person is still speaking, that the
/// lamp's name was heard (`crate::called`).
static NAME_NOTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether, in a busy room, the note waits for someone turned to the lamp
/// (`crate::called::BUSY_ENVIRONMENT`).
static NAME_NOTE_BUSY_FACE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Whether the rule for whom the lamp answers is in its busy form
/// (`crate::attention::BUSY_FLAG`).
static ATTENTION_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the name is held to the day's own bars
/// (`crate::attention::BY_DAY_FLAG`). Only with the switch.
static SPOKEN_TO_BY_DAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether a tap on the lamp's head counts as a call (`--head-touch`, #711).
static HEAD_TOUCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether a name heard close to the lamp counts as a call
/// (`crate::attention::NEAR_FLAG`).
static ATTENTION_NEAR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether requests are judged said to the lamp from close to it
/// (`crate::addressed`): only with the rule that follows them.
static ADDRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Files the provider worker is told of by its flags.
#[derive(Default)]
pub struct Files {
    /// The eyes' newest frame (`--eyes-look`).
    pub look: Option<std::path::PathBuf>,
    /// A look is a full look (`--eyes-look-full`, #836).
    pub look_full: bool,
    /// The conversation's thread (`--thread-file`).
    pub thread: Option<std::path::PathBuf>,
    /// Whether a new session is told how things stand (`--session-fact`).
    pub session_fact: bool,
    /// Whether the session is told how busy the room is (`--room-facts`).
    pub room_facts: bool,
    /// Whether a line said over the lamp's reply and left unanswered is
    /// kept out of the busy room's count (`--cut-ins-not-talk`).
    pub cut_ins_not_talk: bool,
    /// Whether a stuffy room is told beside what a person says
    /// (`--air-facts`, `crate::air_told`).
    pub air_facts: bool,
    /// Whether the link is never given up (`--keep-link`).
    pub keep_link: bool,
    /// Whether input waits for room in the link's queue (`--wait-for-room`).
    pub wait_for_room: bool,
    /// What the model may do other than speak (`--tools`, `--actions-dir`).
    pub tools: crate::tools::Settings,
    /// Whether the lamp's own rule for whom it answers is applied
    /// (`--attention`).
    pub attention: bool,
    /// Whether that rule is in its busy form (`--attention-busy`).
    pub attention_busy: bool,
    /// Whether a name heard close to the lamp counts as a call in a busy
    /// room (`--attention-near`).
    pub attention_near: bool,
    /// Whether "the lamp" said in passing is no call
    /// (`--name-in-passing-ignored`, #829).
    pub name_in_passing_ignored: bool,
    /// Whether an answer is played only when the lamp was spoken to
    /// (`--spoken-to`, #829).
    pub spoken_to: bool,
    /// Whether the name is held to the day's own bars as well
    /// (`--spoken-to-by-day`, `crate::attention::BY_DAY_FLAG`).
    pub spoken_to_by_day: bool,
    /// The same-voice check's model (`--same-voice-model`, #829): a line in
    /// another voice than the conversation's is no part of it. Only with
    /// `spoken_to`.
    pub same_voice: Option<std::path::PathBuf>,
    /// Whether each request is judged said to the lamp from close to it,
    /// published, and let out in a busy room (`--addressed`,
    /// `crate::addressed`).
    pub addressed: bool,
    /// Whether the model is asked to say when it hears the lamp's name
    /// (`--name-signal`).
    pub name_signal: bool,
    /// Whether the model is told beside what it hears that the lamp's
    /// name was heard (`--name-note`).
    pub name_note: bool,
    /// Whether, in a busy room, that waits for someone turned to the lamp
    /// (`--name-note-busy-face`).
    pub name_note_busy_face: bool,
    /// Whether a tap on the lamp's head counts as a call (`--head-touch`).
    pub head_touch: bool,
    /// The lamp's bus, if how the room is gets published on it
    /// (`--bus-root`).
    pub bus_root: Option<std::path::PathBuf>,
    /// Where whom it answered is recorded (`--first-output-log`,
    /// [`crate::first_output`]), if anywhere.
    pub first_output: Option<std::path::PathBuf>,
    /// Whether a write to the service may wait on a link that is behind
    /// (`--provider-writes-patient`, `LAMP_PROVIDER_WRITES`,
    /// [`crate::config::patient_writes`]).
    pub patient_writes: bool,
    /// Whether a short sound let through in a quiet room is held unless
    /// something shows it was said to the lamp (`--sounds-checked`,
    /// `crate::not_speech::CHECKED`). Only with `attention`.
    pub sounds_checked: bool,
    /// Whether, in a conversation, a turn heard in another script or as a
    /// number alone is asked again (`--ask-unclear`, #926). Only with
    /// `attention`.
    pub ask_unclear: bool,
    /// The language hint for the service's transcript of what it hears
    /// (`--transcription-language`, `LAMP_TRANSCRIPTION_LANGUAGE`, #926).
    pub transcription_language: Option<String>,
    /// Whether the lamp's actions wait for the Policy's attention decision
    /// (`--policy-holds-actions`, `crate::policy_holds`, #800). Only with the
    /// bus.
    pub policy_holds: bool,
    /// Whether the model is told, while a request is heard, that its words
    /// name one of the staff (`--staff-note`, `crate::staff_note`). Only
    /// with the tasks part.
    pub staff_note: bool,
    /// Whether the provider file's unset waits are filled in
    /// (`--provider-patience`, `LAMP_PROVIDER_PATIENCE`,
    /// [`ProviderConfig::with_patience`]).
    pub provider_patience: bool,
}

/// The bus's root for this worker, if it publishes.
static BUS_ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
/// The directory whom it answered is recorded in, if any.
static FIRST_OUTPUT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
/// The provider worker's flag for a link that is never given up. A service
/// that runs all day passes it; a bounded run keeps the bounded policy, so
/// that it ends when its provider cannot be reached.
pub const KEEP_LINK_FLAG: &str = "--keep-link";
/// The provider worker's flag to wait for room in Gemini's input queue
/// rather than overrun it (lampOS#892). Passed only with a second voice
/// route configured, whose held line is replayed faster than speech.
pub const WAIT_FOR_ROOM_FLAG: &str = "--wait-for-room";
/// Take a leading `--wait-for-room` off a worker's flags.
pub fn take_wait_for_room(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == WAIT_FOR_ROOM_FLAG => (true, rest),
        _ => (false, flags),
    }
}
/// Take a leading `--keep-link` off a worker's flags.
pub fn take_keep_link(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == KEEP_LINK_FLAG => (true, rest),
        _ => (false, flags),
    }
}
/// What the flags after a provider worker's configuration ask for, in the
/// order `coordinator::provider_arguments` passes them: the continuation,
/// the sight, the files and the camera context. A flag out of its place or
/// unknown is refused (by the camera context's parse, which takes the rest).
pub type Parsed = (
    Continuation,
    Option<crate::sight::Sight>,
    Files,
    Option<VisualSource>,
);
pub fn parse_flags(dir: &std::path::Path, flags: &[String]) -> io::Result<Parsed> {
    let (continuation, flags) = Continuation::take(flags)?;
    let (sight, flags) = crate::sight::Sight::take(flags)?;
    let (look, flags) = crate::sight::take_look(flags)?;
    let (look_full, flags) = crate::sight::take_look_full(flags);
    let (thread, flags) = crate::thread::take_file(flags)?;
    let (session_fact, flags) = crate::session_fact::take(flags);
    let (room_facts, flags) = crate::room_told::take(flags);
    let (cut_ins_not_talk, flags) = crate::room_told::take_cut_ins(flags);
    let (air_facts, flags) = crate::air_told::take(flags);
    let (keep_link, flags) = take_keep_link(flags);
    let (wait_for_room, flags) = take_wait_for_room(flags);
    let (mut tools, flags) = crate::tools::take(flags)?;
    let (attention, flags) = crate::attention::take(flags);
    let (attention_busy, flags) = crate::attention::take_busy(flags);
    let (attention_near, flags) = crate::attention::take_near(flags);
    let (name_in_passing_ignored, flags) = crate::attention::take_passing(flags);
    let (spoken_to, flags) = crate::attention::take_spoken_to(flags);
    let (spoken_to_by_day, flags) = crate::attention::take_by_day(flags);
    let (same_voice, flags) = crate::same_voice::take(flags)?;
    let (addressed, flags) = crate::addressed::take(flags);
    let (name_signal, flags) = crate::attention::take_name(flags);
    let (name_note, name_note_busy_face, flags) = crate::called::take(flags);
    let (head_touch, flags) = crate::attention::take_head_touch(flags);
    let (bus_root, flags) = crate::bus_out::take_root(flags)?;
    let (first_output, flags) = crate::first_output::take(flags)?;
    let (patient_writes, flags) = crate::config::take_writes(flags);
    let (sounds_checked, flags) = crate::not_speech::take_checked(flags);
    let (ask_unclear, flags) = crate::not_speech::take_unclear(flags);
    let (transcription_language, flags) = crate::not_speech::take_transcription(flags);
    let (policy_holds, flags) = crate::policy_holds::take(flags);
    let (staff_note, flags) = crate::staff_note::take(flags);
    let (provider_patience, flags) = crate::config::take_patience(flags);
    // `start_agent_task` says what the staff's note asks, when it is sent.
    if let Some(place) = tools.tasks.as_mut() {
        place.staff_note = staff_note;
    }
    let visual = VisualSource::parse(dir, flags)?;
    let files = Files {
        look,
        look_full,
        thread,
        session_fact,
        room_facts,
        cut_ins_not_talk,
        air_facts,
        keep_link,
        wait_for_room,
        tools,
        attention,
        attention_busy,
        attention_near,
        name_in_passing_ignored,
        spoken_to,
        spoken_to_by_day,
        same_voice,
        addressed,
        name_signal,
        name_note,
        name_note_busy_face,
        head_touch,
        bus_root,
        first_output,
        patient_writes,
        sounds_checked,
        ask_unclear,
        transcription_language,
        policy_holds,
        staff_note,
        provider_patience,
    };
    Ok((continuation, sight, files, visual))
}
/// The eyes' known face as the voice started, before its first session:
/// that session's person, and where the switch's count starts.
static KNOWN_AT_START: std::sync::OnceLock<(String, Instant)> = std::sync::OnceLock::new();
/// When the lamp last looked (a picture went into this session), and when
/// anything was last submitted. A picture in a session's history makes
/// every later answer about half a second slower ([eyes.md]), so once the
/// conversation has paused the session is renewed without it.
static LOOKED_AT_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SUBMITTED_AT_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const RENEW_AFTER_LOOK_QUIET_US: u64 = 20_000_000;

/// How long `Continuation::KeptStart` keeps a next request's start back. In
/// the one real-service run that shows it, an answer's first audio came
/// 1.43 s after its question ended. A proposed value, not a measured need.
pub const KEPT_START_PATIENCE: Duration = Duration::from_millis(2_500);

/// What the transport does when a person goes on speaking before the first
/// answer has begun. Chosen once, by an explicit option, for the worker's
/// lifetime; the policy stays `Require` either way.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Continuation {
    /// As it has always been: the next request's start is written at once
    /// and the barrier's quiet bound counts from that interruption.
    #[default]
    Off,
    /// The next request's start and opening audio are kept in the transport
    /// until the earlier answer begins, and is then interrupted, or
    /// completes, for at most [`KEPT_START_PATIENCE`]; the barrier's quiet
    /// bound counts only while a request's End is withheld; and how each
    /// barrier ended is written to the worker's standard error.
    KeptStart,
}
impl Continuation {
    pub const FLAG: &'static str = "--provider-continuation";
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::KeptStart => "kept-start",
        }
    }
    /// The provider worker's own arguments for this choice: none when off,
    /// so that a worker started without the option is started as before.
    pub fn arguments(self) -> Vec<String> {
        match self {
            Self::Off => Vec::new(),
            Self::KeptStart => vec![Self::FLAG.to_owned(), self.as_str().to_owned()],
        }
    }
    /// Take a leading `--provider-continuation VALUE` off a worker's flags.
    pub fn take(flags: &[String]) -> io::Result<(Self, &[String])> {
        match flags {
            [flag, value, rest @ ..] if flag == Self::FLAG => Ok((value.parse()?, rest)),
            [flag] if flag == Self::FLAG => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provider continuation needs a value",
            )),
            _ => Ok((Self::Off, flags)),
        }
    }
    pub(crate) fn apply(self, config: SessionConfig) -> io::Result<SessionConfig> {
        match self {
            Self::Off => Ok(config),
            Self::KeptStart => config
                .barrier_wait(BarrierWait::AtCommit)
                .barrier_reports(true)
                .successor_start(SuccessorStart::AfterFirstOutput {
                    patience: KEPT_START_PATIENCE,
                })
                .map_err(|error| io::Error::other(format!("provider continuation: {error}"))),
        }
    }
}
impl FromStr for Continuation {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "off" => Ok(Self::Off),
            "kept-start" => Ok(Self::KeptStart),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provider continuation must be off or kept-start",
            )),
        }
    }
}
const CONTROL_SLICE: usize = 16;
const INPUT_SLICE: usize = 8;
const RETAINED_INPUT_US: u64 = 100_000;
/// How long a command waits, first in line, for room in Gemini's input
/// queue: past the link's own write bound (200 ms), so a write that is only
/// slow is waited out and one that is stuck has failed the link first.
const ROOM_WAIT_US: u64 = 250_000;
/// The same past patient writes' 1 s (`LAMP_PROVIDER_WRITES=patient`, the
/// reviewer on #1397): with a second route, a replayed line and speech
/// that fill the queue while a write waits are waited out with it.
const PATIENT_ROOM_WAIT_US: u64 = 1_050_000;
/// The wait in force: [`PATIENT_ROOM_WAIT_US`] once the worker takes patient
/// writes, otherwise [`ROOM_WAIT_US`].
static ROOM_WAIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(ROOM_WAIT_US);
/// The room wait for a worker with patient writes or without.
fn room_wait_us(patient_writes: bool) -> u64 {
    if patient_writes {
        PATIENT_ROOM_WAIT_US
    } else {
        ROOM_WAIT_US
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderInput {
    Start {
        request: u64,
        /// The connection selected at admission, never replaced at dequeue.
        session: u64,
        privacy_generation: u64,
        /// The same utterance going on after the lamp ended it, soon after
        /// the end or after an end sent early (`EndedTurn::goes_on_in`,
        /// `continue_after_early_end` in the coordinator), said only with
        /// `LAMP_CUT_KEEPS_NAME=on` ([`crate::attention::cut_keeps_name`]):
        /// the busy-room rule keeps what it knew of the request before.
        /// Without the switch it is never sent, and the Start is as it was.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        continues: bool,
        /// The request goes on from one whose end was sent early and given
        /// up when the person spoke again (`--early-end-ms`): the model is
        /// told so beside it ([`CUT_OFF_NOTE`]). Left off the wire when false.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        continues_cut_off: bool,
        /// The line began over the lamp's reply (a reply was in hand when it
        /// was admitted): what was heard of it is the lamp's voice as much
        /// as the person's, and the same-voice check never judges it
        /// ([`crate::same_voice`], #829). Left off the wire when false.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        over_the_lamp: bool,
        /// The first request after a hand or the lamp's head stopped its
        /// speech (`hand_stop`, `head_stop`): the model is told the person
        /// stopped it ([`STOPPED_NOTE`], #829, the first builder at 16:33).
        /// Left off the wire when false.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        after_stop: bool,
        /// The line began soon after the lamp's own words finished playing,
        /// not over them (`LAMP_FOLLOWS_ON=on`): the model is told it may
        /// follow on from them ([`crate::follows_on::NOTE`]), unless the
        /// room is busy. Left off the wire when false.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        follows_on: bool,
        /// The line began while the lamp's own answer could be heard: its
        /// reply had sound or was playing, not only in hand. With
        /// `LAMP_CUT_INS_NOT_TALK=on`, left unanswered it isn't counted
        /// toward a busy room (`crate::room_told`, #828). Left off the wire
        /// when false.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        over_the_lamps_voice: bool,
    },
    Audio {
        request: u64,
        privacy_generation: u64,
        sequence: u64,
        read_completed_at_us: u64,
        samples: Vec<i16>,
    },
    End {
        request: u64,
        privacy_generation: u64,
        /// Judged a sound and sent only for the model to judge by its sound
        /// (`LAMP_SHORT_SOUNDS`, [`crate::not_speech::sent_anyway`]): left
        /// unanswered, it is not talk the lamp ignored, and the room
        /// ([`crate::room_told`]) and the run of unanswered requests do not
        /// count it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        sound: bool,
        /// The request took over from the lamp's own answer: a cut-in after
        /// a yield (#926). Without it the End is as it was.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        cut_in: bool,
    },
    /// The request in hand was only the lamp's name, and its call's wait
    /// ran out with nothing more said (`--call-wait-ms`, #782). Sent just
    /// before its End, only then: the model is told so beside the request
    /// ([`CALLED_NOTE`]) and answers as someone who hears their name does.
    Called {
        request: u64,
        privacy_generation: u64,
    },
    /// A whole request in writing, for a turn the lamp begins itself: it is
    /// a start and an end at once and carries no sound.
    Written {
        request: u64,
        session: u64,
        privacy_generation: u64,
        text: String,
        /// The lamp is taking a look: the eyes' newest frame goes with the
        /// note, if they are open and this worker was told where it is.
        #[serde(default)]
        look: bool,
    },
    /// A due occurrence whose durable identity is owned by the coordinator's
    /// consumer. The provider receives data, never permission to run tools.
    Reminder {
        request: u64,
        session: u64,
        privacy_generation: u64,
        text: String,
        late: bool,
        late_by_ms: Option<u64>,
    },
}
impl ProviderInput {
    fn request(&self) -> u64 {
        match self {
            Self::Start { request, .. }
            | Self::Audio { request, .. }
            | Self::End { request, .. }
            | Self::Called { request, .. }
            | Self::Written { request, .. }
            | Self::Reminder { request, .. } => *request,
        }
    }
    fn privacy_generation(&self) -> u64 {
        match self {
            Self::Start {
                privacy_generation, ..
            }
            | Self::Audio {
                privacy_generation, ..
            }
            | Self::End {
                privacy_generation, ..
            }
            | Self::Called {
                privacy_generation, ..
            }
            | Self::Written {
                privacy_generation, ..
            }
            | Self::Reminder {
                privacy_generation, ..
            } => *privacy_generation,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderOutput {
    Ready {
        setup_us: u64,
    },
    Started {
        request: u64,
        /// The connection that actually accepted Start, never current link state.
        session: u64,
        waiting_for_barrier: bool,
    },
    Audio {
        request: u64,
        sequence: u64,
        /// Wrapper event observation, not a socket-read or acoustic timestamp.
        provider_event_at_us: u64,
        source_frames: usize,
        source_offset: usize,
        samples: Vec<i16>,
    },
    Transcript {
        request: Option<u64>,
        text: String,
        finished: bool,
    },
    GenerationComplete {
        request: u64,
    },
    Interrupted {
        request: u64,
    },
    TurnComplete {
        request: u64,
        idle: bool,
    },
    /// What was heard for `request` made no sense in the conversation and
    /// its answer is held (#926): the coordinator asks again. Sent before
    /// the request's `TurnComplete`.
    Unclear {
        request: u64,
        why: String,
    },
    /// A skill's function the model called for `request` was answered with
    /// `"look": true` (a skill's look step, #941): the coordinator takes
    /// the full look for that turn itself, as if the lamp had said it
    /// would look, for `session`, the one that made the call. With
    /// `dropped`, the look is not taken, and why: the answer never reached
    /// the model, or the turn is one the lamp wrote itself.
    LookAsked {
        request: u64,
        session: u64,
        dropped: Option<String>,
    },
    TurnFailed {
        request: u64,
        stage: FailedStage,
        reason: Fault,
    },
    /// A local policy refusal, not completion or a provider failure. The due
    /// occurrence stays pending and ordinary input remains available.
    ReminderDeferred {
        request: u64,
        session: u64,
        reason: ReminderDeferral,
    },
    /// Input up to `request` was abandoned, not continued, while the
    /// transport still kept some of it or had written some of it without an
    /// End. Only under `Continuation::KeptStart`. `dropped_samples` were
    /// kept and never sent. `sent_samples` had already been handed to the
    /// connection: they are not discarded and cannot be recalled, and the
    /// next request is heard after them.
    InputDiscarded {
        request: u64,
        dropped_samples: u64,
        sent_samples: u64,
    },
    OwnershipAssumed {
        request: Option<u64>,
        superseded: Option<u64>,
    },
    /// The upstream output stream has ended, after its retained PCM and turn
    /// outcomes. This is not process EOF: a later local Start refusal can
    /// still produce TurnFailed(NotDelivered), until explicit Stop.
    Ended {
        reason: LinkFailure,
        fault: Fault,
        attempts: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderDeferral {
    Disabled,
    Invalid,
}

/// Compatibility encoding for explicit Typed mode. Turn failures and ownership
/// assumptions share the exact flat wire shape with [`ProviderOutput`].
/// Coordinated link availability instead uses the separate control channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderStatus {
    /// No provider connection. Input admitted from now until `Recovered`
    /// cannot be delivered and will be reported with `TurnFailed`. Output that
    /// arrived before the failure may still follow.
    Unavailable { reason: Fault },
    /// A connection is ready again. `context` says whether the conversation
    /// so far is still known to the provider.
    Recovered {
        outage_us: u64,
        attempts: u32,
        context: RecoveredContext,
        reason: Fault,
    },
    /// The provider will produce no further output for this accepted request.
    /// Its input was not replayed and its answer was not requested again. All
    /// output that did arrive for it precedes this event.
    TurnFailed {
        request: u64,
        stage: FailedStage,
        reason: Fault,
    },
    /// The provider never confirmed that `superseded` stopped. `request`
    /// proceeds on the assumption that it did, so the ownership of its answer
    /// is not qualified evidence.
    OwnershipAssumed {
        request: Option<u64>,
        superseded: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailedStage {
    /// Admitted while no connection existed; nothing of it reached the provider.
    NotDelivered,
    InputOpen,
    AwaitingResponse,
    /// Part of an answer was delivered before the connection was lost.
    Responding,
}
impl From<Stage> for FailedStage {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::InputOpen => Self::InputOpen,
            Stage::AwaitingResponse => Self::AwaitingResponse,
            // A generated answer is settled, never failed.
            Stage::Responding | Stage::Generated => Self::Responding,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveredContext {
    Resumed,
    /// Resumed from a point before the latest exchange.
    ResumedBeforeLatest,
    /// The provider no longer knows the earlier conversation.
    Fresh,
}

/// Fixed failure categories. No server text, URL or credential is carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fault {
    ConnectTimeout,
    SetupTimeout,
    ReadTimeout,
    /// Which write did not finish in time, the message's size in KiB and
    /// its bound (lampOS#705). Absent in records from before.
    WriteTimeout {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        write: Option<lamp_gemini::Write>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kib: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deadline_ms: Option<u32>,
    },
    BarrierTimeout,
    ResponseTimeout,
    StalledResponse,
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
    /// The provider sent something this client does not accept.
    Protocol,
    /// A fault inside this process, including a closed or overloaded queue.
    Local,
}
impl From<Error> for Fault {
    fn from(error: Error) -> Self {
        match error {
            Error::ConnectTimeout => Self::ConnectTimeout,
            Error::SetupTimeout => Self::SetupTimeout,
            Error::ReadTimeout => Self::ReadTimeout,
            Error::WriteTimeout {
                write,
                kib,
                deadline_ms,
            } => Self::WriteTimeout {
                write: Some(write),
                kib: Some(kib),
                deadline_ms: Some(deadline_ms),
            },
            Error::BarrierTimeout => Self::BarrierTimeout,
            Error::ResponseTimeout => Self::ResponseTimeout,
            Error::StalledResponse => Self::StalledResponse,
            Error::CompletionTimeout => Self::CompletionTimeout,
            Error::Authentication => Self::Authentication,
            Error::RateLimited => Self::RateLimited,
            Error::QuotaExceeded => Self::QuotaExceeded,
            Error::ServerUnavailable => Self::ServerUnavailable,
            Error::ServerRejected => Self::ServerRejected,
            Error::ServerGoAway => Self::ServerGoAway,
            Error::Transport => Self::Transport,
            Error::PeerClosed { code } => Self::PeerClosed { code },
            Error::MalformedMessage
            | Error::UnsupportedMessage
            | Error::MessageTooLarge
            | Error::InvalidAudio
            | Error::UnexpectedResponse => Self::Protocol,
            Error::InvalidConfiguration
            | Error::Backpressure
            | Error::OpenActivityOverrun
            | Error::StaleInput
            | Error::InputSequence
            | Error::StaleRequest
            | Error::OverlappingInput
            | Error::InputCancelled
            | Error::NotConnected
            | Error::Closed => Self::Local,
        }
    }
}

/// How a provider outage reaches the coordinator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reporting {
    /// The contract the coordinator implements today. A recovery appears as a
    /// repeated `Ready`; a request that cannot be finished ends the worker.
    Legacy,
    /// Send [`ProviderStatus`] and keep running through a lost request.
    Typed,
    /// Availability uses priority control; ordered PCM and exact outcomes use
    /// the credited data stream. This mode requires a coordinated consumer.
    Coordinated,
}

pub(crate) struct Options {
    pub(crate) reporting: Reporting,
    /// What the model may do other than speak.
    pub(crate) tools: crate::tools::Settings,
    /// The lamp's own rule for whom it answers ([`crate::attention`]), with
    /// where it reads how the room is and whether a conversation is going
    /// on. `None`: every answer is played.
    pub(crate) attention: Option<fn() -> (crate::room_told::Room, bool)>,
    /// The model is asked to say when it hears the lamp's name.
    pub(crate) name_signal: bool,
    /// A short sound let through in a quiet room is answered only when
    /// something shows it was said to the lamp (`--sounds-checked`, #800).
    /// Only with `attention`.
    pub(crate) sounds_checked: bool,
    /// In a conversation, a turn heard in another script or as a number
    /// alone is held and asked again (`--ask-unclear`, #926). Only with
    /// `attention`.
    pub(crate) ask_unclear: bool,
    /// "The lamp" said in passing is no call for the rule
    /// ([`crate::attention::called_by_name`], #829).
    pub(crate) name_in_passing_ignored: bool,
    /// The bus the Policy's decisions are read from when the lamp's actions
    /// wait for them ([`crate::policy_holds`], #800). `None`: they run at
    /// once.
    pub(crate) policy_holds: Option<std::path::PathBuf>,
    /// Input waits for room in Gemini's input queue (`--wait-for-room`).
    pub(crate) wait_for_room: bool,
    /// An answer is played only when the lamp was spoken to, in every room
    /// (`--spoken-to`, #829). Only with `attention`.
    pub(crate) spoken_to: bool,
    /// What makes the lines' voice prints for the same-voice check
    /// ([`crate::same_voice`], #829). Only with `spoken_to`.
    pub(crate) voices: Option<Box<dyn crate::same_voice::Printer>>,
    /// A request whose heard words name one of the staff is told so
    /// beside its sound ([`crate::staff_note`]). Only with the tasks part.
    pub(crate) staff_note: bool,
}
/// How the room is and whether the lamp is in a conversation, now. With
/// `--spoken-to`, none is once people have gone on talking to each other
/// since the lamp last spoke ([`crate::room_told::RoomTold::talked_past_the_lamp`]).
fn attention_now() -> (crate::room_told::Room, bool) {
    let (room, talked_past) = crate::room_told::room()
        .lock()
        .map_or((crate::room_told::Room::Unknown, false), |mut room| {
            (room.at(monotonic_us()), room.talked_past_the_lamp())
        });
    let going_on = !talked_past
        && crate::thread::THREAD
            .lock()
            .is_ok_and(|thread| thread.going_on(Instant::now()));
    (room, going_on)
}
#[cfg(test)]
impl Options {
    /// No function is declared and none is run.
    pub(crate) fn reporting(reporting: Reporting) -> Self {
        Self {
            reporting,
            tools: crate::tools::Settings::default(),
            attention: None,
            name_signal: false,
            sounds_checked: false,
            ask_unclear: false,
            name_in_passing_ignored: false,
            policy_holds: None,
            wait_for_room: false,
            staff_note: false,
            spoken_to: false,
            voices: None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Outbound {
    Output(ProviderOutput),
    Status(ProviderStatus),
}

/// Unsent output of retired requests is dropped, as the capacity contract
/// requires: at playback pace it would delay the next answer by seconds.
/// Survivors keep their order. Link and failure statuses are never dropped.
fn prune_retired_output(output: &mut VecDeque<Outbound>, retired_through: u64) {
    output.retain(|item| {
        let Outbound::Output(event) = item else {
            return true;
        };
        let request = match event {
            ProviderOutput::Ready { .. }
            | ProviderOutput::TurnFailed { .. }
            | ProviderOutput::ReminderDeferred { .. }
            // About a retired request by definition, and still owed.
            | ProviderOutput::InputDiscarded { .. }
            | ProviderOutput::OwnershipAssumed { .. }
            | ProviderOutput::Ended { .. } => None,
            ProviderOutput::Transcript { request, .. } => *request,
            ProviderOutput::Started { request, .. }
            | ProviderOutput::Audio { request, .. }
            | ProviderOutput::GenerationComplete { request }
            | ProviderOutput::Interrupted { request }
            | ProviderOutput::TurnComplete { request, .. }
        | ProviderOutput::Unclear { request, .. }
        | ProviderOutput::LookAsked { request, .. } => Some(*request),
        };
        request.is_none_or(|request| request > retired_through)
    });
}

/// The single refused Start stays in intake until its outcome has reserved
/// space. Input consumption pauses meanwhile; neither a later request nor a
/// full PCM queue can overwrite the report or grow the bounded output queue.
fn queue_undelivered(handoff: &mut InputHandoff, output: &mut VecDeque<Outbound>) {
    if output.len() < MAX_QUEUED_OUTPUT
        && let Some(request) = handoff.undelivered.take()
    {
        output.push_back(Outbound::Status(ProviderStatus::TurnFailed {
            request,
            stage: FailedStage::NotDelivered,
            reason: Fault::Local,
        }));
    }
    if output.len() < MAX_QUEUED_OUTPUT
        && let Some(deferred) = handoff.reminders.pending.take()
    {
        output.push_back(Outbound::Output(ProviderOutput::ReminderDeferred {
            request: deferred.request,
            session: deferred.session,
            reason: deferred.reason,
        }));
    }
}

pub async fn run(channels: WorkerChannels, boot: BootId, config_path: &Path) -> Result<()> {
    run_with_reporting(channels, boot, config_path, Reporting::Legacy).await
}

/// Explicit consumer-contract selection. The default entry point stays Legacy
/// until coordinated admission and terminal handling qualify together.
pub async fn run_with_reporting(
    channels: WorkerChannels,
    boot: BootId,
    config_path: &Path,
    reporting: Reporting,
) -> Result<()> {
    run_with_visual_context(channels, boot, config_path, reporting, None).await
}

/// Optional worker-local camera stream. No JPEG travels through audio IPC.
pub async fn run_with_visual_context(
    channels: WorkerChannels,
    boot: BootId,
    config_path: &Path,
    reporting: Reporting,
    visual: Option<(VisualSource, BootId)>,
) -> Result<()> {
    run_with_continuation(
        channels,
        boot,
        config_path,
        reporting,
        visual,
        Continuation::Off,
        None,
        Files::default(),
    )
    .await
}

/// The same, with an explicit choice of what happens when a person goes on
/// before the first answer has begun.
#[allow(clippy::too_many_arguments)]
pub async fn run_with_continuation(
    channels: WorkerChannels,
    boot: BootId,
    config_path: &Path,
    reporting: Reporting,
    visual: Option<(VisualSource, BootId)>,
    continuation: Continuation,
    sight: Option<crate::sight::Sight>,
    files: Files,
) -> Result<()> {
    if let Some(path) = files.look {
        let _ = LOOK_FILE.set(path);
    }
    LOOK_FULL.store(files.look_full, std::sync::atomic::Ordering::Relaxed);
    SESSION_FACT.store(files.session_fact, std::sync::atomic::Ordering::Relaxed);
    ROOM_FACTS.store(files.room_facts, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut room) = crate::room_told::room().lock() {
        room.leave_cut_ins_out(files.cut_ins_not_talk);
    }
    AIR_FACTS.store(files.air_facts, std::sync::atomic::Ordering::Relaxed);
    NAME_NOTE.store(files.name_note, std::sync::atomic::Ordering::Relaxed);
    HEAD_TOUCH.store(files.head_touch, std::sync::atomic::Ordering::Relaxed);
    NAME_NOTE_BUSY_FACE.store(
        files.name_note_busy_face,
        std::sync::atomic::Ordering::Relaxed,
    );
    ATTENTION_BUSY.store(
        files.attention && files.attention_busy,
        std::sync::atomic::Ordering::Relaxed,
    );
    SPOKEN_TO_BY_DAY.store(
        files.attention && files.spoken_to && files.spoken_to_by_day,
        std::sync::atomic::Ordering::Relaxed,
    );
    ATTENTION_NEAR.store(
        files.attention && files.attention_busy && files.attention_near,
        std::sync::atomic::Ordering::Relaxed,
    );
    ADDRESSED.store(
        files.attention && files.addressed,
        std::sync::atomic::Ordering::Relaxed,
    );
    // The conversation a service that ended a moment ago was in.
    if let Some(path) = files.thread {
        if let Ok(mut thread) = crate::thread::THREAD.lock() {
            thread.reload(&path, Instant::now(), std::time::SystemTime::now());
        }
        let _ = THREAD_FILE.set(path);
    }
    if let Some(root) = files.bus_root.clone() {
        let _ = BUS_ROOT.set(root);
    }
    if let Some(directory) = files.first_output.clone() {
        let _ = FIRST_OUTPUT.set(directory);
    }
    // What it remembers from other days, if this lamp has a memory: each
    // known person's apart when it has eyes that know faces.
    if let Some(socket) = &files.tools.memory {
        crate::remember::start(
            socket.clone(),
            files
                .tools
                .who
                .as_deref()
                .map(crate::known_faces::known_file_of),
        );
        // The rules' typed fact for the policy, when rules are on.
        if let Some(root) = files.bus_root.as_deref().filter(|_| crate::rules::on()) {
            crate::rules::bus_on(root);
        }
    }
    // Who work can go to, if the person may name them.
    if let Some(place) = &files.tools.tasks {
        crate::agent_tasks::start(place);
    }
    let (provider_config, patience) =
        ProviderConfig::load_with_patience(config_path, files.provider_patience)?;
    // What came from the file and what was filled in, so a lamp shows it
    // without its provider file being opened (the reviewer on #1394).
    if let Some(line) = patience {
        eprintln!("lamp-voice provider: {line}");
    }
    let barrier = provider_config.barrier_policy();
    let recovery = if files.keep_link {
        provider_config.serving_recovery_policy()
    } else {
        provider_config.recovery_policy()
    };
    let session = crate::not_speech::with_transcription(
        provider_config.into_session_with_camera(visual.is_some())?,
        files.transcription_language.as_deref(),
    )
    .barrier_policy(barrier);
    ROOM_WAIT.store(
        room_wait_us(files.patient_writes),
        std::sync::atomic::Ordering::Relaxed,
    );
    // A write may wait on a link that is behind (#705).
    let session = if files.patient_writes {
        crate::config::patient_writes(session)?
    } else {
        session
    };
    let config = continuation.apply(session)?;
    // Whose the first session is, read before it opens: the eyes' known
    // face, when each session is one person's (the switch below). The
    // switch's count starts from the same reading.
    if files.tools.memory.is_some()
        && let Some(path) = files
            .tools
            .who
            .as_deref()
            .map(crate::known_faces::known_file_of)
    {
        let known = crate::known_faces::known_now(&path);
        if let Ok(mut thread) = crate::thread::THREAD.lock() {
            thread.at_start(known.as_deref());
        }
        if let Some(name) = known {
            let _ = KNOWN_AT_START.set((name, Instant::now()));
        }
    }
    // Each session it opens is told how the conversation went until then.
    let provider = Supervisor::new(
        crate::thread::Recalling::new(config)
            .tools(files.tools.clone())
            .name_signal(files.name_signal)
            .file(THREAD_FILE.get().cloned()),
        recovery,
    )?;
    let options = Options {
        reporting,
        tools: files.tools,
        attention: files.attention.then_some(attention_now),
        name_signal: files.name_signal,
        sounds_checked: files.attention && files.sounds_checked,
        ask_unclear: files.attention && files.ask_unclear,
        name_in_passing_ignored: files.attention && files.name_in_passing_ignored,
        policy_holds: match (files.policy_holds, &files.bus_root) {
            (true, Some(root)) => Some(root.clone()),
            (true, None) => {
                eprintln!(
                    "lamp-voice provider: the Policy cannot hold the lamp's actions without the bus (LAMP_BUS=on): they run at once"
                );
                None
            }
            (false, _) => None,
        },
        wait_for_room: files.wait_for_room,
        staff_note: files.staff_note,
        spoken_to: files.attention && files.spoken_to,
        // The child starts now: its model loads while the link opens.
        voices: match (files.attention && files.spoken_to, files.same_voice) {
            (true, Some(model)) => match crate::same_voice::Prints::beside_voice(model) {
                Ok(prints) => Some(Box::new(prints) as Box<dyn crate::same_voice::Printer>),
                Err(error) => {
                    eprintln!("lamp-voice provider: the same voice is not judged: {error}");
                    None
                }
            },
            (false, Some(_)) => {
                eprintln!(
                    "lamp-voice provider: the same voice is judged only with LAMP_SPOKEN_TO=on: not judged"
                );
                None
            }
            (_, None) => None,
        },
    };
    let visual = visual
        .map(|(source, worker)| ContextDelivery::new(source, boot, worker))
        .transpose()?;
    serve_with_visual(channels, boot, provider, options, visual, sight).await
}

/// Paired producer/consumer qualification uses the real loop without loading
/// credentials or selecting a cloud endpoint. This seam is absent from builds.
#[cfg(test)]
pub(crate) async fn serve_coordinated_test<C: Connect>(
    channels: WorkerChannels,
    boot: BootId,
    provider: Supervisor<C>,
) -> Result<()> {
    serve(
        channels,
        boot,
        provider,
        Options::reporting(Reporting::Coordinated),
    )
    .await
}

#[cfg(test)]
async fn serve<C: Connect>(
    channels: WorkerChannels,
    boot: BootId,
    provider: Supervisor<C>,
    options: Options,
) -> Result<()> {
    serve_with_visual(channels, boot, provider, options, None, None).await
}

async fn serve_with_visual<C: Connect>(
    mut channels: WorkerChannels,
    boot: BootId,
    mut provider: Supervisor<C>,
    options: Options,
    mut visual: Option<ContextDelivery>,
    mut sight: Option<crate::sight::Sight>,
) -> Result<()> {
    channels.control.send(WorkerEvent::Ready)?;
    let coordinated = options.reporting == Reporting::Coordinated;
    let typed = options.reporting != Reporting::Legacy;
    let mut link_clock = coordinated.then(LinkClock::sample).transpose()?;
    let mut link_status = PendingLink::default();
    let mut upstream_ended = false;
    let mut intake = ProviderIntake::new(boot, monotonic_us());
    intake.handoff.tolerate_outage = typed;
    intake.handoff.wait_for_room = options.wait_for_room;
    intake.handoff.reminders.enabled = options.tools.schedule_speak;
    // The same-voice check (#829), only beside the rule that plays only for
    // a lamp spoken to.
    intake.handoff.voices = options
        .voices
        .filter(|_| options.attention.is_some() && options.spoken_to)
        .map(crate::same_voice::Voices::new);
    let mut ticker = tokio::time::interval(Duration::from_millis(2));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Link status is small and must not wait behind playback-paced audio.
    let mut urgent: VecDeque<Outbound> = VecDeque::with_capacity(4);
    let mut output = VecDeque::with_capacity(MAX_QUEUED_OUTPUT);
    let mut output_sequence = 0u64;
    let mut discards = DiscardLog::default();
    let mut renewed_at = Instant::now();
    // Until when a skill tool's look may still be owed (#941).
    let mut look_owed_until: Option<Instant> = None;
    let mut thread_saved_at = Instant::now();
    // The session last told how things stand, if new ones are.
    let session_fact = SESSION_FACT.load(std::sync::atomic::Ordering::Relaxed);
    let room_facts = ROOM_FACTS.load(std::sync::atomic::Ordering::Relaxed);
    // The air, read from the lamp's bus every few seconds when it is told.
    let air_bus = BUS_ROOT
        .get()
        .filter(|_| AIR_FACTS.load(std::sync::atomic::Ordering::Relaxed))
        .and_then(|root| lamp_bus::Bus::new(root).ok());
    let mut air_told = crate::air_told::AirTold::default();
    // Someone who asked the lamp to know their face, at the lamp: the eyes'
    // file beside their socket, read twice a second; told once a session.
    let known_file = options
        .tools
        .who
        .as_deref()
        .map(crate::known_faces::known_file_of);
    let mut known_read_at: Option<Instant> = None;
    let mut known_told: Option<(SessionId, String)> = None;
    // With a memory too, each session is one person's (#764): whose it is
    // counted from the same file, and a new one opened between turns when
    // that changes. Without both, nothing of this runs.
    let switch_file = known_file
        .as_deref()
        .filter(|_| options.tools.memory.is_some());
    let mut presence = crate::known_faces::Presence::default();
    if let Some((name, at)) = KNOWN_AT_START.get() {
        presence.seen(Some(name.clone()), *at);
    }
    let mut presence_read_at: Option<Instant> = None;
    // The name of the one counted at the lamp, for the name note: a face
    // kept under two names is told one, not each in turn (the reviewer on
    // #1100).
    let mut counted: Option<String> = None;
    let mut switch_asked: Option<SessionId> = None;
    // With a memory, the session is also told that what they say is theirs.
    let theirs = if options.tools.memory.is_some() {
        crate::remember::THEIRS
    } else {
        ""
    };
    // The room microphone's rise over its quiet, for whether a voice is
    // close to the lamp: read from the ears every 100 ms, the last 30 s kept.
    let attention_near = ATTENTION_NEAR.load(std::sync::atomic::Ordering::Relaxed);
    let addressed = ADDRESSED.load(std::sync::atomic::Ordering::Relaxed);
    let ears_bus = BUS_ROOT
        .get()
        .filter(|_| attention_near || addressed)
        .and_then(|root| lamp_bus::Bus::new(root).ok());
    let mut ears_levels: VecDeque<(u64, f32)> = VecDeque::new();
    // The newest request whose output came with another in hand, and was
    // so never judged: said once (#829, 17:21).
    let mut unjudged_told: Option<u64> = None;
    // The room microphone's rise over a request as the near rule judged it,
    // and the request, for its journal lines (#829, the first builder at
    // 15:23 and 17:09).
    let mut judged_rise: Option<(u64, f32)> = None;
    // Whether it is night where the lamp stands, read once a request as it
    // ends, for whether the name heard in a busy room needs a face (#829).
    let mut night_read: Option<(u64, bool)> = None;
    let mut ears_read_at: Option<Instant> = None;
    let mut air_read_at: Option<Instant> = None;
    let mut told_session = None;
    // How the room is, for whoever listens on the lamp's bus. A bus that
    // cannot be used is said once and changes nothing else.
    let mut room_out = BUS_ROOT.get().and_then(|root| {
        crate::bus_out::RoomOut::on(root)
            .map_err(|error| {
                eprintln!("lamp-voice provider: the room is not published on the bus: {error}")
            })
            .ok()
    });
    // The lamp's name as the part that listens for it hears it, for the
    // rule below. With no such part, or no bus, the rule goes by the text
    // alone, as it did.
    let mut name_in = BUS_ROOT.get().and_then(|root| {
        crate::bus_out::NameIn::on(root)
            .inspect_err(|why| {
                eprintln!(
                    "lamp-voice provider: the lamp's name cannot be taken from the bus, so whom it answers goes by the text alone: {why}"
                );
            })
            .ok()
    });
    // A tap on the lamp's head, from `--touch` Controls, counts as a call
    // for the next few seconds (#711). With no bus it counts for nothing.
    let mut taps_in = HEAD_TOUCH
        .load(std::sync::atomic::Ordering::Relaxed)
        .then(|| BUS_ROOT.get())
        .flatten()
        .and_then(|root| {
            crate::bus_out::TapsIn::on(root)
                .inspect_err(|why| {
                    eprintln!(
                        "lamp-voice provider: the head's taps cannot be taken from the bus: {why}"
                    );
                })
                .ok()
        });
    // And, if that is switched on, the model is told so while the person
    // is still speaking.
    let name_note = NAME_NOTE.load(std::sync::atomic::Ordering::Relaxed);
    let busy_needs_face = NAME_NOTE_BUSY_FACE.load(std::sync::atomic::Ordering::Relaxed);
    let attention_busy = ATTENTION_BUSY.load(std::sync::atomic::Ordering::Relaxed);
    let sounds_checked = options.attention.is_some() && options.sounds_checked;
    let spoken_to = options.attention.is_some() && options.spoken_to;
    // The eyes, for whether anyone is turned to the lamp. Read only in a
    // busy room, when a note is due or an answer is decided, for a short
    // sound being checked, and for every answer when it is played only
    // for a lamp spoken to.
    let eyes = BUS_ROOT
        .get()
        .filter(|_| busy_needs_face || attention_busy || addressed || sounds_checked || spoken_to)
        .and_then(|root| lamp_bus::Bus::new(root).ok());
    // Whether each request was said to the lamp from close to it: the eyes'
    // looks of the last 30 s, and the topic it is published on. A bus that
    // cannot be used is said once; the rule then has no such sign.
    let mut looks = crate::addressed::Looks::default();
    // The request judged last, judged once: as its sound ended or, if its
    // answer began first, then.
    let mut addressed_said: Option<lamp_msgs::Addressed> = None;
    let mut addressed_out = BUS_ROOT.get().filter(|_| addressed).and_then(|root| {
        crate::addressed::AddressedOut::on(root)
            .map_err(|error| {
                eprintln!(
                    "lamp-voice provider: whether a request was said to the lamp is not published on the bus: {error}"
                )
            })
            .ok()
    });
    let mut called = crate::called::Called::default();
    // What was heard of the newest request, for the staff's note.
    let mut staff_heard = crate::staff_note::Heard::default();
    let mut desk = ToolDesk::new(options.tools.clone());
    // The lamp's actions wait for the Policy's decision (#800).
    desk.verdicts = options.policy_holds.as_deref().and_then(|root| {
        crate::policy_holds::Verdicts::on(root)
            .inspect_err(|why| {
                eprintln!(
                    "lamp-voice provider: the Policy's decisions cannot be read from the bus, so the lamp's actions run at once: {why}"
                );
            })
            .ok()
    });
    desk.name_signal = options.name_signal;
    let mut attention = crate::attention::Attention::default();
    // When the lamp's sound was last handed on: a turn follows the lamp's
    // last line only if it spoke lately (#1054).
    let mut lamp_spoke_us: Option<u64> = None;
    attention.check_sounds(sounds_checked);
    attention.ask_again_when_unclear(options.attention.is_some() && options.ask_unclear);
    attention.ignore_name_in_passing(options.name_in_passing_ignored);
    attention.speak_when_spoken_to(spoken_to);
    attention.judge_by_day(SPOKEN_TO_BY_DAY.load(std::sync::atomic::Ordering::Relaxed));
    // With it, only talk between people makes the room busy (#829).
    if let Ok(mut room) = crate::room_told::room().lock() {
        room.count_only_talk(spoken_to);
    }
    // The record of whom it answered, off unless set.
    let mut first_output = FIRST_OUTPUT.get().and_then(|directory| {
        match crate::first_output::FirstOutputLog::open(directory) {
            Ok(log) => {
                eprintln!(
                    "lamp-voice provider: whom it answers is recorded in {}",
                    log.path().display()
                );
                Some(log)
            }
            Err(error) => {
                eprintln!("lamp-voice provider: whom it answers is not recorded: {error}");
                None
            }
        }
    });
    // With names checked and the eyes there, the request whose given name
    // the model was told to keep.
    let names_told = options.tools.names_checked && options.tools.who.is_some();
    let mut name_told_for: Option<RequestId> = None;
    let mut words_of: Option<(RequestId, String)> = None;
    // When the name was last heard plainly enough to count as a call.
    let mut sure_name_us: Option<u64> = None;
    unanswered_requests().store(0, std::sync::atomic::Ordering::Relaxed);
    // With LAMP_STALLS=on, a tick's work or a late tick of 50 ms or more is
    // recorded with what held it (#1178).
    let mut stalls = crate::stalls::Stalls::from_environment("provider");
    loop {
        // Control is polled first and never waits for the provider: stop,
        // privacy and retirement are serviced every tick whether a connection
        // is ready, reconnecting, draining or delivering a burst.
        tokio::select! {
            biased;
            _=ticker.tick()=>{
                let stall_pass = stalls.begin();
                let mut control_budget = CONTROL_SLICE;
                let control = intake.drain_control(&mut monotonic_us, &mut || channels.control.receive(), &mut control_budget)?;
                if control == ControlDrain::Stopped { provider.shutdown(); return Ok(()); }
                // Notice a dead connection and start its replacement now, even
                // while no provider event is being taken.
                service_link(&mut provider, link_clock.as_mut(), &mut link_status, |link|
                    channels.control.send(WorkerEvent::ProviderLink { link }))?;
                // The lamp's actions kept for the Policy's decision (#800),
                // each let go if its turn was retired meanwhile.
                desk.release(&provider, |request| intake.handoff.retired(request));
                // What the executive made of the calls it was asked.
                desk.answer(&provider);
                for (request, session, dropped) in desk.take_looks() {
                    // No session renewed under a look still owed: its
                    // written turn goes to the session that saw the call.
                    if dropped.is_none() {
                        look_owed_until = Some(Instant::now() + LOOK_OWED_AT_MOST);
                    }
                    output.push_back(Outbound::Output(ProviderOutput::LookAsked { request, session, dropped }));
                }
                // Each call answered, for the first-output record when
                // there is one (#799); let go when there is none.
                for line in desk.take_lines() {
                    if let Some(log) = first_output.as_mut() {
                        log.tool_call(&line);
                    }
                }
                // A session that has answered nothing for a long run of
                // requests is replaced the next time nothing is owed on it.
                if unanswered_requests().load(std::sync::atomic::Ordering::Relaxed) >= RENEW_AFTER_UNANSWERED
                    && renewed_at.elapsed() >= RENEW_AT_MOST_EVERY
                    && !look_owed(look_owed_until)
                    && provider.renew_when_idle()
                {
                    eprintln!(
                        "lamp-voice provider: {RENEW_AFTER_UNANSWERED} requests in a row went unanswered; renewing the session when it is idle"
                    );
                    renewed_at = Instant::now();
                    unanswered_requests().store(0, std::sync::atomic::Ordering::Relaxed);
                }
                if let Some(out) = room_out.as_mut()
                    && let Ok(mut room) = crate::room_told::room().lock()
                {
                    let now = monotonic_us();
                    out.tell(room.at(now), now);
                }
                // Before the name is read: one that began after the
                // request's sound ended is not that request's.
                if let Some((request, at_us)) = intake.handoff.ended {
                    attention.ended(request, at_us);
                    // Whether it is night, read once as the request ends: a
                    // file and the kernel's clock, off the way to its first
                    // output (Ten on #1158). One whose end this worker never
                    // saw is read at its first output. Only with the day's
                    // own bars, which alone use it (the reviewer on #1158).
                    if attention.judges_by_day() && night_read.is_none_or(|(of, _)| of != request) {
                        night_read = Some((request, crate::local_time::night_here()));
                    }
                    if intake.handoff.ended_cut_in == Some(request) {
                        attention.ended_as_cut_in(request);
                    }
                    if intake.handoff.ended_as_sound == Some(request) {
                        attention.ended_as_sound(request);
                    }
                }
                if addressed
                    && let Some(request) = attention.in_hand()
                    && request.ended_us.is_some()
                    && !crate::addressed::judged(addressed_said.as_ref(), &request)
                {
                    let now = monotonic_us();
                    let said = crate::addressed::judge(request, &ears_levels, &looks, now);
                    attention.set_addressed(said.addressed);
                    if let Some(out) = addressed_out.as_mut()
                        && let Err(error) = out.tell(&said, now)
                    {
                        eprintln!("lamp-voice provider: request {}: whether it was said to the lamp was not published: {error}", said.request);
                    }
                    addressed_said = Some(said);
                }
                if let Some(names) = name_in.as_mut()
                    && let Some((began, score)) = names.newest_scored(monotonic_us())
                {
                    attention.name_heard_scored(began, score);
                    called.name_heard(began, score);
                    if score >= crate::attention::SURE_SCORE {
                        sure_name_us = Some(began);
                    }
                }
                if let Some(taps) = taps_in.as_mut()
                    && let Some(tapped) = taps.newest(monotonic_us())
                {
                    attention.head_tapped(tapped);
                }
                // Heard plainly as this request began: the model is told,
                // beside what it is hearing, once.
                if name_note {
                    let open = intake.handoff.visual_turn().map(|turn| turn.request);
                    if let Some(request) = called.due(open.map(RequestId::get), monotonic_us())
                        && let Some(open) = open
                    {
                        called.told(request);
                        let busy = busy_needs_face
                            && crate::room_told::room().lock().is_ok_and(|mut room| {
                                room.at(monotonic_us()) == crate::room_told::Room::Busy
                            });
                        // A plain hearing of the name stands for a face.
                        let plain = sure_name_us.is_some_and(|at| {
                            monotonic_us().saturating_sub(at) < 3 * crate::attention::NAME_BEFORE_US
                        });
                        let turned = if busy && plain {
                            Some(true)
                        } else if busy {
                            eyes.as_ref().and_then(crate::bus_out::someone_turned_to_lamp)
                        } else {
                            None
                        };
                        match crate::called::busy_gate(busy_needs_face, busy, turned) {
                            Err(why) => {
                                // How close the voice was, so far, for the
                                // first builder's tables (#829, 15:23).
                                let rise = match attention.window().and_then(|window| {
                                    rise_in(&ears_levels, window, monotonic_us())
                                }) {
                                    Some(rise) => format!("the room microphone {rise:.1} dB over its quiet"),
                                    None => "no rise of the room microphone read".to_owned(),
                                };
                                eprintln!(
                                    "lamp-voice provider: request {request}: the lamp's name was heard in a busy room and {why} ({rise}); the model is not told"
                                );
                            }
                            Ok(()) => match add_note(&provider, &mut intake.handoff.noted, open, crate::called::NOTE) {
                                Ok(()) => eprintln!(
                                    "lamp-voice provider: request {request}: the model is told the lamp's name was heard"
                                ),
                                Err(error) => eprintln!(
                                    "lamp-voice provider: request {request}: the lamp's name was heard and the model could not be told: {error}"
                                ),
                            },
                        }
                    }
                }
                // The first request after a hand or the head stopped the
                // lamp: the model is told so, so that it does not pick up
                // what it was saying (#829, the first builder at 16:33).
                if let Some(request) = intake.handoff.take_after_stop() {
                    match add_note(&provider, &mut intake.handoff.noted, request, STOPPED_NOTE) {
                        Ok(()) => eprintln!(
                            "lamp-voice provider: request {}: the model is told the person stopped it",
                            request.get()
                        ),
                        Err(error) => eprintln!(
                            "lamp-voice provider: request {}: the model could not be told the person stopped it: {error}",
                            request.get()
                        ),
                    }
                }
                // Only the lamp's name was said: what follows it is called
                // (#829, the first builder at 16:32).
                if let Some((request, at_us)) = intake.handoff.take_bare_call() {
                    attention.bare_call(request, at_us);
                }
                // A request that goes on from one whose end was sent early
                // and given up: the model is told so, beside what it hears
                // (`--early-end-ms`; docs/early-end-taken-back.md).
                if let Some(request) = intake.handoff.take_cut_off() {
                    match add_note(&provider, &mut intake.handoff.noted, request, CUT_OFF_NOTE) {
                        Ok(()) => eprintln!(
                            "lamp-voice provider: request {}: the model is told it goes on from the request given up",
                            request.get()
                        ),
                        Err(error) => eprintln!(
                            "lamp-voice provider: request {}: the model could not be told it goes on from the request given up: {error}",
                            request.get()
                        ),
                    }
                }
                // The session is told how the room is when that changes,
                // and a new session how it is now, unless a conversation is
                // going on: the thread's note says that.
                if room_facts
                    && let Some(live) = provider.live_session()
                    && let Ok(mut room) = crate::room_told::room().lock()
                {
                    let now = monotonic_us();
                    let going_on = || {
                        crate::thread::THREAD
                            .lock()
                            .is_ok_and(|thread| thread.going_on(Instant::now()))
                    };
                    if let Some(fact) = room.due(live.get(), now) {
                        if !going_on() && provider.try_context(fact).is_some() {
                            room.told(live.get());
                            // The room says more than the plain fact would.
                            told_session = Some(live);
                            eprintln!("lamp-voice provider: told the session {fact}");
                        }
                    } else if let Some(open) = intake.handoff.visual_turn()
                        && open.session == live
                        && let Some(fact) = room.due_in_request(live.get(), now)
                        && !going_on()
                        && add_note(&provider, &mut intake.handoff.noted, open.request, fact).is_ok()
                    {
                        // No quiet moment comes in a meeting: it goes
                        // beside what is being said, as the name does.
                        room.told(live.get());
                        told_session = Some(live);
                        eprintln!(
                            "lamp-voice provider: request {}: told the session inside it {fact}",
                            open.request.get()
                        );
                    }
                }
                // A name told is a name kept (`LAMP_NAMES=checked`, the first
                // builder at 13:43): once a request shows the person giving
                // their name, the model is told to keep it, beside it.
                if names_told
                    && let Some(open) = intake.handoff.visual_turn()
                    && name_told_for != Some(open.request)
                    && words_of.as_ref().is_some_and(|(request, words)| {
                        *request == open.request && crate::known_faces::gives_a_name(words)
                    })
                    && add_note(&provider, &mut intake.handoff.noted, open.request, crate::known_faces::NAME_TOLD_NOTE)
                        .is_ok()
                {
                    name_told_for = Some(open.request);
                    eprintln!(
                        "lamp-voice provider: request {}: they gave their name, and the model is told to keep it",
                        open.request.get()
                    );
                }
                if let Some(bus) = ears_bus.as_ref()
                    && ears_read_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(100))
                {
                    ears_read_at = Some(Instant::now());
                    let now = monotonic_us();
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |since| since.as_millis() as u64);
                    if let Some(level) = bus
                        .latest(lamp_msgs::RoomSound::TOPIC)
                        .ok()
                        .flatten()
                        .filter(|message| now_ms.saturating_sub(message.header.at_unix_ms) < 1_000)
                        .and_then(|message| message.parse::<lamp_msgs::RoomSound>().ok())
                        && !level.own_voice
                        && let Some(floor) = level.floor_db
                    {
                        ears_levels.push_back((now, level.level_db - floor));
                    }
                    while ears_levels.front().is_some_and(|(at, _)| now.saturating_sub(*at) > 30_000_000) {
                        ears_levels.pop_front();
                    }
                    if addressed
                        && let Some(message) = eyes.as_ref().and_then(|eyes| {
                            eyes.latest(lamp_msgs::Faces::TOPIC).ok().flatten()
                        })
                    {
                        looks.read(&message, now);
                    }
                }
                // The air: read every five seconds; a stuffy room is told
                // beside what a person is saying to the lamp, not in a busy
                // room, at most once in three hours (`crate::air_told`).
                if let Some(bus) = air_bus.as_ref() {
                    if air_read_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(5)) {
                        air_read_at = Some(Instant::now());
                        let fresh_ms = crate::air_told::FRESH_FOR.as_millis() as u64;
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |since| since.as_millis() as u64);
                        let air = bus
                            .latest(lamp_msgs::AirState::TOPIC)
                            .ok()
                            .flatten()
                            .filter(|message| now_ms.saturating_sub(message.header.at_unix_ms) < fresh_ms)
                            .and_then(|message| message.parse::<lamp_msgs::AirState>().ok());
                        let stuffy = air.is_some_and(|air| {
                            matches!(air.feel, lamp_msgs::AirFeel::Stuffy | lamp_msgs::AirFeel::VeryStuffy)
                        });
                        air_told.seen(stuffy, air.and_then(|air| air.co2_ppm), monotonic_us());
                    }
                    let now = monotonic_us();
                    let busy = crate::room_told::room()
                        .lock()
                        .is_ok_and(|mut room| room.at(now) == crate::room_told::Room::Busy);
                    if !busy
                        && let Some(live) = provider.live_session()
                        && let Some(open) = intake.handoff.visual_turn()
                        && open.session == live
                        && let Some(note) = air_told.due(now)
                        && add_note(&provider, &mut intake.handoff.noted, open.request, &note).is_ok()
                    {
                        air_told.told(now);
                        eprintln!(
                            "lamp-voice provider: request {}: told the session inside it that the air is stuffy",
                            open.request.get()
                        );
                    }
                }
                // The person at the lamp is someone it knows: what they say
                // is told their name, once a session (`crate::known_faces`);
                // with a session for each person, the name of the one counted.
                if let Some(path) = known_file.as_deref()
                    && known_read_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(500))
                    && let Some(live) = provider.live_session()
                    && let Some(open) = intake.handoff.visual_turn()
                    && open.session == live
                {
                    known_read_at = Some(Instant::now());
                    if let Some(name) = crate::known_faces::known_now(path)
                            .map(|seen| counted.clone().filter(|_| switch_file.is_some()).unwrap_or(seen))
                        && known_told.as_ref() != Some(&(live, name.clone()))
                        && add_note(&provider, &mut intake.handoff.noted, open.request, &(crate::known_faces::note(&name) + theirs)).is_ok()
                    {
                        known_told = Some((live, name));
                        eprintln!(
                            "lamp-voice provider: request {}: told the session who is at the lamp",
                            open.request.get()
                        );
                    }
                }
                // Whose session it is: the person at the lamp changed (gone
                // 30 s, or someone else known for 10 s), so a new session is opened
                // for them, told only their thread and kept things. Asked
                // only between turns, and the session itself waits until
                // nothing is owed on it, so it never cuts anyone off.
                if let Some(path) = switch_file
                    && presence_read_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(500))
                {
                    presence_read_at = Some(Instant::now());
                    let fresh = crate::known_faces::known_now(path);
                    let named = fresh.as_deref().map(crate::remember::person_id);
                    counted = presence.seen(fresh, Instant::now()).map(str::to_owned);
                    let whose = counted.as_deref().map(crate::remember::person_id);
                    let turn_open = intake.handoff.visual_turn().is_some();
                    let due = crate::thread::THREAD.lock().is_ok_and(|mut thread| {
                        thread.at_lamp(whose);
                        thread.seen_now(named);
                        thread.switch_now(turn_open, !output.is_empty(), Instant::now())
                    });
                    let live = provider.live_session();
                    if due
                        && live.is_some()
                        && live != switch_asked
                        && !look_owed(look_owed_until)
                        && provider.renew_when_idle()
                    {
                        switch_asked = live;
                        renewed_at = Instant::now();
                        eprintln!(
                            "lamp-voice provider: the person at the lamp has changed; a new session is opened between turns"
                        );
                    }
                }
                // A new session is told how things stand, once, unless a
                // conversation is going on.
                if session_fact {
                    let live = provider.live_session();
                    if live.is_some() && live != told_session {
                        told_session = live;
                        let going_on = crate::thread::THREAD
                            .lock()
                            .is_ok_and(|thread| thread.going_on(Instant::now()));
                        if !going_on && provider.try_context(crate::session_fact::FACT).is_some() {
                            eprintln!(
                                "lamp-voice provider: told the new session that nobody is talking to the lamp"
                            );
                        }
                    }
                }
                // The thread goes to its file within a second of changing.
                if let Some(path) = THREAD_FILE.get()
                    && thread_saved_at.elapsed() >= Duration::from_secs(1)
                {
                    thread_saved_at = Instant::now();
                    let text = crate::thread::THREAD.lock().ok().and_then(|mut thread| {
                        thread.save(Instant::now(), std::time::SystemTime::now())
                    });
                    // A file that cannot be written loses the thread at the
                    // next restart and nothing else. One written is not
                    // read back as another route's.
                    if let Some(text) = text
                        && crate::thread::write_file(path, &text).is_ok()
                        && let Ok(mut thread) = crate::thread::THREAD.lock()
                    {
                        thread.wrote(path);
                    }
                }
                // And one that holds a picture, once the talk has paused.
                let looked = LOOKED_AT_US.load(std::sync::atomic::Ordering::Relaxed);
                if looked != 0
                    && monotonic_us().saturating_sub(
                        SUBMITTED_AT_US.load(std::sync::atomic::Ordering::Relaxed).max(looked),
                    ) >= RENEW_AFTER_LOOK_QUIET_US
                    && !look_owed(look_owed_until)
                    && provider.renew_when_idle()
                {
                    eprintln!(
                        "lamp-voice provider: the lamp looked and the talk has paused; renewing the session without the picture"
                    );
                    LOOKED_AT_US.store(0, std::sync::atomic::Ordering::Relaxed);
                    renewed_at = Instant::now();
                }
                // Camera revocation also bypasses PCM credit and a full control
                // slice. A partial cloud image write may abandon that connection;
                // ordinary lost-turn handling remains the sole terminal outcome.
                if let Some(visual) = visual.as_mut() {
                    visual.authority(&mut intake.guard, &provider, monotonic_us());
                }
                // Local retirement cannot wait behind a full control slice.
                intake.handoff.synchronize(&provider)?;
                prune_retired_output(&mut output, intake.handoff.retired_through);
                if control == ControlDrain::Deferred { stall_pass.done(); continue; }
                queue_undelivered(&mut intake.handoff, &mut output);
                if intake.handoff.undelivered.is_none() {
                    let received = intake.receive_inputs(&mut monotonic_us, &mut || channels.control.receive(), &mut channels.data, &provider, &mut control_budget);
                    match received {
                        Ok(ControlDrain::Stopped) => { provider.shutdown(); return Ok(()); }
                        Ok(ControlDrain::Deferred) => continue,
                        Ok(ControlDrain::Empty) => {}
                        // Admitted input cannot be held until a connection exists:
                        // it would be answered late or not at all. Say so and end.
                        Err(error) if !provider.is_connected() => return Err(io::Error::other(format!("provider unavailable while input was admitted: {error}")).into()),
                        Err(error) => return Err(error),
                    }
                }
                if let Some(visual) = visual.as_mut() {
                    visual.authority(&mut intake.guard, &provider, monotonic_us());
                    visual.poll(intake.handoff.visual_turn(), &provider, monotonic_us());
                    // Frame reads and privacy use separate transports. Read
                    // priority again immediately before exporting the JPEG.
                    let control = intake.drain_control(&mut monotonic_us, &mut || channels.control.receive(), &mut control_budget)?;
                    if control == ControlDrain::Stopped { provider.shutdown(); return Ok(()); }
                    visual.authority(&mut intake.guard, &provider, monotonic_us());
                    intake.handoff.synchronize(&provider)?;
                    if control == ControlDrain::Empty {
                        visual.export(intake.handoff.visual_turn(), &mut intake.guard, &provider);
                    }
                    visual.send_receipt(|receipt| channels.control.send(WorkerEvent::VisualContext { receipt: receipt.clone() }))?;
                }
                // What the lamp's eyes see, with what the person is saying.
                if let Some(sight) = sight.as_mut() {
                    sight.offer(intake.handoff.visual_turn().map(|turn| turn.request), &provider);
                }
                queue_undelivered(&mut intake.handoff, &mut output);
                // receive_inputs can observe a newer authority too.
                prune_retired_output(&mut output, intake.handoff.retired_through);
                for _ in 0..8 {
                    // Link status goes first; it still needs capacity like
                    // every other packet.
                    let queue = if urgent.is_empty() { &mut output } else { &mut urgent };
                    let Some(front)=queue.front() else {break;};
                    if intake.output_flow.try_send(|| channels.data.send(front))? {
                        // The lamp's own news is news once its sound has been
                        // handed on, owner-checked and credited (#829, #918):
                        // not when it was heard from the service, nor sound
                        // a cancellation dropped before it went.
                        if let Some(Outbound::Output(ProviderOutput::Audio { request, .. })) = queue.front() {
                            attention.spoke(*request, monotonic_us());
                            lamp_spoke_us = Some(monotonic_us());
                        }
                        queue.pop_front();
                    } else { break; }
                }
                stall_pass.done();
            },
            notice=next_notice(&mut provider, coordinated), if !upstream_ended && output.len() < OUTPUT_HIGH_WATER=>{
                if let Ok(notice) = &notice
                    && reminder_notice(&intake.handoff, notice, &provider)
                {
                    continue;
                }
                let reminder_origin = notice.as_ref().ok()
                    .and_then(reminder_policy::lineage)
                    .is_some_and(|lineage| intake.handoff.reminders.is_reminder(lineage));
                if let Ok(Notice::Event(Event::Audio { lineage, .. })) = &notice {
                    desk.sound(lineage.request.get());
                }
                // A request whose words name the staff: the model is told so
                // beside its sound while its input is open, the note changed
                // if a later piece names more (`crate::staff_note`). One that
                // goes on from the one before keeps its words, and is told
                // at once if they named the staff.
                if let (Some(place), Ok(Notice::Event(event))) = (
                    options.tools.tasks.as_ref().filter(|_| options.staff_note),
                    &notice,
                ) {
                    let piece = match event {
                        Event::InputStarted { lineage, .. } => {
                            let request = lineage.request.get();
                            let over = intake.handoff.over_the_lamp == Some(request);
                            if let Some((_, cut_off)) = intake.handoff.went_on.filter(|(went, _)| *went == request) {
                                staff_heard.went_on(request, over, cut_off);
                                Some("")
                            } else {
                                staff_heard.began(request, over);
                                None
                            }
                        }
                        Event::UncorrelatedInputTranscript { text, .. } => Some(text.as_str()),
                        _ => None,
                    };
                    let open = intake.handoff.visual_turn().map(|turn| turn.request);
                    let named = piece
                        .and_then(|piece| staff_heard.piece(piece))
                        .map(|(request, words)| (request, crate::agent_tasks::named_in_words(place, words)));
                    if let Some((request, who)) = named.filter(|(_, who)| !who.is_empty()) {
                        let part = crate::staff_note::note(&who);
                        let who = who.join(" and ");
                        if staff_heard.told() != Some(part.as_str()) {
                            match open.filter(|open| open.get() == request) {
                                None => {
                                    staff_heard.too_late();
                                    eprintln!(
                                        "lamp-voice provider: request {request}: named {who} after its input was closed; the model is not told"
                                    );
                                }
                                Some(open) => {
                                    let text = staff_note_text(intake.handoff.noted.as_ref(), request, staff_heard.told(), &part);
                                    match provider.try_note(open, &text) {
                                        Ok(()) => {
                                            intake.handoff.noted = Some((request, text));
                                            staff_heard.tell(part);
                                            eprintln!(
                                                "lamp-voice provider: request {request}: named {who} in time; the model is told"
                                            );
                                        }
                                        Err(error) => eprintln!(
                                            "lamp-voice provider: request {request}: named {who} in time and the model could not be told: {error}"
                                        ),
                                    }
                                }
                            }
                        }
                    }
                }
                // What the person says in the request in hand, for a name
                // given (`LAMP_NAMES=checked`): its own words only, never an
                // earlier request's.
                if names_told
                    && let Ok(Notice::Event(Event::UncorrelatedInputTranscript { text, .. })) = &notice
                    && let Some(open) = intake.handoff.visual_turn()
                {
                    if words_of.as_ref().map(|(request, _)| *request) != Some(open.request) {
                        words_of = Some((open.request, String::new()));
                    }
                    if let Some((_, words)) = words_of.as_mut()
                        && words.len() < 2_048
                    {
                        words.push_str(text);
                    }
                }
                // Whom the lamp answers: followed for every request, and
                // decided when the first of an answer comes: its sound, its
                // words, or a call for something to be done, which is
                // decided before it is run (on lamp-4ace on 2026-10-14 a
                // light was switched on for the table's talk while the
                // answer's sound was held).
                if let (Some(now), Ok(Notice::Event(event))) = (options.attention, &notice) {
                    match event {
                        Event::InputStarted { lineage, .. } => {
                            let request = lineage.request.get();
                            if intake.handoff.continued == Some(request) {
                                attention.continued_at(request, lineage.session.get(), monotonic_us());
                            } else {
                                attention.began_at(request, lineage.session.get(), monotonic_us());
                            }
                        }
                        Event::UncorrelatedInputTranscript { text, .. } => attention.heard(text),
                        Event::Audio { lineage, .. } | Event::OutputTranscript { lineage, .. } | Event::ToolCall { lineage, .. }
                            if !matches!(event, Event::ToolCall { call, .. } if !crate::attention::answers(&call.name)) => {
                            let sampled_start_us = monotonic_us();
                            let (room, going_on) = now();
                            let request = lineage.request.get();
                            let turned = (attention_busy && room == crate::room_told::Room::Busy
                                || attention.sound_in_hand()
                                || attention.speaks_when_spoken_to())
                                .then(|| eyes.as_ref().and_then(crate::bus_out::someone_turned_to_lamp))
                                .flatten();
                            let sampled_end_us = monotonic_us();
                            if attention_near
                                && ears_bus.is_some()
                                && let Some((began, ended)) = attention.window()
                            {
                                let rise = rise_in(&ears_levels, (began, ended), monotonic_us());
                                attention.set_near(rise.is_some_and(|rise| rise >= crate::attention::NEAR_RISE_DB));
                                judged_rise = rise.map(|rise| (request, rise));
                            }
                            // Said to the lamp from close to it, over the
                            // request's sound so far, if it was not judged
                            // as the sound ended.
                            if addressed
                                && let Some(in_hand) = attention.in_hand().filter(|in_hand| in_hand.request == request)
                                && !crate::addressed::judged(addressed_said.as_ref(), &in_hand)
                            {
                                let now = monotonic_us();
                                let said = crate::addressed::judge(in_hand, &ears_levels, &looks, now);
                                attention.set_addressed(said.addressed);
                                if let Some(out) = addressed_out.as_mut()
                                    && let Err(error) = out.tell(&said, now)
                                {
                                    eprintln!("lamp-voice provider: request {}: whether it was said to the lamp was not published: {error}", said.request);
                                }
                                addressed_said = Some(said);
                            }
                            let lamp_asked = crate::thread::THREAD.lock().is_ok_and(|thread| thread.lamp_asked(request));
                            attention.lamp_asked(lamp_asked);
                            // #829: a line in another voice than the
                            // conversation's is no part of it. Judged once,
                            // while its answer's verdict is still to come
                            // (the reviewer on #1030: every chunk of an
                            // answer said it again).
                            let mut other_voice = false;
                            let judged = intake.handoff.voices.as_mut().filter(|_| going_on && attention.awaits_verdict(request)).map(|voices| {
                                let likeness = voices.likeness(request);
                                (likeness, likeness.is_none().then(|| voices.unjudged(request)).flatten())
                            });
                            // A line long enough to know, judged as without
                            // the check for want of a print: said, never
                            // silently again (#829, lamp-4ace on 18 October,
                            // requests 18 and 19; the reviewer on #1435).
                            if let Some((_, Some(why))) = judged {
                                eprintln!("lamp-voice provider: request {request}: judged without the voice check: {why}");
                            }
                            let going_on = match judged.and_then(|(likeness, _)| likeness) {
                                Some(likeness) if likeness < crate::same_voice::DIFFERENT_BELOW => {
                                    eprintln!("lamp-voice provider: request {request}: another voice than the conversation's (cosine {likeness:.2}): no conversation is going on for it");
                                    other_voice = true;
                                    false
                                }
                                Some(likeness) => {
                                    eprintln!("lamp-voice provider: request {request}: the conversation's voice (cosine {likeness:.2})");
                                    true
                                }
                                None => going_on,
                            };
                            if attention.judges_by_day() && night_read.is_none_or(|(of, _)| of != request) {
                                night_read = Some((request, crate::local_time::night_here()));
                            }
                            attention.set_night(night_read.is_some_and(|(_, night)| night));
                            let verdict = attention.first_output_busy(request, room, going_on, attention_busy, turned);
                            if let (Some(voices), Some(verdict)) = (intake.handoff.voices.as_mut(), verdict) {
                                voices.decided(request, matches!(verdict, crate::attention::Verdict::Play(_)), going_on);
                            }
                            if verdict.is_none()
                                && !attention.judged(request)
                                && unjudged_told != Some(request)
                            {
                                unjudged_told = Some(request);
                                eprintln!(
                                    "lamp-voice provider: request {request}: its output came with request {} in hand, and is not judged",
                                    attention.in_hand_request()
                                );
                            }
                            // Held as unclear: the coordinator asks again. Said
                            // once, when the verdict is given, never again for
                            // the request's later output (lamp-4ace 06:42: eight
                            // in 0.32 s).
                            if verdict.is_some()
                                && let Some(why) = attention.unclear(request)
                            {
                                output.push_back(Outbound::Output(ProviderOutput::Unclear { request, why: why.to_owned() }));
                            }
                            // A request whose answer is held is not talk
                            // between people (#829), but one held as another
                            // voice is: the clearest sign of it (the reviewer
                            // on #1030).
                            if matches!(verdict, Some(crate::attention::Verdict::Hold(_)))
                                && !other_voice
                                && let Ok(mut told) = crate::room_told::room().lock()
                            {
                                told.held(request);
                            }
                            // And that one line ends the conversation: the
                            // two people are talking (the battery's scene 3,
                            // #829, 17 October).
                            if matches!(verdict, Some(crate::attention::Verdict::Hold(_)))
                                && other_voice
                                && let Ok(mut told) = crate::room_told::room().lock()
                            {
                                told.another_voice_held(request);
                            }
                            if let (Some(log), Some(verdict)) = (first_output.as_mut(), verdict) {
                                let (play, reason) = match verdict {
                                    crate::attention::Verdict::Play(why) => (true, why),
                                    crate::attention::Verdict::Hold(why) => (false, why),
                                };
                                let window = attention.window();
                                log.decided(&crate::first_output::Decision {
                                    session: lineage.session.get(),
                                    request,
                                    trigger: match event {
                                        Event::Audio { .. } => "audio",
                                        Event::OutputTranscript { .. } => "output_transcript",
                                        _ => "tool_call",
                                    },
                                    strict: attention_busy,
                                    baseline: attention.baseline(room, going_on, turned),
                                    play,
                                    reason,
                                    sampled_start_us,
                                    sampled_end_us,
                                    evaluated_us: monotonic_us(),
                                    input_began_us: window.map(|(began, _)| began),
                                    input_ended_us: window.and_then(|(_, ended)| ended),
                                    addressed: addressed_said.filter(|said| {
                                        said.session == lineage.session.get() && said.request == request
                                    }),
                                    lamp_asked,
                                });
                            }
                            // The rise the near rule judged, on the lines the
                            // busy-room bars are set from: the true calls held
                            // as well as the answers let out (the reviewer on
                            // #1083).
                            let rise = judged_rise
                                .filter(|(of, _)| *of == request)
                                .map(|(_, rise)| format!(" (the room microphone {rise:.1} dB over its quiet)"))
                                .unwrap_or_default();
                            match verdict {
                                Some(crate::attention::Verdict::Hold(why)) => {
                                    eprintln!("lamp-voice provider: request {request}: its answer is held back: {why}{rise}");
                                }
                                Some(_) if attention.played_for_near() => {
                                    eprintln!("lamp-voice provider: request {request}: its answer is played: its name was heard close to the lamp{rise}");
                                }
                                Some(_) if attention.played_for_its_name() => {
                                    eprintln!("lamp-voice provider: request {request}: its answer is played: the lamp's name was heard in the room's sound{rise}");
                                }
                                // Every other answer let out says why too (the
                                // first builder, #829, 17:21: an answer to the
                                // office played with no verdict in the journal).
                                Some(crate::attention::Verdict::Play(why)) => {
                                    eprintln!("lamp-voice provider: request {request}: its answer is played: {why}{rise}");
                                }
                                None => {}
                            }
                        }
                        _ => {}
                    }
                }
                match notice {
                    Ok(Notice::Ready {..} | Notice::Unavailable {..}) if coordinated => {
                        return Err(io::Error::other("availability escaped the provider control plane").into());
                    },
                    Ok(Notice::Ready {session,context,attempts,elapsed,after})=>{
                        let elapsed_us = elapsed.as_micros().try_into().unwrap_or(u64::MAX);
                        match (after, typed) {
                            (Some(error), true)=>urgent.push_back(Outbound::Status(ProviderStatus::Recovered {
                                outage_us: elapsed_us,
                                attempts,
                                context: match context {
                                    Context::Resumed | Context::Initial => RecoveredContext::Resumed,
                                    Context::ResumedBeforeLatest => RecoveredContext::ResumedBeforeLatest,
                                    Context::Fresh => RecoveredContext::Fresh,
                                },
                                reason: error.into(),
                            })),
                            (after, _)=>{
                                if let Some(error)=after {
                                    // The current contract carries a recovery only as
                                    // a repeated Ready. Fixed categories, no server text.
                                    eprintln!("lamp-voice provider: recovered after {error}; session={} context={context:?} attempts={attempts} outage_ms={}", session.get(), elapsed.as_millis());
                                }
                                output.push_back(Outbound::Output(ProviderOutput::Ready { setup_us: elapsed_us }));
                            },
                        }
                    },
                    Ok(Notice::Event(Event::BarrierAssumed {superseded,successor,..})) if typed=>output.push_back(Outbound::Status(ProviderStatus::OwnershipAssumed {
                        request: successor.map(RequestId::get),
                        superseded: superseded.map(RequestId::get),
                    })),
                    Ok(Notice::Event(Event::ToolCall { lineage, call })) => {
                        let held = attention.holds(lineage.request.get());
                        // The lamp's own work, for a question about it
                        // later (#829).
                        if !held
                            && (crate::agent_tasks::is_tool(&call.name)
                                || crate::tools::is_schedule_tool(&call.name))
                        {
                            attention.worked(monotonic_us());
                        }
                        // The turn, for the Policy's decision about it (#800):
                        // none for a turn the lamp wrote itself. Its name is
                        // known here with the rule for whom it answers, which
                        // follows the turn; without it, not.
                        let request = lineage.request.get();
                        let turn = (crate::attention::own_request()
                            .load(std::sync::atomic::Ordering::Acquire)
                            != request)
                            .then(|| Turn {
                                name_heard: attention.in_hand().is_some_and(|turn| {
                                    turn.request == request
                                        && turn.name_score >= crate::policy_holds::NAME_FROM
                                }),
                                after_unanswered: attention.follows_an_unanswered_line(request),
                                lamp_spoke_lately: lamp_spoke_us.is_some_and(|at| {
                                    monotonic_us().saturating_sub(at)
                                        <= crate::policy_holds::IN_CONVERSATION_US
                                }),
                            });
                        desk.called(&provider, lineage, call, held, turn)
                    }
                    // Calls the service withdrew are not run if they still
                    // wait for the Policy's decision (#800).
                    Ok(Notice::Event(event @ Event::ToolCallsCancelled { .. })) => {
                        if let Event::ToolCallsCancelled { session, ids } = &event {
                            desk.withdrawn(*session, ids);
                        }
                        forward(event, &mut output, &mut output_sequence, &mut discards)?
                    }
                    // The model said this request was not for it: none of
                    // its sound is let out, and none of its words are kept
                    // as something the lamp said.
                    Ok(Notice::Event(Event::Audio { lineage, .. } | Event::OutputTranscript { lineage, .. }))
                        if desk.declined(lineage.request)
                            || desk.quieted(lineage.request)
                            || attention.holds(lineage.request.get()) => {},
                    Ok(Notice::Event(event)) if reminder_origin=>forward_reminder(event, &mut output, &mut output_sequence, &mut discards)?,
                    Ok(Notice::Event(event))=>forward(event, &mut output, &mut output_sequence, &mut discards)?,
                    Ok(Notice::Unavailable {error})=>{
                        if typed {
                            urgent.push_back(Outbound::Status(ProviderStatus::Unavailable { reason: error.into() }));
                        } else {
                            eprintln!("lamp-voice provider: connection lost ({error}); output already received is still delivered");
                        }
                    },
                    Ok(Notice::Settled {lineage,error})=>{
                        // Generation completed and every block was forwarded.
                        // The closed connection is this request's idle barrier.
                        eprintln!("lamp-voice provider: request {} settled by {error} after generation completed", lineage.request.get());
                        crate::remember::finished(lineage.request.get());
                        output.push_back(Outbound::Output(ProviderOutput::TurnComplete {request:lineage.request.get(),idle:true}));
                    },
                    Ok(Notice::TurnLost {lineage,stage,error})=>{
                        let request = lineage.request.get();
                        if !typed {
                            return Err(io::Error::other(format!("{}; request {request} was lost at {stage:?} and is not retried", connection_ended(State::Disconnected(error)))).into());
                        }
                        // Later input for it is swallowed, not sent to a new session.
                        intake.handoff.fail(request);
                        // A newer refused Start must not suppress this older
                        // accepted request's ordered terminal outcome. Refused
                        // Starts are never tracked by the supervisor, so these
                        // two outcome sources are disjoint.
                        output.push_back(Outbound::Status(ProviderStatus::TurnFailed { request, stage: stage.into(), reason: error.into() }));
                    },
                    Err(failure) if coordinated => {
                        // Stop polling the terminal future. The process remains
                        // alive to drain retained output, refuse later input
                        // exactly once, and service privacy and explicit Stop.
                        output.push_back(Outbound::Output(ProviderOutput::Ended {
                            reason: failure.reason.into(), fault: failure.error.into(), attempts: failure.attempts,
                        }));
                        upstream_ended = true;
                    },
                    Err(failure)=>return Err(ended(failure).into()),
                }
                if output.len()>MAX_QUEUED_OUTPUT { return Err(io::Error::other("provider IPC output backlog exceeded bound").into()); }
            }
        }
    }
}

/// The reminder tool veto precedes attention's own-request exemption and all
/// tool execution. Room classification never withholds requested due speech.
fn reminder_notice<C: Connect>(
    handoff: &InputHandoff,
    notice: &Notice,
    provider: &Supervisor<C>,
) -> bool {
    let Some(lineage) = reminder_policy::lineage(notice) else {
        return false;
    };
    let request = lineage.request.get();
    if handoff.reminders.suppresses(request) {
        return true;
    }
    if !handoff.reminders.is_reminder(lineage) {
        return false;
    }
    if let Notice::Event(Event::ToolCall { call, .. }) = notice {
        // Answer before any local retirement. The reply is a refusal only;
        // it cannot invoke Actions, memory, or local name/decline behavior.
        let _ = provider.try_tool_response(
            lineage.session,
            &call.id,
            &call.name,
            &serde_json::json!({"error":"A reminder cannot use tools. Say only the reminder."}),
        );
        true
    } else {
        false
    }
}

/// Where the model's function calls are handed on and their answers come
/// back ([`crate::tools`]). Every call is answered: the service generates
/// nothing more for a request until its calls are.
struct ToolDesk {
    settings: crate::tools::Settings,
    /// The newest request the model said was not for it.
    declined: Option<u64>,
    /// The newest request in which the executive was asked to do
    /// something. Doing what was asked and then saying it was not for the
    /// lamp cannot both be right, and what was done cannot be undone: the
    /// request is taken as the lamp's.
    acted: Option<u64>,
    asked: std::sync::mpsc::Sender<Asked>,
    answers: std::sync::mpsc::Receiver<Asked>,
    /// Calls the executive or the memory has not answered yet.
    waiting: usize,
    /// The model is asked to say when it hears the lamp's name.
    name_signal: bool,
    /// The newest request in which it said so, and when.
    named: Option<(u64, u64)>,
    /// The newest request an answer's sound came for, and when it began.
    sounded: Option<(u64, u64)>,
    /// Each call answered since the worker last took them, for the
    /// first-output record (#799).
    lines: Vec<crate::first_output::ToolCallLine>,
    /// The Policy's decisions, when the lamp's actions wait for them
    /// ([`crate::policy_holds`], #800).
    verdicts: Option<crate::policy_holds::Verdicts>,
    /// The lamp's actions waiting for the turn's decision.
    parked: Vec<Parked>,
    /// The newest request one of whose actions the Policy held: none of its
    /// sound is let out.
    quieted: Option<u64>,
    /// Requests whose skill call was answered with `"look": true` since the
    /// worker last took them, with the call's session and why the look is
    /// dropped, if it is: the coordinator takes their look (#941).
    looks: Vec<(u64, u64, Option<String>)>,
}
/// The longest a skill tool's look is owed after its answer: it begins
/// within `LOOK_BEGINS_WITHIN` and waits `STILL_WAIT` for its still.
const LOOK_OWED_AT_MOST: Duration =
    crate::sight::LOOK_BEGINS_WITHIN.saturating_add(crate::sight::STILL_WAIT);

/// Whether a skill tool's look is still owed, so no session is renewed.
/// ponytail: held to the bound, not cleared as the look's turn begins.
fn look_owed(until: Option<Instant>) -> bool {
    until.is_some_and(|until| Instant::now() < until)
}

/// The turn a call came in, someone's, not the lamp's own.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Turn {
    /// The lamp's name was heard in it ([`crate::policy_holds::NAME_FROM`]).
    name_heard: bool,
    /// It follows a line someone said that the lamp left unanswered, not
    /// the lamp's own last words (#1054).
    after_unanswered: bool,
    /// The lamp's sound went out within
    /// [`crate::policy_holds::IN_CONVERSATION_US`] before it: without that
    /// it follows no line of the lamp's (the reviewer on #1054).
    lamp_spoke_lately: bool,
}
/// One of the lamp's actions kept for the turn's decision.
struct Parked {
    lineage: lamp_gemini::Lineage,
    call: lamp_gemini::ToolCall,
    turn: Turn,
    called_us: u64,
}
/// Calls kept for the first-output record between two ticks at most.
const MOST_CALL_LINES: usize = 64;
/// A call the executive or the memory was asked, with what to tell the
/// model.
struct Asked {
    session: SessionId,
    request: u64,
    call: lamp_gemini::ToolCall,
    told: serde_json::Value,
    /// Whom it went to, for the journal.
    whom: &'static str,
    /// Whether it went to the skills (`skills/`): only their answer can ask
    /// for a look (#941).
    skill: bool,
    /// Whether someone was heard in its request, not a turn the lamp wrote
    /// itself (a hello, a look's own turn): only then can it ask for a look.
    heard: bool,
    answered: bool,
    took: Duration,
    /// When the call reached the worker.
    called_us: u64,
}
impl ToolDesk {
    fn new(settings: crate::tools::Settings) -> Self {
        let (asked, answers) = std::sync::mpsc::channel();
        Self {
            settings,
            declined: None,
            acted: None,
            asked,
            answers,
            waiting: 0,
            name_signal: false,
            named: None,
            sounded: None,
            lines: Vec::new(),
            verdicts: None,
            parked: Vec::new(),
            quieted: None,
            looks: Vec::new(),
        }
    }
    /// The first sound of an answer to `request` has come. Said once for a
    /// request, with how long before it the model said it heard the name,
    /// if it did: the count that decides whether its sign can be used.
    fn sound(&mut self, request: u64) {
        if !self.name_signal || self.sounded.is_some_and(|(sounded, _)| sounded == request) {
            return;
        }
        let now = monotonic_us();
        self.sounded = Some((request, now));
        match self.named {
            Some((named, at)) if named == request => eprintln!(
                "lamp-voice provider: request {request}: first sound {} ms after the model said it heard its name",
                now.saturating_sub(at) / 1_000
            ),
            _ => eprintln!(
                "lamp-voice provider: request {request}: first sound, and the model has not said it heard its name"
            ),
        }
    }
    fn declined(&self, request: RequestId) -> bool {
        self.declined == Some(request.get())
    }
    /// Whether the Policy held one of `request`'s actions (#800).
    fn quieted(&self, request: RequestId) -> bool {
        self.quieted == Some(request.get())
    }
    /// The service withdrew calls: those still waiting for the Policy's
    /// decision are let go, neither run nor answered (the reviewer on #908).
    fn withdrawn(&mut self, session: SessionId, ids: &[String]) {
        self.parked.retain(|parked| {
            let gone = parked.lineage.session == session && ids.contains(&parked.call.id);
            if gone {
                eprintln!(
                    "lamp-voice provider: request {} asked for {}; withdrawn by the service while it waited for the Policy's decision; not run",
                    parked.lineage.request.get(),
                    parked.call.name
                );
            }
            !gone
        });
    }
    /// One call from the model. Names only are logged, never arguments.
    fn called<C: Connect>(
        &mut self,
        provider: &Supervisor<C>,
        lineage: lamp_gemini::Lineage,
        call: lamp_gemini::ToolCall,
        held: bool,
        turn: Option<Turn>,
    ) {
        use crate::tools::Route;
        let request = lineage.request.get();
        let called_us = monotonic_us();
        // Sent, and when: timed as it is sent, before any journal line.
        let tell = |told: &serde_json::Value| {
            let sent = provider
                .try_tool_response(lineage.session, &call.id, &call.name, told)
                .is_ok();
            (sent, monotonic_us())
        };
        // A request whose answer is held back has nothing done for it
        // either: the call is answered, and not run.
        if held
            && matches!(
                crate::tools::route(&self.settings, &call.name),
                Route::Executive(_)
                    | Route::Schedule(_)
                    | Route::Memory(_)
                    | Route::Tasks(_)
                    | Route::Faces(..)
                    | Route::Mark(_)
                    | Route::Onboarding(_)
            )
        {
            let (sent, sent_us) = tell(&crate::tools::declined());
            self.line(
                lineage.session,
                request,
                &call.name,
                called_us,
                sent_us,
                true,
                sent,
            );
            eprintln!(
                "lamp-voice provider: request {request} asked for {}; not done: its answer is held back",
                call.name
            );
            return;
        }
        if self.name_signal && call.name == crate::attention::HEARD_MY_NAME {
            let (sent, sent_us) = tell(&crate::tools::declined());
            self.line(
                lineage.session,
                request,
                &call.name,
                called_us,
                sent_us,
                true,
                sent,
            );
            let now = monotonic_us();
            self.named = Some((request, now));
            match self.sounded {
                Some((sounded, at)) if sounded == request => eprintln!(
                    "lamp-voice provider: request {request}: the model said it heard its name {} ms after its first sound",
                    now.saturating_sub(at) / 1_000
                ),
                _ => eprintln!(
                    "lamp-voice provider: request {request}: the model said it heard its name"
                ),
            }
            return;
        }
        // One of the lamp's actions, with the Policy holding them: kept for
        // the turn's decision, and run, held or let go on a later tick
        // ([`Self::release`], #800). Timers and reminders too: "Yes, give me
        // five minutes", said to someone else, set a timer on lamp-4ace (the
        // first builder on #908, 05:02).
        if let Some(turn) = turn
            && self.verdicts.is_some()
            && crate::policy_holds::holds(
                &crate::tools::route(&self.settings, &call.name),
                &call.name,
            )
            && self.parked.len() < lamp_gemini::MAX_TOOL_CALLS
        {
            eprintln!(
                "lamp-voice provider: request {request} asked for {}; waiting for the Policy's decision",
                call.name
            );
            self.parked.push(Parked {
                lineage,
                call,
                turn,
                called_us,
            });
            return;
        }
        match crate::tools::route(&self.settings, &call.name) {
            Route::Clock => {
                let told = crate::tools::time_now();
                let (sent, sent_us) = tell(&told);
                self.line(
                    lineage.session,
                    request,
                    &call.name,
                    called_us,
                    sent_us,
                    true,
                    sent,
                );
                eprintln!(
                    "lamp-voice provider: request {request}: the model asked the time; told {told}"
                );
            }
            Route::Decline if self.acted == Some(request) => {
                // Every call is answered, this one too. What the model
                // says next is not dropped.
                let (sent, sent_us) = tell(&crate::tools::declined());
                self.line(
                    lineage.session,
                    request,
                    &call.name,
                    called_us,
                    sent_us,
                    true,
                    sent,
                );
                eprintln!(
                    "lamp-voice provider: request {request}: the model said it was not for it after doing what was asked; not taken"
                );
            }
            Route::Decline => {
                self.declined = Some(request);
                let (sent, sent_us) = tell(&crate::tools::declined());
                self.line(
                    lineage.session,
                    request,
                    &call.name,
                    called_us,
                    sent_us,
                    true,
                    sent,
                );
                eprintln!(
                    "lamp-voice provider: request {request}: the model said it was not for it"
                );
            }
            route @ (Route::Executive(_)
            | Route::Schedule(_)
            | Route::Memory(_)
            | Route::Tasks(_)
            | Route::Faces(..)
            | Route::Mark(_)
            | Route::Onboarding(_))
                if self.waiting < lamp_gemini::MAX_TOOL_CALLS =>
            {
                self.run(provider, lineage, call, route, called_us);
            }
            Route::Executive(_)
            | Route::Schedule(_)
            | Route::Memory(_)
            | Route::Tasks(_)
            | Route::Faces(..)
            | Route::Mark(_)
            | Route::Onboarding(_)
            | Route::Nobody => {
                let (sent, sent_us) = tell(&crate::tools::unavailable());
                self.line(
                    lineage.session,
                    request,
                    &call.name,
                    called_us,
                    sent_us,
                    false,
                    sent,
                );
                eprintln!(
                    "lamp-voice provider: request {request} asked for {}, which nobody here runs; told so",
                    call.name
                );
            }
        }
    }
    /// Run one call on a thread of its own, its answer taken on a later
    /// tick ([`Self::answer`]).
    fn run<C: Connect>(
        &mut self,
        provider: &Supervisor<C>,
        lineage: lamp_gemini::Lineage,
        call: lamp_gemini::ToolCall,
        route: crate::tools::Route,
        called_us: u64,
    ) {
        use crate::tools::Route;
        let request = lineage.request.get();
        // Being asked to keep or forget something is not itself
        // something to keep.
        if crate::tools::keeps_out(&route, &call.name) {
            crate::remember::keep_out(request);
        }
        if matches!(
            route,
            Route::Executive(_)
                | Route::Schedule(_)
                | Route::Tasks(_)
                | Route::Faces(..)
                | Route::Mark(_)
        ) {
            self.acted = Some(request);
            // Something done is the lamp answering, words or none (#829,
            // the sixth on #1029). A lookup, an action the Policy held and
            // a held request never come here (the reviewer on #1069).
            if let Ok(mut room) = crate::room_told::room().lock() {
                room.answered_by_a_call(request, monotonic_us());
            }
        }
        // A request the lamp wrote itself was not heard from anyone:
        // a yes to a task cannot come in one.
        let heard =
            crate::attention::own_request().load(std::sync::atomic::Ordering::Acquire) != request;
        let skill =
            matches!(&route, Route::Executive(dir) if dir.ends_with(crate::tools::SKILLS_DIR));
        let asked = self.asked.clone();
        let session = lineage.session;
        let work = call.clone();
        let spawned = std::thread::Builder::new()
            .name("lamp-voice-ask".into())
            .spawn(move || {
                let started = Instant::now();
                let (whom, (told, answered)) =
                    crate::tools::run_call(&route, &work, request, heard);
                let _ = asked.send(Asked {
                    session,
                    request,
                    call: work,
                    told,
                    whom,
                    skill,
                    heard,
                    answered,
                    took: started.elapsed(),
                    called_us,
                });
            });
        match spawned {
            Ok(_) => self.waiting += 1,
            Err(_) => {
                let sent = provider
                    .try_tool_response(
                        lineage.session,
                        &call.id,
                        &call.name,
                        &crate::tools::unavailable(),
                    )
                    .is_ok();
                self.line(
                    lineage.session,
                    request,
                    &call.name,
                    called_us,
                    monotonic_us(),
                    false,
                    sent,
                );
            }
        }
    }
    /// The lamp's actions kept for the Policy's decision (#800), looked at
    /// once a tick: each run, held, or kept a while longer. The decisions
    /// are taken every tick, whether or not an action waits; each is its
    /// own turn's by number, whenever it comes.
    /// A parked call is let go, neither run nor answered, when its turn was
    /// `retired` (its owner gone: cancelled, or a newer turn) or its session
    /// is no longer the live one, looked at right before it would run (the
    /// reviewer on #908). So is one the service withdrew, asked of the
    /// session itself: its cancellation may still wait behind sound while
    /// the output is full, and no provider notice is taken then (the
    /// reviewer on #908, 05:11).
    fn release<C: Connect>(&mut self, provider: &Supervisor<C>, retired: impl Fn(u64) -> bool) {
        let now = monotonic_us();
        let Some(verdicts) = self.verdicts.as_mut() else {
            return;
        };
        verdicts.take();
        for parked in std::mem::take(&mut self.parked) {
            let request = parked.lineage.request.get();
            if provider.live_session() == Some(parked.lineage.session)
                && provider.call_withdrawn(parked.lineage.session, &parked.call.id)
            {
                eprintln!(
                    "lamp-voice provider: request {request} asked for {}; withdrawn by the service while it waited for the Policy's decision; not run",
                    parked.call.name
                );
                continue;
            }
            if retired(request) || provider.live_session() != Some(parked.lineage.session) {
                // Answered all the same, as every call is, so that the
                // service is not left waiting for it (refused, harmlessly,
                // if its session has gone); never run.
                let _ = provider.try_tool_response(
                    parked.lineage.session,
                    &parked.call.id,
                    &parked.call.name,
                    &serde_json::json!({"done": false}),
                );
                eprintln!(
                    "lamp-voice provider: request {request} asked for {}; its turn ended while it waited for the Policy's decision; not run",
                    parked.call.name
                );
                continue;
            }
            let verdict = self
                .verdicts
                .as_ref()
                .and_then(|verdicts| verdicts.verdict(request))
                // A turn after someone else's line, left unanswered, follows
                // that line, not the lamp's, whatever the model answered
                // (#1054, the quiet-room case on #1090).
                // Nor does one when the lamp has not spoken lately: out of
                // conversation it follows no line of the lamp's (the reviewer
                // on #1054).
                .map(|verdict| match verdict {
                    crate::policy_holds::Verdict::StayQuietFollowing
                        if parked.turn.after_unanswered || !parked.turn.lamp_spoke_lately =>
                    {
                        crate::policy_holds::Verdict::StayQuiet
                    }
                    verdict => verdict,
                });
            let waited = Duration::from_micros(now.saturating_sub(parked.called_us));
            let sets = crate::policy_holds::sets(
                &crate::tools::route(&self.settings, &parked.call.name),
                &parked.call.name,
            );
            match crate::policy_holds::gate(verdict, parked.turn.name_heard, waited, sets) {
                crate::policy_holds::Gate::Wait => self.parked.push(parked),
                crate::policy_holds::Gate::Run => {
                    let why = match verdict {
                        _ if parked.turn.name_heard => "its name was heard",
                        Some(crate::policy_holds::Verdict::StayQuietFollowing) => {
                            "the turn follows the lamp's last line, and what it sets on the schedule is not held from it"
                        }
                        Some(_) => "the Policy took the turn as the lamp's",
                        None => "no decision came in time",
                    };
                    eprintln!(
                        "lamp-voice provider: request {request} asked for {}; run after {} ms: {why}",
                        parked.call.name,
                        waited.as_millis()
                    );
                    let route = crate::tools::route(&self.settings, &parked.call.name);
                    if self.waiting < lamp_gemini::MAX_TOOL_CALLS {
                        self.run(
                            provider,
                            parked.lineage,
                            parked.call,
                            route,
                            parked.called_us,
                        );
                    } else {
                        let sent = provider
                            .try_tool_response(
                                parked.lineage.session,
                                &parked.call.id,
                                &parked.call.name,
                                &crate::tools::unavailable(),
                            )
                            .is_ok();
                        self.line(
                            parked.lineage.session,
                            request,
                            &parked.call.name,
                            parked.called_us,
                            monotonic_us(),
                            false,
                            sent,
                        );
                    }
                }
                crate::policy_holds::Gate::Hold => {
                    let sent = provider
                        .try_tool_response(
                            parked.lineage.session,
                            &parked.call.id,
                            &parked.call.name,
                            &crate::policy_holds::held(),
                        )
                        .is_ok();
                    self.quieted = Some(request);
                    self.line(
                        parked.lineage.session,
                        request,
                        &parked.call.name,
                        parked.called_us,
                        monotonic_us(),
                        true,
                        sent,
                    );
                    eprintln!(
                        "lamp-voice provider: request {request} asked for {}; not done: the Policy judged the turn not for the lamp, and its name was not heard ({} ms)",
                        parked.call.name,
                        waited.as_millis()
                    );
                }
            }
        }
    }
    /// Hand back what the executive answered since the last tick. An answer
    /// for a connection that has gone is refused by the supervisor: the
    /// call went with it.
    fn answer<C: Connect>(&mut self, provider: &Supervisor<C>) {
        self.answer_with(
            |done| {
                provider
                    .try_tool_response(done.session, &done.call.id, &done.call.name, &done.told)
                    .is_ok()
            },
            |done, taken| {
                eprintln!(
                "lamp-voice provider: request {} asked for {}; {} {} in {} ms; the answer was {}",
                done.request,
                done.call.name,
                done.whom,
                if done.answered {
                    "answered"
                } else {
                    "did not answer"
                },
                done.took.as_millis(),
                    if taken {
                        "sent"
                    } else {
                        "not sent: its connection has gone"
                    }
                );
            },
        );
    }
    /// As [`Self::answer`], with `send` handing an answer to the connection
    /// and `log` writing its journal line. The time it was sent is taken
    /// as it is sent, before the line: a journal that is slow to take it
    /// is not the call's time (#810 review, 17:23).
    fn answer_with(
        &mut self,
        mut send: impl FnMut(&Asked) -> bool,
        mut log: impl FnMut(&Asked, bool),
    ) {
        while let Ok(done) = self.answers.try_recv() {
            self.waiting = self.waiting.saturating_sub(1);
            let taken = send(&done);
            let sent_us = monotonic_us();
            // The look is the voice's, not the model's to remember (#941),
            // and only a skill's answer asks for it: no other part makes a
            // picture leave the lamp.
            if done.skill
                && done.told["look"] == true
                && !self
                    .looks
                    .iter()
                    .any(|(request, ..)| *request == done.request)
            {
                let dropped = if !done.heard {
                    Some("the lamp's own turn".to_owned())
                } else if !taken {
                    Some("the answer never reached the model".to_owned())
                } else {
                    None
                };
                self.looks.push((done.request, done.session.get(), dropped));
            }
            self.line(
                done.session,
                done.request,
                &done.call.name,
                done.called_us,
                sent_us,
                done.answered,
                taken,
            );
            log(&done, taken);
        }
    }
    /// One call answered, kept for the first-output record.
    #[allow(clippy::too_many_arguments)]
    fn line(
        &mut self,
        session: SessionId,
        request: u64,
        name: &str,
        called_us: u64,
        sent_us: u64,
        answered: bool,
        sent: bool,
    ) {
        if self.lines.len() < MOST_CALL_LINES {
            self.lines.push(crate::first_output::ToolCallLine {
                session: session.get(),
                request,
                name: name.to_owned(),
                called_us,
                sent_us,
                answered,
                sent,
            });
        }
    }
    /// The calls answered since the last take.
    fn take_lines(&mut self) -> Vec<crate::first_output::ToolCallLine> {
        std::mem::take(&mut self.lines)
    }
    /// The requests whose look was asked in a call's answer since the last take.
    fn take_looks(&mut self) -> Vec<(u64, u64, Option<String>)> {
        std::mem::take(&mut self.looks)
    }
}

/// Control upkeep has no PCM queue or credit argument. The callback must be a
/// nonblocking transport send; WouldBlock preserves one original observation.
fn service_link<C: Connect>(
    provider: &mut Supervisor<C>,
    clock: Option<&mut LinkClock>,
    pending: &mut PendingLink,
    send: impl FnMut(ProviderLink) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(clock) = clock {
        clock.tick()?;
        if let Some(update) = provider.poll_link() {
            pending.observe(clock.convert(update)?, monotonic_us())?;
        }
        pending.try_send(send)
    } else {
        provider.maintain();
        Ok(())
    }
}

/// Select one API for the entire worker lifetime. Both futures are cancellation
/// safe, so the control tick never waits for data, backoff or a hung connection.
async fn next_notice<C: Connect>(
    provider: &mut Supervisor<C>,
    coordinated: bool,
) -> std::result::Result<Notice, Failure> {
    if coordinated {
        provider.next_output().await.map(Notice::from)
    } else {
        provider.next().await
    }
}

fn forward(
    event: Event,
    output: &mut VecDeque<Outbound>,
    output_sequence: &mut u64,
    discards: &mut DiscardLog,
) -> Result<()> {
    forward_origin(event, output, output_sequence, discards, true)
}

fn forward_reminder(
    event: Event,
    output: &mut VecDeque<Outbound>,
    output_sequence: &mut u64,
    discards: &mut DiscardLog,
) -> Result<()> {
    // Nothing was heard when the lamp was asked to say a reminder. Its actual
    // audio still counts as speech, but a silent attempt is not room activity.
    forward_origin(event, output, output_sequence, discards, false)
}

fn forward_origin(
    event: Event,
    output: &mut VecDeque<Outbound>,
    output_sequence: &mut u64,
    discards: &mut DiscardLog,
    heard_input: bool,
) -> Result<()> {
    let mut push = |item: ProviderOutput| output.push_back(Outbound::Output(item));
    match event {
        Event::InputStarted {
            lineage,
            waiting_for_barrier,
        } => {
            discards.started();
            if heard_input && let Ok(mut room) = crate::room_told::room().lock() {
                room.heard(lineage.request.get(), monotonic_us());
            }
            push(ProviderOutput::Started {
                request: lineage.request.get(),
                session: lineage.session.get(),
                waiting_for_barrier,
            })
        }
        Event::Audio { lineage, pcm, .. } => {
            unanswered_requests().store(0, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut room) = crate::room_told::room().lock() {
                room.spoke(lineage.request.get(), monotonic_us());
            }
            let provider_event_at_us = monotonic_us();
            let source_frames = pcm.len();
            for (index, part) in pcm.chunks(PACKET_SAMPLES).enumerate() {
                *output_sequence = output_sequence
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("provider output counter exhausted"))?;
                push(ProviderOutput::Audio {
                    request: lineage.request.get(),
                    sequence: *output_sequence,
                    provider_event_at_us,
                    source_frames,
                    source_offset: index * PACKET_SAMPLES,
                    samples: part.to_vec(),
                });
            }
        }
        Event::OutputTranscript {
            lineage,
            text,
            finished,
        } => {
            if let Ok(mut thread) = crate::thread::THREAD.lock() {
                thread.lamp(lineage.request.get(), &text, std::time::Instant::now());
            }
            push_text(&mut push, Some(lineage.request.get()), text, finished)
        }
        Event::UncorrelatedInputTranscript { text, finished, .. } => {
            if let Ok(mut thread) = crate::thread::THREAD.lock() {
                thread.person(&text, finished, std::time::Instant::now());
            }
            push_text(&mut push, None, text, finished);
        }
        Event::ModelText { .. } | Event::VoiceActivity { .. } => {}
        Event::GenerationComplete { lineage } => push(ProviderOutput::GenerationComplete {
            request: lineage.request.get(),
        }),
        Event::Interrupted { lineage } => push(ProviderOutput::Interrupted {
            request: lineage.request.get(),
        }),
        Event::TurnComplete { lineage, idle } => {
            if idle {
                crate::remember::finished(lineage.request.get());
            }
            push(ProviderOutput::TurnComplete {
                request: lineage.request.get(),
                idle,
            })
        }
        // The service chose not to answer. For the coordinator the request is
        // complete and had no audio; the connection and conversation stay.
        Event::Unanswered { lineage, waited } => {
            discards.closed_unanswered(lineage.request.get(), monotonic_us());
            // A sound sent on only for the model to judge, left alone, is
            // not a request the session failed to answer, and is not talk.
            // A reminder attempt has no heard input at all. It must neither
            // change the room nor renew the session as unanswered speech.
            // What it was is the room's own answer, so the line says what
            // was counted (the reviewer on #1291).
            let left = heard_input.then(|| {
                crate::room_told::room()
                    .lock()
                    .map_or(crate::room_told::Left::Talk, |mut room| {
                        room.left_unanswered(lineage.request.get(), monotonic_us())
                    })
            });
            let said = left == Some(crate::room_told::Left::Talk);
            if left == Some(crate::room_told::Left::OverTheLamp) {
                eprintln!(
                    "lamp-voice provider: request {}, said over the lamp's reply, got no answer in {} ms; closed, connection kept, and not counted toward a busy room",
                    lineage.request.get(),
                    waited.as_millis()
                );
            } else if said {
                unanswered_requests().fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                eprintln!(
                    "lamp-voice provider: request {} got no answer in {} ms; closed as unanswered, connection kept",
                    lineage.request.get(),
                    waited.as_millis()
                );
            } else if heard_input {
                eprintln!(
                    "lamp-voice provider: a sound (request {}) got no answer in {} ms, as a sound should; closed, connection kept",
                    lineage.request.get(),
                    waited.as_millis()
                );
            } else {
                eprintln!(
                    "lamp-voice provider: reminder request {} got no answer in {} ms; closed, connection kept",
                    lineage.request.get(),
                    waited.as_millis()
                );
            }
            push(ProviderOutput::TurnComplete {
                request: lineage.request.get(),
                idle: true,
            });
        }
        // Observations for the journal: what a declined request looks like
        // on the wire, and what each exchange costs. Counts and times only.
        Event::WaitingForInput { awaiting, .. } => eprintln!(
            "lamp-voice provider: service waiting for input; unanswered request={:?} since_commit_ms={:?}",
            awaiting.map(|(lineage, _)| lineage.request.get()),
            awaiting.map(|(_, since)| since.as_millis())
        ),
        Event::Refused { error, shape, .. } => {
            eprintln!("lamp-voice provider: refused a server message ({error:?}); fields: {shape}");
        }
        Event::Usage { usage, .. } => eprintln!(
            "lamp-voice provider: usage total={:?} prompt={:?} prompt_audio={:?} response={:?} response_audio={:?} thoughts={:?} cached={:?}{}",
            usage.total,
            usage.prompt,
            usage.prompt_audio,
            usage.response,
            usage.response_audio,
            usage.thoughts,
            usage.cached,
            discards.usage_after(monotonic_us())
        ),
        Event::GoAway { time_left, .. } => eprintln!(
            "lamp-voice provider: service announced a close; time_left_ms={:?}",
            time_left.map(|left| left.as_millis())
        ),
        Event::Discarded { reason, .. } => discards.note(reason, monotonic_us()),
        Event::InputDiscarded {
            through,
            dropped_samples,
            sent_samples,
            ..
        } => {
            eprintln!(
                "lamp-voice provider: input through request {} abandoned: dropped_unsent_samples={dropped_samples} already_sent_samples={sent_samples}",
                through.get()
            );
            push(ProviderOutput::InputDiscarded {
                request: through.get(),
                dropped_samples,
                sent_samples,
            });
        }
        Event::BarrierAssumed {
            superseded,
            successor,
            ..
        } => eprintln!(
            "lamp-voice provider: no confirmation that request {:?} stopped; request {:?} proceeds on that assumption (unqualified ownership)",
            superseded.map(RequestId::get),
            successor.map(RequestId::get)
        ),
        // Answered where the provider is in reach, before this function.
        Event::ToolCall { lineage, .. } => eprintln!(
            "lamp-voice provider: a function call for request {} reached the forwarder unanswered",
            lineage.request.get()
        ),
        Event::ToolCallsCancelled { ids, .. } => eprintln!(
            "lamp-voice provider: the service withdrew {} function call(s)",
            ids.len()
        ),
        // Only a session configured to report its barriers sends these:
        // `Continuation::KeptStart` configures one. Counts and times, no
        // audio and no text.
        Event::Barrier {
            outcome, report, ..
        } => eprintln!(
            "lamp-voice provider: barrier {outcome:?}: superseded={:?} successor={:?} waited_ms={} quiet_ms={} successor_input_open={} end_withheld_ms={:?} withheld audio_frames={} text_frames={} generation_complete={} interrupted={} completions_not_idle={} start_held_ms={:?} start_released_by={:?} retained_samples={} waiting_for_input={} resumable_updates={} unresumable_updates={}",
            report.superseded.map(RequestId::get),
            report.successor.map(RequestId::get),
            report.waited.as_millis(),
            report.quiet.as_millis(),
            report.successor_input_open,
            report.end_withheld.map(|held| held.as_millis()),
            report.audio_frames,
            report.text_frames,
            report.generation_complete,
            report.interrupted,
            report.completions_not_idle,
            report.start_held.map(|held| held.as_millis()),
            report.start_released_by,
            report.retained_samples,
            report.waiting_for_input,
            report.resumable_updates,
            report.unresumable_updates
        ),
    }
    Ok(())
}

/// How long after a request is closed as unanswered output dropped, and
/// the service's usage counts, are said to come after it.
const AFTER_UNANSWERED_US: u64 = 30_000_000;

/// Late provider events never reach the coordinator. Each kind is reported
/// once per worker so a recurring pattern is visible without unbounded output.
/// Output dropped after a request was closed as unanswered is said for that
/// request, once, with how long after the close it came: on lamp-4ace on
/// 17 October at 08:58 usage counts with response audio came two lines
/// after request 39's close at 4,008 ms, and nothing tied them to it (#829,
/// the first builder at 10:02).
#[derive(Default)]
struct DiscardLog {
    seen: [bool; 6],
    /// The newest request closed as unanswered, and when, until the next
    /// request starts.
    unanswered: Option<(u64, u64)>,
    /// The request whose dropped output was last said.
    late_said: Option<u64>,
}
impl DiscardLog {
    fn closed_unanswered(&mut self, request: u64, at_us: u64) {
        self.unanswered = Some((request, at_us));
    }
    fn started(&mut self) {
        self.unanswered = None;
    }
    /// The request closed as unanswered that what comes now follows, and
    /// how many ms after its close.
    fn after_unanswered(&self, now_us: u64) -> Option<(u64, u64)> {
        self.unanswered
            .map(|(request, at_us)| (request, now_us.saturating_sub(at_us)))
            .filter(|(_, since_us)| *since_us <= AFTER_UNANSWERED_US)
            .map(|(request, since_us)| (request, since_us / 1_000))
    }
    /// What a usage line adds: the request closed as unanswered it follows.
    fn usage_after(&self, now_us: u64) -> String {
        self.after_unanswered(now_us)
            .map_or_else(String::new, |(request, ms)| {
                format!(" after request {request} was closed as unanswered {ms} ms before")
            })
    }
    /// The line said for a dropped event after a request was closed as
    /// unanswered, once a request. It says only that the service sent
    /// something: a late silent turn end is dropped the same way as a late
    /// answer, and the usage line's `response_audio` beside it tells which
    /// (the reviewer on #1284).
    fn late_line(&mut self, reason: Discard, now_us: u64) -> Option<String> {
        if !matches!(
            reason,
            Discard::UnownedOutput | Discard::LateOutput | Discard::LateTerminal
        ) {
            return None;
        }
        let (request, ms) = self.after_unanswered(now_us)?;
        if self.late_said == Some(request) {
            return None;
        }
        self.late_said = Some(request);
        Some(format!(
            "lamp-voice provider: request {request}: the service sent something {ms} ms after it was closed as unanswered, and it was dropped ({reason:?})"
        ))
    }
    fn note(&mut self, reason: Discard, now_us: u64) {
        if let Some(line) = self.late_line(reason, now_us) {
            eprintln!("{line}");
        }
        let index = match reason {
            Discard::LateInterruption => 0,
            Discard::LateTerminal => 1,
            Discard::LateOutput => 2,
            Discard::UnownedOutput => 3,
            Discard::UnownedToolCall => 4,
            Discard::StaleToolResponse => 5,
        };
        if !std::mem::replace(&mut self.seen[index], true) {
            eprintln!(
                "lamp-voice provider: dropped a late provider event ({reason:?}); it changed no request"
            );
        }
    }
}

/// Terminal provider failure as one line of fixed categories.
fn ended(failure: Failure) -> io::Error {
    match failure.reason {
        FailureReason::Shutdown => connection_ended(State::Closed),
        FailureReason::Lifecycle => connection_ended(State::Ready),
        reason => io::Error::other(format!(
            "{}; {reason:?} after {} recovery attempt(s)",
            connection_ended(State::Disconnected(failure.error)),
            failure.attempts
        )),
    }
}

/// Gemini state contains only fixed error categories and an optional numeric
/// close code. Never substitute a raw close reason, server body, URL or header.
fn connection_ended(state: State) -> io::Error {
    match state {
        State::Disconnected(error) => {
            io::Error::other(format!("provider connection ended: {error}"))
        }
        State::Closed => io::Error::other("provider connection closed locally"),
        State::Ready => io::Error::other(
            "provider event stream closed without a terminal state: client lifecycle failure",
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlDrain {
    Empty,
    Deferred,
    Stopped,
}

struct RetainedInput {
    command: ProviderInput,
    received_at_us: u64,
    /// Held for room in Gemini's input queue, which bounds it by
    /// [`ROOM_WAIT`] instead of [`RETAINED_INPUT_US`].
    awaiting_room: bool,
}

/// Local process intake; this never grants authority or contacts the provider.
/// Keep at most one original command when the shared control budget is exhausted.
struct ProviderIntake {
    guard: BoundaryGuard,
    handoff: InputHandoff,
    output_flow: OutputSender,
    last_control_us: u64,
    saw_allowed: bool,
    retained: Option<RetainedInput>,
    stopped: bool,
    /// Snapshots skipped because they had expired while this worker was held
    /// up, since the last one it could take.
    skipped_expired: u32,
}
impl ProviderIntake {
    fn new(boot: BootId, now_us: u64) -> Self {
        Self {
            guard: BoundaryGuard::new(boot, MonoTime::from_micros(now_us)),
            handoff: InputHandoff::default(),
            output_flow: OutputSender::default(),
            last_control_us: now_us,
            saw_allowed: false,
            retained: None,
            stopped: false,
            skipped_expired: 0,
        }
    }

    fn stop(&mut self) {
        self.stopped = true;
        self.retained = None;
        self.guard.invalidate();
    }

    fn check_retained(&self, now_us: u64) -> Result<()> {
        if self.stopped {
            return Err(io::Error::other("provider intake stopped").into());
        }
        if self.retained.as_ref().is_some_and(|retained| {
            now_us
                .checked_sub(retained.received_at_us)
                .is_none_or(|age| {
                    age >= if retained.awaiting_room {
                        ROOM_WAIT.load(std::sync::atomic::Ordering::Relaxed) + RETAINED_INPUT_US
                    } else {
                        RETAINED_INPUT_US
                    }
                })
        }) {
            return Err(
                io::Error::other("provider retained input expired or clock regressed").into(),
            );
        }
        Ok(())
    }

    fn drain_control(
        &mut self,
        now: &mut impl FnMut() -> u64,
        receive: &mut impl FnMut() -> io::Result<Option<Control>>,
        remaining: &mut usize,
    ) -> Result<ControlDrain> {
        let result = self.drain_control_inner(now, receive, remaining);
        if result.is_err() {
            self.stop();
        }
        result
    }

    fn drain_control_inner(
        &mut self,
        now: &mut impl FnMut() -> u64,
        receive: &mut impl FnMut() -> io::Result<Option<Control>>,
        remaining: &mut usize,
    ) -> Result<ControlDrain> {
        self.check_retained(now())?;
        let mut empty = false;
        while *remaining > 0 {
            let Some(command) = receive()? else {
                empty = true;
                break;
            };
            *remaining -= 1;
            match command {
                Control::Stop => {
                    self.stop();
                    return Ok(ControlDrain::Stopped);
                }
                Control::Authority { snapshot } => {
                    let at = now();
                    let privacy_changed = self.guard.state().is_some_and(|old| {
                        old.microphone_generation() != snapshot.microphone_generation()
                    });
                    match self.guard.install(MonoTime::from_micros(at), snapshot) {
                        Ok(()) => {}
                        // Queued while this worker was held up (on lamp-4ace
                        // by the kernel re-tuning its SD/MMC hosts, #784):
                        // it lapsed in the queue. The coordinator has sent
                        // fresher ones behind it; wait for those, and let the
                        // heartbeat below end it if none comes.
                        Err(lamp_turn_owner::Error::ExpiredState) => {
                            self.skipped_expired = self.skipped_expired.saturating_add(1);
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    }
                    if self.skipped_expired > 0 {
                        eprintln!(
                            "lamp-voice provider: held up {} ms; {} lapsed authority snapshots skipped, going on",
                            (at.saturating_sub(self.last_control_us)) / 1_000,
                            self.skipped_expired
                        );
                        self.skipped_expired = 0;
                    }
                    self.handoff.observe_authority(snapshot)?;
                    self.last_control_us = at;
                    if self.saw_allowed
                        && (privacy_changed
                            || snapshot.microphone_permission() != Permission::Allowed)
                    {
                        self.stop();
                        return Ok(ControlDrain::Stopped);
                    }
                    self.saw_allowed |= snapshot.microphone_permission() == Permission::Allowed;
                }
                Control::ProviderOutputCapacity { through } => {
                    self.output_flow.grant(through)?;
                }
                Control::StartCapture | Control::ConnectReference { .. } => {
                    return Err(io::Error::other("invalid provider command").into());
                }
            }
        }
        // While a backlog left by a stall is still being read, the fresh
        // snapshot may be further down it: judge the heartbeat once the
        // queue is empty, or after the longest stall a gap may be (#784).
        let silent = now().saturating_sub(self.last_control_us);
        if silent > 1_500_000 || (empty && silent > 250_000) {
            return Err(io::Error::other("provider controller heartbeat expired").into());
        }
        Ok(if empty {
            ControlDrain::Empty
        } else {
            ControlDrain::Deferred
        })
    }

    fn receive_inputs(
        &mut self,
        now: &mut impl FnMut() -> u64,
        receive_control: &mut impl FnMut() -> io::Result<Option<Control>>,
        data: &mut Channel,
        input: &impl InputSink,
        remaining: &mut usize,
    ) -> Result<ControlDrain> {
        let result = self.receive_inputs_inner(now, receive_control, data, input, remaining);
        if result.is_err() {
            self.stop();
        }
        result
    }

    fn receive_inputs_inner(
        &mut self,
        now: &mut impl FnMut() -> u64,
        receive_control: &mut impl FnMut() -> io::Result<Option<Control>>,
        data: &mut Channel,
        input: &impl InputSink,
        remaining: &mut usize,
    ) -> Result<ControlDrain> {
        self.check_retained(now())?;
        self.handoff.synchronize(input)?;
        for _ in 0..INPUT_SLICE {
            if self.handoff.undelivered.is_some() || self.handoff.reminders.pending.is_some() {
                break;
            }
            if self.retained.is_none()
                && let Some(command) = data.receive::<ProviderInput>()?
            {
                self.retained = Some(RetainedInput {
                    command,
                    received_at_us: now(),
                    awaiting_room: false,
                });
            }
            // The parent sends Authority before its dependent input. Both can
            // arrive after the first empty control read, so reread control after
            // data receipt. Never submit while more control may still be queued.
            let control = self.drain_control(now, receive_control, remaining)?;
            if control == ControlDrain::Stopped {
                return Ok(control);
            }
            // Apply observed cancellation even when the bounded slice filled.
            // This preserves Gemini's existing old-response barrier and FIFO End.
            self.handoff.synchronize(input)?;
            if control == ControlDrain::Deferred {
                return Ok(control);
            }
            self.check_retained(now())?;
            let Some(mut retained) = self.retained.take() else {
                break;
            };
            // Gemini's link gives its connection up when its input queue is
            // full, as it is for a moment when a write is slow and input
            // comes faster than speech (a held line replayed, lampOS#892).
            // The command waits here, first in line, and nothing more is
            // read; past ROOM_WAIT it goes, and a queue still full is
            // the link's failure as before. A retired request's End owed
            // for want of room goes first.
            let now_us = now();
            if self.handoff.wait_for_room
                && (!input.has_room() || self.handoff.owes_end())
                && now_us.saturating_sub(retained.received_at_us)
                    < ROOM_WAIT.load(std::sync::atomic::Ordering::Relaxed)
            {
                retained.awaiting_room = true;
                self.retained = Some(retained);
                break;
            }
            // Empty control plus unknown owner is a protocol error, not a reason
            // to invent an authority grace period or relabel this command.
            self.handoff.submit(retained.command, now_us, input)?;
        }
        Ok(ControlDrain::Empty)
    }
}

/// Only successful submissions change the input's open/closed state. Authority
/// and audio use separate sockets, so root ownership is not proof that a Start
/// or End has reached Gemini yet. This tracker retains no PCM or request
/// history, but for the same-voice check's first 3 s of the line in hand.
#[derive(Default)]
struct InputHandoff {
    /// The newest note given and to which request: a request sends only
    /// its newest, so what it was given goes with what it is given next.
    noted: Option<(u64, String)>,
    /// The open request was only the lamp's name, its call's wait run out
    /// (`ProviderInput::Called`): the model is told so with its End.
    called: Option<u64>,
    /// The same-voice check's lines and prints (#829): it keeps the first
    /// 3 s of the line in hand until they are asked for.
    voices: Option<crate::same_voice::Voices<Box<dyn crate::same_voice::Printer>>>,
    authority: Option<Snapshot>,
    highest_seen: u64,
    retired_through: u64,
    retired_sent_through: u64,
    /// Through which request retirement came with no owner at all: the
    /// request was given up, where a newer owner means it was continued.
    /// Kept apart from retirement and passed on by itself: several
    /// authorities can be read in one turn, and an abandonment followed at
    /// once by a newer owner would otherwise be read as a continuation.
    abandoned_through: u64,
    abandoned_sent_through: u64,
    submitted: Option<SubmittedInput>,
    /// A started request that goes on from one ended early and given up,
    /// until the model has been told so.
    cut_off: Option<RequestId>,
    /// The first request after a hand or the head stopped the lamp, until
    /// the model has been told so ([`STOPPED_NOTE`]).
    after_stop: Option<RequestId>,
    /// A request begun soon after the lamp's words finished, until its End:
    /// the model is then told it may follow on ([`crate::follows_on::NOTE`]),
    /// unless the End says it was a sound.
    follows_on: Option<RequestId>,
    /// The newest request whose sound was ended here, and when: a name
    /// heard after that was not said in it (`crate::attention`).
    ended: Option<(u64, u64)>,
    /// The newest request that continues the one before it after an early
    /// end, as its Start said (`ProviderInput::Start::continues`).
    continued: Option<u64>,
    /// The newest request that goes on from the one before, as its Start
    /// said (`continues` or `continues_cut_off`), and whether the one
    /// before was cut off and given up: its words are the earlier one's
    /// too, for the staff's note (`crate::staff_note`).
    went_on: Option<(u64, bool)>,
    /// The newest request begun over the lamp's own answer
    /// (`ProviderInput::Start::over_the_lamp`): its words may be the
    /// lamp's, so it gets no staff's note.
    over_the_lamp: Option<u64>,
    /// The newest request ended as a short sound let through
    /// (`ProviderInput::End { sound: true }`, #800).
    ended_as_sound: Option<u64>,
    /// The newest request ended as a cut-in over the lamp's own answer
    /// (`ProviderInput::End { cut_in: true }`, #926).
    ended_cut_in: Option<u64>,
    /// A request that was only the lamp's name, and when it ended, for the
    /// request that follows it (#829, the first builder at 16:32).
    bare_call: Option<(u64, u64)>,
    /// Requests the provider will not serve. Their remaining input is dropped
    /// here, exactly like retired input, until the coordinator retires them.
    failed_through: u64,
    /// Report an undeliverable request instead of ending the worker.
    tolerate_outage: bool,
    /// Input, and a retired request's End, wait for room in the link's
    /// queue (`--wait-for-room`); without it a full queue ends the
    /// connection as it always has.
    wait_for_room: bool,
    /// The sole unreported request whose selected connection refused its Start.
    /// Drain it before receiving another command; it is never overwritten.
    undelivered: Option<u64>,
    reminders: reminder_policy::Policy,
}

#[derive(Clone, Copy)]
struct SubmittedInput {
    request: RequestId,
    session: SessionId,
    open: bool,
    /// The last block handed over: its capture time on the shared clock,
    /// and the instant that was carried over as.
    captured: Option<(u64, Instant)>,
}

impl InputHandoff {
    /// Whether `request` is no longer the lamp's: retired, given up, or
    /// failed. A call made in it is not to be run any more (#800).
    fn retired(&self, request: u64) -> bool {
        request <= self.retired_through.max(self.failed_through)
    }

    fn defer_reminder(
        &mut self,
        request: u64,
        session: u64,
        reason: ReminderDeferral,
        input: &impl InputSink,
    ) {
        self.reminders.defer(request, session, reason);
        self.fail(request);
        // Local revocation is synchronous and independent of provider queue
        // capacity, even when a refused input has already been retired.
        input.discard(RequestId::new(request).expect("validated reminder request"));
    }

    /// The open request that is the first after a Stop, once: the model is
    /// to be told so beside what it hears.
    fn take_after_stop(&mut self) -> Option<RequestId> {
        let request = self.after_stop.take()?;
        self.visual_turn()
            .filter(|turn| turn.request == request)
            .map(|_| request)
    }

    /// The newest request that was only the lamp's name, and when it
    /// ended, once.
    fn take_bare_call(&mut self) -> Option<(u64, u64)> {
        self.bare_call.take()
    }

    /// The open request that goes on from one ended early, once: the model
    /// is to be told so beside what it hears.
    fn take_cut_off(&mut self) -> Option<RequestId> {
        let request = self.cut_off.take()?;
        self.visual_turn()
            .filter(|turn| turn.request == request)
            .map(|_| request)
    }

    fn visual_turn(&self) -> Option<VisualTurn> {
        let submitted = self.submitted?;
        (submitted.open && submitted.request.get() > self.retired_through.max(self.failed_through))
            .then_some(VisualTurn {
                request: submitted.request,
                session: submitted.session,
            })
    }

    /// Called for every validated control packet, not just the final snapshot in
    /// a tick. An owner followed by cancellation must retire its later queued
    /// Start even if that Start has never been submitted to the provider.
    fn observe_authority(&mut self, state: Snapshot) -> Result<()> {
        if let Some(owner) = state.owner() {
            let request = owner.turn();
            if request <= self.retired_through || request < self.highest_seen {
                return Err(
                    io::Error::other("provider authority resurrected a retired request").into(),
                );
            }
            self.highest_seen = request;
            self.retired_through = self.retired_through.max(request - 1);
        } else {
            self.retired_through = self.retired_through.max(self.highest_seen);
            self.abandoned_through = self.abandoned_through.max(self.highest_seen);
        }
        if let Some(request) = self.given_up_before_sure(state) {
            crate::thread::unsay(request);
        }
        self.authority = Some(state);
        Ok(())
    }

    /// A request whose end was handed on here while the lamp's authority
    /// still said it was listening was ended early (the coordinator's
    /// `--early-end-ms`): the lamp was not yet sure the person had
    /// finished, and it plays nothing until it is. Given up before that,
    /// its answer was never played and its words were never the lamp's.
    /// Without that option this cannot happen: a request's end is sent only
    /// after the authority that stops listening for it.
    fn given_up_before_sure(&self, state: Snapshot) -> Option<u64> {
        let previous = self.authority?;
        let was = previous.owner()?;
        (previous.input_active()
            && state.owner() != Some(was)
            && self.ended.is_some_and(|(request, _)| request == was.turn()))
        .then_some(was.turn())
    }

    fn synchronize(&mut self, input: &impl InputSink) -> Result<()> {
        if self.abandoned_through > self.abandoned_sent_through {
            // No owner followed these: abandoned. A transport that kept any
            // of their input drops what it has not yet sent. First, so that
            // a later request that was continued keeps its words.
            input.discard(RequestId::new(self.abandoned_through)?);
            self.abandoned_sent_through = self.abandoned_through;
            // An abandonment is a retirement too.
            self.retired_sent_through = self.retired_sent_through.max(self.abandoned_through);
        }
        if self.retired_through > self.retired_sent_through {
            // This is synchronous local revocation, independent of queue space
            // or the network. It must happen even if closing the input fails.
            // A newer owner followed these: continued. Their words stay.
            input.retire(RequestId::new(self.retired_through)?);
            self.retired_sent_through = self.retired_through;
        }
        if let Some(submitted) = self.submitted
            && submitted.request.get() <= self.retired_through
        {
            if submitted.open {
                // The End waits for room as any input does (lampOS#892):
                // offered to a full queue it would end a healthy connection.
                // It stays owed, and nothing is offered before it
                // (`owes_end`); the retirement above is already done.
                if self.wait_for_room && !input.has_room() {
                    return Ok(());
                }
                // Enqueued before any successor Start on the same FIFO. Merely
                // dropping a root End that is still on the IPC socket would
                // leave the Gemini actor's old input open and reject the Start.
                match input.end(submitted.request) {
                    // Its connection is gone, or has just been given up for
                    // an input queue that overran: either way there is no
                    // open input to close on the connection that comes next.
                    Err(error)
                        if self.tolerate_outage
                            && (error == Error::NotConnected
                                || error == Error::Backpressure
                                || (error == Error::StaleRequest
                                    && input
                                        .belongs_to_replaced_connection(submitted.request))) => {}
                    other => other?,
                }
            }
            self.submitted = None;
        }
        Ok(())
    }

    /// A retired request's input is open and its End not yet queued.
    fn owes_end(&self) -> bool {
        self.submitted.is_some_and(|submitted| {
            submitted.open && submitted.request.get() <= self.retired_through
        })
    }

    /// The provider will not serve this request. Nothing further of it is
    /// submitted, and it is never treated as open again.
    fn fail(&mut self, request: u64) {
        self.failed_through = self.failed_through.max(request);
        if self
            .submitted
            .is_some_and(|submitted| submitted.request.get() <= request)
        {
            self.submitted = None;
        }
    }

    /// A missing connection or proven old-session input is survivable only in
    /// typed recovery. An arbitrary stale request remains a protocol fault.
    ///
    /// So is an input queue that overran. The link's sender gives its own
    /// connection up when its queue is full, which is how a write stuck on
    /// a stalled network shows first on this side; `lamp-gemini` then
    /// reconnects and reports the request lost. Until 2026-10-13 that
    /// refusal was not counted an outage and ended the worker and the
    /// service with it ("provider unavailable while input was admitted:
    /// Gemini Live Backpressure", lamp-4ace, 16:50:55).
    fn refused(&mut self, request: RequestId, error: Error, input: &impl InputSink) -> Result<()> {
        let was_submitted = self
            .submitted
            .is_some_and(|submitted| submitted.request == request);
        let replaced = was_submitted
            && error == Error::StaleRequest
            && input.belongs_to_replaced_connection(request);
        let overran = was_submitted && error == Error::Backpressure;
        if self.tolerate_outage && (error == Error::NotConnected || replaced || overran) {
            if !was_submitted && self.undelivered.is_some() {
                return Err(io::Error::other("unreported refused Start must be drained").into());
            }
            self.fail(request.get());
            // Audio/End refusal cannot undo an accepted Start. The supervisor
            // reports that request's actual stage after draining its prior PCM.
            if !was_submitted {
                self.undelivered = Some(request.get());
            }
            return Ok(());
        }
        Err(error.into())
    }

    /// These two checked refusals happen before enqueue or supersession. They
    /// cannot create a supervisor terminal receipt, so the worker owes one.
    /// Other errors keep their existing fatal semantics; in particular an
    /// arbitrary StaleRequest is not proof of a replaced connection.
    fn refused_start(&mut self, request: RequestId, refusal: StartRefusal) -> Result<()> {
        let error = match refusal {
            StartRefusal::NotConnected => Error::NotConnected,
            StartRefusal::SessionMismatch { .. } => Error::StaleRequest,
            // The queue overran on the Start itself: nothing of this request
            // was accepted and its connection has been given up. As for a
            // missing connection, the worker owes the report.
            StartRefusal::Refused(Error::Backpressure) if self.tolerate_outage => {
                Error::Backpressure
            }
            StartRefusal::Refused(error) => return Err(error.into()),
        };
        if !self.tolerate_outage {
            return Err(error.into());
        }
        if self.undelivered.is_some() {
            return Err(io::Error::other("unreported refused Start must be drained").into());
        }
        self.fail(request.get());
        self.undelivered = Some(request.get());
        Ok(())
    }

    fn submit(
        &mut self,
        command: ProviderInput,
        now_us: u64,
        input: &impl InputSink,
    ) -> Result<()> {
        self.synchronize(input)?;
        let state = self
            .authority
            .ok_or_else(|| io::Error::other("provider has no authority"))?;
        let now = MonoTime::from_micros(now_us);
        if !state.listening_ready(now)
            || command.privacy_generation() != state.microphone_generation()
        {
            return Err(io::Error::other("provider input lost privacy authority").into());
        }
        if !matches!(command, ProviderInput::Audio { .. }) {
            SUBMITTED_AT_US.store(now_us, std::sync::atomic::Ordering::Relaxed);
        }
        let request = RequestId::new(command.request())?;
        if request.get() <= self.retired_through.max(self.failed_through) {
            // Old PCM retains its old identity and is discarded without reaching
            // the provider. Its sequence/timestamp cannot poison the new input.
            return Ok(());
        }
        if state.owner().map(|owner| owner.turn()) != Some(request.get()) {
            return Err(io::Error::other("provider input has no matching current owner").into());
        }
        match command {
            ProviderInput::Reminder {
                session,
                text,
                late,
                late_by_ms,
                ..
            } => {
                if self.submitted.is_some() {
                    return Err(io::Error::other("provider input already started").into());
                }
                let expected = SessionId::new(session)?;
                let note =
                    match self
                        .reminders
                        .prepare(request.get(), session, &text, late, late_by_ms)
                    {
                        Ok(note) => note,
                        Err(reason) => {
                            self.defer_reminder(request.get(), session, reason, input);
                            return Ok(());
                        }
                    };
                // Reminder data is not a memory instruction, nor something
                // to remember automatically merely because it was spoken.
                crate::remember::keep_out(request.get());
                crate::attention::own_request()
                    .store(request.get(), std::sync::atomic::Ordering::Release);
                if let Err(refusal) = input.written(expected, request, &note, None) {
                    return self.refused_start(request, refusal);
                }
                self.submitted = Some(SubmittedInput {
                    request,
                    session: expected,
                    open: false,
                    captured: None,
                });
            }
            ProviderInput::Start {
                session,
                continues,
                continues_cut_off,
                over_the_lamp,
                over_the_lamps_voice,
                after_stop,
                follows_on,
                ..
            } => {
                if self.submitted.is_some() {
                    return Err(io::Error::other("provider input already started").into());
                }
                let expected = SessionId::new(session)?;
                if let Err(refusal) = input.start(expected, request) {
                    return self.refused_start(request, refusal);
                }
                if continues {
                    self.continued = Some(request.get());
                }
                if continues || continues_cut_off {
                    self.went_on = Some((request.get(), continues_cut_off));
                }
                if over_the_lamp {
                    self.over_the_lamp = Some(request.get());
                }
                if over_the_lamps_voice && let Ok(mut room) = crate::room_told::room().lock() {
                    room.began_over_the_lamp(request.get());
                }
                if let Some(voices) = self.voices.as_mut() {
                    voices.began(request.get(), !over_the_lamp);
                }
                self.submitted = Some(SubmittedInput {
                    request,
                    session: expected,
                    open: true,
                    captured: None,
                });
                self.cut_off = continues_cut_off.then_some(request);
                self.after_stop = after_stop.then_some(request);
                self.follows_on = follows_on.then_some(request);
            }
            ProviderInput::Audio {
                sequence,
                read_completed_at_us,
                samples,
                ..
            } => {
                self.require_open(request)?;
                // Order is decided here, on the clock the capture times were
                // taken with, and before anything is carried over: a block
                // captured before its predecessor in the same request is
                // refused. The instant it would be carried over as cannot
                // show that. A delay between the two readings below can
                // carry an earlier time forward as easily as a later one
                // back, and the transport sees only the instant.
                if let Some((last_us, _)) = self.submitted.and_then(|submitted| submitted.captured)
                    && read_completed_at_us < last_us
                {
                    return Err(io::Error::other("capture time went backwards").into());
                }
                let age = now_us
                    .checked_sub(read_completed_at_us)
                    .map(Duration::from_micros)
                    .filter(|age| *age < lamp_gemini::MAX_INPUT_AGE)
                    .ok_or_else(|| io::Error::other("invalid or stale capture time"))?;
                let mut captured = Instant::now()
                    .checked_sub(age)
                    .ok_or_else(|| io::Error::other("invalid capture age"))?;
                // The age comes from one clock and is taken off another, read
                // a moment later, for every block. Whatever passes between
                // the two readings makes that block seem captured that much
                // later, so the block after it can seem captured before it,
                // and the transport refuses that for good. The shared times
                // are in order, as checked above, so the instants are kept
                // in order: the block is given its predecessor's instant,
                // never an earlier one.
                if let Some(submitted) = self.submitted.as_mut() {
                    if let Some((_, last)) = submitted.captured
                        && captured < last
                    {
                        captured = last;
                    }
                    submitted.captured = Some((read_completed_at_us, captured));
                }
                if let Err(error) = input.audio(request, sequence, captured, &samples) {
                    return self.refused(request, error, input);
                }
                if let Some(voices) = self.voices.as_mut() {
                    voices.heard(request.get(), &samples);
                }
                if let Ok(mut room) = crate::room_told::room().lock() {
                    room.hearing(now_us);
                }
            }
            // Kept for its End: the note and the End go together, or the
            // End alone (the reviewer on #992).
            ProviderInput::Called { .. } => {
                self.require_open(request)?;
                self.called = Some(request.get());
            }
            ProviderInput::End { sound, cut_in, .. } => {
                self.require_open(request)?;
                if self.called.take() == Some(request.get()) {
                    self.bare_call = Some((request.get(), now_us));
                    self.tell_called(request, input);
                }
                // Told with its End, once it is known not to be a sound: a
                // thump let through for the model to judge is never told it
                // may follow on (the reviewer on #1278).
                if self.follows_on.take() == Some(request) && !sound {
                    self.tell_follows_on(request, input);
                }
                if let Err(error) = input.end(request) {
                    return self.refused(request, error, input);
                }
                if let Some(voices) = self.voices.as_mut() {
                    voices.ended(request.get());
                }
                if let Ok(mut room) = crate::room_told::room().lock() {
                    if sound {
                        room.ended_as_sound(request.get(), now_us);
                    } else {
                        room.ended(request.get(), now_us);
                    }
                }
                self.ended = Some((request.get(), now_us));
                if sound {
                    self.ended_as_sound = Some(request.get());
                }
                if cut_in {
                    self.ended_cut_in = Some(request.get());
                }
                if let Some(submitted) = self.submitted.as_mut() {
                    submitted.open = false;
                }
            }
            ProviderInput::Written {
                session,
                text,
                look,
                ..
            } => {
                if self.submitted.is_some() {
                    return Err(io::Error::other("provider input already started").into());
                }
                let expected = SessionId::new(session)?;
                // A look with the eyes shut is said to be one: the model is
                // not left to describe a picture it was not given.
                let picture = look
                    .then(|| LOOK_FILE.get())
                    .flatten()
                    .and_then(|path| {
                        // A full look's still may have been asked at a
                        // side of its own, and be larger (#941).
                        let (within, most) = if LOOK_FULL.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            (crate::sight::FULL_FRESH, lamp_gemini::MAX_LOOK_IMAGE_BYTES)
                        } else {
                            (std::time::Duration::from_secs(1), lamp_gemini::MAX_IMAGE_BYTES)
                        };
                        crate::sight::fresh_up_to(path, std::time::SystemTime::now(), within, most)
                            .inspect_err(|why| {
                                eprintln!(
                                    "lamp-voice provider: the lamp looked: no picture with request {}: {why}",
                                    request.get()
                                )
                            })
                            .ok()
                    })
                    .map(|(jpeg, _)| jpeg);
                let text = if look && picture.is_none() {
                    crate::speak_up::NO_LOOK_NOTE
                } else {
                    text.as_str()
                };
                // The lamp's own turn: nothing was said to anyone else.
                crate::attention::own_request()
                    .store(request.get(), std::sync::atomic::Ordering::Release);
                if let Err(refusal) = input.written(expected, request, text, picture.as_deref()) {
                    return self.refused_start(request, refusal);
                }
                if let Some(jpeg) = picture.as_ref() {
                    eprintln!(
                        "lamp-voice provider: the lamp looked: a picture of {} bytes went with request {}",
                        jpeg.len(),
                        request.get()
                    );
                    LOOKED_AT_US.store(now_us, std::sync::atomic::Ordering::Relaxed);
                }
                // Started and ended at once: nothing more is sent for it.
                self.submitted = Some(SubmittedInput {
                    request,
                    session: expected,
                    open: false,
                    captured: None,
                });
            }
        }
        Ok(())
    }

    /// Tell the model, just before `request`'s End, that it may follow on
    /// from what the lamp said ([`crate::follows_on::NOTE`]), unless the
    /// room is busy, and if the link's input queue has room for the note
    /// and the End both, as [`Self::tell_called`].
    fn tell_follows_on(&mut self, request: RequestId, input: &impl InputSink) {
        let busy = crate::room_told::room()
            .lock()
            .is_ok_and(|mut room| room.at(monotonic_us()) == crate::room_told::Room::Busy);
        if busy {
            eprintln!(
                "lamp-voice provider: request {}: begun soon after the lamp spoke, in a busy room; the model is not told it may follow on",
                request.get()
            );
            return;
        }
        if !input.has_room_for(2) {
            eprintln!(
                "lamp-voice provider: request {}: begun soon after the lamp spoke, but the link's input queue has no room for the note and the end: the model is not told",
                request.get()
            );
            return;
        }
        let text = note_text(self.noted.as_ref(), request.get(), crate::follows_on::NOTE);
        match input.note(request, &text) {
            Ok(()) => {
                self.noted = Some((request.get(), text));
                eprintln!(
                    "lamp-voice provider: request {}: the model is told it may follow on from what the lamp said",
                    request.get()
                );
            }
            Err(error) => eprintln!(
                "lamp-voice provider: request {}: begun soon after the lamp spoke, and the model could not be told: {error}",
                request.get()
            ),
        }
    }

    /// Tell the model, just before `request`'s End, that it was only the
    /// lamp's name ([`CALLED_NOTE`]), if the link's input queue has room
    /// for the note and the End both. The note is a nicety and the End is
    /// not: with room for one, the End goes alone, and the connection is
    /// never ended for want of room the note took.
    fn tell_called(&mut self, request: RequestId, input: &impl InputSink) {
        if !input.has_room_for(2) {
            eprintln!(
                "lamp-voice provider: request {}: only the lamp's name was said, but the link's input queue has no room for the note and the end: the model is not told",
                request.get()
            );
            return;
        }
        let text = note_text(self.noted.as_ref(), request.get(), CALLED_NOTE);
        match input.note(request, &text) {
            Ok(()) => {
                self.noted = Some((request.get(), text));
                eprintln!(
                    "lamp-voice provider: request {}: only the lamp's name was said, and the model is told so",
                    request.get()
                );
            }
            Err(error) => eprintln!(
                "lamp-voice provider: request {}: the model could not be told only its name was said: {error}",
                request.get()
            ),
        }
    }

    fn require_open(&self, request: RequestId) -> Result<()> {
        if !self
            .submitted
            .is_some_and(|submitted| submitted.request == request && submitted.open)
        {
            return Err(io::Error::other("provider input is not open for this request").into());
        }
        Ok(())
    }
}

/// The production sink is the supervised connection's bounded command queue.
/// The small seam lets regressions exercise cross-channel ordering without
/// credentials or a socket.
trait InputSink {
    fn start(
        &self,
        session: SessionId,
        request: RequestId,
    ) -> std::result::Result<(), StartRefusal>;
    fn audio(
        &self,
        request: RequestId,
        sequence: u64,
        captured: Instant,
        samples: &[i16],
    ) -> lamp_gemini::Result<()>;
    fn end(&self, request: RequestId) -> lamp_gemini::Result<()>;
    /// A whole request in writing. A sink that has no way to take one
    /// refuses it as a start it could not make.
    fn written(
        &self,
        _session: SessionId,
        _request: RequestId,
        _text: &str,
        _picture: Option<&[u8]>,
    ) -> std::result::Result<(), StartRefusal> {
        Err(StartRefusal::NotConnected)
    }
    fn retire(&self, request: RequestId);
    /// A note beside the open request (`try_note`). A sink that sends
    /// nothing to a service has nowhere to put it.
    fn note(&self, _request: RequestId, _text: &str) -> lamp_gemini::Result<()> {
        Ok(())
    }
    /// Retirement of a request that was given up and not continued. A sink
    /// that keeps no input treats it as any retirement.
    fn discard(&self, request: RequestId) {
        self.retire(request);
    }
    /// Exact successful submission lineage belongs to an earlier connection.
    /// Absence of this proof never turns StaleRequest into a recoverable error.
    fn belongs_to_replaced_connection(&self, _request: RequestId) -> bool {
        false
    }
    /// Whether the next command offered can be queued without ending the
    /// connection for want of room.
    fn has_room(&self) -> bool {
        true
    }
    /// Whether the next `commands` offered one after another can.
    fn has_room_for(&self, _commands: usize) -> bool {
        true
    }
}

/// What the model is told beside a request that was only the lamp's name,
/// with nothing more said in the call's wait (`--call-wait-ms`, #782). A
/// person who hears their name and nothing more says "Yes?" and waits.
pub(crate) const CALLED_NOTE: &str = "The person said your name, Lamp, and nothing more: they are calling you. Answer as someone who hears their name does, with one short word such as \"Yes?\", and wait for them to go on.";

/// What the model is told beside the first request after a hand or the
/// lamp's head stopped its speech (#829, the first builder at 16:33: after
/// each tap that stopped a story, the story began again). With
/// `LAMP_STOP_TOLD=on` only. In the form of the lamp's other notes, which
/// its character says nobody said aloud: never answered, never mentioned
/// (the first builder on #1148: "Okay, I'll stop" said aloud is no better).
pub(crate) const STOPPED_NOTE: &str = "(For you to know, not to answer: the person stopped you just now, with a touch or the stop button, while you were speaking. What you were saying is over: do not go back to it or start it again unless they ask for it, and say nothing about the stop.)";

/// What the model is told beside a request that goes on from one whose end
/// was sent early and given up when the person spoke again
/// (`--early-end-ms`; docs/early-end-taken-back.md). The same words as
/// `REST_NOTE` in `lamp-gemini`'s `early_end_probe`, which tried them on
/// the service.
pub(crate) const CUT_OFF_NOTE: &str = "The person paused and then went on: the request just before this one was cut off by that pause, and this is the rest of it. Answer all of it as one request.";

/// Give `request` a note beside what is being said, keeping what it was
/// given already: a request sends only its newest note, so the name and
/// the room, due in the same request, go as one.
fn add_note<C: Connect>(
    provider: &Supervisor<C>,
    noted: &mut Option<(u64, String)>,
    request: RequestId,
    part: &str,
) -> std::result::Result<(), Error> {
    let text = note_text(noted.as_ref(), request.get(), part);
    provider.try_note(request, &text)?;
    *noted = Some((request.get(), text));
    Ok(())
}

/// The room microphone's greatest rise over its quiet, in decibels, in a
/// request's window so far (its sound's end, or now): nothing when no level
/// was read in it.
fn rise_in(
    levels: &VecDeque<(u64, f32)>,
    (began, ended): (u64, Option<u64>),
    now_us: u64,
) -> Option<f32> {
    let until = ended.unwrap_or(now_us);
    levels
        .iter()
        .filter(|(at, _)| (began..=until).contains(at))
        .map(|(_, rise)| *rise)
        .reduce(f32::max)
}

/// What a request's note says once the staff's `part` takes the place of
/// the one it was given (`told`), or is added ([`note_text`]).
fn staff_note_text(
    noted: Option<&(u64, String)>,
    request: u64,
    told: Option<&str>,
    part: &str,
) -> String {
    match (noted, told) {
        (Some((given, text)), Some(told)) if *given == request && text.contains(told) => {
            text.replacen(told, part, 1)
        }
        _ => note_text(noted, request, part),
    }
}

/// What a request's note says once `part` is added to what it was given.
fn note_text(noted: Option<&(u64, String)>, request: u64, part: &str) -> String {
    match noted {
        Some((given, text)) if *given == request && !text.contains(part) => {
            format!("{text} {part}")
        }
        Some((given, text)) if *given == request => text.clone(),
        _ => part.to_owned(),
    }
}

impl<C: Connect> InputSink for Supervisor<C> {
    fn start(
        &self,
        session: SessionId,
        request: RequestId,
    ) -> std::result::Result<(), StartRefusal> {
        self.try_start_for_session(session, request)
    }
    fn audio(
        &self,
        request: RequestId,
        sequence: u64,
        captured: Instant,
        samples: &[i16],
    ) -> lamp_gemini::Result<()> {
        self.try_audio(request, sequence, captured, samples)
    }
    fn note(&self, request: RequestId, text: &str) -> lamp_gemini::Result<()> {
        self.try_note(request, text)
    }
    fn end(&self, request: RequestId) -> lamp_gemini::Result<()> {
        self.try_end(request)
    }
    fn written(
        &self,
        session: SessionId,
        request: RequestId,
        text: &str,
        picture: Option<&[u8]>,
    ) -> std::result::Result<(), StartRefusal> {
        self.try_written_for_session(session, request, text, picture)
    }
    fn retire(&self, request: RequestId) {
        Supervisor::retire(self, request);
    }
    fn discard(&self, request: RequestId) {
        Supervisor::discard(self, request);
    }
    fn belongs_to_replaced_connection(&self, request: RequestId) -> bool {
        self.input_belongs_to_replaced_connection(request)
    }
    fn has_room(&self) -> bool {
        self.input_has_room()
    }
    fn has_room_for(&self, commands: usize) -> bool {
        self.input_has_room_for(commands)
    }
}

fn push_text(
    push: &mut impl FnMut(ProviderOutput),
    request: Option<u64>,
    mut text: String,
    finished: bool,
) {
    while text.len() > 3500 {
        let mut split = 3500;
        while !text.is_char_boundary(split) {
            split -= 1;
        }
        let tail = text.split_off(split);
        push(ProviderOutput::Transcript {
            request,
            text,
            finished: false,
        });
        text = tail;
    }
    push(ProviderOutput::Transcript {
        request,
        text,
        finished,
    });
}

#[cfg(test)]
mod tests {
    mod reminders;

    use super::*;
    use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller};
    use std::cell::{Cell, RefCell};

    const NOW: u64 = 10_000_000;

    /// #799: each call answered is kept for the first-output record with
    /// when it came and when its answer was sent, whether the connection
    /// took it or not, and what is taken is not kept again.
    #[test]
    fn each_call_answered_is_kept_for_the_record_with_when_it_came_and_was_sent() {
        let mut desk = ToolDesk::new(crate::tools::Settings::default());
        let asked = |request: u64, name: &str, answered: bool| Asked {
            session: SessionId::new(2).unwrap(),
            request,
            call: lamp_gemini::ToolCall {
                id: format!("call-{request}"),
                name: name.into(),
                args: serde_json::json!({"task": "private words"}),
            },
            told: serde_json::json!({"prepared": true}),
            whom: "the tasks part",
            skill: false,
            heard: true,
            answered,
            took: Duration::from_millis(41),
            called_us: 1_000,
        };
        desk.asked.send(asked(7, "start_agent_task", true)).unwrap();
        desk.asked
            .send(asked(8, "check_agent_task", false))
            .unwrap();
        desk.waiting = 2;
        let before = monotonic_us();
        let mut sent = Vec::new();
        desk.answer_with(
            |done| {
                sent.push(done.request);
                done.request == 7
            },
            |_, _| {},
        );
        assert_eq!(sent, [7, 8]);
        assert_eq!(desk.waiting, 0);
        let lines = desk.take_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].session, lines[0].request, lines[0].name.as_str()),
            (2, 7, "start_agent_task")
        );
        assert_eq!(lines[0].called_us, 1_000);
        assert!(lines[0].sent_us >= before);
        assert_eq!((lines[0].answered, lines[0].sent), (true, true));
        assert_eq!((lines[1].answered, lines[1].sent), (false, false));
        assert!(desk.take_lines().is_empty());
        // Never more than a tick's worth is kept.
        for request in 0..(MOST_CALL_LINES as u64 + 5) {
            desk.line(
                SessionId::new(2).unwrap(),
                request,
                "the_time",
                1,
                2,
                true,
                true,
            );
        }
        assert_eq!(desk.take_lines().len(), MOST_CALL_LINES);
    }

    /// #941: a skill's answer with `"look": true` that reached the model
    /// asks the coordinator for the look once, for the call's session. One
    /// not sent, or in a turn the lamp wrote itself (the look's own turn: a
    /// call there takes no second still, #1146), goes as a dropped look; one
    /// with no look, or another part's answer with a look, asks nothing.
    #[test]
    fn a_call_answered_with_a_look_asks_the_coordinator_for_it_once_it_is_sent() {
        let mut desk = ToolDesk::new(crate::tools::Settings::default());
        let asked = |request: u64, told: serde_json::Value, skill: bool, heard: bool| Asked {
            session: SessionId::new(2).unwrap(),
            request,
            call: lamp_gemini::ToolCall {
                id: format!("call-{request}"),
                name: "check_my_work".into(),
                args: serde_json::json!({}),
            },
            told,
            whom: "the executive",
            skill,
            heard,
            answered: true,
            took: Duration::from_millis(3),
            called_us: 1,
        };
        let look = serde_json::json!({"next": "Say only \"Let me take a look.\"", "look": true});
        desk.asked.send(asked(7, look.clone(), true, true)).unwrap();
        desk.asked.send(asked(7, look.clone(), true, true)).unwrap();
        desk.asked.send(asked(8, look.clone(), true, true)).unwrap();
        desk.asked
            .send(asked(9, serde_json::json!({"next": "Done."}), true, true))
            .unwrap();
        // The actions part's answer with the key: no picture leaves for it.
        desk.asked
            .send(asked(10, look.clone(), false, true))
            .unwrap();
        // The look's own turn, written by the lamp: no second still.
        desk.asked.send(asked(11, look, true, false)).unwrap();
        desk.waiting = 6;
        // Request 8's connection has gone: its answer was never the model's.
        desk.answer_with(|done| done.request != 8, |_, _| {});
        assert_eq!(
            desk.take_looks(),
            [
                (7, 2, None),
                (8, 2, Some("the answer never reached the model".into())),
                (11, 2, Some("the lamp's own turn".into())),
            ]
        );
        assert!(desk.take_looks().is_empty());
    }

    /// The reviewer on #810 (17:23): the time an answer was sent was taken
    /// after its journal line, so a journal slow to take the line (100 ms
    /// in their harness) was counted as the call's. It is taken as it is
    /// sent, before the line.
    #[test]
    fn an_answer_is_timed_as_it_is_sent_not_after_its_journal_line() {
        let mut desk = ToolDesk::new(crate::tools::Settings::default());
        desk.asked
            .send(Asked {
                session: SessionId::new(2).unwrap(),
                request: 9,
                call: lamp_gemini::ToolCall {
                    id: "call-9".into(),
                    name: "check_agent_task".into(),
                    args: serde_json::json!({}),
                },
                told: serde_json::json!({}),
                whom: "the tasks part",
                skill: false,
                heard: true,
                answered: true,
                took: Duration::from_millis(40),
                called_us: 1,
            })
            .unwrap();
        desk.waiting = 1;
        let mut sent_at = 0;
        desk.answer_with(
            |_| {
                sent_at = monotonic_us();
                true
            },
            // A journal that holds the line a tenth of a second.
            |_, _| std::thread::sleep(Duration::from_millis(100)),
        );
        let line = desk.take_lines().pop().unwrap();
        assert!(line.sent_us >= sent_at);
        assert!(
            line.sent_us - sent_at < 20_000,
            "sent {} us after the send",
            line.sent_us - sent_at
        );
    }

    #[test]
    fn started_wire_retains_actual_source_session_and_requires_it_on_decode() {
        let mut output = VecDeque::new();
        forward(
            Event::InputStarted {
                lineage: lamp_gemini::Lineage {
                    session: lamp_gemini::SessionId::new(7).unwrap(),
                    request: RequestId::new(23).unwrap(),
                },
                waiting_for_barrier: true,
            },
            &mut output,
            &mut 0,
            &mut DiscardLog::default(),
        )
        .unwrap();
        let Some(Outbound::Output(started)) = output.pop_front() else {
            panic!("missing Started");
        };
        let value = serde_json::to_value(&started).unwrap();
        assert_eq!(value["session"], 7);
        assert!(matches!(
            serde_json::from_value::<ProviderOutput>(value).unwrap(),
            ProviderOutput::Started {
                request: 23,
                session: 7,
                waiting_for_barrier: true
            }
        ));
        assert!(
            serde_json::from_value::<ProviderOutput>(serde_json::json!({
                "kind":"started", "request":23, "waiting_for_barrier":true
            }))
            .is_err(),
            "a missing source session cannot borrow the current connection"
        );
    }

    #[test]
    fn output_capacity_cannot_renew_authority_or_controller_heartbeat() {
        let mut intake = ProviderIntake::new(BootId::new([9; 16]).unwrap(), NOW);
        let mut budget = CONTROL_SLICE;
        let mut command = Some(Control::ProviderOutputCapacity { through: 2 });
        let error = intake
            .drain_control(
                &mut || NOW + 250_001,
                &mut || Ok(command.take()),
                &mut budget,
            )
            .unwrap_err();
        assert!(error.to_string().contains("heartbeat expired"));
        assert_eq!(intake.last_control_us, NOW);
        assert!(intake.guard.state().is_none());
        assert!(intake.stopped);
    }

    /// A worker held up past its snapshot's expiry (#784) finds the lapsed
    /// one first in its queue: it skips it and takes the fresh one behind it.
    #[test]
    fn a_snapshot_that_lapsed_while_the_worker_was_held_up_is_skipped_for_a_fresh_one() {
        let mut rig = SocketRig::new();
        let lapsed = rig.owner.snapshot(time()).unwrap();
        let late = lapsed.expires_at().as_micros() + 1;
        rig.owner
            .set_capture(
                MonoTime::from_micros(late),
                CaptureState::RetainingUntil(MonoTime::from_micros(late + 10)),
            )
            .unwrap();
        let fresh = rig.owner.snapshot(MonoTime::from_micros(late)).unwrap();
        for snapshot in [lapsed, fresh] {
            rig.parent
                .control
                .send(Control::Authority { snapshot })
                .unwrap();
        }
        assert_eq!(rig.tick(late).unwrap(), ControlDrain::Empty);
        assert!(!rig.intake.stopped);
        assert_eq!(rig.intake.last_control_us, late);
        assert_eq!(rig.intake.skipped_expired, 0);
    }

    #[test]
    fn retired_output_is_removed_before_new_output_without_changing_survivor_order() {
        let mut queue = VecDeque::new();
        for sequence in 0..MAX_QUEUED_OUTPUT - 4 {
            queue.push_back(Outbound::Output(ProviderOutput::Audio {
                request: 1,
                sequence: sequence as u64 + 1,
                provider_event_at_us: NOW,
                source_frames: PACKET_SAMPLES,
                source_offset: 0,
                samples: vec![1; PACKET_SAMPLES],
            }));
        }
        queue.push_back(Outbound::Output(ProviderOutput::TurnComplete {
            request: 1,
            idle: true,
        }));
        // A failure report is never dropped, even for a request since retired.
        queue.push_back(Outbound::Status(ProviderStatus::TurnFailed {
            request: 1,
            stage: FailedStage::Responding,
            reason: Fault::Transport,
        }));
        queue.push_back(Outbound::Output(ProviderOutput::Started {
            request: 2,
            session: 1,
            waiting_for_barrier: true,
        }));
        queue.push_back(Outbound::Output(ProviderOutput::Transcript {
            request: None,
            text: "unattributed".into(),
            finished: true,
        }));
        prune_retired_output(&mut queue, 1);
        assert_eq!(queue.len(), 3);
        assert!(matches!(
            queue.pop_front(),
            Some(Outbound::Status(ProviderStatus::TurnFailed {
                request: 1,
                ..
            }))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(Outbound::Output(ProviderOutput::Started { request: 2, .. }))
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(Outbound::Output(ProviderOutput::Transcript {
                request: None,
                ..
            }))
        ));
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Start(u64),
        Audio(u64, u64, Vec<i16>),
        End(u64),
        EndBlocked(u64),
        Retire(u64),
        Note(u64, String),
    }

    /// Models FIFO submission and synchronous retirement, not cloud timing.
    /// Gemini's own transport tests cover the old-response barrier separately.
    #[derive(Default)]
    struct RecordingSink {
        calls: RefCell<Vec<Call>>,
        /// The capture instant each accepted audio block was handed over with.
        captured: RefCell<Vec<Instant>>,
        open: Cell<Option<u64>>,
        retired: Cell<u64>,
        reject_end: Cell<bool>,
        reject_start: Cell<bool>,
        disconnected: Cell<bool>,
        /// The next thing offered overruns the link's input queue: it is
        /// refused as backpressure and the connection is gone from then
        /// on, as with the real sender.
        overrun: Cell<bool>,
        /// Zero means the initial session (1); tests can install a replacement.
        session: Cell<u64>,
        /// The free places in the link's input queue, if bounded.
        room: Cell<Option<usize>>,
    }
    impl InputSink for RecordingSink {
        fn start(
            &self,
            session: SessionId,
            request: RequestId,
        ) -> std::result::Result<(), StartRefusal> {
            if self.disconnected.get() {
                return Err(StartRefusal::NotConnected);
            }
            if self.overrun.replace(false) {
                self.disconnected.set(true);
                return Err(StartRefusal::Refused(lamp_gemini::Error::Backpressure));
            }
            let live = SessionId::new(self.session.get().max(1)).unwrap();
            if live != session {
                return Err(StartRefusal::SessionMismatch { live });
            }
            if self.reject_start.get() {
                return Err(StartRefusal::Refused(lamp_gemini::Error::Backpressure));
            }
            if self.open.get().is_some() || request.get() <= self.retired.get() {
                return Err(StartRefusal::Refused(lamp_gemini::Error::OverlappingInput));
            }
            self.open.set(Some(request.get()));
            self.calls.borrow_mut().push(Call::Start(request.get()));
            Ok(())
        }
        fn audio(
            &self,
            request: RequestId,
            sequence: u64,
            captured: Instant,
            samples: &[i16],
        ) -> lamp_gemini::Result<()> {
            if self.disconnected.get() {
                return Err(lamp_gemini::Error::NotConnected);
            }
            if self.overrun.replace(false) {
                self.disconnected.set(true);
                return Err(lamp_gemini::Error::Backpressure);
            }
            if self.open.get() != Some(request.get()) || request.get() <= self.retired.get() {
                return Err(lamp_gemini::Error::StaleRequest);
            }
            self.captured.borrow_mut().push(captured);
            self.calls
                .borrow_mut()
                .push(Call::Audio(request.get(), sequence, samples.to_vec()));
            Ok(())
        }
        fn end(&self, request: RequestId) -> lamp_gemini::Result<()> {
            if self.disconnected.get() {
                return Err(lamp_gemini::Error::NotConnected);
            }
            if self.overrun.replace(false) {
                self.disconnected.set(true);
                return Err(lamp_gemini::Error::Backpressure);
            }
            if self.reject_end.get() {
                self.calls
                    .borrow_mut()
                    .push(Call::EndBlocked(request.get()));
                return Err(lamp_gemini::Error::Backpressure);
            }
            if self.open.get() != Some(request.get()) {
                return Err(lamp_gemini::Error::StaleRequest);
            }
            self.calls.borrow_mut().push(Call::End(request.get()));
            self.open.set(None);
            Ok(())
        }
        fn retire(&self, request: RequestId) {
            self.retired.set(self.retired.get().max(request.get()));
            self.calls.borrow_mut().push(Call::Retire(request.get()));
        }
        fn note(&self, request: RequestId, text: &str) -> lamp_gemini::Result<()> {
            self.calls
                .borrow_mut()
                .push(Call::Note(request.get(), text.to_owned()));
            Ok(())
        }
        fn has_room_for(&self, commands: usize) -> bool {
            self.room.get().is_none_or(|room| room >= commands)
        }
    }

    /// The reviewer on #992: a bare call's note goes with its End, and only
    /// when the link's input queue has room for both, so the End never
    /// meets a queue the note filled. With room for one, the End goes alone.
    #[test]
    fn a_bare_calls_note_goes_with_its_end_only_when_both_fit() {
        for (room, told) in [(None, true), (Some(2), true), (Some(1), false)] {
            let mut owner = controller();
            let mut handoff = InputHandoff::default();
            let sink = RecordingSink::default();
            sink.room.set(room);
            let request = authorize(&mut owner, &mut handoff);
            handoff
                .submit(start(request, &handoff), NOW, &sink)
                .unwrap();
            let called = ProviderInput::Called {
                request,
                privacy_generation: generation(&handoff),
            };
            handoff.submit(called, NOW, &sink).unwrap();
            // Nothing goes for the call until its End.
            assert_eq!(*sink.calls.borrow(), vec![Call::Start(request)]);
            handoff.submit(end(request, &handoff), NOW, &sink).unwrap();
            let mut expected = vec![Call::Start(request)];
            if told {
                expected.push(Call::Note(request, note_text(None, request, CALLED_NOTE)));
            }
            expected.push(Call::End(request));
            assert_eq!(*sink.calls.borrow(), expected, "{room:?}");
        }
    }

    fn controller() -> Controller {
        let mut owner = Controller::new(BootId::new([9; 16]).unwrap(), time());
        owner
            .set_microphone_permission(time(), Permission::Allowed)
            .unwrap();
        owner
            .set_capture(
                time(),
                CaptureState::RetainingUntil(MonoTime::from_micros(NOW + 1_000_000)),
            )
            .unwrap();
        owner
            .set_admission(
                time(),
                AdmissionState::OpenUntil(MonoTime::from_micros(NOW + 1_000_000)),
            )
            .unwrap();
        owner
    }
    fn time() -> MonoTime {
        MonoTime::from_micros(NOW)
    }
    fn authorize(owner: &mut Controller, handoff: &mut InputHandoff) -> u64 {
        let turn = owner.admit(time(), AdmittedInput::NewTurn).unwrap();
        handoff
            .observe_authority(owner.snapshot(time()).unwrap())
            .unwrap();
        turn.turn()
    }
    fn generation(handoff: &InputHandoff) -> u64 {
        handoff.authority.unwrap().microphone_generation()
    }
    fn start(request: u64, handoff: &InputHandoff) -> ProviderInput {
        ProviderInput::Start {
            session: 1,
            request,
            privacy_generation: generation(handoff),
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        }
    }

    #[test]
    fn start_wire_requires_an_explicit_selected_session() {
        let command = serde_json::json!({
            "kind":"start", "request":1, "session":7, "privacy_generation":2
        });
        assert!(matches!(
            serde_json::from_value::<ProviderInput>(command.clone()).unwrap(),
            ProviderInput::Start {
                request: 1,
                session: 7,
                privacy_generation: 2,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            }
        ));
        for field in ["request", "session", "privacy_generation"] {
            let mut missing = command.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<ProviderInput>(missing).is_err());
        }
        let mut unknown = command;
        unknown["current_session"] = serde_json::json!(8);
        assert!(serde_json::from_value::<ProviderInput>(unknown).is_err());
    }

    #[test]
    fn a_start_that_continues_says_so_and_one_that_does_not_is_as_it_was() {
        let start = |continues| ProviderInput::Start {
            request: 4,
            session: 7,
            privacy_generation: 2,
            continues,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        };
        // Without `LAMP_CUT_KEEPS_NAME` the coordinator never sets it, and
        // the line on the worker's socket is the one it always was.
        assert_eq!(
            serde_json::to_string(&start(false)).unwrap(),
            r#"{"kind":"start","request":4,"session":7,"privacy_generation":2}"#
        );
        let said = serde_json::to_value(start(true)).unwrap();
        assert_eq!(said["continues"], serde_json::json!(true));
        assert!(matches!(
            serde_json::from_value::<ProviderInput>(said).unwrap(),
            ProviderInput::Start {
                continues: true,
                ..
            }
        ));
    }

    /// A request that goes on from one ended early says so on the wire;
    /// every other Start is written exactly as it was before the flag, and
    /// a Start written without it reads as not going on from anything.
    #[test]
    fn a_start_is_written_as_before_unless_it_goes_on_from_one_cut_off() {
        let plain = ProviderInput::Start {
            request: 1,
            session: 7,
            privacy_generation: 2,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        };
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            r#"{"kind":"start","request":1,"session":7,"privacy_generation":2}"#
        );
        let cut_off = ProviderInput::Start {
            request: 2,
            session: 7,
            privacy_generation: 2,
            continues: false,
            continues_cut_off: true,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        };
        let written = serde_json::to_value(&cut_off).unwrap();
        assert_eq!(written["continues_cut_off"], true);
        assert!(matches!(
            serde_json::from_value::<ProviderInput>(written).unwrap(),
            ProviderInput::Start {
                continues_cut_off: true,
                ..
            }
        ));
        let stopped = ProviderInput::Start {
            request: 3,
            session: 7,
            privacy_generation: 2,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: true,
            follows_on: false,
            over_the_lamps_voice: false,
        };
        assert_eq!(serde_json::to_value(&stopped).unwrap()["after_stop"], true);
        let old =
            serde_json::json!({"kind":"start","request":1,"session":7,"privacy_generation":2});
        assert!(matches!(
            serde_json::from_value::<ProviderInput>(old).unwrap(),
            ProviderInput::Start {
                continues_cut_off: false,
                ..
            }
        ));
    }

    /// The note is the one `early_end_probe` tried on the service, word for
    /// word (docs/early-end-taken-back.md).
    #[test]
    fn the_cut_off_note_is_the_one_tried_on_the_service() {
        let probe = include_str!("../../gemini/examples/early_end_probe/trials.rs");
        assert!(probe.contains(&format!("pub const REST_NOTE: &str = \"{CUT_OFF_NOTE}\";")));
    }

    /// A block's capture time is carried over to the transport's clock by
    /// reading two clocks, one after the other, for every block. A delay
    /// between the two readings for one block must not make the next block,
    /// captured after it, seem captured before it: the transport refuses
    /// capture times that go backwards, and for good.
    #[test]
    fn a_delay_between_the_two_clock_readings_never_puts_a_later_block_before_an_earlier_one() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let request = authorize(&mut owner, &mut handoff);
        let sink = RecordingSink::default();
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        let block = |sequence, read_completed_at_us, handoff: &InputHandoff| ProviderInput::Audio {
            request,
            privacy_generation: generation(handoff),
            sequence,
            read_completed_at_us,
            samples: vec![1; 160],
        };
        // One millisecond old when the shared clock is read for it.
        handoff
            .submit(block(1, NOW - 1_000, &handoff), NOW, &sink)
            .unwrap();
        // Captured a tenth of a millisecond later. By the shared clock fifty
        // milliseconds have passed when it is handed over; by the other
        // clock, next to none. That is what a delay of fifty milliseconds
        // between the first block's two readings looks like from here.
        handoff
            .submit(block(2, NOW - 900, &handoff), NOW + 50_000, &sink)
            .unwrap();
        {
            let captured = sink.captured.borrow();
            assert!(
                captured[1] >= captured[0],
                "the later block was handed over as captured {:?} before the earlier one",
                captured[0] - captured[1]
            );
        }
        // A capture time that really is earlier has nothing to do with the
        // two readings. It is refused on the shared clock, and nothing of
        // it is handed over.
        assert!(
            handoff
                .submit(block(3, NOW - 100_000, &handoff), NOW + 50_000, &sink)
                .is_err()
        );
        assert_eq!(sink.captured.borrow().len(), 2);
    }

    /// Positive controls for the refusal below. Order is decided on the
    /// shared clock, within one request: the same time twice is in order; a
    /// refusal hands nothing over and remembers nothing; and a newer request
    /// begins its own order, so an opening it kept from before the older
    /// request's last block is accepted.
    #[test]
    fn capture_order_is_decided_per_request_and_a_refusal_changes_nothing() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        let block = |request, sequence, read_completed_at_us, handoff: &InputHandoff| {
            ProviderInput::Audio {
                request,
                privacy_generation: generation(handoff),
                sequence,
                read_completed_at_us,
                samples: vec![1; 160],
            }
        };
        handoff
            .submit(block(old, 1, NOW - 5_000, &handoff), NOW, &sink)
            .unwrap();
        // The same shared time again is not earlier.
        handoff
            .submit(block(old, 2, NOW - 5_000, &handoff), NOW, &sink)
            .unwrap();
        let kept = handoff.submitted.unwrap().captured;
        assert_eq!(kept.map(|(at_us, _)| at_us), Some(NOW - 5_000));
        {
            let captured = sink.captured.borrow();
            assert!(captured[1] >= captured[0]);
        }
        // One microsecond earlier, with no delay anywhere: refused by name.
        let calls = sink.calls.borrow().len();
        let error = handoff
            .submit(block(old, 3, NOW - 5_001, &handoff), NOW, &sink)
            .unwrap_err();
        assert!(
            error.to_string().contains("capture time went backwards"),
            "{error}"
        );
        assert_eq!(sink.calls.borrow().len(), calls);
        assert_eq!(sink.captured.borrow().len(), 2);
        assert_eq!(handoff.submitted.unwrap().captured, kept);
        // A newer request: its first block was captured before the older
        // request's last, and that is its own affair.
        let new = authorize(&mut owner, &mut handoff);
        handoff.synchronize(&sink).unwrap();
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        handoff
            .submit(block(new, 1, NOW - 50_000, &handoff), NOW, &sink)
            .unwrap();
        assert_eq!(sink.captured.borrow().len(), 3);
        assert_eq!(
            handoff.submitted.unwrap().captured.map(|(at_us, _)| at_us),
            Some(NOW - 50_000)
        );
    }

    #[test]
    fn audit_shared_clock_regression_cannot_be_hidden_by_a_later_clock_gap() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let request = authorize(&mut owner, &mut handoff);
        let sink = RecordingSink::default();
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        // A valid previously accepted pair. At the following Instant read,
        // 100 ms has elapsed since that handoff. Its shared capture timestamp
        // was 200 ms old at that first handoff, hence this Instant is 300 ms old.
        let prior_us = NOW - 200_000;
        let prior = Instant::now() - Duration::from_millis(300);
        handoff.submitted.as_mut().unwrap().captured = Some((prior_us, prior));
        sink.captured.borrow_mut().push(prior);
        // The second shared-clock sample is 50 ms after the first handoff;
        // another 50 ms passes before its Instant read. The original capture
        // time really regresses by 1 us. No sleep or timeout widening is needed
        // to seed this legal prior state and a delayed current clock sample.
        let result = handoff.submit(
            ProviderInput::Audio {
                request,
                privacy_generation: generation(&handoff),
                sequence: 2,
                read_completed_at_us: prior_us - 1,
                samples: vec![2; 160],
            },
            NOW + 50_000,
            &sink,
        );
        let captured = sink.captured.borrow();
        assert!(
            result.is_err() || captured.last().is_some_and(|at| *at < prior),
            "a genuinely regressing shared capture timestamp was accepted and mapped forward by {:?}; Gemini's Instant-only order check cannot reject it",
            captured.last().unwrap().checked_duration_since(prior)
        );
    }

    #[test]
    fn zero_session_fails_without_a_submission_or_not_delivered_receipt() {
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let request = authorize(&mut owner, &mut handoff);
        let sink = RecordingSink::default();
        assert!(
            handoff
                .submit(
                    ProviderInput::Start {
                        request,
                        session: 0,
                        privacy_generation: generation(&handoff),
                        continues: false,
                        continues_cut_off: false,
                        over_the_lamp: false,
                        after_stop: false,
                        follows_on: false,
                        over_the_lamps_voice: false,
                    },
                    NOW,
                    &sink
                )
                .is_err()
        );
        assert!(sink.calls.borrow().is_empty());
        assert!(handoff.submitted.is_none());
        assert!(handoff.undelivered.is_none());
        assert_eq!(handoff.failed_through, 0);
    }

    #[test]
    fn checked_session_refusal_is_typed_only_and_never_spends_the_live_input() {
        for tolerate_outage in [false, true] {
            let mut owner = controller();
            let mut handoff = InputHandoff {
                tolerate_outage,
                ..InputHandoff::default()
            };
            let request = authorize(&mut owner, &mut handoff);
            let sink = RecordingSink::default();
            sink.session.set(2);
            // The checked comparison must precede every mutating input call.
            let result = handoff.submit(start(request, &handoff), NOW, &sink);
            assert_eq!(result.is_ok(), tolerate_outage);
            assert_eq!(handoff.undelivered, tolerate_outage.then_some(request));
            assert_eq!(
                handoff.failed_through,
                if tolerate_outage { request } else { 0 }
            );
            assert!(handoff.submitted.is_none());
            assert!(sink.calls.borrow().is_empty());
            assert_eq!(sink.retired.get(), 0);
            assert!(sink.open.get().is_none());
        }
    }
    fn audio(request: u64, sequence: u64, sample: i16, handoff: &InputHandoff) -> ProviderInput {
        ProviderInput::Audio {
            request,
            privacy_generation: generation(handoff),
            sequence,
            read_completed_at_us: NOW - 200_000,
            samples: vec![sample; 160],
        }
    }
    fn end(request: u64, handoff: &InputHandoff) -> ProviderInput {
        ProviderInput::End {
            request,
            privacy_generation: generation(handoff),
            sound: false,
            cut_in: false,
        }
    }

    fn intake_channels() -> (
        crate::process::SessionDirectory,
        WorkerChannels,
        WorkerChannels,
    ) {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let parent_boot = BootId::new([9; 16]).unwrap();
        let worker_boot = BootId::new([8; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(&directory.path, "provider", parent_boot, worker_boot, false)
                .unwrap();
        let mut worker =
            WorkerChannels::bind(&directory.path, "provider", parent_boot, worker_boot, true)
                .unwrap();
        parent.connect(&directory.path, "provider", false).unwrap();
        worker.connect(&directory.path, "provider", true).unwrap();
        (directory, parent, worker)
    }

    #[test]
    fn separate_socket_authority_in_empty_drain_gap_precedes_new_start() {
        let (_directory, mut parent, mut worker) = intake_channels();
        let mut owner = controller();
        let mut intake = ProviderIntake::new(BootId::new([9; 16]).unwrap(), NOW);
        let sink = RecordingSink::default();
        let initial = owner.snapshot(time()).unwrap();
        parent
            .control
            .send(Control::Authority { snapshot: initial })
            .unwrap();
        let mut initial_budget = CONTROL_SLICE;
        intake
            .drain_control(
                &mut || NOW,
                &mut || worker.control.receive(),
                &mut initial_budget,
            )
            .unwrap();
        let turn = owner.admit(time(), AdmittedInput::NewTurn).unwrap();
        let state = owner.snapshot(time()).unwrap();
        let mut injected = false;
        let mut control_budget = CONTROL_SLICE;
        assert_eq!(
            intake
                .drain_control(
                    &mut || NOW,
                    &mut || {
                        let control = worker.control.receive()?;
                        if !injected {
                            assert!(control.is_none());
                            injected = true;
                            // Force the legal interleaving: parent sends control before data,
                            // but both sends occur after the worker observed an empty control socket.
                            parent
                                .control
                                .send(Control::Authority { snapshot: state })?;
                            parent.data.send(ProviderInput::Start {
                                session: 1,
                                request: turn.turn(),
                                privacy_generation: state.microphone_generation(),
                                continues: false,
                                continues_cut_off: false,
                                over_the_lamp: false,
                                after_stop: false,
                                follows_on: false,
                                over_the_lamps_voice: false,
                            })?;
                        }
                        Ok(control)
                    },
                    &mut control_budget
                )
                .unwrap(),
            ControlDrain::Empty
        );
        assert_eq!(
            intake
                .receive_inputs(
                    &mut || NOW,
                    &mut || worker.control.receive(),
                    &mut worker.data,
                    &sink,
                    &mut control_budget
                )
                .unwrap(),
            ControlDrain::Empty
        );
        assert!(injected);
        assert_eq!(*sink.calls.borrow(), vec![Call::Start(turn.turn())]);
    }

    struct SocketRig {
        _directory: crate::process::SessionDirectory,
        parent: WorkerChannels,
        worker: WorkerChannels,
        owner: Controller,
        intake: ProviderIntake,
        sink: RecordingSink,
    }
    impl SocketRig {
        fn new() -> Self {
            let (directory, parent, worker) = intake_channels();
            let mut rig = Self {
                _directory: directory,
                parent,
                worker,
                owner: controller(),
                intake: ProviderIntake::new(BootId::new([9; 16]).unwrap(), NOW),
                sink: RecordingSink::default(),
            };
            rig.owner.admit(time(), AdmittedInput::NewTurn).unwrap();
            let state = rig.owner.snapshot(time()).unwrap();
            rig.parent
                .control
                .send(Control::Authority { snapshot: state })
                .unwrap();
            rig.parent
                .data
                .send(ProviderInput::Start {
                    session: 1,
                    request: 1,
                    privacy_generation: state.microphone_generation(),
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                })
                .unwrap();
            assert_eq!(rig.tick(NOW).unwrap(), ControlDrain::Empty);
            assert_eq!(*rig.sink.calls.borrow(), vec![Call::Start(1)]);
            rig
        }
        fn tick(&mut self, at: u64) -> Result<ControlDrain> {
            let mut budget = CONTROL_SLICE;
            let control = self.intake.drain_control(
                &mut || at,
                &mut || self.worker.control.receive(),
                &mut budget,
            )?;
            if control == ControlDrain::Stopped {
                return Ok(control);
            }
            self.intake.handoff.synchronize(&self.sink)?;
            if control == ControlDrain::Deferred {
                return Ok(control);
            }
            self.intake.receive_inputs(
                &mut || at,
                &mut || self.worker.control.receive(),
                &mut self.worker.data,
                &self.sink,
                &mut budget,
            )
        }
    }

    #[test]
    fn separate_socket_interruption_retires_old_data_and_keeps_new_prefix() {
        let mut rig = SocketRig::new();
        let new = rig
            .owner
            .admit(time(), AdmittedInput::Interruption)
            .unwrap();
        let state = rig.owner.snapshot(time()).unwrap();
        // Already queued old audio is intentionally malformed; once retired it
        // must not poison the next question's sequence, samples or capture age.
        rig.parent
            .data
            .send(ProviderInput::Audio {
                request: 1,
                privacy_generation: state.microphone_generation(),
                sequence: u64::MAX,
                read_completed_at_us: u64::MAX,
                samples: vec![],
            })
            .unwrap();
        let mut injected = false;
        let mut budget = CONTROL_SLICE;
        assert_eq!(
            rig.intake
                .drain_control(
                    &mut || NOW,
                    &mut || {
                        let control = rig.worker.control.receive()?;
                        if !injected {
                            assert!(control.is_none());
                            injected = true;
                            rig.parent
                                .control
                                .send(Control::Authority { snapshot: state })?;
                            for command in [
                                ProviderInput::Start {
                                    session: 1,
                                    request: new.turn(),
                                    privacy_generation: state.microphone_generation(),
                                    continues: false,
                                    continues_cut_off: false,
                                    over_the_lamp: false,
                                    after_stop: false,
                                    follows_on: false,
                                    over_the_lamps_voice: false,
                                },
                                ProviderInput::Audio {
                                    request: new.turn(),
                                    privacy_generation: state.microphone_generation(),
                                    sequence: 1,
                                    read_completed_at_us: NOW - 200_000,
                                    samples: vec![21; 160],
                                },
                                ProviderInput::End {
                                    request: new.turn(),
                                    privacy_generation: state.microphone_generation(),
                                    sound: false,
                                    cut_in: false,
                                },
                            ] {
                                rig.parent.data.send(command)?;
                            }
                        }
                        Ok(control)
                    },
                    &mut budget
                )
                .unwrap(),
            ControlDrain::Empty
        );
        assert_eq!(
            rig.intake
                .receive_inputs(
                    &mut || NOW,
                    &mut || rig.worker.control.receive(),
                    &mut rig.worker.data,
                    &rig.sink,
                    &mut budget
                )
                .unwrap(),
            ControlDrain::Empty
        );
        assert_eq!(
            *rig.sink.calls.borrow(),
            vec![
                Call::Start(1),
                Call::Retire(1),
                Call::End(1),
                Call::Start(2),
                Call::Audio(2, 1, vec![21; 160]),
                Call::End(2)
            ]
        );
    }

    #[test]
    fn separate_socket_stop_privacy_and_coalesced_reopen_precede_input() {
        for mode in 0..3 {
            let mut rig = SocketRig::new();
            let audio = audio(1, 1, 41, &rig.intake.handoff);
            let priority = if mode == 0 {
                Control::Stop
            } else {
                rig.owner
                    .set_microphone_permission(time(), Permission::Denied)
                    .unwrap();
                if mode == 2 {
                    rig.owner
                        .set_microphone_permission(time(), Permission::Allowed)
                        .unwrap();
                }
                Control::Authority {
                    snapshot: rig.owner.snapshot(time()).unwrap(),
                }
            };
            let mut pending = Some((priority, audio));
            let mut budget = CONTROL_SLICE;
            assert_eq!(
                rig.intake
                    .drain_control(
                        &mut || NOW,
                        &mut || {
                            let control = rig.worker.control.receive()?;
                            if let Some((priority, command)) = pending.take() {
                                assert!(control.is_none());
                                rig.parent.control.send(priority)?;
                                rig.parent.data.send(command)?;
                            }
                            Ok(control)
                        },
                        &mut budget
                    )
                    .unwrap(),
                ControlDrain::Empty
            );
            assert_eq!(
                rig.intake
                    .receive_inputs(
                        &mut || NOW,
                        &mut || rig.worker.control.receive(),
                        &mut rig.worker.data,
                        &rig.sink,
                        &mut budget
                    )
                    .unwrap(),
                ControlDrain::Stopped
            );
            assert_eq!(*rig.sink.calls.borrow(), vec![Call::Start(1)]);
            assert!(rig.intake.retained.is_none());
            assert!(rig.tick(NOW).is_err());
        }
    }

    #[test]
    fn full_control_budget_defers_one_original_input_and_expires_at_original_receipt() {
        for expire in [false, true] {
            let mut rig = SocketRig::new();
            rig.owner
                .admit(time(), AdmittedInput::Interruption)
                .unwrap();
            let mut states: VecDeque<_> = (0..17)
                .map(|_| rig.owner.snapshot(time()).unwrap())
                .collect();
            let privacy = states[0].microphone_generation();
            let mut budget = CONTROL_SLICE;
            assert_eq!(
                rig.intake
                    .drain_control(
                        &mut || NOW,
                        &mut || rig.worker.control.receive(),
                        &mut budget
                    )
                    .unwrap(),
                ControlDrain::Empty
            );
            // Queue new input and authority after the initial drain. Further
            // authority packets are fed one per receive, independent of the OS
            // Unix datagram queue limit.
            rig.parent
                .data
                .send(ProviderInput::Start {
                    session: 1,
                    request: 2,
                    privacy_generation: privacy,
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                })
                .unwrap();
            rig.parent
                .data
                .send(ProviderInput::Audio {
                    request: 2,
                    privacy_generation: privacy,
                    sequence: 1,
                    read_completed_at_us: NOW - 200_000,
                    samples: vec![51; 160],
                })
                .unwrap();
            rig.parent
                .control
                .send(Control::Authority {
                    snapshot: states.pop_front().unwrap(),
                })
                .unwrap();
            assert_eq!(
                rig.intake
                    .receive_inputs(
                        &mut || NOW,
                        &mut || {
                            let control = rig.worker.control.receive()?;
                            if control.is_some()
                                && let Some(snapshot) = states.pop_front()
                            {
                                rig.parent.control.send(Control::Authority { snapshot })?;
                            }
                            Ok(control)
                        },
                        &mut rig.worker.data,
                        &rig.sink,
                        &mut budget
                    )
                    .unwrap(),
                ControlDrain::Deferred
            );
            assert_eq!(budget, 0);
            assert!(states.is_empty()); // Last authority remains on the control socket.
            let retained = rig.intake.retained.as_ref().unwrap();
            assert_eq!(retained.received_at_us, NOW);
            assert_eq!(retained.command.request(), 2);
            assert_eq!(
                *rig.sink.calls.borrow(),
                vec![Call::Start(1), Call::Retire(1), Call::End(1)]
            );
            if expire {
                let error = rig.tick(NOW + RETAINED_INPUT_US).unwrap_err();
                assert!(error.to_string().contains("retained input expired"));
                assert!(rig.intake.retained.is_none());
                assert_eq!(
                    *rig.sink.calls.borrow(),
                    vec![Call::Start(1), Call::Retire(1), Call::End(1)]
                );
            } else {
                assert_eq!(rig.tick(NOW + 1).unwrap(), ControlDrain::Empty);
                assert!(rig.intake.retained.is_none());
                assert_eq!(
                    *rig.sink.calls.borrow(),
                    vec![
                        Call::Start(1),
                        Call::Retire(1),
                        Call::End(1),
                        Call::Start(2),
                        Call::Audio(2, 1, vec![51; 160])
                    ]
                );
            }
        }
    }

    #[test]
    fn intake_keeps_eight_command_slice_and_does_not_wait_for_unknown_authority() {
        let mut rig = SocketRig::new();
        for sequence in 1..=9 {
            rig.parent
                .data
                .send(audio(1, sequence, sequence as i16, &rig.intake.handoff))
                .unwrap();
        }
        rig.tick(NOW).unwrap();
        assert_eq!(
            rig.sink
                .calls
                .borrow()
                .iter()
                .filter(|call| matches!(call, Call::Audio(..)))
                .count(),
            INPUT_SLICE
        );
        rig.tick(NOW).unwrap();
        assert_eq!(rig.sink.calls.borrow().len(), 10);
        rig.parent
            .data
            .send(ProviderInput::Start {
                session: 1,
                request: 2,
                privacy_generation: generation(&rig.intake.handoff),
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            })
            .unwrap();
        let error = rig.tick(NOW).unwrap_err();
        assert!(error.to_string().contains("no matching current owner"));
        assert!(rig.intake.retained.is_none());
        assert!(rig.intake.stopped);
    }

    #[test]
    fn selected_session_survives_mixed_control_deferral_and_refused_tail_drain() {
        let (_directory, mut parent, mut worker) = intake_channels();
        let mut owner = controller();
        let mut intake = ProviderIntake::new(BootId::new([9; 16]).unwrap(), NOW);
        intake.handoff.tolerate_outage = true;
        let sink = RecordingSink::default();
        sink.session.set(2);
        let request = authorize(&mut owner, &mut intake.handoff);
        let privacy_generation = generation(&intake.handoff);
        let mut controls: VecDeque<_> = (0..17)
            .map(|i| {
                if i % 2 == 0 {
                    Control::Authority {
                        snapshot: owner.snapshot(time()).unwrap(),
                    }
                } else {
                    Control::ProviderOutputCapacity { through: 2 }
                }
            })
            .collect();
        parent.control.send(controls.pop_front().unwrap()).unwrap();
        for command in [
            start(request, &intake.handoff),
            audio(request, 1, 61, &intake.handoff),
            end(request, &intake.handoff),
        ] {
            parent.data.send(command).unwrap();
        }
        // Receive the original data, then interleave authority/capacity packets
        // on the separate urgent socket until the shared control slice is full.
        let mut budget = CONTROL_SLICE;
        assert_eq!(
            intake
                .receive_inputs(
                    &mut || NOW,
                    &mut || {
                        let packet = worker.control.receive()?;
                        if packet.is_some()
                            && let Some(next) = controls.pop_front()
                        {
                            parent.control.send(next)?;
                        }
                        Ok(packet)
                    },
                    &mut worker.data,
                    &sink,
                    &mut budget
                )
                .unwrap(),
            ControlDrain::Deferred
        );
        assert_eq!(budget, 0);
        assert!(controls.is_empty());
        let retained = intake.retained.as_ref().unwrap();
        assert_eq!(retained.received_at_us, NOW);
        assert!(matches!(
            retained.command,
            ProviderInput::Start { session: 1, .. }
        ));
        assert!(sink.calls.borrow().is_empty());

        let mut budget = CONTROL_SLICE;
        assert_eq!(
            intake
                .receive_inputs(
                    &mut || NOW + 1,
                    &mut || worker.control.receive(),
                    &mut worker.data,
                    &sink,
                    &mut budget
                )
                .unwrap(),
            ControlDrain::Empty
        );
        assert_eq!(intake.handoff.undelivered, Some(request));
        let mut output = VecDeque::new();
        queue_undelivered(&mut intake.handoff, &mut output);
        let mut budget = CONTROL_SLICE;
        intake
            .receive_inputs(
                &mut || NOW + 2,
                &mut || worker.control.receive(),
                &mut worker.data,
                &sink,
                &mut budget,
            )
            .unwrap();
        queue_undelivered(&mut intake.handoff, &mut output);
        assert_eq!(output.len(), 1);
        assert!(
            matches!(output[0], Outbound::Status(ProviderStatus::TurnFailed {
            request: got, stage: FailedStage::NotDelivered, ..
        }) if got == request)
        );
        assert!(
            sink.calls.borrow().is_empty(),
            "A's prefix/tail must not reach B"
        );
        assert!(intake.handoff.submitted.is_none());

        let next = owner.admit(time(), AdmittedInput::NewTurn).unwrap();
        parent
            .control
            .send(Control::Authority {
                snapshot: owner.snapshot(time()).unwrap(),
            })
            .unwrap();
        for command in [
            ProviderInput::Start {
                request: next.turn(),
                session: 2,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            },
            ProviderInput::Audio {
                request: next.turn(),
                privacy_generation,
                sequence: 1,
                read_completed_at_us: NOW - 200_000,
                samples: vec![71; 160],
            },
            ProviderInput::End {
                request: next.turn(),
                privacy_generation,
                sound: false,
                cut_in: false,
            },
        ] {
            parent.data.send(command).unwrap();
        }
        let mut budget = CONTROL_SLICE;
        intake
            .receive_inputs(
                &mut || NOW + 3,
                &mut || worker.control.receive(),
                &mut worker.data,
                &sink,
                &mut budget,
            )
            .unwrap();
        assert_eq!(
            *sink.calls.borrow(),
            vec![
                Call::Retire(request),
                Call::Start(next.turn()),
                Call::Audio(next.turn(), 1, vec![71; 160]),
                Call::End(next.turn())
            ]
        );
        queue_undelivered(&mut intake.handoff, &mut output);
        assert_eq!(
            output.len(),
            1,
            "refused input has exactly one terminal report"
        );
    }

    #[test]
    fn intake_does_not_extend_snapshot_capture_or_controller_leases() {
        for capture_expiry in [false, true] {
            let mut rig = SocketRig::new();
            if capture_expiry {
                rig.owner
                    .set_capture(
                        time(),
                        CaptureState::RetainingUntil(MonoTime::from_micros(NOW + 10)),
                    )
                    .unwrap();
                rig.parent
                    .control
                    .send(Control::Authority {
                        snapshot: rig.owner.snapshot(time()).unwrap(),
                    })
                    .unwrap();
                rig.tick(NOW).unwrap();
            }
            let expires = rig
                .intake
                .handoff
                .authority
                .unwrap()
                .expires_at()
                .as_micros();
            rig.parent
                .data
                .send(audio(1, 1, 71, &rig.intake.handoff))
                .unwrap();
            assert!(rig.tick(expires).is_err());
            assert_eq!(*rig.sink.calls.borrow(), vec![Call::Start(1)]);
        }
        let mut rig = SocketRig::new();
        let error = rig.tick(NOW + 250_001).unwrap_err();
        assert!(error.to_string().contains("controller heartbeat expired"));
        assert!(rig.intake.stopped);
    }

    #[test]
    fn priority_transfer_closes_old_input_before_successor_and_discards_queued_old_data() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        handoff
            .submit(audio(old, 1, 11, &handoff), NOW, &sink)
            .unwrap();

        let new = authorize(&mut owner, &mut handoff);
        handoff.synchronize(&sink).unwrap();
        // These messages were already in the other IPC socket before transfer.
        for command in [
            audio(old, 2, 99, &handoff),
            end(old, &handoff),
            start(old, &handoff),
            end(old, &handoff),
        ] {
            handoff.submit(command, NOW, &sink).unwrap();
        }
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        handoff
            .submit(audio(new, 1, 21, &handoff), NOW, &sink)
            .unwrap();
        handoff
            .submit(audio(new, 2, 22, &handoff), NOW, &sink)
            .unwrap();
        handoff.submit(end(new, &handoff), NOW, &sink).unwrap();
        assert_eq!(
            *sink.calls.borrow(),
            vec![
                Call::Start(old),
                Call::Audio(old, 1, vec![11; 160]),
                Call::Retire(old),
                Call::End(old),
                Call::Start(new),
                Call::Audio(new, 1, vec![21; 160]),
                Call::Audio(new, 2, vec![22; 160]),
                Call::End(new),
            ]
        );
    }

    #[test]
    fn an_already_submitted_end_is_not_repeated_on_retirement() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        handoff.submit(end(old, &handoff), NOW, &sink).unwrap();
        let new = authorize(&mut owner, &mut handoff);
        handoff.synchronize(&sink).unwrap();
        handoff.synchronize(&sink).unwrap();
        handoff.submit(end(old, &handoff), NOW, &sink).unwrap();
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        assert_eq!(
            *sink.calls.borrow(),
            vec![
                Call::Start(old),
                Call::End(old),
                Call::Retire(old),
                Call::Start(new)
            ]
        );
    }

    #[test]
    fn overtaken_start_never_creates_a_phantom_old_input_or_end() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        let new = authorize(&mut owner, &mut handoff);
        for command in [
            start(old, &handoff),
            audio(old, 1, 11, &handoff),
            end(old, &handoff),
        ] {
            handoff.submit(command, NOW, &sink).unwrap();
        }
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        assert_eq!(
            *sink.calls.borrow(),
            vec![Call::Retire(old), Call::Start(new)]
        );
    }

    /// The end sent early (`--early-end-ms`): a request ended here while
    /// the lamp still listened is told apart from every request ended as
    /// before, and only it, when it is given up, has its words unsaid.
    #[test]
    fn only_a_request_given_up_before_the_lamp_was_sure_is_unsaid() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        // As always: the authority stops listening, then the end comes, and
        // the request is later replaced by a new one.
        let first = authorize(&mut owner, &mut handoff);
        handoff.submit(start(first, &handoff), NOW, &sink).unwrap();
        let held = owner.snapshot(time()).unwrap().owner().unwrap();
        owner.input_ended(time(), held).unwrap();
        handoff
            .observe_authority(owner.snapshot(time()).unwrap())
            .unwrap();
        handoff.submit(end(first, &handoff), NOW, &sink).unwrap();
        owner.cancel_turn(time(), held).unwrap();
        let next = owner.admit(time(), AdmittedInput::Interruption).unwrap();
        assert_eq!(
            handoff.given_up_before_sure(owner.snapshot(time()).unwrap()),
            None
        );
        handoff
            .observe_authority(owner.snapshot(time()).unwrap())
            .unwrap();
        // Ended early: the end comes while the authority still listens.
        handoff
            .submit(start(next.turn(), &handoff), NOW, &sink)
            .unwrap();
        handoff
            .submit(end(next.turn(), &handoff), NOW, &sink)
            .unwrap();
        // Confirmed: the authority stops listening. Nothing is unsaid.
        owner.input_ended(time(), next).unwrap();
        assert_eq!(
            handoff.given_up_before_sure(owner.snapshot(time()).unwrap()),
            None
        );
        // Given up before that, and continued: unsaid. (The handoff has
        // seen only the authority that still listened.)
        owner.cancel_turn(time(), next).unwrap();
        let third = owner.admit(time(), AdmittedInput::Interruption).unwrap();
        assert_eq!(
            handoff.given_up_before_sure(owner.snapshot(time()).unwrap()),
            Some(next.turn())
        );
        // And given up with nobody after it.
        owner.cancel_turn(time(), third).unwrap();
        assert_eq!(
            handoff.given_up_before_sure(owner.snapshot(time()).unwrap()),
            Some(next.turn())
        );
        // A request that never got as far as its end is not this.
        handoff.ended = None;
        assert_eq!(
            handoff.given_up_before_sure(owner.snapshot(time()).unwrap()),
            None
        );
    }

    #[test]
    fn coalesced_owner_then_cancellation_retires_a_start_not_yet_received() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        let snapshot = owner.snapshot(time()).unwrap();
        owner
            .cancel_turn(time(), snapshot.owner().unwrap())
            .unwrap();
        handoff
            .observe_authority(owner.snapshot(time()).unwrap())
            .unwrap();
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        handoff.submit(end(old, &handoff), NOW, &sink).unwrap();
        assert_eq!(*sink.calls.borrow(), vec![Call::Retire(old)]);
        assert!(handoff.observe_authority(snapshot).is_err());
    }

    #[test]
    fn a_future_or_unknown_request_cannot_borrow_current_authority() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let current = authorize(&mut owner, &mut handoff);
        for command in [
            start(0, &handoff),
            start(current + 1, &handoff),
            audio(current + 1, 1, 90, &handoff),
            end(current + 1, &handoff),
        ] {
            assert!(handoff.submit(command, NOW, &sink).is_err());
        }
        assert!(sink.calls.borrow().is_empty());
        handoff
            .submit(start(current, &handoff), NOW, &sink)
            .unwrap();
    }

    #[test]
    fn input_requires_a_successful_start_and_cannot_reopen_after_end() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let current = authorize(&mut owner, &mut handoff);
        assert!(
            handoff
                .submit(audio(current, 1, 10, &handoff), NOW, &sink)
                .is_err()
        );
        assert!(handoff.submit(end(current, &handoff), NOW, &sink).is_err());
        handoff
            .submit(start(current, &handoff), NOW, &sink)
            .unwrap();
        assert!(
            handoff
                .submit(start(current, &handoff), NOW, &sink)
                .is_err()
        );
        handoff.submit(end(current, &handoff), NOW, &sink).unwrap();
        assert!(handoff.submit(end(current, &handoff), NOW, &sink).is_err());
        assert!(
            handoff
                .submit(audio(current, 2, 20, &handoff), NOW, &sink)
                .is_err()
        );
        assert_eq!(
            *sink.calls.borrow(),
            vec![Call::Start(current), Call::End(current)]
        );
    }

    #[test]
    fn close_backpressure_revokes_output_but_never_starts_a_successor() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        let new = authorize(&mut owner, &mut handoff);
        sink.reject_end.set(true);
        assert!(handoff.submit(start(new, &handoff), NOW, &sink).is_err());
        assert_eq!(
            *sink.calls.borrow(),
            vec![Call::Start(old), Call::Retire(old), Call::EndBlocked(old)]
        );
        assert_eq!(handoff.submitted.unwrap().request.get(), old);
        assert!(handoff.submitted.unwrap().open);
    }

    #[test]
    fn rejected_start_is_not_later_closed_as_if_it_had_been_submitted() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        sink.reject_start.set(true);
        assert!(handoff.submit(start(old, &handoff), NOW, &sink).is_err());
        sink.reject_start.set(false);
        let new = authorize(&mut owner, &mut handoff);
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        assert_eq!(
            *sink.calls.borrow(),
            vec![Call::Retire(old), Call::Start(new)]
        );
    }

    #[test]
    fn outage_after_accepted_start_never_reports_that_request_as_not_delivered() {
        for refuse_end in [false, true] {
            let mut owner = controller();
            let mut handoff = InputHandoff {
                tolerate_outage: true,
                ..InputHandoff::default()
            };
            let sink = RecordingSink::default();
            let request = authorize(&mut owner, &mut handoff);
            handoff
                .submit(start(request, &handoff), NOW, &sink)
                .unwrap();
            handoff
                .submit(audio(request, 1, 7, &handoff), NOW, &sink)
                .unwrap();
            sink.disconnected.set(true);
            let refused = if refuse_end {
                end(request, &handoff)
            } else {
                audio(request, 2, 8, &handoff)
            };
            handoff.submit(refused, NOW, &sink).unwrap();
            assert_eq!(
                handoff.undelivered, None,
                "the supervisor owns the accepted request's actual terminal stage"
            );
            assert_eq!(handoff.failed_through, request);
            assert!(handoff.submitted.is_none());
            // The remaining input is discarded even after connectivity returns.
            sink.disconnected.set(false);
            handoff
                .submit(audio(request, 3, 9, &handoff), NOW, &sink)
                .unwrap();
            handoff.submit(end(request, &handoff), NOW, &sink).unwrap();
            assert_eq!(
                *sink.calls.borrow(),
                vec![Call::Start(request), Call::Audio(request, 1, vec![7; 160])]
            );
        }
    }

    #[test]
    fn an_overrun_input_queue_is_an_outage_and_not_the_end_of_the_worker() {
        // After an accepted Start, on its audio or on its end: the request
        // is failed here, the supervisor owns its terminal stage, and the
        // rest of what was said in it is discarded.
        for overrun_on_end in [false, true] {
            let mut owner = controller();
            let mut handoff = InputHandoff {
                tolerate_outage: true,
                ..InputHandoff::default()
            };
            let sink = RecordingSink::default();
            let request = authorize(&mut owner, &mut handoff);
            handoff
                .submit(start(request, &handoff), NOW, &sink)
                .unwrap();
            handoff
                .submit(audio(request, 1, 7, &handoff), NOW, &sink)
                .unwrap();
            sink.overrun.set(true);
            let refused = if overrun_on_end {
                end(request, &handoff)
            } else {
                audio(request, 2, 8, &handoff)
            };
            handoff.submit(refused, NOW, &sink).unwrap();
            assert_eq!(handoff.undelivered, None);
            assert_eq!(handoff.failed_through, request);
            assert!(handoff.submitted.is_none());
            handoff
                .submit(audio(request, 3, 9, &handoff), NOW, &sink)
                .unwrap();
            handoff.submit(end(request, &handoff), NOW, &sink).unwrap();
            assert_eq!(
                *sink.calls.borrow(),
                vec![Call::Start(request), Call::Audio(request, 1, vec![7; 160])]
            );
            // The next thing said, on the connection that replaced it, is
            // an ordinary request.
            sink.disconnected.set(false);
            sink.open.set(None);
            let next = authorize(&mut owner, &mut handoff);
            handoff.submit(start(next, &handoff), NOW, &sink).unwrap();
            assert_eq!(handoff.submitted.unwrap().request.get(), next);
        }
        // On the Start itself: nothing of the request was accepted, and the
        // worker owes the report that it was not delivered.
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let sink = RecordingSink::default();
        let request = authorize(&mut owner, &mut handoff);
        sink.overrun.set(true);
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        assert_eq!(handoff.undelivered.take(), Some(request));
        assert_eq!(handoff.failed_through, request);
        assert!(sink.calls.borrow().is_empty());
        // While the request before is being closed for its successor.
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        let new = authorize(&mut owner, &mut handoff);
        sink.overrun.set(true);
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        assert_eq!(handoff.undelivered.take(), Some(new));
        assert!(handoff.submitted.is_none());
        // Without typed recovery it still ends the worker, as before.
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let request = authorize(&mut owner, &mut handoff);
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        sink.overrun.set(true);
        assert!(
            handoff
                .submit(audio(request, 1, 7, &handoff), NOW, &sink)
                .is_err()
        );
    }

    #[test]
    fn outage_before_start_reports_not_delivered_once_and_never_replays_input() {
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let sink = RecordingSink::default();
        sink.disconnected.set(true);
        let request = authorize(&mut owner, &mut handoff);
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        assert_eq!(handoff.undelivered.take(), Some(request));
        sink.disconnected.set(false);
        for command in [
            start(request, &handoff),
            audio(request, 1, 9, &handoff),
            end(request, &handoff),
        ] {
            handoff.submit(command, NOW, &sink).unwrap();
        }
        assert_eq!(handoff.undelivered, None);
        assert!(sink.calls.borrow().is_empty());
    }

    #[test]
    fn typed_recovery_does_not_swallow_an_unproven_stale_request() {
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let sink = RecordingSink::default();
        let request = authorize(&mut owner, &mut handoff);
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        assert!(
            handoff
                .refused(RequestId::new(request).unwrap(), Error::StaleRequest, &sink)
                .is_err()
        );
        assert_eq!(handoff.failed_through, 0);
        assert_eq!(handoff.undelivered, None);
        assert_eq!(handoff.submitted.unwrap().request.get(), request);
    }

    #[test]
    fn pending_not_delivered_report_cannot_be_overwritten_by_a_new_refused_start() {
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let sink = RecordingSink::default();
        sink.disconnected.set(true);
        let old = authorize(&mut owner, &mut handoff);
        handoff.submit(start(old, &handoff), NOW, &sink).unwrap();
        let next = authorize(&mut owner, &mut handoff);
        assert!(handoff.submit(start(next, &handoff), NOW, &sink).is_err());
        assert_eq!(handoff.undelivered, Some(old));
        assert_eq!(handoff.failed_through, old);
        let mut output = VecDeque::new();
        queue_undelivered(&mut handoff, &mut output);
        handoff.submit(start(next, &handoff), NOW, &sink).unwrap();
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(output.len(), 2);
        for request in [old, next] {
            assert!(
                matches!(output.pop_front(), Some(Outbound::Status(ProviderStatus::TurnFailed {
                request: reported, stage: FailedStage::NotDelivered, ..
            })) if reported == request)
            );
        }
    }

    #[test]
    fn refused_start_outcome_waits_for_capacity_without_growing_output() {
        let mut handoff = InputHandoff {
            undelivered: Some(7),
            ..InputHandoff::default()
        };
        let mut output = VecDeque::new();
        for sequence in 1..=MAX_QUEUED_OUTPUT {
            output.push_back(Outbound::Output(ProviderOutput::Audio {
                request: 6,
                sequence: sequence as u64,
                provider_event_at_us: NOW,
                source_frames: PACKET_SAMPLES,
                source_offset: 0,
                samples: vec![2; PACKET_SAMPLES],
            }));
        }
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(handoff.undelivered, Some(7));
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
        output.pop_front();
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(handoff.undelivered, None);
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
        assert!(matches!(
            output.back(),
            Some(Outbound::Status(ProviderStatus::TurnFailed {
                request: 7,
                stage: FailedStage::NotDelivered,
                ..
            }))
        ));
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
    }

    #[test]
    fn refused_start_pauses_intake_before_more_pcm_but_does_not_block_stop() {
        let (_directory, mut parent, mut worker) = intake_channels();
        let mut owner = controller();
        let mut intake = ProviderIntake::new(BootId::new([9; 16]).unwrap(), NOW);
        intake.handoff.tolerate_outage = true;
        let request = authorize(&mut owner, &mut intake.handoff);
        let sink = RecordingSink::default();
        sink.disconnected.set(true);
        for command in [
            start(request, &intake.handoff),
            audio(request, 1, 8, &intake.handoff),
            end(request, &intake.handoff),
        ] {
            parent.data.send(command).unwrap();
        }
        let mut budget = CONTROL_SLICE;
        assert_eq!(
            intake
                .receive_inputs(
                    &mut || NOW,
                    &mut || worker.control.receive(),
                    &mut worker.data,
                    &sink,
                    &mut budget
                )
                .unwrap(),
            ControlDrain::Empty
        );
        assert_eq!(intake.handoff.undelivered, Some(request));
        assert!(
            matches!(worker.data.receive::<ProviderInput>().unwrap(), Some(ProviderInput::Audio { request: received, .. }) if received == request)
        );
        parent.control.send(Control::Stop).unwrap();
        assert_eq!(
            intake
                .drain_control(&mut || NOW, &mut || worker.control.receive(), &mut budget)
                .unwrap(),
            ControlDrain::Stopped
        );
        assert!(intake.stopped);
        assert!(sink.calls.borrow().is_empty());
    }

    #[test]
    fn privacy_generation_and_expired_authority_still_prevent_audio() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let current = authorize(&mut owner, &mut handoff);
        handoff
            .submit(start(current, &handoff), NOW, &sink)
            .unwrap();
        let mut wrong_generation = audio(current, 1, 10, &handoff);
        if let ProviderInput::Audio {
            privacy_generation, ..
        } = &mut wrong_generation
        {
            *privacy_generation += 1;
        }
        assert!(handoff.submit(wrong_generation, NOW, &sink).is_err());
        let expires = handoff.authority.unwrap().expires_at().as_micros();
        assert!(
            handoff
                .submit(audio(current, 1, 10, &handoff), expires, &sink)
                .is_err()
        );
        owner
            .set_microphone_permission(time(), Permission::Denied)
            .unwrap();
        handoff
            .observe_authority(owner.snapshot(time()).unwrap())
            .unwrap();
        assert!(
            handoff
                .submit(audio(current, 1, 10, &handoff), NOW, &sink)
                .is_err()
        );
        assert!(
            sink.calls
                .borrow()
                .iter()
                .all(|call| !matches!(call, Call::Audio(..)))
        );
    }

    #[test]
    fn retained_prefix_keeps_capture_age_and_rejects_stale_or_future_samples() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let current = authorize(&mut owner, &mut handoff);
        handoff
            .submit(start(current, &handoff), NOW, &sink)
            .unwrap();
        handoff
            .submit(audio(current, 1, 10, &handoff), NOW, &sink)
            .unwrap();
        for captured_at in [NOW - 1_000_000, NOW + 1] {
            let mut invalid = audio(current, 2, 90, &handoff);
            if let ProviderInput::Audio {
                read_completed_at_us,
                ..
            } = &mut invalid
            {
                *read_completed_at_us = captured_at;
            }
            assert!(handoff.submit(invalid, NOW, &sink).is_err());
        }
        assert_eq!(
            *sink.calls.borrow(),
            vec![Call::Start(current), Call::Audio(current, 1, vec![10; 160])]
        );
    }

    #[test]
    fn retired_capture_metadata_does_not_poison_successor_sequence_or_prefix() {
        let mut owner = controller();
        let mut handoff = InputHandoff::default();
        let sink = RecordingSink::default();
        let old = authorize(&mut owner, &mut handoff);
        let new = authorize(&mut owner, &mut handoff);
        let mut stale = audio(old, u64::MAX, 99, &handoff);
        if let ProviderInput::Audio {
            read_completed_at_us,
            samples,
            ..
        } = &mut stale
        {
            *read_completed_at_us = u64::MAX;
            samples.clear();
        }
        handoff.submit(stale, NOW, &sink).unwrap();
        handoff.submit(start(new, &handoff), NOW, &sink).unwrap();
        handoff
            .submit(audio(new, 1, 20, &handoff), NOW, &sink)
            .unwrap();
        assert_eq!(
            *sink.calls.borrow(),
            vec![
                Call::Retire(old),
                Call::Start(new),
                Call::Audio(new, 1, vec![20; 160])
            ]
        );
    }

    #[test]
    fn event_eof_preserves_sanitized_peer_close_code() {
        let error = connection_ended(State::Disconnected(lamp_gemini::Error::PeerClosed {
            code: Some(4029),
        }));
        assert_eq!(
            error.to_string(),
            "provider connection ended: Gemini Live PeerClosed { code: Some(4029) }"
        );
        assert_eq!(
            connection_ended(State::Disconnected(lamp_gemini::Error::ServerRejected)).to_string(),
            "provider connection ended: Gemini Live ServerRejected"
        );
    }

    #[test]
    fn event_eof_distinguishes_client_lifecycle_failure_from_local_shutdown() {
        assert!(
            connection_ended(State::Ready)
                .to_string()
                .contains("client lifecycle failure")
        );
        assert_eq!(
            connection_ended(State::Closed).to_string(),
            "provider connection closed locally"
        );
    }

    // ---- Supervised provider connection over real local IPC -------------
    //
    // The service is scripted in memory; no credential, TLS or cloud request.
    // Authority uses the real monotonic clock because `serve` does.
    use lamp_gemini::{
        Credential, GOOGLE_ENDPOINT, SessionConfig,
        testing::{Dial, audio as provider_audio, connector},
    };
    use serde_json::json;
    use tokio::{sync::mpsc::UnboundedReceiver, task::JoinHandle};

    /// Everything the worker can put on the data channel once the proposed
    /// statuses are accepted: the shape the coordinator would decode.
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Status(ProviderStatus),
        Output(ProviderOutput),
    }

    /// What a test request's Start says besides its request
    /// ([`LiveRig::speak_as`]).
    #[derive(Clone, Copy, Default)]
    struct StartSays {
        after_stop: bool,
        continues_cut_off: bool,
        over_the_lamp: bool,
        follows_on: bool,
        over_the_lamps_voice: bool,
    }

    struct LiveRig {
        _directory: crate::process::SessionDirectory,
        parent: WorkerChannels,
        output_flow: crate::provider_flow::OutputReceiver,
        owner: Controller,
        snapshot: Snapshot,
        dials: UnboundedReceiver<Dial>,
        worker: JoinHandle<Result<()>>,
    }
    fn real_time() -> MonoTime {
        MonoTime::from_micros(monotonic_us())
    }
    /// A test's own logical clock (#1128), for a test on a paused runtime.
    /// The worker's leases, heartbeat bound and waits, and the rig's
    /// heartbeats and capture times, read the scenario clock; the rig's
    /// ticks move it two milliseconds at a time with Tokio's, so a wait of
    /// seconds passes in no time and the two clocks never part.
    fn logical_clock() -> crate::scenario_clock::Scope {
        let scope = crate::scenario_clock::Scope::enter(Duration::from_secs(60)).unwrap();
        scope.set_pump(|_| Ok(())).unwrap();
        scope
    }
    impl LiveRig {
        fn start(policy: RecoveryPolicy) -> Self {
            Self::start_with(policy, Options::reporting(Reporting::Legacy))
        }
        fn start_with(policy: RecoveryPolicy, options: Options) -> Self {
            Self::start_with_barrier(policy, options, BARRIER)
        }
        fn start_with_barrier(
            policy: RecoveryPolicy,
            options: Options,
            barrier: BarrierPolicy,
        ) -> Self {
            Self::start_all(policy, options, barrier, false, Continuation::Off)
        }
        fn start_camera() -> Self {
            Self::start_all(
                quick_policy(),
                Options::reporting(Reporting::Coordinated),
                BARRIER,
                true,
                Continuation::Off,
            )
        }
        fn start_all(
            policy: RecoveryPolicy,
            options: Options,
            barrier: BarrierPolicy,
            camera: bool,
            continuation: Continuation,
        ) -> Self {
            Self::start_capacity(
                policy,
                options,
                barrier,
                camera,
                continuation,
                lamp_gemini::MAX_WIRE_BYTES * 2,
            )
        }
        /// With this many bytes between the worker and the scripted service.
        fn start_capacity(
            policy: RecoveryPolicy,
            options: Options,
            barrier: BarrierPolicy,
            camera: bool,
            continuation: Continuation,
            capacity: usize,
        ) -> Self {
            Self::start_configured(
                policy,
                options,
                barrier,
                camera,
                continuation,
                capacity,
                |config| config,
            )
        }

        fn start_configured(
            policy: RecoveryPolicy,
            options: Options,
            barrier: BarrierPolicy,
            camera: bool,
            continuation: Continuation,
            capacity: usize,
            configure: impl FnOnce(SessionConfig) -> SessionConfig,
        ) -> Self {
            let (directory, parent, worker) = intake_channels();
            let boot = BootId::new([9; 16]).unwrap();
            let mut owner = Controller::new(boot, real_time());
            owner
                .set_microphone_permission(real_time(), Permission::Allowed)
                .unwrap();
            // The configuration the worker itself builds, with its own choice
            // of continuation applied by the same function.
            let config = continuation
                .apply(
                    SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
                        .unwrap()
                        .resumption(RESUMPTION)
                        .barrier_policy(barrier),
                )
                .unwrap()
                // What the worker's sessions are told of, as in the service.
                .tools(
                    crate::tools::declared(&options.tools)
                        .into_iter()
                        .chain(options.name_signal.then(crate::attention::name_tool))
                        .collect(),
                )
                .unwrap();
            let (dialer, dials) =
                lamp_gemini::testing::connector_with_capacity(configure(config), capacity);
            let provider = Supervisor::new(dialer, policy).unwrap();
            let visual = if camera {
                owner
                    .set_camera_permission(real_time(), Permission::Allowed)
                    .unwrap();
                Some(
                    ContextDelivery::new(
                        camera_test_source(&directory.path),
                        boot,
                        BootId::new([8; 16]).unwrap(),
                    )
                    .unwrap(),
                )
            } else {
                None
            };
            let snapshot = owner.snapshot(real_time()).unwrap();
            let mut rig = Self {
                _directory: directory,
                parent,
                output_flow: crate::provider_flow::OutputReceiver::default(),
                owner,
                snapshot,
                dials,
                worker: tokio::spawn(serve_with_visual(
                    worker, boot, provider, options, visual, None,
                )),
            };
            rig.publish();
            rig
        }
        /// One coordinator heartbeat: fresh input leases and authority.
        fn publish(&mut self) {
            self.heartbeat().unwrap();
        }
        /// One coordinator tick. In a test that entered a scope (#1128) it is
        /// two milliseconds of the scenario clock and then two of Tokio's, so
        /// the two clocks keep together on a paused runtime. Without a scope it
        /// is a sleep of `unscoped`, as each of the rig's loops always waited.
        async fn tick(&mut self, unscoped: Duration) {
            let stepped = crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
            tokio::time::sleep(if stepped {
                Duration::from_millis(2)
            } else {
                unscoped
            })
            .await;
        }
        fn heartbeat(&mut self) -> io::Result<()> {
            let at = monotonic_us();
            let now = MonoTime::from_micros(at);
            let lease = MonoTime::from_micros(at + 100_000);
            self.owner
                .set_capture(now, CaptureState::RetainingUntil(lease))
                .unwrap();
            self.owner
                .set_admission(now, AdmissionState::OpenUntil(lease))
                .unwrap();
            self.snapshot = self.owner.snapshot(now).unwrap();
            self.parent.control.send(Control::Authority {
                snapshot: self.snapshot,
            })
        }
        /// Heartbeats for `duration` of Tokio's time, which is the host's
        /// unless the test's runtime is paused.
        async fn pump(&mut self, duration: Duration) {
            let until = tokio::time::Instant::now() + duration;
            while tokio::time::Instant::now() < until {
                self.publish();
                self.tick(Duration::from_millis(5)).await;
            }
        }
        async fn output(&mut self) -> ProviderOutput {
            match self.wire().await {
                Wire::Output(output) => output,
                Wire::Status(status) => panic!("unexpected provider status: {status:?}"),
            }
        }
        /// One packet, if one has arrived, counted against output capacity
        /// and its capacity returned as an empty reply buffer would.
        fn take(&mut self) -> Option<Wire> {
            let wire = self.parent.data.receive::<Wire>().unwrap()?;
            self.output_flow.received().unwrap();
            self.output_flow
                .replenish(0, &mut self.parent.control)
                .unwrap();
            Some(wire)
        }
        async fn wire(&mut self) -> Wire {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                self.publish();
                if let Some(wire) = self.take() {
                    return wire;
                }
                assert!(!self.worker.is_finished(), "worker ended before output");
                assert!(Instant::now() < deadline, "no provider output in time");
                self.tick(Duration::from_millis(2)).await;
            }
        }
        /// Heartbeat until the worker attempts a connection.
        async fn dial(&mut self) -> Dial {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                self.publish();
                if let Ok(dial) = self.dials.try_recv() {
                    return dial;
                }
                assert!(Instant::now() < deadline, "no connection attempt in time");
                self.tick(Duration::from_millis(2)).await;
            }
        }
        /// Heartbeat until the worker ends; its result and the wait.
        async fn exit(&mut self) -> (Result<()>, Duration) {
            let started = Instant::now();
            while !self.worker.is_finished() {
                assert!(started.elapsed() < Duration::from_secs(3), "worker hung");
                // The worker may close its socket at any point from here on.
                let _ = self.heartbeat();
                self.tick(Duration::from_millis(1)).await;
            }
            ((&mut self.worker).await.unwrap(), started.elapsed())
        }
        /// Heartbeat until the scripted service has the worker's next message.
        async fn service_message(&mut self, dial: &mut Dial) -> serde_json::Value {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                self.publish();
                let stepped =
                    crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
                let wait = Duration::from_millis(if stepped { 2 } else { 5 });
                if let Some(message) = dial.service.try_receive(wait).await {
                    return message;
                }
                assert!(Instant::now() < deadline, "service saw no client message");
            }
        }
        async fn ready(&mut self) -> Dial {
            let mut dial = self.dial().await;
            let setup = self.service_message(&mut dial).await;
            assert_eq!(
                setup["setup"].get("sessionResumption").is_some(),
                dial.resumed
            );
            dial.service.send(json!({"setupComplete": {}})).await;
            assert!(matches!(self.output().await, ProviderOutput::Ready { .. }));
            dial
        }
        async fn link(&mut self) -> crate::provider_link::ProviderLink {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                self.publish();
                match self.parent.control.receive::<WorkerEvent>().unwrap() {
                    Some(WorkerEvent::Ready) | None => {}
                    Some(WorkerEvent::ProviderLink { link }) => {
                        link.validate(monotonic_us()).unwrap();
                        return link;
                    }
                    Some(other) => panic!("unexpected provider control event: {other:?}"),
                }
                assert!(
                    !self.worker.is_finished(),
                    "worker ended before link observation"
                );
                assert!(Instant::now() < deadline, "no link observation in time");
                self.tick(Duration::from_millis(2)).await;
            }
        }
        async fn coordinated_ready(&mut self) -> Dial {
            let mut dial = self.dial().await;
            assert!(self.service_message(&mut dial).await.get("setup").is_some());
            dial.service.send(json!({"setupComplete": {}})).await;
            let link = self.link().await;
            assert!(matches!(
                link.state,
                crate::provider_link::ProviderLinkState::Ready {
                    context: crate::provider_link::LinkContext::Initial,
                    ..
                }
            ));
            assert!(
                self.take().is_none(),
                "availability must not consume PCM credit"
            );
            dial
        }
        async fn stop(&mut self) {
            self.parent.control.send(Control::Stop).unwrap();
            self.exit().await.0.unwrap();
        }
        /// Admit a request and submit its start, one block and optional end.
        async fn speak(&mut self, dial: &mut Dial, interruption: bool, end: bool) -> u64 {
            self.speak_told(dial, interruption, end, false).await
        }

        /// The same, its Start saying whether it is the first request after
        /// a Stop.
        async fn speak_told(
            &mut self,
            dial: &mut Dial,
            interruption: bool,
            end: bool,
            after_stop: bool,
        ) -> u64 {
            let says = StartSays {
                after_stop,
                ..StartSays::default()
            };
            self.speak_as(dial, interruption, end, says).await
        }
        /// [`Self::speak_told`], with whatever else its Start says.
        async fn speak_as(
            &mut self,
            dial: &mut Dial,
            interruption: bool,
            end: bool,
            StartSays {
                after_stop,
                continues_cut_off,
                over_the_lamp,
                follows_on,
                over_the_lamps_voice,
            }: StartSays,
        ) -> u64 {
            let input = if interruption {
                AdmittedInput::Interruption
            } else {
                AdmittedInput::NewTurn
            };
            let owner = self.owner.admit(real_time(), input).unwrap();
            let request = owner.turn();
            self.publish();
            let privacy_generation = self.snapshot.microphone_generation();
            self.parent
                .data
                .send(ProviderInput::Start {
                    session: dial.session.get(),
                    request,
                    privacy_generation,
                    continues: false,
                    continues_cut_off,
                    over_the_lamp,
                    after_stop,
                    follows_on,
                    over_the_lamps_voice,
                })
                .unwrap();
            self.parent
                .data
                .send(ProviderInput::Audio {
                    request,
                    privacy_generation,
                    sequence: 1,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![3; 160],
                })
                .unwrap();
            assert!(matches!(
                self.output().await,
                ProviderOutput::Started { request: started, .. } if started == request
            ));
            if end {
                self.owner.input_ended(real_time(), owner).unwrap();
                self.publish();
                self.parent
                    .data
                    .send(ProviderInput::End {
                        request,
                        privacy_generation,
                        sound: false,
                        cut_in: false,
                    })
                    .unwrap();
            }
            // The service sees exactly this request's boundaries and audio.
            let mut expected = vec!["activityStart", "audio"];
            if end {
                expected.push("activityEnd");
            }
            for field in expected {
                let message = self.service_message(dial).await;
                assert!(
                    message["realtimeInput"].get(field).is_some(),
                    "expected {field}, service received {message}"
                );
            }
            request
        }
    }
    fn camera_test_source(directory: &Path) -> VisualSource {
        VisualSource {
            socket: directory.join(crate::camera_worker::FRAME_SOCKET),
            camera: BootId::new([7; 16]).unwrap(),
            source: "desk".into(),
            width: 320,
            height: 240,
            max_frame_bytes: 128 * 1024,
        }
    }

    #[tokio::test]
    async fn visual_camera_missing_or_closed_before_first_frame_preserves_complete_audio() {
        for close_permission in [false, true] {
            let mut rig = LiveRig::start_camera();
            let mut dial = rig.coordinated_ready().await;
            let request = rig.speak(&mut dial, false, false).await;
            if close_permission {
                rig.owner
                    .set_camera_permission(real_time(), Permission::Denied)
                    .unwrap();
            }
            // Open input continues normally while an absent camera times out.
            let until = Instant::now() + Duration::from_millis(280);
            let mut receipt = None;
            while Instant::now() < until {
                rig.publish();
                while let Some(event) = rig.parent.control.receive::<WorkerEvent>().unwrap() {
                    match event {
                        WorkerEvent::VisualContext { receipt: got } => receipt = Some(got),
                        other => panic!("unexpected control: {other:?}"),
                    }
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            let receipt = receipt.expect("camera failure must be visible");
            assert_eq!(
                (receipt.request, receipt.session),
                (request, dial.session.get())
            );
            assert_eq!(
                receipt.outcome,
                if close_permission {
                    crate::visual_context::Outcome::PrivacyClosed
                } else {
                    crate::visual_context::Outcome::Expired
                }
            );
            rig.parent
                .data
                .send(ProviderInput::End {
                    request,
                    privacy_generation: rig.snapshot.microphone_generation(),
                    sound: false,
                    cut_in: false,
                })
                .unwrap();
            assert!(
                rig.service_message(&mut dial).await["realtimeInput"]
                    .get("activityEnd")
                    .is_some()
            );
            dial.service.send(provider_audio(14, 480)).await;
            dial.service
                .send(json!({"serverContent":{"generationComplete":true}}))
                .await;
            dial.service.send(idle_complete()).await;
            assert!(
                matches!(rig.output().await,ProviderOutput::Audio { request: got, samples, .. }
                if got==request && samples==vec![14;480])
            );
            assert!(
                matches!(rig.output().await,ProviderOutput::GenerationComplete { request: got } if got==request)
            );
            assert!(
                matches!(rig.output().await,ProviderOutput::TurnComplete { request: got, idle:true } if got==request)
            );
            assert!(rig.take().is_none());
            rig.stop().await;
        }
    }

    #[tokio::test]
    async fn visual_real_camera_link_reaches_same_input_without_blocking_its_audio_or_end() {
        use lamp_camera::frames::{FrameMeta, FrameServer, Role};
        let mut rig = LiveRig::start_camera();
        let source = camera_test_source(&rig._directory.path);
        let mut camera = FrameServer::bind(
            &source.socket,
            BootId::new([9; 16]).unwrap(),
            &[(Role::Context, BootId::new([8; 16]).unwrap())],
            source.max_frame_bytes,
        )
        .unwrap();
        let mut dial = rig.coordinated_ready().await;
        let request = rig.speak(&mut dial, false, false).await;
        let deadline = Instant::now() + Duration::from_secs(1);
        while !camera.wants_frame() {
            rig.publish();
            camera.service(monotonic_us());
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let at = monotonic_us();
        let meta = FrameMeta {
            request: 1,
            camera_worker: source.camera,
            capture_epoch: 1,
            frame_sequence: 11,
            grant: rig.owner.camera_grant(real_time()).unwrap(),
            source: source.source,
            format: *b"MJPG",
            width: 320,
            height: 240,
            driver_sequence: 1,
            driver_timestamp: None,
            dequeue_started_at_us: at - 100,
            dequeue_completed_at_us: at - 50,
            copied_at_us: at,
            derived: None,
        };
        let jpeg = [255, 216, 255, 224, 6, 7, 255, 217];
        assert_eq!(camera.offer_meta(&meta, &jpeg, monotonic_us()).unwrap(), 1);
        // This is a real private stream feeding the production provider loop.
        let mut receipt = None;
        while receipt.is_none() {
            rig.publish();
            camera.service(monotonic_us());
            if let Some(event) = rig.parent.control.receive::<WorkerEvent>().unwrap() {
                let WorkerEvent::VisualContext { receipt: got } = event else {
                    panic!("unexpected {event:?}")
                };
                receipt = Some(got);
            }
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, crate::visual_context::Outcome::Queued);
        assert_eq!(
            (receipt.request, receipt.session, receipt.frame_sequence),
            (request, dial.session.get(), Some(11))
        );
        let image = rig.service_message(&mut dial).await;
        assert_eq!(image["realtimeInput"]["video"]["mimeType"], "image/jpeg");
        rig.parent
            .data
            .send(ProviderInput::Audio {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
                sequence: 2,
                read_completed_at_us: monotonic_us(),
                samples: vec![19; 160],
            })
            .unwrap();
        rig.parent
            .data
            .send(ProviderInput::End {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        assert!(
            rig.service_message(&mut dial).await["realtimeInput"]
                .get("audio")
                .is_some()
        );
        assert!(
            rig.service_message(&mut dial).await["realtimeInput"]
                .get("activityEnd")
                .is_some()
        );
        assert!(
            dial.service
                .try_receive(Duration::from_millis(10))
                .await
                .is_none(),
            "no repeated image"
        );
        rig.stop().await;
    }

    fn quick_policy() -> RecoveryPolicy {
        RecoveryPolicy {
            first_backoff: Duration::from_millis(20),
            max_backoff: Duration::from_millis(40),
            ..RecoveryPolicy::default()
        }
    }
    fn resumable() -> serde_json::Value {
        json!({"sessionResumptionUpdate":{"newHandle":"HANDLE-ONE","resumable":true}})
    }
    fn idle_complete() -> serde_json::Value {
        json!({"serverContent":{"turnComplete":true,"interactionStatus":"IDLE"}})
    }

    async fn memory_link<C: Connect>(
        provider: &mut Supervisor<C>,
        clock: &mut LinkClock,
        pending: &mut PendingLink,
    ) -> ProviderLink {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let mut received = None;
                service_link(provider, Some(&mut *clock), pending, |link| {
                    received = Some(link);
                    Ok(())
                })
                .unwrap();
                if let Some(link) = received {
                    return link;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("in-memory provider link")
    }

    fn memory_provider(
        policy: RecoveryPolicy,
    ) -> (
        Supervisor<lamp_gemini::testing::FakeConnector>,
        UnboundedReceiver<Dial>,
        LinkClock,
        PendingLink,
    ) {
        let config = SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
            .unwrap()
            .resumption(RESUMPTION)
            .barrier_policy(BARRIER);
        let (connector, dials) = connector(config);
        (
            Supervisor::new(connector, policy).unwrap(),
            dials,
            LinkClock::sample().unwrap(),
            PendingLink::default(),
        )
    }

    fn function_call(id: &str, name: &str) -> serde_json::Value {
        json!({"toolCall":{"functionCalls":[{"id":id,"name":name,"args":{"colour":"blue"}}]}})
    }
    fn typed_with(tools: crate::tools::Settings) -> Options {
        Options {
            reporting: Reporting::Typed,
            tools,
            attention: None,
            name_signal: false,
            sounds_checked: false,
            ask_unclear: false,
            name_in_passing_ignored: false,
            policy_holds: None,
            wait_for_room: false,
            staff_note: false,
            spoken_to: false,
            voices: None,
        }
    }

    /// The model is asked to say when it hears the lamp's name, and does.
    /// The call is answered as a silent function, nothing is held back on
    /// account of it, and the answer is played as it would have been.
    #[tokio::test]
    async fn the_models_sign_that_it_heard_its_name_is_answered_silently_and_changes_nothing_played()
     {
        let mut rig = LiveRig::start_with(
            quick_policy(),
            Options {
                reporting: Reporting::Typed,
                tools: crate::tools::Settings::default(),
                attention: None,
                name_signal: true,
                sounds_checked: false,
                ask_unclear: false,
                name_in_passing_ignored: false,
                policy_holds: None,
                wait_for_room: false,
                staff_note: false,
                spoken_to: false,
                voices: None,
            },
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(function_call("n1", crate::attention::HEARD_MY_NAME))
            .await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"n1","name":"heard_my_name","response":{"ok":true},"scheduling":"SILENT"}
            ]}})
        );
        dial.service.send(provider_audio(5, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: played, .. } if played == request
        ));
        rig.stop().await;
    }

    /// The lamp's own rule, through the worker, in a room it knows to be
    /// busy. A new session hears two people talk and answers one of them:
    /// none of that answer gets out. Its second request is a call, written
    /// the way the real service writes one, and is played. From its third
    /// request on the model is trusted as before.
    #[tokio::test]
    async fn a_new_sessions_first_answers_to_talk_in_a_busy_room_are_held_and_later_ones_are_the_models()
     {
        fn busy() -> (crate::room_told::Room, bool) {
            (crate::room_told::Room::Busy, false)
        }
        let heard = |text: &str| json!({"serverContent":{"inputTranscription":{"text":text}}});
        let said = |text: &str| json!({"serverContent":{"outputTranscription":{"text":text,"finished":true}}});
        let mut rig = LiveRig::start_with(
            quick_policy(),
            Options {
                reporting: Reporting::Typed,
                tools: crate::tools::Settings::default(),
                attention: Some(busy),
                name_signal: false,
                sounds_checked: false,
                ask_unclear: false,
                name_in_passing_ignored: false,
                policy_holds: None,
                wait_for_room: false,
                staff_note: false,
                spoken_to: false,
                voices: None,
            },
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(heard("How was your weekend?")).await;
        // What was heard is passed on as before; the answer is not.
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Transcript { request: None, text, .. } if text == "How was your weekend?"
        ));
        dial.service.send(provider_audio(9, 480)).await;
        dial.service
            .send(said("It was so quiet and peaceful!"))
            .await;
        dial.service.send(provider_audio(9, 480)).await;
        dial.service.send(idle_complete()).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { request: done, idle: true } if done == request
        ));
        assert!(
            rig.take().is_none(),
            "sound or words of a held answer got out"
        );

        rig.owner
            .complete_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        // The session's second request, written the way the service
        // writes a call: played.
        let next = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(heard("Milan, how was your weekend?"))
            .await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Transcript { request: None, .. }
        ));
        dial.service.send(provider_audio(7, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: played, .. } if played == next
        ));
        dial.service.send(idle_complete()).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { .. }
        ));
        // Its third: the session has heard enough, and the same talk that
        // was held on its first request is the model's to judge.
        rig.owner
            .complete_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        let third = rig.speak(&mut dial, false, true).await;
        dial.service.send(heard("How was your weekend?")).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Transcript { request: None, .. }
        ));
        dial.service.send(provider_audio(6, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: played, .. } if played == third
        ));
        rig.stop().await;
    }

    /// Two people talk near the lamp and the model says so with the
    /// function it was given. The call is answered at once, the sound the
    /// service still sends for that request never reaches the coordinator,
    /// its words are not kept as something the lamp said, and the request
    /// still ends in order. The next request is heard and answered aloud.
    #[tokio::test]
    async fn a_request_the_model_says_was_not_for_it_lets_no_sound_out_and_the_next_is_answered() {
        let mut rig = LiveRig::start_with(
            quick_policy(),
            typed_with(crate::tools::Settings {
                decline: true,
                exec: None,
                memory: None,
                clock: false,
                ..Default::default()
            }),
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(function_call("c1", crate::tools::NOT_FOR_ME))
            .await;
        // Answered as a function nothing is to be said on account of.
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c1","name":"not_for_me","response":{"ok":true},"scheduling":"SILENT"}
            ]}})
        );
        // The model speaks all the same, in the turn it asked in.
        dial.service.send(provider_audio(9, 480)).await;
        dial.service
            .send(json!({"serverContent":{"outputTranscription":{"text":"Sounds like a plan!","finished":true}}}))
            .await;
        dial.service.send(idle_complete()).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { request: done, idle: true } if done == request
        ));
        assert!(
            rig.take().is_none(),
            "sound or words of a declined request got out"
        );

        rig.owner
            .complete_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        let next = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(7, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: heard, .. } if heard == next
        ));
        rig.stop().await;
    }

    /// The first request of a rig with the tasks part, and the staff's note
    /// on or off (`noted`), heard as `before` in pieces before its end is
    /// asked for and as `after` once its end was sent: the service
    /// messages up to the request's end, and any in the 300 ms after
    /// `after`.
    async fn heard_around_the_end(
        noted: bool,
        before: &[&str],
        after: &[&str],
    ) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        let home = crate::process::SessionDirectory::create().unwrap();
        let mut options = typed_with(crate::tools::Settings {
            tasks: Some(crate::agent_tasks::Place {
                directory: home.path.clone(),
                target: crate::agent_tasks::DEFAULT_TARGET.into(),
                who: false,
                lead_in: false,
                asked: false,
                needs: None,
                staff_note: noted,
            }),
            ..Default::default()
        });
        options.staff_note = noted;
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, false).await;
        let heard = |piece: &str| json!({"serverContent":{"inputTranscription":{"text":piece}}});
        for piece in before {
            dial.service.send(heard(piece)).await;
        }
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        // The pieces come on the service's socket and the end on the
        // parent's channel, which the worker reads in no set order: the
        // pieces are given time to be taken first, the owner published
        // meanwhile, as the coordinator does.
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            rig.publish();
        }
        rig.parent
            .data
            .send(ProviderInput::End {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        let mut sent = Vec::new();
        while sent.last().is_none_or(|last: &serde_json::Value| {
            last["realtimeInput"].get("activityEnd").is_none()
        }) {
            sent.push(rig.service_message(&mut dial).await);
        }
        for piece in after {
            dial.service.send(heard(piece)).await;
        }
        let mut later = Vec::new();
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            rig.publish();
            if let Some(message) = dial.service.try_receive(Duration::from_millis(5)).await {
                later.push(message);
            }
        }
        rig.stop().await;
        (sent, later)
    }

    /// A request whose heard words name one of the staff ("ask Claude Code
    /// to …", lamp-4ace 2026-10-16 about 21:05) is told so beside its sound
    /// while its input is open, and the note reaches the service before the
    /// request's end, so the model has it before it answers.
    #[tokio::test]
    async fn a_request_that_names_one_of_the_staff_is_told_so_before_its_end() {
        let (sent, _) = heard_around_the_end(
            true,
            &[
                "Can you ask Claude",
                " Code to write a short poem about the sea?",
            ],
            &[],
        )
        .await;
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(
            sent[0]["realtimeInput"]["text"],
            crate::staff_note::note(&["Claude Code"]),
            "{sent:?}"
        );
    }

    /// A name said in a later piece of the request is matched with all its
    /// words so far and noted before its end. (A later name that changes
    /// the note, "ask Claude Code and get Codex …", is
    /// `the_staffs_part_of_a_note_takes_the_place_of_the_last` with
    /// `agent_tasks::tests::the_words_of_a_request_name_the_staff`: this
    /// rig's tasks part knows Claude Code alone.)
    #[tokio::test]
    async fn a_name_said_later_in_the_request_is_noted_before_its_end() {
        let (sent, _) = heard_around_the_end(
            true,
            &[
                "Write a haiku about coffee",
                " and ask Claude",
                " Code to check it.",
            ],
            &[],
        )
        .await;
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(
            sent[0]["realtimeInput"]["text"],
            crate::staff_note::note(&["Claude Code"]),
            "{sent:?}"
        );
    }

    /// A request whose words name none of the staff is given no note; nor
    /// is one that names them with the switch off.
    #[tokio::test]
    async fn no_note_without_a_name_or_without_the_switch() {
        let (sent, _) =
            heard_around_the_end(true, &["Write me a short", " poem about the sea."], &[]).await;
        assert_eq!(sent.len(), 1, "{sent:?}");
        let (sent, _) = heard_around_the_end(
            false,
            &["Hey Lamp, ask Claude Code", " to write a haiku."],
            &[],
        )
        .await;
        assert_eq!(sent.len(), 1, "{sent:?}");
    }

    /// Words that name the staff only after the request's end was sent
    /// come too late: nothing more is sent for it.
    #[tokio::test]
    async fn words_that_name_the_staff_after_the_end_send_nothing() {
        let (sent, later) = heard_around_the_end(
            true,
            &["Write a haiku about coffee,"],
            &[" and ask Claude Code to check it."],
        )
        .await;
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(later.is_empty(), "{later:?}");
    }

    /// A rig with the tasks part and the staff's note on.
    fn staff_noted_rig(home: &crate::process::SessionDirectory) -> LiveRig {
        let mut options = typed_with(crate::tools::Settings {
            tasks: Some(crate::agent_tasks::Place {
                directory: home.path.clone(),
                target: crate::agent_tasks::DEFAULT_TARGET.into(),
                who: false,
                lead_in: false,
                asked: false,
                needs: None,
                staff_note: true,
            }),
            ..Default::default()
        });
        options.staff_note = true;
        LiveRig::start_with(quick_policy(), options)
    }

    /// End `request`, giving its words time to be taken first, and the
    /// service messages up to its end.
    async fn end_heard(rig: &mut LiveRig, dial: &mut Dial, request: u64) -> Vec<serde_json::Value> {
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            rig.publish();
        }
        rig.parent
            .data
            .send(ProviderInput::End {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        let mut sent = Vec::new();
        while sent.last().is_none_or(|last: &serde_json::Value| {
            last["realtimeInput"].get("activityEnd").is_none()
        }) {
            sent.push(rig.service_message(dial).await);
        }
        sent
    }

    /// The reviewer on #1173: "Hey Lamp, ask Claude Code, … to write a
    /// haiku." The early end goes at the pause and the name is written
    /// after it, too late for that request. The rest of it, which goes on
    /// from the one cut off, keeps its words and is told, beside the note
    /// that it is the rest of the request, before its own end.
    #[tokio::test]
    async fn a_request_that_goes_on_from_one_cut_off_is_told_what_the_words_before_named() {
        let home = crate::process::SessionDirectory::create().unwrap();
        let mut rig = staff_noted_rig(&home);
        let mut dial = rig.ready().await;
        let heard = |piece: &str| json!({"serverContent":{"inputTranscription":{"text":piece}}});
        let first = rig.speak(&mut dial, false, false).await;
        let sent = end_heard(&mut rig, &mut dial, first).await;
        assert_eq!(sent.len(), 1, "{sent:?}");
        // The words come after the early end; the service closes that
        // request with nothing said, as for one given up.
        dial.service.send(heard("Hey Lamp, ask Claude Code,")).await;
        dial.service.send(idle_complete()).await;
        while !matches!(rig.output().await, ProviderOutput::TurnComplete { .. }) {}
        rig.owner
            .complete_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        let rest = rig
            .speak_as(
                &mut dial,
                false,
                false,
                StartSays {
                    continues_cut_off: true,
                    ..StartSays::default()
                },
            )
            .await;
        dial.service
            .send(heard(" to write a haiku about coffee."))
            .await;
        let sent = end_heard(&mut rig, &mut dial, rest).await;
        assert_eq!(sent.len(), 2, "{sent:?}");
        let note = sent[0]["realtimeInput"]["text"].as_str().unwrap();
        assert!(note.contains(CUT_OFF_NOTE), "{note}");
        assert!(
            note.contains(&crate::staff_note::note(&["Claude Code"])),
            "{note}"
        );
        rig.stop().await;
    }

    /// #828: a request whose Start says it began while the lamp's answer
    /// could be heard is marked so in the room, with `--cut-ins-not-talk`
    /// (the room's switch), so that left unanswered it is not counted toward
    /// a busy room. One begun with a reply only in hand, the lamp silent,
    /// is not (the reviewer on #1291), nor is one begun after the lamp
    /// finished. The flag is taken right after `--room-facts`, by both
    /// workers.
    #[tokio::test]
    async fn a_request_begun_over_the_lamps_voice_is_kept_out_of_the_busy_count_with_the_switch() {
        let dir = std::path::Path::new("/run/lamp-voice");
        let flags = [
            crate::room_told::FLAG,
            crate::room_told::CUT_INS_FLAG,
            crate::attention::FLAG,
        ]
        .map(str::to_owned);
        let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
        assert!(files.room_facts && files.cut_ins_not_talk && files.attention);
        let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
        assert!(files.cut_ins_not_talk);
        let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
        assert!(!files.cut_ins_not_talk);
        // Puts the process-wide room back when dropped, a failed assertion
        // included.
        struct PutBack(Option<crate::room_told::RoomTold>);
        impl Drop for PutBack {
            fn drop(&mut self) {
                if let Some(old) = self.0.take() {
                    *crate::room_told::room()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = old;
                }
            }
        }
        let _put_back = PutBack(Some(std::mem::take(
            &mut *crate::room_told::room().lock().unwrap(),
        )));
        crate::room_told::room()
            .lock()
            .unwrap()
            .leave_cut_ins_out(true);
        let marked = |request: u64| {
            crate::room_told::room()
                .lock()
                .unwrap()
                .kept_out_over_the_lamp(request)
        };
        // A line after the lamp finished, then one begun over a reply: only
        // in hand, the lamp silent, or with the lamp's answer heard.
        for (voice, marks) in [(false, false), (true, true)] {
            let mut rig = LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Typed));
            let mut dial = rig.ready().await;
            let after = rig
                .speak_as(&mut dial, false, true, StartSays::default())
                .await;
            assert!(!marked(after));
            let over = rig
                .speak_as(
                    &mut dial,
                    true,
                    false,
                    StartSays {
                        over_the_lamp: true,
                        over_the_lamps_voice: voice,
                        ..StartSays::default()
                    },
                )
                .await;
            assert_eq!(marked(over), marks, "heard {voice}");
            rig.stop().await;
        }
    }

    /// The reviewer on #1173: a request begun over the lamp's own answer
    /// may be its read-back ("… to Claude Code. Shall I start it?") with a
    /// "yes" said over it: it gets no note.
    #[tokio::test]
    async fn a_request_begun_over_the_lamp_gets_no_staff_note() {
        let home = crate::process::SessionDirectory::create().unwrap();
        let mut rig = staff_noted_rig(&home);
        let mut dial = rig.ready().await;
        let request = rig
            .speak_as(
                &mut dial,
                false,
                false,
                StartSays {
                    over_the_lamp: true,
                    ..StartSays::default()
                },
            )
            .await;
        dial.service
            .send(json!({"serverContent":{"inputTranscription":{"text":"to ask Claude Code. Yes, go ahead."}}}))
            .await;
        let sent = end_heard(&mut rig, &mut dial, request).await;
        assert_eq!(sent.len(), 1, "{sent:?}");
        rig.stop().await;
    }

    /// `--staff-note` is read after `--policy-holds-actions`, and sets the
    /// tasks part's switch too, so `start_agent_task` says what the note
    /// asks; out of its place it is refused; without it, both are off.
    #[test]
    fn the_staff_note_is_read_after_the_policy_holds_and_reaches_the_tasks_part() {
        let dir = std::path::Path::new("/run/test");
        let flags =
            |list: &[&str]| -> Vec<String> { list.iter().map(|flag| (*flag).to_owned()).collect() };
        let (_, _, files, _) = parse_flags(
            dir,
            &flags(&[
                crate::agent_tasks::FLAG,
                "/run/tasks",
                crate::policy_holds::FLAG,
                crate::staff_note::FLAG,
            ]),
        )
        .unwrap();
        assert!(files.policy_holds && files.staff_note);
        assert!(files.tools.tasks.as_ref().unwrap().staff_note);
        let (_, _, files, _) =
            parse_flags(dir, &flags(&[crate::agent_tasks::FLAG, "/run/tasks"])).unwrap();
        assert!(!files.staff_note && !files.tools.tasks.as_ref().unwrap().staff_note);
        assert!(
            parse_flags(
                dir,
                &flags(&[crate::staff_note::FLAG, crate::policy_holds::FLAG])
            )
            .is_err()
        );
    }

    /// The staff's part of a request's note takes the place of the one it
    /// was given, beside the other parts; first given, it is added.
    #[test]
    fn the_staffs_part_of_a_note_takes_the_place_of_the_last() {
        let name = crate::called::NOTE;
        let one = crate::staff_note::note(&["Claude Code"]);
        let both = crate::staff_note::note(&["Claude Code", "Codex"]);
        let first = staff_note_text(Some(&(7, name.to_owned())), 7, None, &one);
        assert_eq!(first, format!("{name} {one}"));
        assert_eq!(
            staff_note_text(Some(&(7, first)), 7, Some(&one), &both),
            format!("{name} {both}")
        );
        assert_eq!(
            staff_note_text(Some(&(6, name.to_owned())), 7, None, &one),
            one
        );
    }

    /// #782: a request that was only the lamp's name, whose call's wait ran
    /// out, is told so beside it, and the note reaches the service before
    /// the request's end, so the model answers with it.
    #[tokio::test]
    async fn a_bare_call_is_told_to_the_model_before_its_end() {
        let mut rig = LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Typed));
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, false).await;
        rig.parent
            .data
            .send(ProviderInput::Called {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
            })
            .unwrap();
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        rig.parent
            .data
            .send(ProviderInput::End {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        let note = rig.service_message(&mut dial).await;
        assert_eq!(note["realtimeInput"]["text"], CALLED_NOTE, "{note}");
        let end = rig.service_message(&mut dial).await;
        assert!(end["realtimeInput"].get("activityEnd").is_some(), "{end}");
        rig.stop().await;
    }

    /// #829, the first builder at 10:02: output dropped after a request was
    /// closed as unanswered, and usage counts after it, are said for that
    /// request with how long after the close they came, within 30 s and
    /// until the next request starts. Dropped output is said once a request.
    #[test]
    fn what_comes_after_an_unanswered_close_is_said_for_its_request() {
        const S: u64 = 1_000_000;
        let mut log = DiscardLog::default();
        assert_eq!(log.usage_after(10 * S), "");
        assert_eq!(log.late_line(Discard::UnownedOutput, 10 * S), None);
        log.closed_unanswered(39, 100 * S);
        assert_eq!(log.after_unanswered(100 * S + 500_000), Some((39, 500)));
        assert_eq!(
            log.usage_after(102 * S),
            " after request 39 was closed as unanswered 2000 ms before"
        );
        assert_eq!(log.late_line(Discard::LateInterruption, 101 * S), None);
        assert_eq!(
            log.late_line(Discard::UnownedOutput, 101 * S).as_deref(),
            Some(
                "lamp-voice provider: request 39: the service sent something 1000 ms after it was closed as unanswered, and it was dropped (UnownedOutput)"
            )
        );
        // Once a request: the rest of what came is not said again.
        assert_eq!(log.late_line(Discard::UnownedOutput, 101 * S + 1), None);
        assert_eq!(log.late_line(Discard::LateTerminal, 102 * S), None);
        assert_eq!(log.after_unanswered(130 * S), Some((39, 30_000)));
        assert_eq!(log.after_unanswered(130 * S + 1), None);
        log.started();
        assert_eq!(log.after_unanswered(101 * S), None);
        assert_eq!(log.usage_after(101 * S), "");
        // Through the events: the close marks it, and the next request's
        // start ends it. A reminder's close takes the same path without
        // counting as unanswered talk.
        let lineage = |request| lamp_gemini::Lineage {
            session: lamp_gemini::SessionId::new(7).unwrap(),
            request: RequestId::new(request).unwrap(),
        };
        let mut output = VecDeque::new();
        let mut log = DiscardLog::default();
        forward_reminder(
            Event::Unanswered {
                lineage: lineage(23),
                waited: Duration::from_millis(4_008),
            },
            &mut output,
            &mut 0,
            &mut log,
        )
        .unwrap();
        assert!(matches!(
            log.after_unanswered(monotonic_us()),
            Some((23, _))
        ));
        forward_reminder(
            Event::InputStarted {
                lineage: lineage(24),
                waiting_for_barrier: false,
            },
            &mut output,
            &mut 0,
            &mut log,
        )
        .unwrap();
        assert_eq!(log.after_unanswered(monotonic_us()), None);
    }

    /// The rise a request's journal lines give, and the near rule judges:
    /// the greatest in its window, to its sound's end or now, and nothing
    /// when no level was read in it.
    #[test]
    fn a_requests_rise_is_the_greatest_in_its_window() {
        let levels = VecDeque::from([(5, 30.0), (10, 12.5), (12, 19.5), (15, 8.0), (30, 40.0)]);
        assert_eq!(rise_in(&levels, (10, Some(15)), 99), Some(19.5));
        assert_eq!(rise_in(&levels, (10, None), 20), Some(19.5));
        assert_eq!(rise_in(&levels, (10, None), 30), Some(40.0));
        assert_eq!(rise_in(&levels, (16, Some(29)), 99), None);
        assert_eq!(rise_in(&VecDeque::new(), (0, None), 99), None);
    }

    /// #829, the first builder at 16:33: the first request after a hand or
    /// the head stopped the lamp is told so beside it, so that the model
    /// does not pick up what it was saying.
    #[tokio::test]
    async fn the_first_request_after_a_stop_tells_the_model_it_was_stopped() {
        let mut rig = LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Typed));
        let mut dial = rig.ready().await;
        let request = rig.speak_told(&mut dial, false, false, true).await;
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        rig.parent
            .data
            .send(ProviderInput::End {
                request,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        // What the service is given for the request, up to its end.
        let mut told = Vec::new();
        loop {
            let message = rig.service_message(&mut dial).await;
            if let Some(text) = message["realtimeInput"]["text"].as_str() {
                told.push(text.to_owned());
            }
            if message["realtimeInput"].get("activityEnd").is_some() {
                break;
            }
        }
        assert_eq!(told, [STOPPED_NOTE], "request {request}");
        rig.stop().await;
    }

    /// #829, the first builder at 09:01 on 17 October: a request begun
    /// soon after the lamp's words finished, as its Start says
    /// (`LAMP_FOLLOWS_ON=on`), is told with its End that it may follow on,
    /// in a room not known to be busy. In a busy room it is not, nor is one
    /// whose Start doesn't say so, nor one that ends as a sound sent on for
    /// the model to judge (the reviewer on #1278).
    #[tokio::test]
    async fn a_request_begun_soon_after_the_lamp_spoke_is_told_it_may_follow_on() {
        let room = |busy: bool| {
            let mut room = crate::room_told::RoomTold::new();
            if busy {
                let now = monotonic_us();
                for request in 1..=3 {
                    room.heard(request, now);
                    room.ended(request, now);
                    assert!(room.unanswered(request, now));
                }
                assert_eq!(room.at(now), crate::room_told::Room::Busy);
            }
            room
        };
        // Puts the process-wide room back when dropped, a failed assertion
        // included (El on #1278).
        struct PutBack(Option<crate::room_told::RoomTold>);
        impl Drop for PutBack {
            fn drop(&mut self) {
                if let Some(old) = self.0.take() {
                    *crate::room_told::room()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = old;
                }
            }
        }
        for (follows_on, busy, sound, told_it) in [
            (true, false, false, true),
            (true, true, false, false),
            (false, false, false, false),
            (true, false, true, false),
        ] {
            let _put_back = PutBack(Some(std::mem::replace(
                &mut *crate::room_told::room().lock().unwrap(),
                room(busy),
            )));
            let mut rig = LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Typed));
            let mut dial = rig.ready().await;
            let request = rig
                .speak_as(
                    &mut dial,
                    false,
                    false,
                    StartSays {
                        follows_on,
                        ..StartSays::default()
                    },
                )
                .await;
            let owner = rig.snapshot.owner().unwrap();
            rig.owner.input_ended(real_time(), owner).unwrap();
            rig.publish();
            rig.parent
                .data
                .send(ProviderInput::End {
                    request,
                    privacy_generation: rig.snapshot.microphone_generation(),
                    sound,
                    cut_in: false,
                })
                .unwrap();
            let mut told = Vec::new();
            loop {
                let message = rig.service_message(&mut dial).await;
                if let Some(text) = message["realtimeInput"]["text"].as_str() {
                    told.push(text.to_owned());
                }
                if message["realtimeInput"].get("activityEnd").is_some() {
                    break;
                }
            }
            let expected: &[&str] = if told_it {
                &[crate::follows_on::NOTE]
            } else {
                &[]
            };
            assert_eq!(
                told, expected,
                "follows on {follows_on}, busy {busy}, sound {sound}"
            );
            rig.stop().await;
        }
    }

    /// The name and the room due in the same request go as one note, each
    /// once; a later request starts afresh.
    #[test]
    fn a_requests_note_keeps_what_it_was_given() {
        let name = crate::called::NOTE;
        let busy = crate::room_told::BUSY;
        let first = note_text(None, 7, name);
        assert_eq!(first, name);
        let both = note_text(Some(&(7, first)), 7, busy);
        assert_eq!(both, format!("{name} {busy}"));
        assert_eq!(note_text(Some(&(7, both.clone())), 7, busy), both);
        assert_eq!(note_text(Some(&(7, both)), 8, busy), busy);
    }

    /// Someone asks the time and the model asks the lamp's clock. The call
    /// is answered at once, in this process, with the minute as the clock
    /// has it, and the model then speaks in the same request.
    #[tokio::test]
    async fn a_call_for_the_time_is_answered_at_once_from_the_lamps_clock() {
        let mut rig = LiveRig::start_with(
            quick_policy(),
            typed_with(crate::tools::Settings {
                decline: false,
                exec: None,
                memory: None,
                clock: true,
                ..Default::default()
            }),
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        let asked = Instant::now();
        dial.service
            .send(function_call("t1", crate::tools::THE_TIME))
            .await;
        let answer = rig.service_message(&mut dial).await;
        assert!(asked.elapsed() < Duration::from_secs(1));
        let response = &answer["toolResponse"]["functionResponses"][0];
        assert_eq!(response["id"], "t1");
        assert_eq!(response["name"], "the_time");
        // The clock as this machine has it, or that it could not be read;
        // never nothing, which would leave the service waiting.
        let told = &response["response"];
        assert!(
            told["now"]
                .as_str()
                .is_some_and(|now| now.starts_with("It is "))
                || told["error"].is_string(),
            "{answer}"
        );
        dial.service.send(provider_audio(9, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: heard, .. } if heard == request
        ));
        rig.stop().await;
    }

    /// The model asks for something the lamp can do. The executive, in a
    /// process of its own, is asked over its socket and what it established
    /// goes back to the model under the call's identifier. The voice's loop
    /// is not held while it is asked.
    #[tokio::test]
    async fn a_call_to_remember_is_asked_of_the_memory_and_what_it_established_goes_back() {
        use std::io::{BufRead, Write};
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("memory.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // The journal's writer may ask what is kept on the same socket:
        // every request is answered, and the one to remember is given back.
        let memory = std::thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request = serde_json::from_str::<serde_json::Value>(&line).unwrap();
                if request["op"] == "remember" {
                    stream.write_all(b"{\"ok\":true,\"id\":1}\n").unwrap();
                    return request;
                }
                stream.write_all(b"{\"ok\":true,\"entries\":[]}\n").unwrap();
            }
        });
        let mut rig = LiveRig::start_with(
            quick_policy(),
            typed_with(crate::tools::Settings {
                decline: false,
                exec: None,
                memory: Some(socket),
                clock: false,
                ..Default::default()
            }),
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(json!({"toolCall":{"functionCalls":[
                {"id":"m1","name":"remember","args":{"text":"takes coffee black"}}
            ]}}))
            .await;
        dial.service.send(idle_complete()).await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"m1","name":"remember","response":{"remembered":true}}
            ]}})
        );
        let asked = memory.join().unwrap();
        assert_eq!(asked["op"], "remember");
        assert_eq!(asked["text"], "takes coffee black");
        assert!(
            asked["at_unix_ms"]
                .as_u64()
                .is_some_and(|at| at > 1_700_000_000_000)
        );
        // The lamp then says so, in the same request.
        dial.service.send(provider_audio(5, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: heard, .. } if heard == request
        ));
        rig.stop().await;
    }

    /// The table's talk in a busy room, and the model asks for the light
    /// (lamp-4ace, 2026-10-14, 11:57). The rule decides at the call: the
    /// answer is held, so the call is answered and never reaches the
    /// executive.
    #[tokio::test]
    async fn a_call_in_a_request_whose_answer_is_held_is_answered_and_not_run() {
        fn busy() -> (crate::room_told::Room, bool) {
            (crate::room_told::Room::Busy, false)
        }
        let home = crate::process::SessionDirectory::create().unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET))
                .unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut rig = LiveRig::start_with(
            quick_policy(),
            Options {
                reporting: Reporting::Typed,
                tools: crate::tools::Settings {
                    decline: false,
                    exec: Some(home.path.clone()),
                    memory: None,
                    clock: false,
                    ..Default::default()
                },
                attention: Some(busy),
                name_signal: false,
                sounds_checked: false,
                ask_unclear: false,
                name_in_passing_ignored: false,
                policy_holds: None,
                wait_for_room: false,
                staff_note: false,
                spoken_to: false,
                voices: None,
            },
        );
        let mut dial = rig.ready().await;
        rig.speak(&mut dial, false, true).await;
        dial.service
            .send(json!({"serverContent":{"inputTranscription":{"text":"Turn on the light, it is dark in here."}}}))
            .await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Transcript { request: None, .. }
        ));
        dial.service.send(function_call("c9", "light_on")).await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c9","name":"light_on","response":{"ok":true}}
            ]}})
        );
        // Nobody knocked on the executive's door. (That the held answer's
        // sound is dropped is the busy-room test's above.)
        assert!(listener.accept().is_err());
        rig.stop().await;
    }

    /// #941, #1146: a skill's call answered with a look, through the
    /// worker's loop: once its answer has gone to the model, the
    /// coordinator is asked for the look, for the call's request and
    /// session.
    #[tokio::test]
    async fn a_skill_call_answered_with_a_look_asks_the_coordinator_for_it() {
        use std::io::{BufRead, Write};
        let home = crate::process::SessionDirectory::create().unwrap();
        let skills = home.path.join(crate::tools::SKILLS_DIR);
        std::fs::create_dir(&skills).unwrap();
        std::fs::write(
            skills.join(crate::tools::CATALOG_FILE),
            r#"{"tools":[{"name":"check_my_work","description":"A test."}]}"#,
        )
        .unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(skills.join(crate::tools::CALL_SOCKET)).unwrap();
        let skill = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream
                .write_all(b"{\"ok\":true,\"result\":{\"next\":\"Look.\",\"look\":true}}\n")
                .unwrap();
        });
        let mut rig = LiveRig::start_with(
            quick_policy(),
            typed_with(crate::tools::Settings {
                decline: false,
                exec: Some(home.path.clone()),
                memory: None,
                clock: false,
                ..Default::default()
            }),
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(function_call("c8", "check_my_work"))
            .await;
        dial.service.send(idle_complete()).await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c8","name":"check_my_work","response":{"next":"Look.","look":true}}
            ]}})
        );
        skill.join().unwrap();
        let asked = loop {
            match rig.output().await {
                ProviderOutput::LookAsked {
                    request,
                    session,
                    dropped,
                } => break (request, session, dropped),
                _ => continue,
            }
        };
        assert_eq!(asked.0, request);
        assert!(asked.1 > 0);
        assert_eq!(asked.2, None);
        rig.stop().await;
    }

    #[tokio::test]
    async fn a_call_for_the_executive_is_asked_over_its_socket_and_its_answer_goes_back() {
        use std::io::{BufRead, Write};
        let home = crate::process::SessionDirectory::create().unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET))
                .unwrap();
        let executive = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream
                .write_all(b"{\"ok\":true,\"result\":{\"light\":\"blue\"}}\n")
                .unwrap();
            serde_json::from_str::<serde_json::Value>(&line).unwrap()
        });
        let mut rig = LiveRig::start_with(
            quick_policy(),
            typed_with(crate::tools::Settings {
                decline: false,
                exec: Some(home.path.clone()),
                memory: None,
                clock: false,
                ..Default::default()
            }),
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(function_call("c7", "light_colour")).await;
        dial.service.send(idle_complete()).await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c7","name":"light_colour","response":{"light":"blue"}}
            ]}})
        );
        assert_eq!(
            executive.join().unwrap(),
            json!({"op":"call","name":"light_colour","args":{"colour":"blue"}})
        );
        // The answer the model then speaks is the same request's, aloud.
        dial.service.send(provider_audio(5, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: heard, .. } if heard == request
        ));
        rig.stop().await;
    }

    /// The model does what it was asked and then says the request was not
    /// for it. Both cannot be right, and what was done cannot be undone:
    /// the call is answered and not taken, and what the model says next
    /// in that request is played. Seen on lamp-4ace on 2026-10-14, three
    /// requests in a row: the light changed each time and the lamp said
    /// nothing.
    #[tokio::test]
    async fn a_request_the_lamp_acted_on_is_not_one_it_can_then_call_not_for_it() {
        use std::io::{BufRead, Write};
        let home = crate::process::SessionDirectory::create().unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET))
                .unwrap();
        let executive = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream
                .write_all(b"{\"ok\":true,\"result\":{\"result\":\"My light is off.\"}}\n")
                .unwrap();
        });
        let mut rig = LiveRig::start_with(
            quick_policy(),
            typed_with(crate::tools::Settings {
                decline: true,
                exec: Some(home.path.clone()),
                memory: None,
                clock: false,
                ..Default::default()
            }),
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(function_call("c1", "light_off")).await;
        dial.service.send(idle_complete()).await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c1","name":"light_off","response":{"result":"My light is off."}}
            ]}})
        );
        executive.join().unwrap();
        // Its second thought, which is still answered.
        dial.service
            .send(function_call("c2", crate::tools::NOT_FOR_ME))
            .await;
        assert_eq!(
            rig.service_message(&mut dial).await,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c2","name":"not_for_me","response":{"ok":true},"scheduling":"SILENT"}
            ]}})
        );
        // What it then says is heard.
        dial.service.send(provider_audio(5, 480)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: heard, .. } if heard == request
        ));
        dial.service.send(idle_complete()).await;
        loop {
            match rig.output().await {
                ProviderOutput::TurnComplete { request: done, .. } if done == request => break,
                ProviderOutput::Audio { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        rig.owner
            .complete_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        // The next request is its own: declined, it is silent as before.
        let next = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(function_call("c3", crate::tools::NOT_FOR_ME))
            .await;
        rig.service_message(&mut dial).await;
        dial.service.send(provider_audio(9, 480)).await;
        dial.service.send(idle_complete()).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { request: done, idle: true } if done == next
        ));
        assert!(rig.take().is_none(), "a declined request's sound got out");
        rig.stop().await;
    }

    /// With no executive, with one that is not there, and for the voice's
    /// own function while it is switched off, the model is told at once, in
    /// words it can say, and the worker goes on.
    #[tokio::test]
    async fn a_call_nobody_can_run_is_answered_as_unavailable_and_the_worker_goes_on() {
        let home = crate::process::SessionDirectory::create().unwrap();
        for (tools, name) in [
            (crate::tools::Settings::default(), "light_on"),
            (crate::tools::Settings::default(), crate::tools::NOT_FOR_ME),
            (
                crate::tools::Settings {
                    decline: false,
                    exec: Some(home.path.clone()),
                    memory: None,
                    clock: false,
                    ..Default::default()
                },
                "light_on",
            ),
        ] {
            let mut rig = LiveRig::start_with(quick_policy(), typed_with(tools));
            let mut dial = rig.ready().await;
            let request = rig.speak(&mut dial, false, true).await;
            dial.service.send(function_call("c1", name)).await;
            dial.service.send(idle_complete()).await;
            let answer = rig.service_message(&mut dial).await;
            assert_eq!(
                answer["toolResponse"]["functionResponses"][0]["response"],
                crate::tools::unavailable(),
                "{name}"
            );
            // Nothing was declined: what the model says next is played.
            dial.service.send(provider_audio(5, 480)).await;
            assert!(matches!(
                rig.output().await,
                ProviderOutput::Audio { request: heard, .. } if heard == request
            ));
            assert!(!rig.worker.is_finished());
            rig.stop().await;
        }
    }

    /// The next dial of a supervisor that is serviced the way the worker
    /// services it: by its link upkeep alone. No link report may be made
    /// while it is awaited.
    async fn memory_dial<C: Connect>(
        provider: &mut Supervisor<C>,
        clock: &mut LinkClock,
        pending: &mut PendingLink,
        dials: &mut UnboundedReceiver<Dial>,
    ) -> Dial {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                service_link(provider, Some(&mut *clock), pending, |link| {
                    panic!("no link report was expected before the next dial: {link:?}")
                })
                .unwrap();
                if let Ok(dial) = dials.try_recv() {
                    break dial;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("in-memory provider dial")
    }
    /// The policy of a service that runs all day, with waits a test can sit
    /// through.
    fn kept_policy() -> RecoveryPolicy {
        RecoveryPolicy {
            endless: true,
            require_context: false,
            first_backoff: Duration::from_millis(2),
            max_backoff: Duration::from_millis(4),
            ..RecoveryPolicy::default()
        }
    }

    /// A lamp that starts before its network. Every report the kept link
    /// makes goes through the same conversion and checks as in the service:
    /// no connection on the first session, then ready on a later one, as a
    /// link that came back without an earlier conversation. Under the
    /// bounded policy the first failed connection ends the worker's link.
    #[tokio::test]
    async fn a_kept_link_whose_first_connection_fails_reports_no_connection_then_a_fresh_ready() {
        use crate::provider_link::{LinkContext, ProviderLinkState};
        use lamp_gemini::testing::next_dial;
        let (mut provider, mut dials, mut clock, mut pending) = memory_provider(kept_policy());
        drop(next_dial(&mut dials).await);
        let down = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(matches!(down.state, ProviderLinkState::Unavailable { .. }));
        assert_eq!((down.revision, down.session), (1, 1));
        drop(memory_dial(&mut provider, &mut clock, &mut pending, &mut dials).await);
        let mut third = memory_dial(&mut provider, &mut clock, &mut pending, &mut dials).await;
        third.service.accept().await;
        let up = memory_link(&mut provider, &mut clock, &mut pending).await;
        let ProviderLinkState::Ready {
            attempts,
            context,
            outage_started_at_us,
            after,
            ..
        } = up.state
        else {
            panic!("expected ready, got {up:?}")
        };
        assert_eq!((up.revision, up.session), (2, 3));
        assert_eq!((attempts, context), (3, LinkContext::Fresh));
        assert!(after.is_some());
        assert_eq!(outage_started_at_us, Some(down.observed_at_us));
        assert!(provider.is_connected());

        let (mut provider, mut dials, mut clock, mut pending) = memory_provider(quick_policy());
        drop(next_dial(&mut dials).await);
        assert!(matches!(
            memory_link(&mut provider, &mut clock, &mut pending)
                .await
                .state,
            ProviderLinkState::Terminal { .. }
        ));
    }

    /// Five losses inside the window that allows three, then an outage of
    /// twenty failed attempts. Each time the link comes back with a report
    /// the service accepts: the attempt count stops at sixteen, the most a
    /// report may carry.
    #[tokio::test]
    async fn a_kept_link_outlives_a_dropping_connection_and_a_long_outage_with_reports_that_pass() {
        use crate::provider_link::ProviderLinkState;
        use lamp_gemini::testing::next_dial;
        let (mut provider, mut dials, mut clock, mut pending) = memory_provider(kept_policy());
        let mut dial = next_dial(&mut dials).await;
        dial.service.accept().await;
        let mut session = memory_link(&mut provider, &mut clock, &mut pending)
            .await
            .session;
        for _ in 0..5 {
            dial.service.send(resumable()).await;
            dial.service.close(1011, "internal").await;
            assert!(matches!(
                memory_link(&mut provider, &mut clock, &mut pending)
                    .await
                    .state,
                ProviderLinkState::Unavailable { .. }
            ));
            dial = memory_dial(&mut provider, &mut clock, &mut pending, &mut dials).await;
            dial.service.accept().await;
            let back = memory_link(&mut provider, &mut clock, &mut pending).await;
            assert!(matches!(
                back.state,
                ProviderLinkState::Ready { attempts: 1, .. }
            ));
            assert!(back.session > session);
            session = back.session;
        }
        dial.service.send(resumable()).await;
        dial.service.close(1011, "internal").await;
        assert!(matches!(
            memory_link(&mut provider, &mut clock, &mut pending)
                .await
                .state,
            ProviderLinkState::Unavailable { .. }
        ));
        for _ in 0..20 {
            drop(memory_dial(&mut provider, &mut clock, &mut pending, &mut dials).await);
        }
        let mut last = memory_dial(&mut provider, &mut clock, &mut pending, &mut dials).await;
        last.service.accept().await;
        let back = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(matches!(
            back.state,
            ProviderLinkState::Ready { attempts: 16, .. }
        ));
        assert_eq!(back.session, session + 21);
        assert!(provider.is_connected());
    }

    #[test]
    fn keep_link_is_taken_from_the_front_of_a_workers_flags() {
        let flags = [KEEP_LINK_FLAG.to_owned(), "rest".to_owned()];
        let (kept, rest) = take_keep_link(&flags);
        assert!(kept);
        assert_eq!(rest, ["rest"]);
        let (kept, rest) = take_keep_link(&flags[1..]);
        assert!(!kept);
        assert_eq!(rest, ["rest"]);
    }

    /// Audit regression: root selected A before the worker received this Start.
    /// Advance the real in-memory supervisor to B before consuming that command.
    /// The selected session travels unchanged through the handoff. Refusal
    /// produces one local outcome; neither its opening PCM nor tail reaches B.
    #[tokio::test]
    async fn audit_delayed_start_selected_on_a_must_not_reach_recovered_b() {
        use crate::provider_link::ProviderLinkState;
        use lamp_gemini::testing::next_dial;
        let (mut provider, mut dials, mut clock, mut pending) = memory_provider(quick_policy());
        let mut first = next_dial(&mut dials).await;
        first.service.accept().await;
        let selected = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(matches!(selected.state, ProviderLinkState::Ready { .. }));
        let mut owner = controller();
        let mut handoff = InputHandoff {
            tolerate_outage: true,
            ..InputHandoff::default()
        };
        let request = authorize(&mut owner, &mut handoff);
        let queued_before_outage = ProviderInput::Start {
            request,
            session: selected.session,
            privacy_generation: generation(&handoff),
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        };

        first.service.send(resumable()).await;
        first
            .service
            .close(1012, "controlled audit transition")
            .await;
        assert!(matches!(
            memory_link(&mut provider, &mut clock, &mut pending)
                .await
                .state,
            ProviderLinkState::Unavailable { .. }
        ));
        let mut second = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                service_link(&mut provider, Some(&mut clock), &mut pending, |_| {
                    panic!("replacement cannot be ready before setup")
                })
                .unwrap();
                if let Ok(dial) = dials.try_recv() {
                    break dial;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        second.service.accept().await;
        let replacement = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(matches!(replacement.state, ProviderLinkState::Ready { .. }));
        assert!(replacement.session > selected.session);

        handoff
            .submit(queued_before_outage, NOW, &provider)
            .unwrap();
        assert_eq!(handoff.undelivered, Some(request));
        assert!(handoff.submitted.is_none());
        let mut output = VecDeque::new();
        queue_undelivered(&mut handoff, &mut output);
        queue_undelivered(&mut handoff, &mut output);
        for command in [
            start(request, &handoff),
            audio(request, 1, 7, &handoff),
            end(request, &handoff),
        ] {
            handoff.submit(command, NOW, &provider).unwrap();
        }
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(output.len(), 1);
        assert!(matches!(output.pop_front(), Some(Outbound::Status(
            ProviderStatus::TurnFailed { request: got, stage: FailedStage::NotDelivered,
                reason: Fault::Local }
        )) if got == request));
        assert!(
            second
                .service
                .try_receive(Duration::from_millis(10))
                .await
                .is_none()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), next_notice(&mut provider, true))
                .await
                .is_err(),
            "refused Start must not create a supervisor outcome or Started"
        );

        // A fresh, explicitly B-selected request works; the old refusal cannot
        // borrow B, poison B's input queue, or replay the previous prefix.
        let fresh = authorize(&mut owner, &mut handoff);
        handoff
            .submit(
                ProviderInput::Start {
                    request: fresh,
                    session: replacement.session,
                    privacy_generation: generation(&handoff),
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                },
                NOW,
                &provider,
            )
            .unwrap();
        handoff
            .submit(audio(fresh, 1, 23, &handoff), NOW, &provider)
            .unwrap();
        handoff
            .submit(end(fresh, &handoff), NOW, &provider)
            .unwrap();
        assert!(matches!(next_notice(&mut provider, true).await.unwrap(),
            Notice::Event(Event::InputStarted { lineage, .. })
                if lineage.request.get() == fresh && lineage.session.get() == replacement.session));
        assert!(
            second.service.receive().await["realtimeInput"]
                .get("activityStart")
                .is_some()
        );
        assert!(
            second.service.receive().await["realtimeInput"]
                .get("audio")
                .is_some()
        );
        assert!(
            second.service.receive().await["realtimeInput"]
                .get("activityEnd")
                .is_some()
        );
        assert!(
            second
                .service
                .try_receive(Duration::from_millis(10))
                .await
                .is_none()
        );
        // At the actual worker sink boundary, a late A-selected Start cannot
        // retire B's already accepted response. Authority-driven cancellation
        // is separate and is not invoked by this refusal.
        assert_eq!(
            InputSink::start(
                &provider,
                SessionId::new(selected.session).unwrap(),
                RequestId::new(fresh + 1).unwrap()
            ),
            Err(StartRefusal::SessionMismatch {
                live: second.session
            })
        );
        second.service.send(provider_audio(29, 960)).await;
        assert!(matches!(next_notice(&mut provider, true).await.unwrap(),
            Notice::Event(Event::Audio { lineage, pcm, .. })
                if lineage.request.get() == fresh && lineage.session == second.session
                    && pcm == vec![29; 960]));
        assert!(
            second
                .service
                .try_receive(Duration::from_millis(10))
                .await
                .is_none()
        );
        provider.shutdown();
    }

    /// A successful local enqueue is not proof that activityStart was written.
    #[tokio::test]
    async fn audit_accepted_start_can_fail_before_started_acknowledgement() {
        use crate::provider_link::ProviderLinkState;
        use lamp_gemini::testing::next_dial;
        for ended in [false, true] {
            let (mut provider, mut dials, mut clock, mut pending) =
                memory_provider(RecoveryPolicy {
                    max_attempts: 0,
                    ..RecoveryPolicy::default()
                });
            let mut first = next_dial(&mut dials).await;
            first.service.accept().await;
            assert!(matches!(
                memory_link(&mut provider, &mut clock, &mut pending)
                    .await
                    .state,
                ProviderLinkState::Ready { .. }
            ));
            let request = RequestId::new(1).unwrap();
            provider.try_start(request).unwrap();
            if ended {
                provider.try_end(request).unwrap();
            }
            // No await between enqueue and peer drop: the current-thread actor
            // has not consumed Start, so it cannot have emitted InputStarted.
            drop(first);
            assert!(matches!(
                memory_link(&mut provider, &mut clock, &mut pending)
                    .await
                    .state,
                ProviderLinkState::Unavailable { .. }
            ));
            assert!(matches!(
                memory_link(&mut provider, &mut clock, &mut pending)
                    .await
                    .state,
                ProviderLinkState::Terminal { .. }
            ));
            let expected = if ended {
                Stage::AwaitingResponse
            } else {
                Stage::InputOpen
            };
            let actual = next_notice(&mut provider, true).await.unwrap();
            eprintln!("audit: accepted_end={ended} first_ordered_notice={actual:?}");
            assert!(matches!(actual, Notice::TurnLost { stage, .. } if stage == expected));
            provider.shutdown();
        }
    }

    #[tokio::test]
    async fn coordinated_memory_control_reconnects_while_prior_answer_is_unconsumed() {
        use crate::provider_link::{LinkContext, ProviderLinkState};
        use lamp_gemini::testing::next_dial;
        let (mut provider, mut dials, mut clock, mut pending) = memory_provider(quick_policy());
        let mut first = next_dial(&mut dials).await;
        first.service.accept().await;
        assert!(matches!(
            memory_link(&mut provider, &mut clock, &mut pending)
                .await
                .state,
            ProviderLinkState::Ready { .. }
        ));
        let request = RequestId::new(1).unwrap();
        provider.try_start(request).unwrap();
        assert!(matches!(
            next_notice(&mut provider, true).await.unwrap(),
            Notice::Event(Event::InputStarted { .. })
        ));
        provider.try_end(request).unwrap();
        first.service.receive().await;
        first.service.receive().await;
        first.service.send(resumable()).await;
        first.service.send(provider_audio(7, 1_920)).await;
        first.service.close(1012, "unconsumed old answer").await;
        let unavailable = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(matches!(
            unavailable.state,
            ProviderLinkState::Unavailable { .. }
        ));
        // This is the same control upkeep called by serve at zero output credit.
        // Do not call next_output until B's setup has completed and been reported.
        let mut second = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                service_link(&mut provider, Some(&mut clock), &mut pending, |_| {
                    panic!("no setup yet")
                })
                .unwrap();
                if let Ok(dial) = dials.try_recv() {
                    break dial;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert!(second.resumed);
        second.service.accept().await;
        let ready = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(ready.revision > unavailable.revision);
        assert_eq!(ready.session, second.session.get());
        assert!(matches!(ready.state, ProviderLinkState::Ready {
            context: LinkContext::Resumed, outage_started_at_us: Some(at), ..
        } if at == unavailable.observed_at_us));
        assert!(
            matches!(next_notice(&mut provider, true).await.unwrap(), Notice::Event(Event::Audio { pcm, .. }) if pcm == vec![7; 1_920])
        );
        assert!(
            matches!(next_notice(&mut provider, true).await.unwrap(), Notice::TurnLost {
            lineage, stage: Stage::Responding, ..
        } if lineage.request == request)
        );
        provider.shutdown();
    }

    #[tokio::test]
    async fn coordinated_memory_blocked_link_send_keeps_latest_source_stamp_and_shutdown() {
        use crate::provider_link::ProviderLinkState;
        use lamp_gemini::testing::next_dial;
        let (mut provider, mut dials, mut clock, mut pending) = memory_provider(quick_policy());
        let mut first = next_dial(&mut dials).await;
        first.service.accept().await;
        memory_link(&mut provider, &mut clock, &mut pending).await;
        first.service.send(resumable()).await;
        first.service.close(1012, "coalesced outage").await;
        let mut latest = None;
        let mut second = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                service_link(&mut provider, Some(&mut clock), &mut pending, |link| {
                    latest = Some(link);
                    Err(io::ErrorKind::WouldBlock.into())
                })
                .unwrap();
                if let Ok(dial) = dials.try_recv() {
                    break dial;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            latest.unwrap().state,
            ProviderLinkState::Unavailable { .. }
        ));
        second.service.accept().await;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                service_link(&mut provider, Some(&mut clock), &mut pending, |link| {
                    latest = Some(link);
                    Err(io::ErrorKind::WouldBlock.into())
                })
                .unwrap();
                if matches!(latest.unwrap().state, ProviderLinkState::Ready { .. }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let original = latest.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            memory_link(&mut provider, &mut clock, &mut pending).await,
            original
        );
        // Another unanswered setup must not defer local shutdown. A blocked
        // control callback cannot keep the supervisor alive or replay input.
        second.service.close(1012, "hung setup").await;
        memory_link(&mut provider, &mut clock, &mut pending).await;
        let _hanging = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                service_link(&mut provider, Some(&mut clock), &mut pending, |_| {
                    Err(io::ErrorKind::WouldBlock.into())
                })
                .unwrap();
                if let Ok(dial) = dials.try_recv() {
                    break dial;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        provider.shutdown();
        let terminal = memory_link(&mut provider, &mut clock, &mut pending).await;
        assert!(matches!(
            terminal.state,
            ProviderLinkState::Terminal {
                reason: LinkFailure::Shutdown,
                ..
            }
        ));
        assert_eq!(
            next_notice(&mut provider, true).await.unwrap_err().reason,
            FailureReason::Shutdown
        );
    }

    #[tokio::test]
    async fn coordinated_memory_terminal_control_does_not_erase_the_answer_or_turn_failure() {
        use crate::provider_link::ProviderLinkState;
        use lamp_gemini::testing::next_dial;
        let (mut provider, mut dials, mut clock, mut pending) =
            memory_provider(RecoveryPolicy::DISABLED);
        let mut first = next_dial(&mut dials).await;
        first.service.accept().await;
        memory_link(&mut provider, &mut clock, &mut pending).await;
        let request = RequestId::new(1).unwrap();
        provider.try_start(request).unwrap();
        next_notice(&mut provider, true).await.unwrap();
        provider.try_end(request).unwrap();
        first.service.receive().await;
        first.service.receive().await;
        first.service.send(provider_audio(9, 2_000)).await;
        first
            .service
            .close(1012, "terminal with retained audio")
            .await;
        assert!(matches!(
            memory_link(&mut provider, &mut clock, &mut pending)
                .await
                .state,
            ProviderLinkState::Unavailable { .. }
        ));
        assert!(matches!(
            memory_link(&mut provider, &mut clock, &mut pending)
                .await
                .state,
            ProviderLinkState::Terminal { .. }
        ));
        assert!(
            matches!(next_notice(&mut provider, true).await.unwrap(), Notice::Event(Event::Audio { pcm, .. }) if pcm == vec![9; 2_000])
        );
        assert!(
            matches!(next_notice(&mut provider, true).await.unwrap(), Notice::TurnLost { lineage, stage: Stage::Responding, .. } if lineage.request == request)
        );
        assert_eq!(
            next_notice(&mut provider, true).await.unwrap_err().reason,
            FailureReason::NotRecoverable
        );
        assert_eq!(
            provider.try_start(RequestId::new(2).unwrap()),
            Err(Error::NotConnected)
        );
        assert!(dials.try_recv().is_err());
        provider.shutdown();
    }

    #[tokio::test]
    async fn coordinated_reconnect_and_link_status_progress_without_any_pcm_credit() {
        use crate::provider_link::{LinkContext, ProviderLinkState};
        let mut rig =
            LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Coordinated));
        let mut dial = rig.coordinated_ready().await;
        dial.service.send(resumable()).await;
        let request = rig.speak(&mut dial, false, true).await;
        // Consume the two granted packets but return no credit. Read them
        // promptly, so this is output backpressure rather than stale IPC.
        dial.service.send(provider_audio(7, 1_920)).await;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut consumed = 0;
        while consumed < 2 {
            rig.publish();
            if let Some(ProviderOutput::Audio {
                request: got,
                samples,
                ..
            }) = rig.parent.data.receive().unwrap()
            {
                assert_eq!(got, request);
                assert_eq!(samples, vec![7; 960]);
                rig.output_flow.received().unwrap();
                consumed += 1;
            }
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        dial.service.send(provider_audio(8, 1_920)).await;
        dial.service.close(1012, "scripted restart").await;
        let unavailable = rig.link().await;
        assert!(matches!(
            unavailable.state,
            ProviderLinkState::Unavailable { .. }
        ));
        let mut next = rig.dial().await;
        let setup = rig.service_message(&mut next).await;
        assert!(setup["setup"].get("sessionResumption").is_some());
        next.service.send(json!({"setupComplete":{}})).await;
        let recovered = rig.link().await;
        // This is the first resume from A's advertised point. Resumed does
        // not acknowledge that the point includes the latest client messages;
        // later-connection input with only an inherited A handle is a separate
        // ResumedBeforeLatest case covered by the supervisor tests.
        assert!(
            matches!(recovered.state, ProviderLinkState::Ready {
            context: LinkContext::Resumed, outage_started_at_us: Some(at), ..
        } if at == unavailable.observed_at_us),
            "recovered={recovered:?}; unavailable={unavailable:?}"
        );
        assert!(recovered.session > unavailable.session);
        assert!(recovered.revision > unavailable.revision);
        assert!(
            rig.parent
                .data
                .receive::<ProviderOutput>()
                .unwrap()
                .is_none()
        );
        // The old answer and its exact terminal stage still follow in order.
        rig.output_flow
            .replenish(0, &mut rig.parent.control)
            .unwrap();
        for _ in 0..2 {
            assert!(matches!(rig.output().await, ProviderOutput::Audio {
                request: got, samples, ..
            } if got == request && samples == vec![8; 960]));
        }
        assert!(
            matches!(rig.wire().await, Wire::Status(ProviderStatus::TurnFailed {
            request: got, stage: FailedStage::Responding, ..
        }) if got == request)
        );
        rig.stop().await;
    }

    #[tokio::test]
    async fn coordinated_terminal_link_precedes_ordered_pcm_outcome_and_single_ended() {
        use crate::provider_link::ProviderLinkState;
        let mut rig = LiveRig::start_with(
            RecoveryPolicy::DISABLED,
            Options::reporting(Reporting::Coordinated),
        );
        let mut dial = rig.coordinated_ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(9, 3_840)).await;
        // Drain only the two already granted packets, withholding more credit.
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut consumed = 0;
        while consumed < 2 {
            rig.publish();
            if let Some(ProviderOutput::Audio { request: got, .. }) =
                rig.parent.data.receive().unwrap()
            {
                assert_eq!(got, request);
                rig.output_flow.received().unwrap();
                consumed += 1;
            }
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        dial.service.close(1012, "terminal").await;
        let mut revisions = Vec::new();
        loop {
            let link = rig.link().await;
            revisions.push(link.revision);
            if matches!(link.state, ProviderLinkState::Terminal { .. }) {
                break;
            }
            assert!(matches!(link.state, ProviderLinkState::Unavailable { .. }));
        }
        assert!(revisions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!rig.worker.is_finished());
        assert!(
            rig.parent
                .data
                .receive::<ProviderOutput>()
                .unwrap()
                .is_none()
        );
        rig.output_flow
            .replenish(0, &mut rig.parent.control)
            .unwrap();
        for _ in 0..2 {
            assert!(matches!(rig.output().await, ProviderOutput::Audio {
                request: got, samples, ..
            } if got == request && samples == vec![9; 960]));
        }
        assert!(
            matches!(rig.wire().await, Wire::Status(ProviderStatus::TurnFailed {
            request: got, stage: FailedStage::Responding, ..
        }) if got == request)
        );
        assert!(matches!(rig.output().await, ProviderOutput::Ended { .. }));
        rig.pump(Duration::from_millis(25)).await;
        assert!(rig.take().is_none(), "Ended must be emitted once");
        assert!(
            rig.dials.try_recv().is_err(),
            "disabled recovery must not replay input"
        );
        // An in-flight Start can arrive after Ended. It needs its own terminal
        // receipt; upstream termination is not a silent local intake cutoff.
        let old = rig.snapshot.owner().unwrap();
        rig.owner.cancel_turn(real_time(), old).unwrap();
        let newer = rig
            .owner
            .admit(real_time(), AdmittedInput::NewTurn)
            .unwrap();
        rig.publish();
        let privacy_generation = rig.snapshot.microphone_generation();
        rig.parent
            .data
            .send(ProviderInput::Start {
                session: 1,
                request: newer.turn(),
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            })
            .unwrap();
        assert!(
            matches!(rig.wire().await, Wire::Status(ProviderStatus::TurnFailed {
            request: got, stage: FailedStage::NotDelivered, ..
        }) if got == newer.turn())
        );
        rig.parent
            .data
            .send(ProviderInput::Audio {
                request: newer.turn(),
                privacy_generation,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![3; 160],
            })
            .unwrap();
        rig.parent
            .data
            .send(ProviderInput::End {
                request: newer.turn(),
                privacy_generation,
                sound: false,
                cut_in: false,
            })
            .unwrap();
        rig.pump(Duration::from_millis(25)).await;
        assert!(
            rig.take().is_none(),
            "refused Start has exactly one outcome"
        );
        assert!(
            rig.dials.try_recv().is_err(),
            "terminal upstream must never reconnect"
        );
        assert!(
            dial.service
                .try_receive(Duration::from_millis(5))
                .await
                .is_none()
        );
        rig.stop().await;
    }

    #[tokio::test]
    async fn coordinated_stop_does_not_wait_for_hung_reconnect_or_data_credit() {
        use crate::provider_link::ProviderLinkState;
        let mut rig =
            LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Coordinated));
        let mut dial = rig.coordinated_ready().await;
        dial.service.send(resumable()).await;
        dial.service.close(1012, "outage").await;
        assert!(matches!(
            rig.link().await.state,
            ProviderLinkState::Unavailable { .. }
        ));
        let mut hanging = rig.dial().await;
        assert!(
            rig.service_message(&mut hanging)
                .await
                .get("setup")
                .is_some()
        );
        // Setup is deliberately left unanswered. Local control owns shutdown.
        rig.parent.control.send(Control::Stop).unwrap();
        let (result, elapsed) = rig.exit().await;
        result.unwrap();
        assert!(
            elapsed < Duration::from_millis(250),
            "Stop took {elapsed:?}"
        );
        assert!(
            hanging
                .service
                .try_receive(Duration::from_millis(10))
                .await
                .is_none()
        );
    }

    #[test]
    fn coordinated_full_control_send_is_bounded_and_does_not_consume_stop() {
        use crate::provider_link::{ProviderLink, ProviderLinkState};
        let (_directory, mut parent, mut worker) = intake_channels();
        let mut blocked = false;
        for _ in 0..4_096 {
            match worker.control.send(WorkerEvent::Ready) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    blocked = true;
                    break;
                }
                Err(error) => panic!("unexpected control send error: {error}"),
            }
        }
        assert!(blocked, "control socket must have finite capacity");
        let mut pending = PendingLink::default();
        pending
            .observe(
                ProviderLink {
                    revision: 1,
                    session: 1,
                    observed_at_us: monotonic_us(),
                    source_clock: None,
                    state: ProviderLinkState::Unavailable {
                        reason: Fault::Transport,
                    },
                },
                monotonic_us(),
            )
            .unwrap();
        pending
            .try_send(|link| worker.control.send(WorkerEvent::ProviderLink { link }))
            .unwrap();
        parent.control.send(Control::Stop).unwrap();
        let mut intake = ProviderIntake::new(BootId::new([9; 16]).unwrap(), monotonic_us());
        let mut budget = CONTROL_SLICE;
        assert_eq!(
            intake
                .drain_control(
                    &mut monotonic_us,
                    &mut || worker.control.receive(),
                    &mut budget
                )
                .unwrap(),
            ControlDrain::Stopped
        );
        let mut recovered = 0;
        pending
            .try_send(|_| {
                recovered += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(recovered, 1, "WouldBlock must retain exactly one status");
    }

    #[tokio::test]
    async fn coordinated_privacy_revocation_stops_a_pending_connection() {
        let mut rig =
            LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Coordinated));
        let mut dial = rig.coordinated_ready().await;
        dial.service.send(resumable()).await;
        dial.service.close(1012, "outage").await;
        let _unavailable = rig.link().await;
        let mut hanging = rig.dial().await;
        assert!(
            rig.service_message(&mut hanging)
                .await
                .get("setup")
                .is_some()
        );
        rig.owner
            .set_microphone_permission(real_time(), Permission::Denied)
            .unwrap();
        rig.snapshot = rig.owner.snapshot(real_time()).unwrap();
        rig.parent
            .control
            .send(Control::Authority {
                snapshot: rig.snapshot,
            })
            .unwrap();
        let result = tokio::time::timeout(Duration::from_millis(250), &mut rig.worker)
            .await
            .unwrap()
            .unwrap();
        result.unwrap();
        assert!(
            hanging
                .service
                .try_receive(Duration::from_millis(10))
                .await
                .is_none()
        );
    }

    #[test]
    fn coordinated_terminal_and_failure_receipts_keep_the_existing_full_output_bound() {
        let mut output = VecDeque::from([Outbound::Output(ProviderOutput::Ended {
            reason: LinkFailure::NotRecoverable,
            fault: Fault::Transport,
            attempts: 0,
        })]);
        for request in 1..MAX_QUEUED_OUTPUT as u64 {
            output.push_back(Outbound::Output(ProviderOutput::TurnFailed {
                request,
                stage: FailedStage::Responding,
                reason: Fault::Transport,
            }));
        }
        // These are terminal receipts, not permission to play: cancellation
        // cannot erase them just because their immutable owner was retired.
        prune_retired_output(&mut output, u64::MAX);
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
        let mut handoff = InputHandoff {
            undelivered: Some(999),
            ..InputHandoff::default()
        };
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
        assert_eq!(handoff.undelivered, Some(999));
        assert!(matches!(
            output.pop_front(),
            Some(Outbound::Output(ProviderOutput::Ended { .. }))
        ));
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
        assert_eq!(handoff.undelivered, None);
        assert!(matches!(
            output.back(),
            Some(Outbound::Status(ProviderStatus::TurnFailed {
                request: 999,
                stage: FailedStage::NotDelivered,
                ..
            }))
        ));
        queue_undelivered(&mut handoff, &mut output);
        assert_eq!(output.len(), MAX_QUEUED_OUTPUT);
    }

    #[tokio::test]
    async fn typed_replaced_session_drops_old_input_without_failing_the_new_connection() {
        use lamp_gemini::{LinkUpdate, LinkUpdateKind, OutputNotice, testing::next_dial};

        async fn link<C: Connect>(provider: &mut Supervisor<C>) -> LinkUpdate {
            tokio::time::timeout(Duration::from_secs(2), async {
                let mut tick = tokio::time::interval(Duration::from_millis(1));
                loop {
                    tick.tick().await;
                    if let Some(update) = provider.poll_link() {
                        return update;
                    }
                }
            })
            .await
            .expect("link observation")
        }

        // Exercise Audio, End and the implicit End before successor Start in
        // both modes. The production Legacy contract must still fail closed.
        for case in 0..6 {
            let mode = case % 3;
            let typed = case >= 3;
            let config =
                SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
                    .unwrap()
                    .resumption(RESUMPTION)
                    .barrier_policy(BARRIER);
            let (connector, mut dials) = connector(config);
            let mut provider = Supervisor::new(connector, quick_policy()).unwrap();
            let mut first = next_dial(&mut dials).await;
            first.service.accept().await;
            assert!(matches!(
                link(&mut provider).await.kind,
                LinkUpdateKind::Ready { .. }
            ));
            let mut owner = controller();
            let mut handoff = InputHandoff {
                tolerate_outage: typed,
                ..InputHandoff::default()
            };
            let old = authorize(&mut owner, &mut handoff);
            handoff
                .submit(start(old, &handoff), NOW, &provider)
                .unwrap();
            assert!(matches!(
                provider.next_output().await.unwrap(),
                OutputNotice::Event(Event::InputStarted { .. })
            ));
            assert!(
                first.service.receive().await["realtimeInput"]
                    .get("activityStart")
                    .is_some()
            );
            handoff
                .submit(audio(old, 1, 7, &handoff), NOW, &provider)
                .unwrap();
            assert!(
                first.service.receive().await["realtimeInput"]
                    .get("audio")
                    .is_some()
            );
            first.service.send(resumable()).await;
            first.service.close(1011, "test disconnect").await;
            assert!(matches!(
                link(&mut provider).await.kind,
                LinkUpdateKind::Unavailable { .. }
            ));
            let mut replacement = next_dial(&mut dials).await;
            assert!(replacement.resumed);
            replacement.service.accept().await;
            assert!(matches!(
                link(&mut provider).await.kind,
                LinkUpdateKind::Ready { .. }
            ));
            assert!(provider.is_connected());

            if mode < 2 {
                let command = if mode == 0 {
                    audio(old, 2, 8, &handoff)
                } else {
                    end(old, &handoff)
                };
                let result = handoff.submit(command, NOW, &provider);
                if !typed {
                    assert!(result.unwrap_err().to_string().contains("StaleRequest"));
                    provider.shutdown();
                    continue;
                }
                result.expect("known old-session input is discarded");
                assert_eq!(handoff.undelivered, None);
                assert!(
                    matches!(provider.next_output().await.unwrap(), OutputNotice::TurnLost { lineage, stage: Stage::InputOpen, .. } if lineage.request.get() == old)
                );
                assert!(
                    replacement
                        .service
                        .try_receive(Duration::from_millis(10))
                        .await
                        .is_none()
                );
            }
            let next = authorize(&mut owner, &mut handoff);
            let result = handoff.submit(
                ProviderInput::Start {
                    request: next,
                    session: replacement.session.get(),
                    privacy_generation: generation(&handoff),
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                },
                NOW,
                &provider,
            );
            if !typed {
                assert!(result.unwrap_err().to_string().contains("StaleRequest"));
                provider.shutdown();
                continue;
            }
            result.expect("new connection remains usable");
            assert!(
                matches!(provider.next_output().await.unwrap(), OutputNotice::Event(Event::InputStarted { lineage, .. }) if lineage.request.get() == next)
            );
            assert!(
                replacement.service.receive().await["realtimeInput"]
                    .get("activityStart")
                    .is_some()
            );
            assert_eq!(handoff.undelivered, None);
            provider.shutdown();
        }
    }

    #[tokio::test]
    async fn exhausted_output_credit_preserves_input_and_priority_stop() {
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(provider_audio(5, lamp_gemini::MAX_OUTPUT_SAMPLES))
            .await;
        // Consume the two already-reserved packets without returning capacity.
        // Nothing else may be timestamped/sent while playback has no space.
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                rig.publish();
                if let Some(output) = rig.parent.data.receive::<ProviderOutput>().unwrap() {
                    rig.output_flow.received().unwrap();
                    assert!(
                        matches!(output, ProviderOutput::Audio { request: current, .. } if current == request)
                    );
                    break;
                }
                assert!(Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
        rig.pump(Duration::from_millis(150)).await;
        assert!(
            rig.parent
                .data
                .receive::<ProviderOutput>()
                .unwrap()
                .is_none()
        );
        assert!(!rig.worker.is_finished());

        let next = rig
            .owner
            .admit(real_time(), AdmittedInput::Interruption)
            .unwrap();
        rig.publish();
        let privacy_generation = rig.snapshot.microphone_generation();
        rig.parent
            .data
            .send(ProviderInput::Start {
                session: 1,
                request: next.turn(),
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            })
            .unwrap();
        rig.parent
            .data
            .send(ProviderInput::Audio {
                request: next.turn(),
                privacy_generation,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![9; 160],
            })
            .unwrap();
        // The new microphone activity reaches the scripted service even while
        // all provider-output credits are exhausted. No old audio is relabelled.
        let start = rig.service_message(&mut dial).await;
        assert!(start["realtimeInput"].get("activityStart").is_some());
        let audio = rig.service_message(&mut dial).await;
        assert!(audio["realtimeInput"].get("audio").is_some());
        assert!(
            rig.parent
                .data
                .receive::<ProviderOutput>()
                .unwrap()
                .is_none()
        );
        rig.parent.control.send(Control::Stop).unwrap();
        let (result, elapsed) = rig.exit().await;
        assert!(result.is_ok());
        println!(
            "MEASURED stop_during_zero_output_credit_us={} (host control send to worker task end)",
            elapsed.as_micros()
        );
        assert!(elapsed < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn complete_answer_then_idle_provider_loss_recovers_with_context_and_a_second_ready() {
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        let request = rig.speak(&mut dial, false, true).await;
        // 2,000 frames arrive as one provider message; the service then closes
        // the idle session, as measured on V1 after 86-198 s without a turn.
        dial.service.send(provider_audio(5, 2_000)).await;
        dial.service
            .send(json!({"serverContent":{"generationComplete":true}}))
            .await;
        dial.service.send(idle_complete()).await;
        dial.service.close(1008, "The operation was aborted").await;
        let mut frames = 0;
        for offset in [0, 960, 1_920] {
            let ProviderOutput::Audio {
                request: owner,
                source_frames,
                source_offset,
                samples,
                ..
            } = rig.output().await
            else {
                panic!("expected provider audio");
            };
            assert_eq!(
                (owner, source_frames, source_offset),
                (request, 2_000, offset)
            );
            assert!(samples.iter().all(|sample| *sample == 5));
            frames += samples.len();
        }
        assert_eq!(frames, 2_000);
        assert!(matches!(
            rig.output().await,
            ProviderOutput::GenerationComplete { request: done } if done == request
        ));
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { request: done, idle: true } if done == request
        ));
        // The coordinator finishes the turn; the worker reconnects on its own.
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.complete_turn(real_time(), owner).unwrap();
        let mut redial = rig.ready().await;
        assert!(redial.resumed);
        assert_eq!(redial.session.get(), 2);
        // The next question is served by the resumed session.
        let next = rig.speak(&mut redial, false, true).await;
        redial.service.send(provider_audio(6, 240)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: owner, .. } if owner == next
        ));
        rig.parent.control.send(Control::Stop).unwrap();
        assert!(rig.exit().await.0.is_ok());
    }

    #[tokio::test]
    async fn answer_generated_before_the_service_closed_is_settled_not_failed() {
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(5, 480)).await;
        dial.service
            .send(json!({"serverContent":{"generationComplete":true}}))
            .await;
        dial.service.close(1008, "idle").await;
        assert!(matches!(rig.output().await, ProviderOutput::Audio { .. }));
        assert!(matches!(
            rig.output().await,
            ProviderOutput::GenerationComplete { .. }
        ));
        // No idle completion ever came from the service. The closed connection
        // is the barrier, so the fully delivered answer can finish normally.
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { request: done, idle: true } if done == request
        ));
        rig.ready().await;
    }

    #[tokio::test]
    async fn provider_loss_before_an_answer_completes_ends_the_worker_naming_request_and_stage() {
        for (stage, expected) in [(0, "InputOpen"), (1, "AwaitingResponse"), (2, "Responding")] {
            let mut rig = LiveRig::start(quick_policy());
            let mut dial = rig.ready().await;
            dial.service.send(resumable()).await;
            let request = rig.speak(&mut dial, false, stage > 0).await;
            if stage == 2 {
                dial.service.send(provider_audio(5, 480)).await;
                assert!(matches!(rig.output().await, ProviderOutput::Audio { .. }));
            }
            dial.service.close(1011, "PRIVATE BACKEND DETAIL").await;
            let (result, _) = rig.exit().await;
            let message = result.unwrap_err().to_string();
            assert!(
                message.contains("PeerClosed { code: Some(1011) }"),
                "{message}"
            );
            assert!(
                message.contains(&format!("request {request} was lost at {expected}")),
                "{message}"
            );
            assert!(!message.contains("PRIVATE"));
        }
    }

    #[tokio::test]
    async fn stop_and_heartbeat_are_serviced_while_a_reconnect_hangs() {
        let mut rig = LiveRig::start(RecoveryPolicy {
            outage_budget: Duration::from_secs(5),
            ..quick_policy()
        });
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        dial.service.close(1008, "idle").await;
        // The service never answers the reconnect. For longer than the 250 ms
        // controller heartbeat bound the worker must keep draining control.
        let _hung = rig.dial().await;
        rig.pump(Duration::from_millis(400)).await;
        assert!(!rig.worker.is_finished());
        assert!(
            rig.parent
                .data
                .receive::<ProviderOutput>()
                .unwrap()
                .is_none()
        );
        rig.parent.control.send(Control::Stop).unwrap();
        let (result, waited) = rig.exit().await;
        assert!(result.is_ok());
        println!(
            "MEASURED stop_during_hung_reconnect_us={} (local socket send to worker task end, host)",
            waited.as_micros()
        );
        assert!(waited < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn input_admitted_during_an_outage_ends_the_worker_instead_of_being_held() {
        let mut rig = LiveRig::start(RecoveryPolicy {
            outage_budget: Duration::from_secs(5),
            ..quick_policy()
        });
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        dial.service.close(1008, "idle").await;
        let _hung = rig.dial().await;
        let owner = rig
            .owner
            .admit(real_time(), AdmittedInput::NewTurn)
            .unwrap();
        rig.publish();
        rig.parent
            .data
            .send(ProviderInput::Start {
                session: 1,
                request: owner.turn(),
                privacy_generation: rig.snapshot.microphone_generation(),
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            })
            .unwrap();
        let message = rig.exit().await.0.unwrap_err().to_string();
        assert!(
            message.contains("provider unavailable while input was admitted"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn exhausted_recovery_and_quota_end_with_fixed_categories() {
        let mut rig = LiveRig::start(RecoveryPolicy {
            max_attempts: 2,
            ..quick_policy()
        });
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        dial.service.close(1008, "idle").await;
        drop(rig.dial().await);
        drop(rig.dial().await);
        let message = rig.exit().await.0.unwrap_err().to_string();
        assert!(
            message.contains("AttemptsExhausted after 2 recovery attempt(s)"),
            "{message}"
        );

        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        dial.service
            .close(1011, "You exceeded your current quota PRIVATE")
            .await;
        let message = rig.exit().await.0.unwrap_err().to_string();
        assert_eq!(
            message,
            "provider connection ended: Gemini Live QuotaExceeded; NotRecoverable after 0 recovery attempt(s)"
        );

        // No handle was ever issued: the conversation could not be restored,
        // so the loss is reported rather than hidden behind a fresh session.
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        dial.service.close(1008, "idle").await;
        let message = rig.exit().await.0.unwrap_err().to_string();
        assert!(message.contains("NoResumableContext"), "{message}");
    }

    #[tokio::test]
    async fn fast_provider_burst_is_held_by_flow_control_and_arrives_complete_in_order() {
        // Built before anything is on a real-time lease: encoding megabytes of
        // scripted audio must not be what makes this harness miss a heartbeat.
        const MESSAGES: usize = 40;
        const FRAMES: usize = lamp_gemini::MAX_OUTPUT_SAMPLES;
        let burst: Vec<_> = (0..MESSAGES)
            .map(|index| provider_audio(index as i16, FRAMES))
            .collect();
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        // 80 s of speech offered at once: 40 provider messages of 2 s each,
        // 2,000 IPC packets. The coordinator now grants at most two packets
        // in flight, so its backlog would pass the 512-packet bound that used to
        // end it. The IPC freshness rule means the coordinator side must keep
        // reading; it does, as fast as packets arrive.
        let mut service = dial.service;
        let script = tokio::spawn(async move {
            service.burst(burst).await;
            service.send(idle_complete()).await;
            service
        });
        let started = Instant::now();
        let (mut packets, mut last_sequence, mut frames) = (0, 0, 0usize);
        loop {
            match rig.output().await {
                ProviderOutput::Audio {
                    request: owner,
                    sequence,
                    source_offset,
                    samples,
                    ..
                } => {
                    if packets == 0 {
                        // Backpressure reached the service: with most of the
                        // burst unread it is still blocked on its socket.
                        assert!(!script.is_finished());
                    }
                    assert_eq!(owner, request);
                    assert!(sequence > last_sequence);
                    last_sequence = sequence;
                    // Every block carries its message index: none is lost,
                    // duplicated or reordered.
                    let expected = (frames / FRAMES) as i16;
                    assert_eq!(source_offset, frames % FRAMES);
                    assert!(samples.iter().all(|sample| *sample == expected));
                    frames += samples.len();
                    packets += 1;
                }
                ProviderOutput::TurnComplete { idle: true, .. } => break,
                other => panic!("unexpected provider output: {other:?}"),
            }
        }
        assert_eq!((packets, frames), (MESSAGES * 50, MESSAGES * FRAMES));
        println!(
            "MEASURED burst_80s_audio_2000_packets_delivered_ms={} (scripted service to local IPC reader, host)",
            started.elapsed().as_millis()
        );
        let _service = script.await.unwrap();
        rig.parent.control.send(Control::Stop).unwrap();
        assert!(rig.exit().await.0.is_ok());
    }

    #[tokio::test]
    async fn late_provider_events_never_reach_the_coordinator_for_a_newer_request() {
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        let first = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(5, 240)).await;
        dial.service.send(idle_complete()).await;
        assert!(matches!(rig.output().await, ProviderOutput::Audio { .. }));
        assert!(matches!(
            rig.output().await,
            ProviderOutput::TurnComplete { request, idle: true } if request == first
        ));
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.complete_turn(real_time(), owner).unwrap();
        // The person starts a follow-up. The interruption that its explicit
        // activityStart requested, and a duplicate completion, arrive late.
        let second = rig.speak(&mut dial, false, false).await;
        dial.service.send(provider_audio(66, 240)).await;
        dial.service
            .send(json!({"serverContent":{"interrupted":true}}))
            .await;
        dial.service.send(idle_complete()).await;
        rig.pump(Duration::from_millis(120)).await;
        assert!(
            rig.parent
                .data
                .receive::<ProviderOutput>()
                .unwrap()
                .is_none()
        );
        // The follow-up ends and is answered under its own identity only.
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        rig.parent
            .data
            .send(ProviderInput::End {
                request: second,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        rig.pump(Duration::from_millis(40)).await;
        dial.service.send(provider_audio(7, 240)).await;
        let ProviderOutput::Audio {
            request, samples, ..
        } = rig.output().await
        else {
            panic!("expected the follow-up answer");
        };
        assert_eq!(request, second);
        assert!(samples.iter().all(|sample| *sample == 7));
    }

    // ---- Speaking speed through the output capacity contract ------------
    //
    // Playback, the pause and the scripted service run on Tokio's clock, the
    // IPC and authority bounds on the scenario clock: one 2 ms step a
    // coordinator tick. With the runtime paused (the 20 s answer) the two
    // advance together, and an answer of seconds takes a fraction of that
    // (#823). The harness
    // plays the coordinator exactly as `provider-flow-control.md` specifies:
    // every received packet is counted, and capacity is granted from the free
    // space of a reply buffer that drains at 24,000 samples per second.

    const LARGEST: usize = lamp_gemini::MAX_OUTPUT_SAMPLES;
    const REPLY_BUFFER: usize = crate::provider_flow::MAX_REPLY_SAMPLES;

    /// The coordinator's reply buffer and speaker, reduced to their arithmetic.
    /// Playback advances by Tokio's time only: time spent held or with
    /// nothing to play is never made up afterwards.
    struct Speaker {
        /// Samples the buffer may hold. The real coordinator has 30 s; a test
        /// may give it less so a short answer still exhausts capacity.
        room: usize,
        received: u64,
        played: u64,
        /// When playback was last advanced; set by the first audio.
        at: Option<tokio::time::Instant>,
        /// Microsecond-samples not yet amounting to a whole sample.
        fraction: u64,
        held: Duration,
        /// Samples of playing time lost with an empty buffer.
        starved: u64,
        /// The largest backlog ever held.
        peak: usize,
    }
    impl Speaker {
        fn new(room: usize) -> Self {
            Self {
                room,
                received: 0,
                played: 0,
                at: None,
                fraction: 0,
                held: Duration::ZERO,
                starved: 0,
                peak: 0,
            }
        }
        fn hear(&mut self, samples: usize) {
            self.at.get_or_insert_with(tokio::time::Instant::now);
            self.received += samples as u64;
        }
        /// Advance playback to now, unless it is on `hold`, and return the
        /// backlog.
        fn backlog(&mut self, hold: bool) -> usize {
            if let Some(last) = self.at {
                let now = tokio::time::Instant::now();
                self.at = Some(now);
                let elapsed = now - last;
                if hold {
                    self.held += elapsed;
                } else {
                    self.fraction += elapsed.as_micros() as u64 * 24_000;
                    let due = self.fraction / 1_000_000;
                    self.fraction %= 1_000_000;
                    let waiting = self.received - self.played;
                    self.played += due.min(waiting);
                    self.starved += due.saturating_sub(waiting);
                }
            }
            let backlog = (self.received - self.played) as usize;
            self.peak = self.peak.max(backlog);
            backlog
        }
        /// Cancel what is queued, as the coordinator does on a new admission.
        fn flush(&mut self) {
            self.played = self.received;
        }
    }
    impl LiveRig {
        /// One coordinator tick: heartbeat, then capacity from the speaker's
        /// free space. A smaller `room` is expressed as space already taken.
        fn grant(&mut self, speaker: &mut Speaker, hold: bool) {
            self.publish();
            let backlog = speaker.backlog(hold);
            assert!(backlog <= speaker.room, "reply buffer overran: {backlog}");
            self.output_flow
                .replenish(
                    REPLY_BUFFER - speaker.room + backlog,
                    &mut self.parent.control,
                )
                .unwrap();
        }
        /// Every packet that has arrived, each counted against capacity. An
        /// excess over what was granted fails here.
        fn arrived(&mut self) -> Vec<Wire> {
            let mut all = Vec::new();
            while let Some(wire) = self.parent.data.receive::<Wire>().unwrap() {
                self.output_flow.received().unwrap();
                all.push(wire);
            }
            all
        }
    }
    struct Playback {
        packets: u64,
        frames: usize,
        /// First tick to the idle completion arriving.
        completed: Duration,
        /// First tick to the last sample leaving the speaker.
        finished: Duration,
        held: Duration,
        starved: Duration,
        peak: usize,
    }
    impl LiveRig {
        /// Take one answer of `total` samples at speaking speed, to the end of
        /// its playback. Playback stops once, for `pause`, after `pause_after`
        /// samples have been played.
        async fn play_at_speaking_speed(
            &mut self,
            request: u64,
            room: usize,
            total: u64,
            pause_after: u64,
            pause: Duration,
        ) -> Playback {
            let mut speaker = Speaker::new(room);
            let started = tokio::time::Instant::now();
            let (mut packets, mut frames) = (0u64, 0usize);
            let mut completed = None;
            // When the last sample left the speaker, and the playing time
            // lost to an empty buffer up to then.
            let mut finished = None;
            loop {
                // One coordinator tick is one scenario step, when a scope is
                // active, however long the worker and the service held this
                // thread since the last one.
                crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
                let hold = speaker.played >= pause_after && speaker.held < pause;
                self.grant(&mut speaker, hold);
                for wire in self.arrived() {
                    assert!(completed.is_none(), "output after completion: {wire:?}");
                    match wire {
                        Wire::Output(ProviderOutput::Audio {
                            request: owner,
                            source_offset,
                            samples,
                            ..
                        }) => {
                            assert_eq!(owner, request);
                            // Every block carries its message index: nothing
                            // lost, repeated or reordered.
                            let expected = (frames / LARGEST) as i16;
                            assert_eq!(source_offset, frames % LARGEST);
                            assert!(samples.iter().all(|sample| *sample == expected));
                            frames += samples.len();
                            packets += 1;
                            speaker.hear(samples.len());
                        }
                        Wire::Output(ProviderOutput::GenerationComplete { .. }) => {}
                        Wire::Output(ProviderOutput::TurnComplete { idle: true, .. }) => {
                            completed = Some(started.elapsed());
                        }
                        other => panic!("unexpected at speaking speed: {other:?}"),
                    }
                }
                if finished.is_none() && speaker.played == total {
                    finished = Some((started.elapsed(), speaker.starved));
                }
                if let (Some(completed), Some((finished, starved))) = (completed, finished) {
                    return Playback {
                        packets,
                        frames,
                        completed,
                        finished,
                        held: speaker.held,
                        starved: Duration::from_micros(starved * 1_000_000 / 24_000),
                        peak: speaker.peak,
                    };
                }
                assert!(!self.worker.is_finished(), "worker ended mid-answer");
                assert!(
                    started.elapsed() < Duration::from_secs(total / 24_000 + 120),
                    "answer never finished"
                );
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    }
    /// How long the scripted network stalls inside a long answer, and how many
    /// largest messages then arrive with no gap between them.
    const STALL: Duration = Duration::from_secs(3);
    const BURST: u64 = 5;

    /// The service side of one long answer: largest messages at ten times
    /// speaking speed, a network stall a third of the way in followed by a
    /// ten-second burst, pings answered throughout, and idle completion
    /// withheld for the answer's own duration, as the service assumes
    /// playback. Returns how long the writing took, and the service: dropping
    /// it would be a connection loss.
    fn long_answer(
        mut service: lamp_gemini::testing::FakeService,
        seconds: u64,
    ) -> JoinHandle<(Duration, lamp_gemini::testing::FakeService)> {
        tokio::spawn(async move {
            let started = tokio::time::Instant::now();
            let messages = seconds / 2;
            let stall_at = messages / 3;
            for index in 0..messages {
                if index == stall_at {
                    service.rest(STALL).await;
                }
                service.send(provider_audio(index as i16, LARGEST)).await;
                if !(stall_at..stall_at + BURST - 1).contains(&index) {
                    service.rest(Duration::from_millis(200)).await;
                }
            }
            let written = started.elapsed();
            service
                .send(json!({"serverContent":{"generationComplete":true}}))
                .await;
            service
                .rest(Duration::from_secs(seconds).saturating_sub(started.elapsed()))
                .await;
            service.send(idle_complete()).await;
            (written, service)
        })
    }
    /// A `seconds` long answer through a reply buffer of `room` samples, with
    /// playback held for `pause` once five seconds have been played.
    ///
    /// The coordinator's input leases (100 ms) and the worker's heartbeat
    /// bound (250 ms) run on the stepped scenario clock, two milliseconds per
    /// coordinator tick. On this test's one thread the worker and the
    /// scripted service decode and encode the largest messages between two
    /// ticks; that held a tick back past 100 ms in debug builds, so the
    /// coordinator lost the turn, the worker retired the answer it was still
    /// receiving, and the session ended on BarrierTimeout or ReadTimeout. A
    /// coordinator is a process of its own on a lamp. Playback, the pause and
    /// the service's pacing are on Tokio's clock; on a paused runtime it moves
    /// 2 ms a tick with the scenario clock (#823).
    async fn speaking_speed(seconds: u64, room: usize, pause: Duration) {
        let scope =
            crate::scenario_clock::Scope::enter(Duration::from_secs(seconds + 150)).unwrap();
        scope.set_pump(|_| Ok(())).unwrap();
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        let script = long_answer(dial.service, seconds);
        let total = seconds * 24_000;
        let played = rig
            .play_at_speaking_speed(request, room, total, 5 * 24_000, pause)
            .await;
        let (written, _service) = script.await.unwrap();
        assert_eq!(played.frames as u64, total);
        assert_eq!(played.packets, seconds * 25);
        println!(
            "MEASURED speaking_speed seconds={seconds} reply_buffer_s={:.1} hold_ms={} playback_finished_ms={} completion_received_ms={} starved_ms={} service_wrote_in_ms={} packets={} peak_backlog_s={:.2} (scripted service through output capacity to a local IPC reader and a modeled speaker, on Tokio's clock)",
            room as f64 / 24_000.0,
            played.held.as_millis(),
            played.finished.as_millis(),
            played.completed.as_millis(),
            played.starved.as_millis(),
            written.as_millis(),
            played.packets,
            played.peak as f64 / 24_000.0
        );
        // Playback was held for the whole pause and never ran dry: the stall
        // and the pause were both absorbed by what was already buffered.
        assert!(played.held >= pause, "{:?}", played.held);
        assert!(
            played.starved < Duration::from_millis(100),
            "{:?}",
            played.starved
        );
        // So the answer took its own length plus the pause, and no longer.
        let expected = Duration::from_secs(seconds) + played.held + played.starved;
        assert!(
            played.finished >= expected && played.finished <= expected + Duration::from_secs(1),
            "{:?} against {expected:?}",
            played.finished
        );
        // Its completion needs capacity like any packet and still arrives.
        assert!(played.completed <= played.finished + Duration::from_secs(1));
        // Capacity really was exhausted: the buffer filled and stayed bounded.
        assert!(played.peak <= room);
        if total as usize > room {
            assert!(played.peak + 2 * PACKET_SAMPLES >= room, "{}", played.peak);
        }
        // The service was not slowed to playback speed.
        assert!(
            written <= STALL + Duration::from_secs(seconds / 4 + 3),
            "{written:?}"
        );
        rig.parent.control.send(Control::Stop).unwrap();
        assert!(rig.exit().await.0.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity() {
        // A small buffer so a short answer exhausts capacity for most of its
        // length. Playback also stops for four seconds with the buffer full.
        speaking_speed(20, 5 * 24_000, Duration::from_secs(4)).await;
    }

    #[tokio::test]
    #[ignore = "one minute of real time; run explicitly"]
    async fn sixty_second_answer_follows_output_capacity() {
        speaking_speed(60, REPLY_BUFFER, Duration::from_secs(10)).await;
    }

    #[tokio::test]
    #[ignore = "three minutes of real time; run explicitly"]
    async fn three_minute_answer_follows_output_capacity() {
        speaking_speed(180, REPLY_BUFFER, Duration::from_secs(20)).await;
    }

    /// On a paused clock (#1128): playback and the service run on Tokio's
    /// time, and so does every bound here, so an answer held behind the
    /// queued audio would still show as seconds of delay.
    #[tokio::test(start_paused = true)]
    async fn interrupting_a_capacity_paced_answer_is_not_delayed_by_its_queued_audio() {
        let answer: Vec<_> = (0..20)
            .map(|index| provider_audio(index, LARGEST))
            .collect();
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        let old = rig.speak(&mut dial, false, true).await;
        // Forty seconds generated at once into a two-second reply buffer: the
        // worker holds its full backlog and the transport holds the rest.
        let mut speaker = Speaker::new(2 * 24_000);
        // One coordinator tick while only the old answer may arrive.
        let hear = |rig: &mut LiveRig, speaker: &mut Speaker| {
            rig.grant(speaker, false);
            for wire in rig.arrived() {
                let Wire::Output(ProviderOutput::Audio {
                    request, samples, ..
                }) = wire
                else {
                    panic!("audio only");
                };
                assert_eq!(request, old);
                speaker.hear(samples.len());
            }
        };
        for message in answer {
            dial.service.send(message).await;
            hear(&mut rig, &mut speaker);
        }
        let started = tokio::time::Instant::now();
        while speaker.played < 2 * 24_000 {
            hear(&mut rig, &mut speaker);
            assert!(
                started.elapsed() < Duration::from_secs(8),
                "playback stalled"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        // The person interrupts. The coordinator drops what it had queued.
        let owner = rig
            .owner
            .admit(real_time(), AdmittedInput::Interruption)
            .unwrap();
        let new = owner.turn();
        speaker.flush();
        rig.grant(&mut speaker, false);
        let interrupted_at = tokio::time::Instant::now();
        let privacy_generation = rig.snapshot.microphone_generation();
        rig.parent
            .data
            .send(ProviderInput::Start {
                session: 1,
                request: new,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            })
            .unwrap();
        rig.parent
            .data
            .send(ProviderInput::Audio {
                request: new,
                privacy_generation,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![3; 160],
            })
            .unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        rig.parent
            .data
            .send(ProviderInput::End {
                request: new,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        // The scripted service confirms the interruption and answers. Packets
        // keep being counted and capacity granted while it does.
        let mut stale = 0u64;
        let mut first_new = None;
        let mut step = 0;
        while first_new.is_none() {
            rig.grant(&mut speaker, false);
            for wire in rig.arrived() {
                match wire {
                    Wire::Output(ProviderOutput::Audio {
                        request, samples, ..
                    }) if request == new => {
                        assert!(samples.iter().all(|sample| *sample == 99));
                        first_new.get_or_insert(interrupted_at.elapsed());
                    }
                    // Already sent before the interruption: at most the two
                    // packets of outstanding capacity. Discarded on arrival.
                    Wire::Output(ProviderOutput::Audio { .. }) => stale += 1,
                    Wire::Output(_) => {}
                    Wire::Status(status) => panic!("unexpected status: {status:?}"),
                }
            }
            if let Some(message) = dial.service.try_receive(Duration::from_millis(2)).await {
                let input = &message["realtimeInput"];
                let expected = ["activityStart", "audio", "activityEnd"][step];
                assert!(input.get(expected).is_some(), "{message}");
                step += 1;
                if step == 2 {
                    dial.service
                        .send(json!({"serverContent":{"interrupted":true}}))
                        .await;
                    dial.service.send(idle_complete()).await;
                } else if step == 3 {
                    dial.service.send(provider_audio(99, 960)).await;
                }
            }
            assert!(
                interrupted_at.elapsed() < Duration::from_secs(5),
                "new answer delayed"
            );
        }
        let first_new = first_new.unwrap();
        println!(
            "MEASURED interruption_under_capacity new_answer_first_packet_ms={} stale_packets_after_interruption={stale} (admission to first packet of the new answer at a local IPC reader, scripted service, on Tokio's clock)",
            first_new.as_millis()
        );
        // The cancelled answer still had about 36 s queued, 15 s of it in this
        // worker. At playback pace that would have come first.
        assert!(first_new < Duration::from_secs(1), "{first_new:?}");
        assert!(stale <= 2, "{stale} stale packets");
    }

    // ---- Proposed typed outage events ------------------------------------

    #[test]
    fn proposed_status_events_have_a_fixed_wire_shape_beside_provider_output() {
        for (status, expected) in [
            (
                ProviderStatus::Unavailable {
                    reason: Fault::PeerClosed { code: Some(1008) },
                },
                json!({"kind":"unavailable","reason":{"category":"peer_closed","code":1008}}),
            ),
            (
                ProviderStatus::Recovered {
                    outage_us: 1_500_000,
                    attempts: 2,
                    context: RecoveredContext::ResumedBeforeLatest,
                    reason: Fault::ServerGoAway,
                },
                json!({"kind":"recovered","outage_us":1_500_000,"attempts":2,
                    "context":"resumed_before_latest","reason":{"category":"server_go_away"}}),
            ),
            (
                ProviderStatus::TurnFailed {
                    request: 7,
                    stage: FailedStage::AwaitingResponse,
                    reason: Fault::StalledResponse,
                },
                json!({"kind":"turn_failed","request":7,"stage":"awaiting_response",
                    "reason":{"category":"stalled_response"}}),
            ),
            (
                ProviderStatus::OwnershipAssumed {
                    request: Some(8),
                    superseded: None,
                },
                json!({"kind":"ownership_assumed","request":8,"superseded":null}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(Outbound::Status(status.clone())).unwrap(),
                expected
            );
            assert_eq!(
                serde_json::from_value::<ProviderStatus>(expected).unwrap(),
                status
            );
        }
        // Existing outputs keep their exact shape inside the same queue.
        assert_eq!(
            serde_json::to_value(Outbound::Output(ProviderOutput::TurnComplete {
                request: 3,
                idle: true
            }))
            .unwrap(),
            json!({"kind":"turn_complete","request":3,"idle":true})
        );
        // Every transport error has a category; none carries text.
        assert_eq!(Fault::from(Error::QuotaExceeded), Fault::QuotaExceeded);
        assert_eq!(Fault::from(Error::MalformedMessage), Fault::Protocol);
        assert_eq!(Fault::from(Error::Backpressure), Fault::Local);
        assert_eq!(Fault::from(Error::OpenActivityOverrun), Fault::Local);
        // A write past its bound says which write, its size and its bound
        // (lampOS#705), and a record from before still reads.
        let stalled = Fault::from(Error::WriteTimeout {
            write: lamp_gemini::Write::Picture,
            kib: 245,
            deadline_ms: 200,
        });
        assert_eq!(
            serde_json::to_value(ProviderStatus::Unavailable { reason: stalled }).unwrap(),
            json!({"kind":"unavailable","reason":{"category":"write_timeout",
                "write":"picture","kib":245,"deadline_ms":200}})
        );
        assert_eq!(
            serde_json::from_value::<ProviderStatus>(
                json!({"kind":"unavailable","reason":{"category":"write_timeout"}})
            )
            .unwrap(),
            ProviderStatus::Unavailable {
                reason: Fault::WriteTimeout {
                    write: None,
                    kib: None,
                    deadline_ms: None
                }
            }
        );
        assert!(
            serde_json::from_value::<ProviderStatus>(json!({"kind":"unavailable",
            "reason":{"category":"transport"},"detail":"x"}))
            .is_err()
        );
    }

    #[tokio::test]
    async fn typed_reporting_fails_one_turn_and_keeps_serving_after_a_lost_answer() {
        let mut rig = LiveRig::start_with(quick_policy(), Options::reporting(Reporting::Typed));
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        let lost = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(5, 1_920)).await;
        dial.service.close(1011, "PRIVATE BACKEND DETAIL").await;
        // In order on the wire: what arrived, then the explicit failure. The
        // link status is not held behind the audio.
        let mut seen = Vec::new();
        let mut frames = 0;
        let mut redial = None;
        while !seen.contains(&"recovered") || !seen.contains(&"turn_failed") {
            if redial.is_none()
                && let Ok(mut attempt) = rig.dials.try_recv()
            {
                let setup = rig.service_message(&mut attempt).await;
                assert_eq!(setup["setup"]["sessionResumption"]["handle"], "HANDLE-ONE");
                attempt.service.send(json!({"setupComplete": {}})).await;
                redial = Some(attempt);
            }
            rig.publish();
            // Statuses are packets too: each is counted and capacity returned.
            match rig.take() {
                Some(Wire::Output(ProviderOutput::Audio {
                    request, samples, ..
                })) => {
                    assert_eq!(request, lost);
                    assert!(!seen.contains(&"turn_failed"), "audio after its failure");
                    frames += samples.len();
                }
                Some(Wire::Status(ProviderStatus::Unavailable { reason })) => {
                    assert_eq!(reason, Fault::PeerClosed { code: Some(1011) });
                    seen.push("unavailable");
                }
                Some(Wire::Status(ProviderStatus::TurnFailed {
                    request,
                    stage,
                    reason,
                })) => {
                    assert_eq!((request, stage), (lost, FailedStage::Responding));
                    assert_eq!(reason, Fault::PeerClosed { code: Some(1011) });
                    seen.push("turn_failed");
                }
                Some(Wire::Status(ProviderStatus::Recovered {
                    context,
                    attempts,
                    reason,
                    ..
                })) => {
                    assert_eq!((context, attempts), (RecoveredContext::Resumed, 1));
                    assert_eq!(reason, Fault::PeerClosed { code: Some(1011) });
                    seen.push("recovered");
                }
                Some(other) => panic!("unexpected: {other:?}"),
                None => tokio::time::sleep(Duration::from_millis(2)).await,
            }
            assert!(
                !rig.worker.is_finished(),
                "typed reporting keeps the worker alive"
            );
        }
        assert_eq!(frames, 1_920);
        assert_eq!(
            seen.iter().filter(|kind| **kind == "turn_failed").count(),
            1
        );
        assert!(
            seen.iter().position(|kind| *kind == "unavailable")
                < seen.iter().position(|kind| *kind == "recovered")
        );
        // The coordinator fails that turn and the next question is served.
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.cancel_turn(real_time(), owner).unwrap();
        let mut redial = redial.unwrap();
        let next = rig.speak(&mut redial, false, true).await;
        redial.service.send(provider_audio(6, 240)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request, .. } if request == next
        ));
    }

    /// lamp-4ace, 2026-10-13 16:50:55: the network stalled, what was being
    /// said overran the link's input queue, and the worker ended with
    /// "provider unavailable while input was admitted: Gemini Live
    /// Backpressure", taking the voice service with it. Here the scripted
    /// service stops reading, so the worker's writes really block.
    #[tokio::test]
    async fn a_stalled_link_fails_the_request_in_hand_and_the_worker_serves_the_next() {
        // As the lamp runs: a new session each time, with nothing resumed.
        let mut rig = LiveRig::start_capacity(
            RecoveryPolicy {
                outage_budget: Duration::from_secs(5),
                require_context: false,
                ..quick_policy()
            },
            Options::reporting(Reporting::Typed),
            BARRIER,
            false,
            Continuation::Off,
            2_048,
        );
        let mut dial = rig.ready().await;
        // Someone starts speaking; the service has the start and the first
        // block, and then reads nothing more.
        let request = rig.speak(&mut dial, false, false).await;
        let privacy_generation = rig.snapshot.microphone_generation();
        let mut failed = None;
        let mut sequence = 1;
        let deadline = Instant::now() + Duration::from_secs(4);
        while failed.is_none() {
            assert!(Instant::now() < deadline, "the stall was never noticed");
            assert!(
                !rig.worker.is_finished(),
                "the worker ended on a stalled link"
            );
            for _ in 0..4 {
                sequence += 1;
                // Once the request has failed the worker discards the rest.
                let _ = rig.parent.data.send(ProviderInput::Audio {
                    request,
                    privacy_generation,
                    sequence,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![3; 160],
                });
            }
            rig.pump(Duration::from_millis(4)).await;
            if rig.worker.is_finished() {
                let ended = (&mut rig.worker).await.unwrap();
                panic!("the worker ended on a stalled link: {ended:?}");
            }
            while let Some(wire) = rig.take() {
                if let Wire::Status(ProviderStatus::TurnFailed {
                    request: lost,
                    stage,
                    ..
                }) = wire
                {
                    assert_eq!(lost, request);
                    failed = Some(stage);
                }
            }
        }
        // Lost with its input open: nothing of it is called delivered.
        assert_eq!(failed, Some(FailedStage::InputOpen));
        // The worker is alive, a new connection is made, and the next thing
        // said is served on it.
        let mut redial = rig.dial().await;
        let setup = rig.service_message(&mut redial).await;
        assert!(setup.get("setup").is_some());
        redial.service.send(json!({"setupComplete": {}})).await;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(Instant::now() < deadline, "no recovery reported");
            if let Wire::Status(ProviderStatus::Recovered { .. }) = rig.wire().await {
                break;
            }
        }
        rig.owner
            .cancel_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        let next = rig.speak(&mut redial, false, true).await;
        assert!(next > request);
        assert!(!rig.worker.is_finished());
    }

    /// lampOS#892, the reviewer's proof: input faster than speech (a held
    /// line replayed, two blocks every 2 ms) meets a write that stalls for
    /// 90 ms, within the link's 200 ms write bound. Gemini's input queue
    /// fills; the worker waits for room instead of offering what would end
    /// the connection, so every block arrives in order and nothing is lost.
    #[tokio::test]
    async fn a_short_stall_under_fast_input_waits_for_room_and_loses_nothing() {
        const BLOCKS: u64 = 190;
        let mut rig = LiveRig::start_capacity(
            RecoveryPolicy {
                outage_budget: Duration::from_secs(5),
                require_context: false,
                ..quick_policy()
            },
            Options {
                wait_for_room: true,
                ..Options::reporting(Reporting::Typed)
            },
            BARRIER,
            false,
            Continuation::Off,
            2_048,
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, false).await;
        let privacy_generation = rig.snapshot.microphone_generation();
        // The service reads nothing for 90 ms, then everything it is sent.
        let mut service = dial.service;
        let reader = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(90)).await;
            let mut sequence = Vec::new();
            while (sequence.len() as u64) < BLOCKS - 1 {
                let Some(message) = service.try_receive(Duration::from_secs(1)).await else {
                    break;
                };
                if message["realtimeInput"].get("audio").is_some() {
                    sequence.push(message);
                }
            }
            (sequence.len() as u64, service)
        });
        let mut pending: VecDeque<ProviderInput> = VecDeque::new();
        let mut next = 2;
        let deadline = Instant::now() + Duration::from_secs(3);
        while next <= BLOCKS || !pending.is_empty() {
            assert!(Instant::now() < deadline, "the input was never all taken");
            rig.publish();
            for _ in 0..2 {
                if next <= BLOCKS {
                    pending.push_back(ProviderInput::Audio {
                        request,
                        privacy_generation,
                        sequence: next,
                        read_completed_at_us: monotonic_us(),
                        samples: vec![3; 160],
                    });
                    next += 1;
                }
            }
            while let Some(command) = pending.front() {
                match rig.parent.data.send(command) {
                    Ok(()) => {
                        pending.pop_front();
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("provider input: {error}"),
                }
            }
            while let Some(wire) = rig.take() {
                assert!(
                    !matches!(wire, Wire::Status(ProviderStatus::TurnFailed { .. })),
                    "the request was lost to a short stall: {wire:?}"
                );
            }
            assert!(
                !rig.worker.is_finished(),
                "the worker ended on a short stall"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        // Held open, so the connection outlives the check.
        let (received, _service) = reader.await.unwrap();
        assert_eq!(received, BLOCKS - 1);
        rig.pump(Duration::from_millis(200)).await;
        while let Some(wire) = rig.take() {
            assert!(
                !matches!(
                    wire,
                    Wire::Status(
                        ProviderStatus::TurnFailed { .. } | ProviderStatus::Unavailable { .. }
                    )
                ),
                "the link was given up for a short stall: {wire:?}"
            );
        }
        assert!(!rig.worker.is_finished());
    }

    /// The reviewer's second proof on lampOS#892: the request is cancelled
    /// while the queue is full in the same stall. Its End (which writes
    /// nothing: a cancelled activity is left for the next to continue) waits
    /// for room as its audio did, goes before anything of the next request,
    /// and the same connection serves the next request.
    #[tokio::test]
    async fn a_cancellation_in_a_short_stall_waits_for_room_and_keeps_the_link() {
        let mut rig = LiveRig::start_capacity(
            RecoveryPolicy {
                outage_budget: Duration::from_secs(5),
                require_context: false,
                ..quick_policy()
            },
            Options {
                wait_for_room: true,
                ..Options::reporting(Reporting::Typed)
            },
            BARRIER,
            false,
            Continuation::Off,
            2_048,
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, false).await;
        let privacy_generation = rig.snapshot.microphone_generation();
        // The service reads nothing for 90 ms, then what it is sent.
        let mut service = dial.service;
        let reader = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(90)).await;
            let mut blocks = 0;
            while let Some(message) = service.try_receive(Duration::from_millis(300)).await {
                if message["realtimeInput"].get("audio").is_some() {
                    blocks += 1;
                }
            }
            (blocks, service)
        });
        // Two blocks every 2 ms, by the clock, for 60 ms fill the queue;
        // then it is cancelled.
        let started = Instant::now();
        let mut sequence = 1;
        while started.elapsed() < Duration::from_millis(60) {
            rig.publish();
            while sequence < 1 + started.elapsed().as_millis() as u64 {
                sequence += 1;
                // A full socket keeps the rest at the source, as the coordinator does.
                let _ = rig.parent.data.send(ProviderInput::Audio {
                    request,
                    privacy_generation,
                    sequence,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![3; 160],
                });
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        rig.owner
            .cancel_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            rig.publish();
            while let Some(wire) = rig.take() {
                assert!(
                    !matches!(wire, Wire::Status(ProviderStatus::Unavailable { .. })),
                    "the link was given up for a cancellation in a short stall: {wire:?}"
                );
            }
            assert!(
                !rig.worker.is_finished(),
                "the worker ended on a cancellation in a short stall"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let (blocks, service) = reader.await.unwrap();
        assert!(blocks > 0);
        // The same connection serves the next request, which continues the
        // activity the cancelled one left open, through to its end.
        dial.service = service;
        let owner = rig
            .owner
            .admit(real_time(), AdmittedInput::NewTurn)
            .unwrap();
        let next = owner.turn();
        assert!(next > request);
        rig.publish();
        let privacy_generation = rig.snapshot.microphone_generation();
        for command in [
            ProviderInput::Start {
                session: dial.session.get(),
                request: next,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            },
            ProviderInput::Audio {
                request: next,
                privacy_generation,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![3; 160],
            },
        ] {
            rig.parent.data.send(&command).unwrap();
        }
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Started { request: started, .. } if started == next
        ));
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        rig.parent
            .data
            .send(&ProviderInput::End {
                request: next,
                privacy_generation,
                sound: false,
                cut_in: false,
            })
            .unwrap();
        loop {
            let message = rig.service_message(&mut dial).await;
            if message["realtimeInput"].get("activityEnd").is_some() {
                break;
            }
        }
        assert!(rig.dials.try_recv().is_err(), "a new connection was made");
        assert!(!rig.worker.is_finished());
    }

    /// Off unless set: without `--wait-for-room` (no second route) the same
    /// fast input through the same short stall overruns the queue and ends
    /// the connection, reported as the request lost, exactly as before.
    #[tokio::test]
    async fn without_the_flag_a_full_queue_still_ends_the_connection_as_before() {
        let mut rig = LiveRig::start_capacity(
            RecoveryPolicy {
                outage_budget: Duration::from_secs(5),
                require_context: false,
                ..quick_policy()
            },
            Options::reporting(Reporting::Typed),
            BARRIER,
            false,
            Continuation::Off,
            2_048,
        );
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, false).await;
        let privacy_generation = rig.snapshot.microphone_generation();
        // Read nothing for the whole test: the stall outlasts the input.
        let _service = dial.service;
        let started = Instant::now();
        let mut sequence = 1;
        let mut lost = None;
        while lost.is_none() && started.elapsed() < Duration::from_millis(90) {
            rig.publish();
            while sequence < 1 + started.elapsed().as_millis() as u64 {
                sequence += 1;
                let _ = rig.parent.data.send(ProviderInput::Audio {
                    request,
                    privacy_generation,
                    sequence,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![3; 160],
                });
            }
            while let Some(wire) = rig.take() {
                if let Wire::Status(ProviderStatus::TurnFailed { reason, .. }) = wire {
                    lost = Some(reason);
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(lost, Some(Fault::Local));
    }

    /// A command waits for room past the link's write bound: 200 ms today,
    /// 1 s with patient writes (the reviewer's P3 on #1397).
    #[test]
    fn the_room_wait_is_past_the_write_bound_with_patient_writes_or_without() {
        let today = room_wait_us(false);
        assert!(today > 200_000 && today < 1_000_000, "{today}");
        let patient = u64::try_from(crate::config::PATIENT_WRITE.as_micros()).unwrap();
        assert!(room_wait_us(true) > patient, "{}", room_wait_us(true));
    }

    #[test]
    fn wait_for_room_is_taken_after_keep_link_and_only_when_given() {
        let flags = [
            KEEP_LINK_FLAG.to_owned(),
            WAIT_FOR_ROOM_FLAG.to_owned(),
            "rest".to_owned(),
        ];
        let (kept, rest) = take_keep_link(&flags);
        assert!(kept);
        let (wait, rest) = take_wait_for_room(rest);
        assert!(wait);
        assert_eq!(rest, ["rest"]);
        assert!(!take_wait_for_room(rest).0);
    }

    #[tokio::test]
    async fn typed_reporting_fails_a_question_admitted_during_an_outage_once_and_never_sends_it() {
        let mut rig = LiveRig::start_with(
            RecoveryPolicy {
                outage_budget: Duration::from_secs(5),
                ..quick_policy()
            },
            Options::reporting(Reporting::Typed),
        );
        let mut dial = rig.ready().await;
        dial.service.send(resumable()).await;
        dial.service.close(1008, "idle").await;
        assert!(matches!(
            rig.wire().await,
            Wire::Status(ProviderStatus::Unavailable { .. })
        ));
        // The reconnect is unanswered when the person speaks.
        let mut redial = rig.dial().await;
        let owner = rig
            .owner
            .admit(real_time(), AdmittedInput::NewTurn)
            .unwrap();
        let request = owner.turn();
        rig.publish();
        let privacy_generation = rig.snapshot.microphone_generation();
        rig.parent
            .data
            .send(ProviderInput::Start {
                session: 1,
                request,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            })
            .unwrap();
        assert!(matches!(
            rig.wire().await,
            Wire::Status(ProviderStatus::TurnFailed { request: failed, stage: FailedStage::NotDelivered, .. })
                if failed == request
        ));
        // Its remaining input keeps arriving until the coordinator reacts.
        for sequence in 1..=5 {
            rig.parent
                .data
                .send(ProviderInput::Audio {
                    request,
                    privacy_generation,
                    sequence,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![3; 160],
                })
                .unwrap();
        }
        rig.parent
            .data
            .send(ProviderInput::End {
                request,
                privacy_generation,
                sound: false,
                cut_in: false,
            })
            .unwrap();
        // The connection comes back while that question is still the owner.
        let setup = rig.service_message(&mut redial).await;
        assert!(setup["setup"].get("sessionResumption").is_some());
        redial.service.send(json!({"setupComplete": {}})).await;
        assert!(matches!(
            rig.wire().await,
            Wire::Status(ProviderStatus::Recovered { .. })
        ));
        rig.pump(Duration::from_millis(100)).await;
        // Reported once; nothing of it reached the new session, late or at all.
        assert!(rig.parent.data.receive::<Wire>().unwrap().is_none());
        assert!(
            redial
                .service
                .try_receive(Duration::from_millis(20))
                .await
                .is_none()
        );
        rig.owner.cancel_turn(real_time(), owner).unwrap();
        let next = rig.speak(&mut redial, false, true).await;
        assert!(next > request);
    }

    #[tokio::test(start_paused = true)]
    async fn production_policy_never_commits_a_successor_without_a_terminal_barrier() {
        let _clock = logical_clock();
        let mut rig = LiveRig::start(quick_policy());
        let mut dial = rig.ready().await;
        let cut = rig.speak(&mut dial, false, true).await;
        let resumed = rig.speak(&mut dial, true, false).await;
        assert!(resumed > cut);
        let owner = rig.snapshot.owner().unwrap();
        rig.owner.input_ended(real_time(), owner).unwrap();
        rig.publish();
        rig.parent
            .data
            .send(ProviderInput::End {
                request: resumed,
                privacy_generation: rig.snapshot.microphone_generation(),
                sound: false,
                cut_in: false,
            })
            .unwrap();
        // The opening audio was already delivered. Only End must wait. A
        // silent service cannot prove that a later answer belongs to this turn.
        let (result, _) = rig.exit().await;
        let error = result.unwrap_err().to_string();
        assert!(error.contains("BarrierTimeout"), "{error}");
        assert!(
            dial.service
                .try_receive(Duration::from_millis(20))
                .await
                .is_none(),
            "successor was committed without a provider terminal event"
        );
    }

    /// On a paused clock (#823): the barrier's quiet passes only once every
    /// task is idle, and the wait for it is read on Tokio's time.
    #[tokio::test(start_paused = true)]
    async fn resumed_question_survives_a_silent_service_and_the_assumption_is_reported() {
        // The person continues after the endpointer cut the question. The
        // service says nothing about the cut-off request for the whole bound.
        for reporting in [Reporting::Legacy, Reporting::Typed] {
            let mut rig = LiveRig::start_with_barrier(
                quick_policy(),
                Options::reporting(reporting),
                BarrierPolicy::AssumeAfterQuiet,
            );
            let mut dial = rig.ready().await;
            let cut = rig.speak(&mut dial, false, true).await;
            let resumed_at = tokio::time::Instant::now();
            let resumed = rig.speak(&mut dial, true, false).await;
            // Its opening is already with the service, in order and whole.
            let owner = rig.snapshot.owner().unwrap();
            rig.owner.input_ended(real_time(), owner).unwrap();
            rig.publish();
            rig.parent
                .data
                .send(ProviderInput::End {
                    request: resumed,
                    privacy_generation: rig.snapshot.microphone_generation(),
                    sound: false,
                    cut_in: false,
                })
                .unwrap();
            if reporting == Reporting::Typed {
                assert!(matches!(
                    rig.wire().await,
                    Wire::Status(ProviderStatus::OwnershipAssumed { request: Some(new), superseded: Some(old) })
                        if new == resumed && old == cut
                ));
            }
            let message = rig.service_message(&mut dial).await;
            assert!(message["realtimeInput"].get("activityEnd").is_some());
            let waited = resumed_at.elapsed();
            assert!(waited >= Duration::from_millis(1_900), "{waited:?}");
            dial.service.send(provider_audio(8, 240)).await;
            assert!(matches!(
                rig.output().await,
                ProviderOutput::Audio { request, .. } if request == resumed
            ));
            assert!(!rig.worker.is_finished());
        }
    }

    /// `Continuation::KeptStart` through the worker's own loop: the real
    /// intake, hand-off and session, against the scripted service, with
    /// input at the rate it is spoken and its own capture times.
    mod kept_start {
        use super::*;
        use lamp_gemini::testing::sent_audio;

        impl LiveRig {
            fn start_kept() -> Self {
                Self::start_kept_reporting(Reporting::Legacy)
            }
            /// `Coordinated` is what the command line starts the worker with.
            fn start_kept_reporting(reporting: Reporting) -> Self {
                Self::start_all(
                    quick_policy(),
                    Options::reporting(reporting),
                    BARRIER,
                    false,
                    Continuation::KeptStart,
                )
            }
            async fn kept_ready(&mut self, reporting: Reporting) -> Dial {
                if reporting == Reporting::Coordinated {
                    self.coordinated_ready().await
                } else {
                    self.ready().await
                }
            }
            /// A person speaks again: a newly admitted request and its
            /// Start. With the start kept the service sees nothing of it.
            async fn begin(&mut self, dial: &Dial, input: AdmittedInput) -> u64 {
                self.begin_continuing(dial, input, false).await
            }
            /// The same, its Start saying whether it continues the request
            /// the lamp ended early (`LAMP_CUT_KEEPS_NAME`).
            async fn begin_continuing(
                &mut self,
                dial: &Dial,
                input: AdmittedInput,
                continues: bool,
            ) -> u64 {
                let owner = self.owner.admit(real_time(), input).unwrap();
                let request = owner.turn();
                self.publish();
                self.parent
                    .data
                    .send(ProviderInput::Start {
                        session: dial.session.get(),
                        request,
                        privacy_generation: self.snapshot.microphone_generation(),
                        continues,
                        continues_cut_off: false,
                        over_the_lamp: false,
                        after_stop: false,
                        follows_on: false,
                        over_the_lamps_voice: false,
                    })
                    .unwrap();
                loop {
                    match self.output().await {
                        ProviderOutput::Started {
                            request: started, ..
                        } if started == request => return request,
                        // The earlier request's own lifecycle may come first.
                        ProviderOutput::Interrupted { .. }
                        | ProviderOutput::TurnComplete { .. }
                        | ProviderOutput::InputDiscarded { .. } => {}
                        other => panic!("unexpected before the start: {other:?}"),
                    }
                }
            }
            /// Blocks of 10 ms, one every 10 ms, each read now. Every
            /// sample of a block is `base` plus its sequence.
            async fn say(&mut self, request: u64, blocks: std::ops::Range<u64>, base: i16) {
                for sequence in blocks {
                    self.publish();
                    self.parent
                        .data
                        .send(ProviderInput::Audio {
                            request,
                            privacy_generation: self.snapshot.microphone_generation(),
                            sequence,
                            read_completed_at_us: monotonic_us(),
                            samples: vec![base + sequence as i16; 160],
                        })
                        .unwrap();
                    self.pump(Duration::from_millis(10)).await;
                }
            }
            /// The current request's input ends.
            fn finish(&mut self, request: u64) {
                let owner = self.snapshot.owner().unwrap();
                assert_eq!(owner.turn(), request);
                self.owner.input_ended(real_time(), owner).unwrap();
                self.publish();
                self.parent
                    .data
                    .send(ProviderInput::End {
                        request,
                        privacy_generation: self.snapshot.microphone_generation(),
                        sound: false,
                        cut_in: false,
                    })
                    .unwrap();
            }
            /// What the scripted service receives until it has received
            /// nothing for `quiet` of Tokio's time: "start", "end", or the
            /// first sample of an audio block.
            async fn heard(&mut self, dial: &mut Dial, quiet: Duration) -> Vec<String> {
                let mut all = Vec::new();
                let mut last = tokio::time::Instant::now();
                while last.elapsed() < quiet {
                    self.publish();
                    let Some(message) = dial.service.try_receive(Duration::from_millis(5)).await
                    else {
                        continue;
                    };
                    last = tokio::time::Instant::now();
                    let input = &message["realtimeInput"];
                    all.push(if input.get("activityStart").is_some() {
                        "start".to_owned()
                    } else if input.get("activityEnd").is_some() {
                        "end".to_owned()
                    } else {
                        let samples = sent_audio(&message)
                            .unwrap_or_else(|| panic!("unexpected client message {message}"));
                        assert_eq!(samples.len(), 160);
                        assert!(samples.iter().all(|sample| *sample == samples[0]));
                        samples[0].to_string()
                    });
                }
                all
            }
            /// The next output that is not the earlier request's lifecycle.
            async fn answer(&mut self) -> ProviderOutput {
                loop {
                    match self.output().await {
                        ProviderOutput::Interrupted { .. }
                        | ProviderOutput::TurnComplete { .. } => {}
                        other => return other,
                    }
                }
            }
        }
        const QUIET: Duration = Duration::from_millis(60);
        fn blocks(range: std::ops::Range<u64>, base: i16) -> Vec<String> {
            range
                .map(|sequence| (base + sequence as i16).to_string())
                .collect()
        }
        fn interrupted() -> serde_json::Value {
            json!({"serverContent":{"interrupted":true}})
        }

        #[tokio::test]
        async fn a_resumed_question_is_kept_until_the_first_answer_begins_and_exactly_it_is_delivered()
         {
            resumed_question_kept_and_delivered(Reporting::Legacy).await;
        }

        /// The same through the contract the command line selects.
        #[tokio::test]
        async fn a_resumed_question_is_kept_and_delivered_through_the_coordinated_contract() {
            resumed_question_kept_and_delivered(Reporting::Coordinated).await;
        }

        async fn resumed_question_kept_and_delivered(reporting: Reporting) {
            let mut rig = LiveRig::start_kept_reporting(reporting);
            let mut dial = rig.kept_ready(reporting).await;
            let cut = rig.speak(&mut dial, false, true).await;
            let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
            assert!(resumed > cut);
            // A third of a second of speech. None of it reaches the service.
            rig.say(resumed, 1..31, 0).await;
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            // The first answer begins, and goes on a little.
            dial.service.send(provider_audio(7, 240)).await;
            dial.service.send(provider_audio(7, 240)).await;
            let mut expected = vec!["start".to_owned()];
            expected.extend(blocks(1..31, 0));
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            // Interrupted while producing, the service confirms.
            dial.service.send(interrupted()).await;
            dial.service.send(idle_complete()).await;
            // The person is still speaking: straight through now, then End.
            rig.say(resumed, 31..41, 0).await;
            rig.finish(resumed);
            let mut expected = blocks(31..41, 0);
            expected.push("end".to_owned());
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            // Nothing of the first answer was ever the coordinator's, and
            // the next audio is the resumed request's.
            dial.service.send(provider_audio(8, 240)).await;
            assert!(matches!(
                rig.answer().await,
                ProviderOutput::Audio { request, samples, .. } if request == resumed && samples == [8; 240]
            ));
            assert!(!rig.worker.is_finished());
            rig.stop().await;
        }

        /// #782 through the worker's own loop, in a busy room, as lamp-4ace
        /// runs it (kept start). "Hey Lamp, ... about lamps," is ended early
        /// at the comma, and "just in its reply ..." goes on as a new
        /// request. Only the first part's text shows the call. Without the
        /// switch the rest is held as not said to the lamp; with it, the
        /// rest keeps the first part's text and its answer is played.
        async fn the_rest_after_a_cut(continues: bool) -> bool {
            fn busy() -> (crate::room_told::Room, bool) {
                (crate::room_told::Room::Busy, false)
            }
            let heard = |text: &str| json!({"serverContent":{"inputTranscription":{"text":text}}});
            let mut rig = LiveRig::start_all(
                quick_policy(),
                Options {
                    reporting: Reporting::Typed,
                    tools: crate::tools::Settings::default(),
                    attention: Some(busy),
                    name_signal: false,
                    sounds_checked: false,
                    ask_unclear: false,
                    name_in_passing_ignored: false,
                    policy_holds: None,
                    wait_for_room: false,
                    staff_note: false,
                    spoken_to: false,
                    voices: None,
                },
                BARRIER,
                false,
                Continuation::KeptStart,
            );
            let mut dial = rig.kept_ready(Reporting::Typed).await;
            let cut = rig.speak(&mut dial, false, true).await;
            dial.service
                .send(heard("Hey Lamp, can you ask Claude Code about lamps,"))
                .await;
            assert!(matches!(
                rig.output().await,
                ProviderOutput::Transcript { .. }
            ));
            let resumed = rig
                .begin_continuing(&dial, AdmittedInput::Interruption, continues)
                .await;
            assert!(resumed > cut);
            rig.say(resumed, 1..31, 0).await;
            // The first answer begins: the kept question goes to the
            // service, the first answer is interrupted, the rest is said.
            dial.service.send(provider_audio(7, 240)).await;
            rig.heard(&mut dial, QUIET).await;
            dial.service.send(interrupted()).await;
            dial.service.send(idle_complete()).await;
            rig.say(resumed, 31..41, 0).await;
            rig.finish(resumed);
            rig.heard(&mut dial, QUIET).await;
            dial.service
                .send(heard("just in its reply, without making any files?"))
                .await;
            dial.service.send(provider_audio(8, 240)).await;
            dial.service.send(idle_complete()).await;
            let mut played = false;
            loop {
                match rig.output().await {
                    ProviderOutput::Audio { request, .. } if request == resumed => played = true,
                    ProviderOutput::TurnComplete { request, .. } if request == resumed => break,
                    _ => {}
                }
            }
            rig.stop().await;
            played
        }

        /// #829 through the worker's own loop, in a busy room: "I left the
        /// lamp on in the kitchen", said by one person to another, shows a
        /// call to the rule without the switch and its answer is played;
        /// with `--name-in-passing-ignored` it is held, as other talk is.
        async fn the_lamp_spoken_of(text: &str, ignored: bool) -> bool {
            fn busy() -> (crate::room_told::Room, bool) {
                (crate::room_told::Room::Busy, false)
            }
            let heard = |text: &str| json!({"serverContent":{"inputTranscription":{"text":text}}});
            let mut rig = LiveRig::start_all(
                quick_policy(),
                Options {
                    reporting: Reporting::Typed,
                    tools: crate::tools::Settings::default(),
                    attention: Some(busy),
                    name_signal: false,
                    sounds_checked: false,
                    ask_unclear: false,
                    name_in_passing_ignored: ignored,
                    policy_holds: None,
                    wait_for_room: false,
                    staff_note: false,
                    spoken_to: false,
                    voices: None,
                },
                BARRIER,
                false,
                Continuation::Off,
            );
            let mut dial = rig.ready().await;
            let request = rig.speak(&mut dial, false, true).await;
            dial.service.send(heard(text)).await;
            dial.service.send(provider_audio(7, 240)).await;
            dial.service.send(idle_complete()).await;
            let mut played = false;
            loop {
                match rig.output().await {
                    ProviderOutput::Audio { request: of, .. } if of == request => played = true,
                    ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
                    _ => {}
                }
            }
            rig.stop().await;
            played
        }

        const SPOKEN_OF: [&str; 4] = [
            "I left the lamp on in the kitchen.",
            "I bought a new lamp yesterday.",
            "The old lamp stopped working.",
            "Did you see her desk lamp?",
        ];
        /// Calls with the name at the end, set off by a comma (#864's review).
        const CALLED_AT_THE_END: [&str; 2] = ["Can you explain that, Lamp?", "What is this, Lamp?"];

        #[tokio::test]
        async fn the_lamp_spoken_of_is_a_call_in_a_busy_room_without_the_switch() {
            for text in SPOKEN_OF {
                assert!(the_lamp_spoken_of(text, false).await, "{text}");
            }
        }

        #[tokio::test]
        async fn the_lamp_spoken_of_is_held_in_a_busy_room_with_the_switch() {
            for text in SPOKEN_OF {
                assert!(!the_lamp_spoken_of(text, true).await, "{text}");
            }
        }

        #[tokio::test]
        async fn a_call_with_the_name_at_the_end_is_played_with_the_switch_and_without() {
            for text in CALLED_AT_THE_END {
                for ignored in [false, true] {
                    assert!(the_lamp_spoken_of(text, ignored).await, "{text}");
                }
            }
        }

        #[tokio::test]
        async fn the_rest_after_a_cut_is_held_in_a_busy_room_without_the_switch() {
            assert!(!the_rest_after_a_cut(false).await);
        }

        #[tokio::test]
        async fn the_rest_after_a_cut_keeps_the_call_and_is_played_with_the_switch() {
            assert!(the_rest_after_a_cut(true).await);
        }

        #[tokio::test]
        async fn two_resumed_utterances_are_delivered_in_order_as_one_question_with_one_end() {
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let second = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(second, 1..11, 0).await;
            rig.finish(second);
            // They go on again before anything has been answered.
            let third = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(third, 1..11, 100).await;
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            dial.service.send(provider_audio(7, 240)).await;
            let mut expected = vec!["start".to_owned()];
            expected.extend(blocks(1..11, 0));
            expected.extend(blocks(1..11, 100));
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            dial.service.send(interrupted()).await;
            dial.service.send(idle_complete()).await;
            rig.finish(third);
            // One End: the second request was never committed by itself.
            assert_eq!(rig.heard(&mut dial, QUIET).await, ["end"]);
            dial.service.send(provider_audio(9, 240)).await;
            assert!(matches!(
                rig.answer().await,
                ProviderOutput::Audio { request, .. } if request == third
            ));
            rig.stop().await;
        }

        #[tokio::test]
        async fn a_first_answer_that_completes_with_nothing_lets_the_resumed_question_start() {
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(resumed, 1..11, 0).await;
            rig.finish(resumed);
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            dial.service.send(idle_complete()).await;
            let mut expected = vec!["start".to_owned()];
            expected.extend(blocks(1..11, 0));
            expected.push("end".to_owned());
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            dial.service.send(provider_audio(9, 240)).await;
            assert!(matches!(
                rig.answer().await,
                ProviderOutput::Audio { request, .. } if request == resumed
            ));
            rig.stop().await;
        }

        #[tokio::test(start_paused = true)]
        async fn a_stop_or_a_mute_while_the_question_is_kept_writes_none_of_it() {
            let _clock = logical_clock();
            // Under both contracts: the second is the command line's.
            for (reporting, mute) in [
                (Reporting::Legacy, false),
                (Reporting::Legacy, true),
                (Reporting::Coordinated, false),
                (Reporting::Coordinated, true),
            ] {
                let mut rig = LiveRig::start_kept_reporting(reporting);
                let mut dial = rig.kept_ready(reporting).await;
                rig.speak(&mut dial, false, true).await;
                let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
                rig.say(resumed, 1..21, 0).await;
                assert!(rig.heard(&mut dial, QUIET).await.is_empty());
                if mute {
                    rig.owner
                        .set_microphone_permission(real_time(), Permission::Denied)
                        .unwrap();
                    rig.snapshot = rig.owner.snapshot(real_time()).unwrap();
                    rig.parent
                        .control
                        .send(Control::Authority {
                            snapshot: rig.snapshot,
                        })
                        .unwrap();
                } else {
                    rig.parent.control.send(Control::Stop).unwrap();
                }
                // The worker ends by itself. No further heartbeat: a muted
                // owner has no lease to renew.
                let started = Instant::now();
                while !rig.worker.is_finished() {
                    assert!(started.elapsed() < Duration::from_secs(3), "worker hung");
                    rig.tick(Duration::from_millis(1)).await;
                }
                (&mut rig.worker).await.unwrap().unwrap();
                // Kept, then dropped: not a start, not a block.
                assert!(
                    dial.service
                        .try_receive(Duration::from_millis(50))
                        .await
                        .is_none(),
                    "{reporting:?} mute={mute}"
                );
            }
        }

        #[tokio::test(start_paused = true)]
        async fn a_stop_while_the_kept_question_is_being_written_ends_the_writing() {
            let _clock = logical_clock();
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(resumed, 1..151, 0).await;
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            dial.service.send(provider_audio(7, 240)).await;
            let first = rig.service_message(&mut dial).await;
            assert!(first["realtimeInput"].get("activityStart").is_some());
            rig.parent.control.send(Control::Stop).unwrap();
            rig.exit().await.0.unwrap();
            let mut written = 0;
            while let Some(message) = dial.service.try_receive(Duration::from_millis(50)).await {
                assert!(sent_audio(&message).is_some(), "{message}");
                written += 1;
            }
            assert!(written < 150, "all of it was written after the stop");
        }

        #[tokio::test]
        async fn the_service_closing_while_the_question_is_kept_writes_none_of_it_and_replays_none()
        {
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(resumed, 1..11, 0).await;
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            dial.service.close(1008, "quota exceeded").await;
            // Whatever the worker does next about the lost connection, the
            // kept question was never written to it, and nothing of it is
            // written to any connection that replaces it.
            let deadline = Instant::now() + Duration::from_millis(600);
            while Instant::now() < deadline && !rig.worker.is_finished() {
                let _ = rig.heartbeat();
                if let Ok(mut redial) = rig.dials.try_recv() {
                    while let Some(message) =
                        redial.service.try_receive(Duration::from_millis(100)).await
                    {
                        assert!(
                            message.get("realtimeInput").is_none(),
                            "replayed to a new connection: {message}"
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }

        /// On a paused clock (#823): the patience and the strict wait pass
        /// only once every task is idle, and are read on Tokio's time.
        #[tokio::test(start_paused = true)]
        async fn with_no_first_answer_the_patience_ends_in_the_strict_timeout_and_no_end() {
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(resumed, 1..21, 0).await;
            rig.finish(resumed);
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            // Nothing from the service at all. After the patience the start
            // and the kept question are written anyway.
            let kept_since = tokio::time::Instant::now();
            let mut expected = vec!["start".to_owned()];
            expected.extend(blocks(1..21, 0));
            let mut wire = Vec::new();
            while wire.len() < expected.len() {
                assert!(kept_since.elapsed() < Duration::from_secs(4), "{wire:?}");
                wire.extend(rig.heard(&mut dial, QUIET).await);
            }
            assert_eq!(wire, expected);
            assert!(kept_since.elapsed() >= KEPT_START_PATIENCE - Duration::from_millis(400));
            // And then the strict wait, which nothing satisfies.
            let (result, _) = {
                let started = tokio::time::Instant::now();
                while !rig.worker.is_finished() {
                    assert!(started.elapsed() < Duration::from_secs(4), "worker hung");
                    let _ = rig.heartbeat();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                ((&mut rig.worker).await.unwrap(), started.elapsed())
            };
            let error = result.unwrap_err().to_string();
            assert!(error.contains("BarrierTimeout"), "{error}");
            assert!(
                dial.service
                    .try_receive(Duration::from_millis(20))
                    .await
                    .is_none(),
                "the question was committed without a confirmation"
            );
        }

        /// A request given up while its opening is kept, with no newer
        /// owner: abandoned, not continued. The worker sees that in the
        /// authority it already receives and tells the transport, which
        /// drops what it kept. None of it reaches the service, and the next
        /// question is heard alone.
        #[tokio::test]
        async fn an_abandoned_kept_question_is_dropped_unsent_and_the_next_one_is_heard_alone() {
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let abandoned = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(abandoned, 1..11, 0).await;
            // The coordinator gives the turn up: no owner follows it.
            let owner = rig.snapshot.owner().unwrap();
            rig.owner.cancel_turn(real_time(), owner).unwrap();
            // The typed outcome: ten blocks dropped unsent, none sent.
            assert!(matches!(
                rig.answer().await,
                ProviderOutput::InputDiscarded { request, dropped_samples: 1_600, sent_samples: 0 }
                    if request == abandoned
            ));
            assert!(rig.heard(&mut dial, QUIET).await.is_empty());
            // The first answer begins and is interrupted: the start alone.
            dial.service.send(provider_audio(7, 240)).await;
            assert_eq!(rig.heard(&mut dial, QUIET).await, ["start"]);
            dial.service.send(interrupted()).await;
            dial.service.send(idle_complete()).await;
            assert!(
                rig.heard(&mut dial, Duration::from_millis(150))
                    .await
                    .is_empty()
            );
            // The next question continues an activity that holds nothing.
            let next = rig.begin(&dial, AdmittedInput::NewTurn).await;
            rig.say(next, 1..6, 200).await;
            rig.finish(next);
            let mut expected = blocks(1..6, 200);
            expected.push("end".to_owned());
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            dial.service.send(provider_audio(9, 240)).await;
            assert!(matches!(
                rig.answer().await,
                ProviderOutput::Audio { request, .. } if request == next
            ));
            rig.stop().await;
        }

        /// The boundary. Abandoned after the kept words were written, there
        /// is nothing left to drop: they are in an activity with no End,
        /// and the outcome says sent, not discarded. The next question is
        /// heard after them, under its own End.
        #[tokio::test]
        async fn an_abandonment_after_the_words_were_written_is_reported_as_sent_not_discarded() {
            let mut rig = LiveRig::start_kept();
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let abandoned = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(abandoned, 1..11, 0).await;
            dial.service.send(provider_audio(7, 240)).await;
            let mut expected = vec!["start".to_owned()];
            expected.extend(blocks(1..11, 0));
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            let owner = rig.snapshot.owner().unwrap();
            rig.owner.cancel_turn(real_time(), owner).unwrap();
            assert!(matches!(
                rig.answer().await,
                ProviderOutput::InputDiscarded { request, dropped_samples: 0, sent_samples: 1_600 }
                    if request == abandoned
            ));
            dial.service.send(interrupted()).await;
            dial.service.send(idle_complete()).await;
            // Never committed by itself.
            assert!(
                rig.heard(&mut dial, Duration::from_millis(150))
                    .await
                    .is_empty()
            );
            let next = rig.begin(&dial, AdmittedInput::NewTurn).await;
            rig.say(next, 1..6, 200).await;
            rig.finish(next);
            let mut expected = blocks(1..6, 200);
            expected.push("end".to_owned());
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
            rig.stop().await;
        }

        #[tokio::test]
        async fn without_the_option_a_resumed_question_is_written_at_once_as_before() {
            let mut rig = LiveRig::start(quick_policy());
            let mut dial = rig.ready().await;
            rig.speak(&mut dial, false, true).await;
            let resumed = rig.begin(&dial, AdmittedInput::Interruption).await;
            rig.say(resumed, 1..4, 0).await;
            let mut expected = vec!["start".to_owned()];
            expected.extend(blocks(1..4, 0));
            assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        }
    }

    /// Review of the night of 2026-10-14: tests only
    /// (`docs/voice-review-2026-10-14.md`).
    mod review_2026_10_14;

    /// Short sounds let through and checked (#800): tests only.
    mod sounds;

    /// A name told is a name kept (`LAMP_NAMES=checked`): tests only.
    mod names;
    /// The Policy holds the lamp's actions (#800): tests only.
    mod policy_holds;
    /// The lamp speaks when spoken to (#829, step 2): tests only.
    mod spoken_to;
}
