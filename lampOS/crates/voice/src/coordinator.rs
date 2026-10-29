//! Finite, explicitly directed voice qualification on real hardware.
//!
//! This integration has optional guarded ring cues, but no addressee inference or automatic motor gestures.
//! Refuse to present its directed-session results as general desk-conversation
//! qualification. Audio-critical failures end the run; optional ring failure disables
//! presentation while retaining the child and truthful cleanup evidence.
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use crate::{
    activity::{Activity, EndpointDecision, ObservedAudio, TurnDetector},
    admission::{
        AcceptanceBasis, Accepted, CaptureLineage, Context as AdmissionContext, Decision, Hold,
        InputAdmission, Rejection, Step as AdmissionStep, Update as AdmissionUpdate,
    },
    camera_config::CameraConfig,
    camera_worker::CameraEvent,
    choreography::RingChoreographer,
    diagnostic_control::{self, DiagnosticFinished, DiagnosticStream},
    fixture_provider::{CueSink, Fixture, FixtureInfo, FixtureOptions},
    hand_controls::{ControlsOptions, HandControls},
    options::{
        AudioWorkerOptions, DirectedOptions, NoiseSuppression, PlayoutStart, SessionOptions,
        TurnTaking,
    },
    privacy::{PrivacyGate, PrivacyState},
    process::{SessionDirectory, Worker, boot_text, new_boot},
    provider_flow::{MAX_REPLY_SAMPLES, OutputReceiver, PACKET_SAMPLES},
    provider_link::{ProviderLink, ProviderLinkState},
    provider_worker::{ProviderInput, ProviderOutput},
    ring_wire::{RingBlankReason, RingFeedback},
    service_privacy::{self, PhysicalMute, SoftwareMute},
    session_lifetime::{Lifetime, SessionClock},
    session_state::{SessionState, YieldInterval},
    termination::Termination,
    transport::{Channel, WorkerChannels},
    turn_taking::{Disposition, FRUITLESS_YIELDS, PlaybackMask, RetainedHistoryLoss},
    visual_context::Source as VisualSource,
    wire::{
        Control, MicrophoneFrame, PlaybackDiscardReason, PlaybackGapPhase, SpeakerChunk,
        WorkerEvent,
    },
};
use lamp_audio::{
    duplex::{Confirmation, Hearing, Step as DuplexStep},
    playout::{StartGate, StartNeed},
};
#[cfg(not(test))]
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, OutputKind,
    OutputOwner, OutputPermit, Permission, PlaybackToken, Snapshot, TurnOwner,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
mod camera;
pub mod motor;
use motor::{OptionalMotor, PreparedMotor};
mod cleanup;
mod cloud;
use cleanup::{Failures as ShutdownFailures, Stage as CleanupStage};
mod early_end;
pub mod early_finalize;
mod endpoint;
mod hand;
mod holding;
mod key_refusals;
mod look_note;
mod model_refusing;
mod notices;
mod open_name;
mod requested_media;
use requested_media::RequestedMedia;
mod second;
mod service;
mod speaker_tone;
use camera::{OptionalCamera, PreparedCamera, service_camera};
use early_end::{EarlyEnd, Heard as HeardAfterEnd};
use endpoint::EndpointRig;
mod optional_ring;
mod ring_trace;
use cloud::{CloudAvailability, CloudRequest, RequestOutcome};
#[cfg(test)]
use notices::select_failure_notice;
use notices::{FailureNotices, NoticeDelivery, Outage, SpeechOrigin, select_failure_notice_in};
use optional_ring::OptionalRing;
use ring_trace::RingTrace;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const MAX_DIRECTED_SECONDS: u64 = 600;
const INPUT_READINESS_TIMEOUT: Duration = Duration::from_secs(25);
/// `on` for a service to speak for itself when the internet cannot be
/// reached (#830, `docs/offline.md`). Silent until the offline recordings
/// are provisioned with `--failure-notice-dir`.
pub const OFFLINE_VOICE_ENVIRONMENT: &str = "LAMP_OFFLINE_VOICE";
/// `on`, with `LAMP_OFFLINE_VOICE` and `LAMP_REACH_PROBE`, for a link down
/// while the internet answers (a relay outage) and nothing else can answer:
/// speech is then a local-only turn that says the service's line once, with
/// the offline breath, instead of being dropped without a word (#830, the
/// first builder's (C)). Silent until its recording is provisioned. Off
/// unless set.
pub const SERVICE_DOWN_LINE_ENVIRONMENT: &str = "LAMP_SERVICE_DOWN_LINE";
const DIAGNOSTIC_SHUTDOWN_GRACE: Duration = Duration::from_millis(750);
const INPUT_LEASE_US: u64 = 100_000;

/// Only explicitly scoped unit-test scenarios replace this clock. Child
/// startup/shutdown deadlines continue using Instant::now directly.
#[inline]
fn session_instant() -> Instant {
    #[cfg(test)]
    {
        crate::scenario_clock::instant()
    }
    #[cfg(not(test))]
    {
        Instant::now()
    }
}
/// A capture that stops for less than this is a gap, not the end of the
/// voice: its late frames are dropped and it goes on. On lamp-4ace the
/// kernel re-tuning the SD/MMC hosts held the microphone's USB reads for
/// about 0.3 s some eleven times a day, and each ended the whole voice
/// (#784). The lease itself still lapses at `INPUT_LEASE_US`, so nothing is
/// retained or admitted while the capture is stalled.
const CAPTURE_GAP_US: u64 = 1_500_000;
/// How long a just-asked lamp keeps the floor from the next voice in a busy
/// room while its answer has not begun. Answers begin 1.0 to 1.5 s after the
/// question ends.
const HOLD_FLOOR_US: u64 = 1_800_000;
/// `LAMP_WAIT_KEEPS_ANSWER=on` (#1123): while an answer is awaited and has
/// not sounded yet, a new sound takes the turn only once it has gone on, not
/// as soon as it begins. On lamp-52e6 on 2026-10-16 (the sixth's 16:5x)
/// short sounds with no words, 0.6 to 1.4 s long, admitted 1.4 to 2.6 s
/// after a question, ended 4 of 10 waiting answers as `user_interrupted`;
/// the owner's session on lamp-4ace had dozens. Off unless set.
pub const WAIT_KEEPS_ANSWER_ENVIRONMENT: &str = "LAMP_WAIT_KEEPS_ANSWER";

/// `LAMP_SECOND_PLAYOUT_START_MS=0..500` (#2135, the first builder on
/// #1722 at 17:46): the start horizon of a reply the second route answers,
/// in place of the unit's `--playout-start-ms` (350 on the lamps). That 350
/// was set for Live, whose audio fell up to 153 ms short of real time after
/// its first burst ([`PlayoutStart::LONGEST_MS`]); every second-route answer
/// pays it too, between its first audio and its first sound. Unset, the
/// unit's horizon as before.
pub const SECOND_PLAYOUT_START_ENVIRONMENT: &str = "LAMP_SECOND_PLAYOUT_START_MS";

/// [`SECOND_PLAYOUT_START_ENVIRONMENT`]'s horizon, when `value` is one
/// `--playout-start-ms` takes (0 to 500); anything else is none.
fn second_playout_start_from(value: Option<&str>) -> Option<PlayoutStart> {
    value.and_then(|value| value.parse().ok())
}

/// [`SECOND_PLAYOUT_START_ENVIRONMENT`] in this process's environment, read
/// once: the coordinator is one a process.
fn second_playout_start() -> Option<PlayoutStart> {
    static READ: std::sync::OnceLock<Option<PlayoutStart>> = std::sync::OnceLock::new();
    *READ.get_or_init(|| {
        second_playout_start_from(
            std::env::var(SECOND_PLAYOUT_START_ENVIRONMENT)
                .ok()
                .as_deref(),
        )
    })
}

/// The start horizon of a reply begun while `role` answers: the second
/// route's own, when it has one, while the second route's worker is the
/// provider; else the unit's.
fn playout_for(unit: PlayoutStart, second: Option<PlayoutStart>, role: &str) -> PlayoutStart {
    match second {
        Some(second) if role == "second" => second,
        _ => unit,
    }
}
/// How long after its trigger a new sound waits, with the switch, before it
/// may take the turn; and how long before its own decision deadline it is
/// decided at the latest, so that a real follow-up is never let go for
/// lateness (the duplex hold gives 1.2 s; the standard one 200 ms, which
/// leaves nearly nothing to wait).
const WAIT_KEEPS_FOR_US: u64 = 1_000_000;
const WAIT_DECIDES_BEFORE_US: u64 = 100_000;
/// How long after its trigger talk over a busy room's own line stays
/// pending for the lamp's name before it is let go (`busy_room`):
/// `lamp-name` says a name only once it is over, and on lamp-52e6 on 22 Oct
/// "Lamp, wait." was said 482 ms after it began, after its talk had been
/// let go on its first blocks, and the say went on `spoken`.
const OWN_LINE_NAME_LAG_US: u64 = 600_000;
/// A sound quiet this long when its wait is over has ended: it is let go.
/// In the detector's blocks of 10 ms, as its pause is counted.
const WAIT_QUIET_BLOCKS: u16 = 30;
/// An answer awaited longer than this after the person stopped may not come
/// at all, and a short "hello?" then is said to a silent lamp: a new sound
/// is decided as before (the reviewer on #1130). The backup voice gives up a
/// turn that has not begun in 3 s (`second_provider::BEGIN_WITHIN`); the
/// sounds that cut answers on lamp-52e6 came 1.4 to 2.6 s after a question.
const WAIT_KEEPS_WITHIN_US: u64 = 3_000_000;
/// A sound that begins this soon after one let go had stopped is decided as
/// before, at once: two in a row are more likely talk than a cough, and a
/// follow-up with a pause in it (0.7 s, 0.4 s of quiet, 0.6 s) then loses
/// only its first piece, not all of it (the reviewer on #1130). Two sounds
/// further apart than this are each let go.
const AFTER_LET_GO_US: u64 = 1_000_000;

/// `LAMP_OWN_LINE_KEEPS=on` (#2220, El on lamp-06c1, 20 Oct): a line the
/// lamp starts on its own (a skill's say, a reminder, a line of the setup
/// walk, news it tells) waiting for its first
/// audio is not ended by room sound. On lamp-06c1 two of three of Welcome
/// back's hellos were dropped as `user_interrupted` by a sound the room made
/// about 2 s in, before any of their audio. Off unless set.
pub const OWN_LINE_KEEPS_ENVIRONMENT: &str = "LAMP_OWN_LINE_KEEPS";

fn own_line_keeps_from_environment() -> bool {
    std::env::var(OWN_LINE_KEEPS_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// `LAMP_OWN_LINE_OVER_TALK=on` (#2602's design, a63e on 22 Oct: a say
/// ended `dropped` under office talk): while a line the lamp started on its
/// own is sounding, talk over it is let go and the line goes on, when the
/// room was busy as the line began ([`crate::body_link::Attention::busy_lately`]),
/// the lamp's name wasn't heard in it, no face turned to the lamp spoke
/// through it, and `lamp-name` heard no stop word in it. Needs no other
/// switch. Off unless set: talk over the line is a cut-in, as before.
pub const OWN_LINE_OVER_TALK_ENVIRONMENT: &str = "LAMP_OWN_LINE_OVER_TALK";

fn own_line_over_talk_from_environment() -> bool {
    std::env::var(OWN_LINE_OVER_TALK_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
}

/// How long after a sound's last speech the eyes' looks still count for it
/// ([`OwnLineWait::theirs`]): their mouth reading comes after the sound.
const OWN_LINE_EYES_LAG_US: u64 = 300_000;

/// A sound let go while the lamp's own line waited, at its deadline or as
/// the line began to sound, kept while it goes on (#2602, El on lamp-06c1
/// at 22:21:49 on 21 Oct, and at 08:17–09:08 on 22 Oct): room talk let go
/// rose again as a new sound 52 ms (deadline), and 56 and 67 ms (sounded),
/// after the hello's first audio and cut it. Its continuation is that
/// sound, never a cut-in.
/// It has ended once it has been quiet as long as the wait's own quiet
/// ([`WAIT_QUIET_BLOCKS`], 300 ms), and a sound that begins after that is a
/// new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LetGoSound {
    /// The turn of the line it was let go for.
    turn: u64,
    /// The last block heard as speech in it.
    last_speech_us: u64,
}

const LET_GO_SOUND_GAP_US: u64 = WAIT_QUIET_BLOCKS as u64 * BLOCK_US;

impl LetGoSound {
    fn new(turn: u64, last_speech_us: u64) -> Self {
        Self {
            turn,
            last_speech_us,
        }
    }

    /// The sound kept after `own` under `turn`, its last speech at
    /// `last_speech_us`: however the wait ends while a sound is held, at its
    /// deadline (`deadline`), because the line began to sound (`sounded`,
    /// El on lamp-06c1 on 22 Oct: room talk let go at the hello's first
    /// audio came back 56 and 67 ms after it and cut it), or going on from
    /// one let go (`goes_on`), the held sound stays that sound while it goes
    /// on. No other outcome keeps one.
    fn let_go(own: WhileOwnLineWaits, turn: u64, last_speech_us: u64) -> Option<Self> {
        match own {
            WhileOwnLineWaits::LetGo(_) => Some(Self::new(turn, last_speech_us)),
            _ => None,
        }
    }

    /// The sound after one more block read at `at_us`: speech carries it
    /// on, and quiet as long as [`LET_GO_SOUND_GAP_US`] ends it.
    fn heard(self, at_us: u64, speech: bool) -> Option<Self> {
        if speech {
            Some(Self {
                last_speech_us: self.last_speech_us.max(at_us),
                ..self
            })
        } else if at_us.saturating_sub(self.last_speech_us) >= LET_GO_SOUND_GAP_US {
            None
        } else {
            Some(self)
        }
    }

    /// Whether `candidate`, under `turn`, is this sound going on: the same
    /// line's turn, begun before a gap of quiet after its last speech.
    fn goes_on(self, turn: u64, candidate: &crate::admission::Candidate) -> bool {
        self.turn == turn
            && candidate.first_read_at_us < self.last_speech_us.saturating_add(LET_GO_SOUND_GAP_US)
    }
}

/// What a new sound does while a line the lamp started on its own waits for
/// its first audio (`LAMP_OWN_LINE_KEEPS`, [`while_own_line_waits`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WhileOwnLineWaits {
    /// Not such a line, it has sounded, or the switch is off: as before.
    AsBefore,
    /// The lamp's name was heard in it, or with `LAMP_WAIT_KEEPS_ANSWER=words`
    /// its words ask something: decided as before, at once.
    Decide,
    /// The eyes saw a face speaking through it, turned to the lamp: the
    /// person the line is for, not the room (#2287's follow-up). Decided as
    /// before, at once.
    Theirs,
    /// Not yet: it stays pending.
    Wait,
    /// Talk over a busy room's own line, sounding, with no name, stop word
    /// or face in it yet: it stays pending, the line playing on, until
    /// `lamp-name` has had time to say a name begun in it
    /// ([`OWN_LINE_NAME_LAG_US`]). Never a wait that lets it go as `sounded`.
    ForName,
    /// The line began to sound, or the sound's own deadline came, or it is
    /// a sound let go before going on ([`LetGoSound`]): let go as
    /// the room's, and the line goes on.
    LetGo(&'static str),
    /// With `LAMP_WAIT_KEEPS_ANSWER=words`, its words only told the lamp to
    /// stop: let go, never asked of the model, and the line is stopped, as a
    /// stop said before an answer stops it (#2287's follow-up).
    Stop,
}

/// A line the lamp started on its own, with no person's words it answers: a
/// skill's say, a reminder, news it tells, or the turn `lamp_began` (a line
/// of the setup walk).
fn own_line(current: &Reply, lamp_began: Option<u64>) -> bool {
    current.input_sequence == 0
        && (current.say.is_some()
            || current.reminder.is_some()
            || !current.news.is_empty()
            || lamp_began == Some(current.owner.turn()))
}

/// A line the lamp started on its own, sounding ([`Reply::reached_speaker`])
/// in a room busy as it began (`busy`, `LAMP_OWN_LINE_OVER_TALK`): talk over
/// it is the room's. The duplex mask doesn't yield to it, and a candidate
/// with no name, stop word or face turned to the lamp is let go.
fn busy_room_line(current: &Reply, busy: bool, lamp_began: Option<u64>) -> bool {
    busy && own_line(current, lamp_began) && current.reached_speaker()
}

/// What the own line's wait knows besides the candidate and the reply.
#[derive(Clone, Copy, Debug, Default)]
struct OwnLineWait {
    /// [`OWN_LINE_KEEPS_ENVIRONMENT`].
    on: bool,
    /// [`own_line`]'s turn the lamp began itself.
    lamp_began: Option<u64>,
    /// The candidate has already waited.
    waited: bool,
    /// This block was heard by the mask with echo evidence, or the mask has
    /// no ears, as for the answer's wait ([`AnswerWait::heard`]): a block
    /// without is kept only among the mask's last reads, so a sound held
    /// longer could not be held whole (the reviewer on #2242, P3 1).
    heard: bool,
    /// When the lamp's name was last heard beginning.
    name_began_us: Option<u64>,
    /// The eyes saw a face speaking, turned to the lamp, in at least half of
    /// their looks since the sound's first read
    /// ([`crate::speaking_faces::SpeakingFaces::turned_speakers`]).
    theirs: bool,
    /// The candidate is a sound let go while this line waited, going on
    /// ([`LetGoSound::goes_on`]).
    let_go_goes_on: bool,
    /// [`OWN_LINE_OVER_TALK_ENVIRONMENT`] is on and the room was busy as the
    /// line began ([`crate::body_link::Attention::busy_lately`]).
    busy: bool,
    /// When `lamp-name` last heard a stop word beginning (`stop/heard`).
    stop_began_us: Option<u64>,
}

/// Whether `current` is a line the lamp started on its own that hasn't
/// sounded, with `on`: a sound may wait for it, so a stream for a waited
/// sound's words is kept ready (`LAMP_WAIT_KEEPS_ANSWER=words`).
fn own_line_awaited(current: Option<&Reply>, on: bool, lamp_began: Option<u64>) -> bool {
    current.is_some_and(|current| {
        on && own_line(current, lamp_began) && !current.had_audio && current.playback.is_none()
    })
}

/// [`WhileOwnLineWaits`] for `candidate` at `at_us` (`LAMP_OWN_LINE_KEEPS`):
/// while a line the lamp started on its own has not sounded, a new sound
/// heard with echo evidence waits, unless the lamp's name began in it (up
/// to `CALL_BEFORE_US` before its trigger) or the eyes saw a face speaking
/// through it turned to the lamp: that is the person the line is for, and
/// their "not now" is no room's talk. One that waited is let go once the
/// line sounds, or 100 ms before its own decision deadline. One let go
/// either way that goes on past the line's first audio is still that
/// sound and let go again (#2602), unless the lamp's name or its person is
/// heard in it. A sound that begins once the line sounds, or after a gap,
/// is a cut-in over the lamp's voice, judged as before, so words said over
/// the lamp still stop it, unless the room was `busy` as the line began
/// (`LAMP_OWN_LINE_OVER_TALK`): then talk with no name, no stop word and no
/// face turned to the lamp in it is let go as the room's (`busy_room`), kept
/// pending first for [`OWN_LINE_NAME_LAG_US`] after its trigger so a name
/// `lamp-name` says late still counts ([`WhileOwnLineWaits::ForName`]), once
/// the line has reached the speaker ([`Reply::reached_speaker`]: the speaker
/// accepted some of it, so a line paused since still counts): audio queued,
/// or handed over and not yet accepted, is no sounding line yet. While it
/// sounds so, the duplex mask doesn't yield to that talk
/// ([`crate::turn_taking::PlaybackMask::room_talk`]), so the line plays on
/// with no gap.
fn while_own_line_waits(
    wait: OwnLineWait,
    reply: Option<&Reply>,
    candidate: &crate::admission::Candidate,
    at_us: u64,
) -> WhileOwnLineWaits {
    let OwnLineWait {
        on,
        lamp_began,
        waited,
        heard,
        name_began_us,
        theirs,
        let_go_goes_on,
        busy,
        stop_began_us,
    } = wait;
    let Some(current) = reply.filter(|current| (on || busy) && own_line(current, lamp_began))
    else {
        return WhileOwnLineWaits::AsBefore;
    };
    if !heard {
        return WhileOwnLineWaits::AsBefore;
    }
    let in_it = |began: Option<u64>| {
        began.is_some_and(|began| {
            began.saturating_add(crate::activity::CALL_BEFORE_US) >= candidate.trigger_read_at_us
        })
    };
    let named = in_it(name_began_us);
    if current.had_audio || current.playback.is_some() {
        return if on && waited {
            WhileOwnLineWaits::LetGo("sounded")
        } else if on && let_go_goes_on && !named && !theirs {
            WhileOwnLineWaits::LetGo("goes_on")
        } else if busy
            && busy_room_line(current, busy, lamp_began)
            && !named
            && !theirs
            && !in_it(stop_began_us)
        {
            // The name or a stop word is said once it is over: until it
            // could have been, the talk stays pending.
            if at_us
                < candidate
                    .trigger_read_at_us
                    .saturating_add(OWN_LINE_NAME_LAG_US)
                    .min(
                        candidate
                            .decision_deadline_us
                            .saturating_sub(WAIT_DECIDES_BEFORE_US),
                    )
            {
                WhileOwnLineWaits::ForName
            } else {
                WhileOwnLineWaits::LetGo("busy_room")
            }
        } else {
            WhileOwnLineWaits::AsBefore
        };
    }
    if !on {
        return WhileOwnLineWaits::AsBefore;
    }
    if named {
        return WhileOwnLineWaits::Decide;
    }
    if theirs {
        return WhileOwnLineWaits::Theirs;
    }
    if at_us
        >= candidate
            .decision_deadline_us
            .saturating_sub(WAIT_DECIDES_BEFORE_US)
    {
        return WhileOwnLineWaits::LetGo("deadline");
    }
    WhileOwnLineWaits::Wait
}

/// `own`, [`while_own_line_waits`] for `candidate`, with its words
/// (`LAMP_WAIT_KEEPS_ANSWER=words`) as the answer's wait hears them
/// ([`with_waited_words`]): a sound that waits or is let go is judged by
/// them too. A lone stop, or in a room not `busy` a lone put-off ("Not
/// now.", "Later.", [`crate::wait_words::only_a_put_off`]), stops the line,
/// never asked of the model. In a busy room a put-off is as usual between
/// colleagues ("No thanks.", "In a minute.") as to the lamp, so it is the
/// room's, as other words are: in a busy office two colleagues' talk has
/// words too (the reviewer's P2 on #2308 and P3 on #2322). A face turned to
/// the lamp and speaking is decided before this ([`WhileOwnLineWaits::Theirs`]).
/// Any other `own` is as it is.
#[allow(clippy::too_many_arguments)]
fn own_line_with_words(
    words: &mut crate::wait_words::WaitWords,
    own: WhileOwnLineWaits,
    candidate: &crate::admission::Candidate,
    audio: &[ObservedAudio],
    ended: bool,
    pause_blocks: u16,
    last_speech_us: u64,
    busy: bool,
    at_us: u64,
) -> (WhileOwnLineWaits, Option<WordsTold>) {
    let as_answer = match own {
        WhileOwnLineWaits::Wait => WhileAnswerWaits::Wait,
        WhileOwnLineWaits::LetGo(why) => WhileAnswerWaits::LetGo(why),
        _ => return (own, None),
    };
    let (then, told) = with_waited_words(
        words,
        as_answer,
        candidate,
        audio,
        ended,
        pause_blocks,
        last_speech_us,
        false,
        at_us,
    );
    let (own, told) = match then {
        // Words that ask something: a put-off stops the line as a stop does;
        // anything else is the room's, and the sound is as it was.
        WhileAnswerWaits::Decide if !busy && words.put_off(candidate.id) => (
            WhileOwnLineWaits::Stop,
            told.map(|told| WordsTold {
                took: false,
                stop: true,
                ..told
            }),
        ),
        WhileAnswerWaits::Decide => (
            own,
            told.map(|told| WordsTold {
                took: false,
                ..told
            }),
        ),
        WhileAnswerWaits::Wait => (WhileOwnLineWaits::Wait, told),
        WhileAnswerWaits::LetGo(why) => (WhileOwnLineWaits::LetGo(why), told),
        WhileAnswerWaits::Stop => (WhileOwnLineWaits::Stop, told),
    };
    (own, told)
}

/// Whether `value`, the voice's `LAMP_WAIT_KEEPS_ANSWER`, turns the wait on:
/// `on`, or `words`, and nothing else.
fn wait_keeps_answer(value: Option<&str>) -> bool {
    value == Some("on") || wait_hears_words(value)
}

/// Whether `value` has the wait judge a waited sound by its words too
/// (`words`, #1123 step 2, [`crate::wait_words`]).
fn wait_hears_words(value: Option<&str>) -> bool {
    value == Some(crate::wait_words::VALUE)
}

/// Whether the voice's environment turns the wait on, and whether with the
/// words.
fn wait_keeps_answer_from_environment() -> (bool, bool) {
    let value = std::env::var(WAIT_KEEPS_ANSWER_ENVIRONMENT).ok();
    (
        wait_keeps_answer(value.as_deref()),
        wait_hears_words(value.as_deref()),
    )
}

/// What a new sound does while an answer is awaited and has not sounded
/// yet ([`WAIT_KEEPS_ANSWER_ENVIRONMENT`], #1123).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WhileAnswerWaits {
    /// Decided as before: no answer to someone waits unheard, the switch is
    /// off, the lamp's name was heard in it, or the sound has gone on long
    /// enough.
    Decide,
    /// Not yet: it stays pending, its audio kept from its start.
    Wait,
    /// It ended, or went quiet, before it could take the turn: let go, and
    /// the answer goes on.
    LetGo(&'static str),
    /// Its words only told the lamp to stop (`LAMP_WAIT_KEEPS_ANSWER=words`,
    /// [`crate::wait_words::only_a_stop`]): let go, never asked of the
    /// model, and the answer it waits for is stopped, as a stop said over
    /// the lamp's speech stops it (the reviewer's P3 on #1277).
    Stop,
}

/// What the wait knows besides the candidate and the reply.
#[derive(Clone, Copy, Debug, Default)]
struct AnswerWait {
    /// [`WAIT_KEEPS_ANSWER_ENVIRONMENT`].
    on: bool,
    /// The candidate has already waited: it keeps the wait's rules until
    /// its decision once the answer's audio has arrived, for a cough is no
    /// less a cough then (the first builder on #1130), as far as they can
    /// still be judged (`sounding`).
    waited: bool,
    /// The lamp's answer is sounding as the mask hears this block
    /// ([`reply_sounding`]): a person's speech now scores 0.0 until the mask
    /// has matched the lamp's sound, so the pause counted from here on means
    /// nothing (the first builder's re-check on #1130).
    sounding: bool,
    /// The candidate was decided once the answer sounded: it stays decided.
    decided: bool,
    /// This block was heard by the mask with echo evidence, or the mask has
    /// no ears. A block without is kept only among the last reads (80, and
    /// 0.6 s), so a waited sound could not be held (the first builder's
    /// re-check on #1130).
    heard: bool,
    /// When the person's request, the one answered, ended.
    asked_until_us: Option<u64>,
    /// When the lamp's name was last heard beginning (`name/rising` or
    /// `name/heard`).
    name_began_us: Option<u64>,
    /// When the sound last let go had stopped: the last block the detector
    /// heard as speech before it was let go.
    let_go_ended_us: Option<u64>,
}

/// Whether the lamp has something to yield, as the duplex mask hears a
/// block: reply audio waiting to be handed over, or the lamp's sound still
/// accepted, queued or within its tail. While it has, the mask scores a
/// person's speech 0.0 until it has matched the lamp's own sound.
fn reply_sounding(mask: &PlaybackMask, reply: &Option<Reply>, at_us: u64) -> bool {
    !mask.available(at_us)
        || reply.as_ref().is_some_and(|current| {
            current.cloud.started && current.local_input_ended && !current.pcm.is_empty()
        })
}

/// Whether a waited sound that `on` decided may still be held for its
/// words: `on` decided it because the answer began to sound, and the mask
/// heard this block with echo evidence. A block without is kept only among
/// the mask's last reads, so a sound held then loses its first reads and
/// the decision fails (the reviewer's P1 on #1352).
fn may_hold_once_decided(wait: AnswerWait) -> bool {
    wait.waited && wait.sounding && wait.heard
}

/// #1712's B (the first builder at 06:44, `LAMP_MOTION_KEEPS_ANSWER=on`):
/// what the body's motion makes of `candidate`, a new sound while an answer
/// is awaited and has not sounded (the request in flight, its first words
/// not yet played). One that began as the body moved a joint is the lamp's
/// own sound, its motors, as its playout is, and takes no turn
/// ([`crate::body_motion::Judged::Moved`]). The lamp's name heard with it
/// is still a call (as for [`while_answer_waits`]). Anything else is heard
/// as before: no answer awaited, an answer that has sounded, or the lamp
/// sounding now, where the duplex hears what is its own.
fn body_heard(
    judged: impl FnOnce() -> crate::body_motion::Judged,
    reply: Option<&Reply>,
    sounding: bool,
    name_began_us: Option<u64>,
    candidate: &crate::admission::Candidate,
) -> crate::body_motion::Judged {
    let awaited = reply.is_some_and(|current| current.local_input_ended && !current.had_audio);
    let named = name_began_us.is_some_and(|began| {
        began.saturating_add(crate::activity::CALL_BEFORE_US) >= candidate.trigger_read_at_us
    });
    if awaited && !sounding && !named {
        judged()
    } else {
        crate::body_motion::Judged::Still
    }
}

/// `LAMP_MOTION_VOICE` (#1712's B, the first builder at 03:27 on 21 Oct):
/// what becomes of `candidate`, a sound [`body_heard`] found begun as the
/// body moved. Its own speech blocks, from its first read to `at_us`,
/// against the asker's level as the asker-first rule keeps it, and the
/// levels it was judged on ([`crate::motion_voice`]).
fn motion_voice_heard(
    mode: crate::motion_voice::Mode,
    asker_first: Option<&crate::asker_first::AskerFirst>,
    reply: Option<&Reply>,
    candidate: &crate::admission::Candidate,
    at_us: u64,
) -> (
    crate::motion_voice::Decision,
    Option<crate::asker_first::Levels>,
) {
    let levels = asker_first.zip(reply).and_then(|(rule, current)| {
        rule.levels_since(current.owner.turn(), candidate.first_read_at_us, at_us)
    });
    (crate::motion_voice::decide(mode, levels.as_ref()), levels)
}

/// [`WhileAnswerWaits`] for `candidate` at `at_us`: `ended` if its sound is
/// over, and the detector's pause in blocks.
fn while_answer_waits(
    wait: AnswerWait,
    reply: Option<&Reply>,
    candidate: &crate::admission::Candidate,
    ended: bool,
    pause_blocks: u16,
    at_us: u64,
) -> WhileAnswerWaits {
    // Only an answer to someone heard: not a skill's hello, the lamp's look,
    // reminder, news or "hm?", which a short "hi" or "not now" may answer
    // (the first builder on #1130). A written turn has no input.
    let Some(current) = reply.filter(|current| current.input_sequence > 0) else {
        return WhileAnswerWaits::Decide;
    };
    if !wait.on || !(answer_unheard(current) || wait.waited) || !wait.heard {
        return WhileAnswerWaits::Decide;
    }
    // Once the answer sounds, the sound is judged by its pause as it stood
    // then: one that had ended or gone quiet for 300 ms is let go, the
    // answer goes on; one still going is decided at once, by the duplex
    // rules for speech over the lamp, and stays decided. Its pause from
    // here on is the mask's silence, not the person's (the first builder's
    // re-check on #1130).
    if wait.waited && (wait.sounding || wait.decided) {
        return if !wait.decided && (ended || pause_blocks >= WAIT_QUIET_BLOCKS) {
            WhileAnswerWaits::LetGo("quiet")
        } else {
            WhileAnswerWaits::Decide
        };
    }
    if !wait.waited {
        let long_awaited = wait
            .asked_until_us
            .is_none_or(|until| at_us.saturating_sub(until) > WAIT_KEEPS_WITHIN_US);
        let after_let_go = wait.let_go_ended_us.is_some_and(|stopped| {
            candidate.trigger_read_at_us.saturating_sub(stopped) < AFTER_LET_GO_US
        });
        if long_awaited || after_let_go {
            return WhileAnswerWaits::Decide;
        }
    }
    // The lamp's name, from the sound's start on (a hearing may begin up to
    // `CALL_BEFORE_US` before the detector is sure): "Lamp, … stop" with its
    // pause at the 1 s mark is someone talking to it (the reviewer on #1130).
    if wait.name_began_us.is_some_and(|began| {
        began.saturating_add(crate::activity::CALL_BEFORE_US) >= candidate.trigger_read_at_us
    }) {
        return WhileAnswerWaits::Decide;
    }
    if ended {
        return WhileAnswerWaits::LetGo("ended");
    }
    let decide_at = candidate
        .trigger_read_at_us
        .saturating_add(WAIT_KEEPS_FOR_US)
        .min(
            candidate
                .decision_deadline_us
                .saturating_sub(WAIT_DECIDES_BEFORE_US),
        );
    if at_us < decide_at {
        WhileAnswerWaits::Wait
    } else if pause_blocks >= WAIT_QUIET_BLOCKS {
        WhileAnswerWaits::LetGo("quiet")
    } else {
        WhileAnswerWaits::Decide
    }
}

/// Whether a block is heard with the lamp silent, for a soft start
/// (`LAMP_SOFT_START`, `TurnDetector::lamp_silent`): its sound not reaching
/// the microphone by the echo evidence (`sounding`), nothing of a reply to
/// yield as the duplex mask hears the block ([`reply_sounding`]: audio still
/// to hand over, or the lamp's sound accepted, queued or within its tail),
/// and no busy answer kept. A soft start's additional lead never reaches back
/// past a block that was not, so not into the lamp's own voice as this
/// software evidence tells it. The ordinary 300 ms before the start is kept
/// as without the switch, and may still hold the lamp's last sound.
///
/// Not the reply's playback token: it is held from a reply's start until
/// its audio is retired, so on lamp-4ace on 25 October the lines begun after
/// an answer, or after a bare "Lamp." whose answer waited, kept no soft start
/// at all, and two lost their "what" as before.
fn lamp_silent(sounding: bool, keeping_answer: bool, reply_sounding: bool) -> bool {
    !sounding && !reply_sounding && !keeping_answer
}

/// Whether `current` is an answer to someone's request that has ended, none
/// of which has arrived or sounded: what a new sound may wait for.
fn answer_unheard(current: &Reply) -> bool {
    current.input_sequence > 0
        && current.local_input_ended
        && !current.had_audio
        && current.playback.is_none()
}

/// Whether a new sound may still wait for `current`'s answer at `now_us`,
/// so a stream for its words is kept ready: the answer is unheard
/// ([`answer_unheard`]) and the request it answers ended no more than
/// [`WAIT_KEEPS_WITHIN_US`] ago (`asked_until_us`): past that, or with no
/// known end, a new sound is decided as before ([`while_answer_waits`]).
/// A long answer (an agent's, tens of seconds) keeps no idle stream open
/// to the relay (the seventh's P3 on #1277).
fn words_awaited(current: Option<&Reply>, asked_until_us: Option<u64>, now_us: u64) -> bool {
    current.is_some_and(answer_unheard)
        && asked_until_us.is_some_and(|until| now_us.saturating_sub(until) <= WAIT_KEEPS_WITHIN_US)
}

/// What a waited sound's words did, for the record (#1123, step 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WordsTold {
    /// Its words that ask something, or none back in time.
    words: Option<usize>,
    /// It was decided at once for them.
    took: bool,
    /// They only told the lamp to stop.
    stop: bool,
    asked_at_us: u64,
}

/// `waits`, [`while_answer_waits`] for `candidate`, with its words
/// (`LAMP_WAIT_KEEPS_ANSWER=words`, [`crate::wait_words`]): `audio` its
/// kept audio, `ended` if its sound is over, and the detector's pause in
/// blocks. Its audio goes to the stream while it waits, and its words are
/// asked for once it has been quiet for
/// [`crate::wait_words::ASK_AFTER_QUIET_BLOCKS`], well before its
/// endpoint. What they did is told once a sound. `last_speech_us` is when
/// the last block heard as speech was captured: a stop counts only when
/// none came after its words were asked for. `answer_sounding` is whether
/// `on` decided a sound that waited because the answer began to sound, on a
/// block the mask heard with echo evidence (`AnswerWait::waited`,
/// `sounding` and `heard`, [`may_hold_once_decided`]).
#[allow(clippy::too_many_arguments)]
fn with_waited_words(
    words: &mut crate::wait_words::WaitWords,
    waits: WhileAnswerWaits,
    candidate: &crate::admission::Candidate,
    audio: &[ObservedAudio],
    ended: bool,
    pause_blocks: u16,
    last_speech_us: u64,
    answer_sounding: bool,
    at_us: u64,
) -> (WhileAnswerWaits, Option<WordsTold>) {
    use crate::wait_words::{Then, Verdict};
    // Whether its words were asked for and no new speech has begun since:
    // speech after the ask is the person going on (the reviewer's P2 on
    // #1352).
    let quiet_since_asked = |words: &crate::wait_words::WaitWords| {
        words.holds() == Some(candidate.id)
            && words
                .asked_through_us()
                .is_some_and(|through| last_speech_us <= through)
    };
    let decided = waits == WhileAnswerWaits::Decide;
    let lets_go = matches!(waits, WhileAnswerWaits::LetGo(_));
    if decided {
        // `on` has decided (the answer began to sound inside its 300 ms of
        // quiet, or its 1 s mark came). A sound whose words were asked for
        // and that has been quiet since is still judged by them, so a stop
        // just before the answer sounds is never asked of the model (the
        // seventh's P2 on #1352). Any other is as `on` decided.
        if !quiet_since_asked(words) {
            return (waits, None);
        }
    } else if lets_go || ended || pause_blocks >= crate::wait_words::ASK_AFTER_QUIET_BLOCKS {
        words.ask(candidate.id, audio, at_us);
    } else {
        words.hear(candidate.id, audio);
    }
    let mut verdict = words.verdict(candidate.id);
    // A stop counts only if no new speech has begun since its words were
    // asked for. "Wait, … what about tomorrow?" with a comma's pause is
    // asked for at the pause and comes back "Wait.": the person went on, so
    // it takes the turn from its start, as any word does.
    if let Verdict::Stop(count) = verdict
        && !quiet_since_asked(words)
    {
        verdict = Verdict::Words(count);
    }
    let deadline_us = candidate
        .decision_deadline_us
        .saturating_sub(WAIT_DECIDES_BEFORE_US);
    let then = if decided {
        crate::wait_words::then_once_decided(verdict, answer_sounding, ended, at_us, deadline_us)
    } else {
        crate::wait_words::then(lets_go, verdict, ended, at_us, deadline_us)
    };
    let waits = match then {
        Then::AsBefore => return (waits, None),
        Then::Wait => return (WhileAnswerWaits::Wait, None),
        Then::Take(_) => WhileAnswerWaits::Decide,
        Then::Stop => WhileAnswerWaits::Stop,
        Then::LetGo => waits,
    };
    let told = words.tell().then(|| WordsTold {
        words: match verdict {
            Verdict::Words(count) | Verdict::Stop(count) => Some(count),
            Verdict::Pending => None,
        },
        took: matches!(then, Then::Take(_)),
        stop: then == Then::Stop,
        asked_at_us: words.asked_at_us().unwrap_or(at_us),
    });
    (waits, told)
}
/// One capture block.
const BLOCK_US: u64 = 10_000;
/// A candidate the asker-first rule keeps waiting is judged on what has
/// been heard once its decision deadline, or the end of its oldest block's
/// read history ([`crate::turn_taking::HISTORY_US`]), is this near, so that
/// it is never lost to either while it waits (`crate::asker_first`). One
/// refused for its history leaves the detector mid-utterance, and what
/// follows is lost too.
const ASKER_LAST_CHANCE_US: u64 = 100_000;
/// How long an answer to something asked in a busy room is not stopped for a
/// voice over it. The first time the lamp kept its turn there, on lamp-4ace,
/// it began "It's a beautiful, bright blue" under a colleague's sentence and
/// was stopped by that same sentence 1.2 s later. Most answers are shorter
/// than this; a story can still be stopped once it has passed.
const KEEP_ANSWER_US: u64 = 4_000_000;
/// How soon after the lamp ended a turn speech that begins again still goes
/// on with it (`LAMP_CUT_KEEPS_NAME`, #782), from the block that ended it to
/// the block that set the new speech off, both by the capture clock. The
/// pauses at a comma that ended a turn on lamp-4ace on 2026-10-15 were heard
/// as 400 to 560 ms of quiet, so the person went on within 160 ms of an end
/// at 400 ms; a second also covers an end at the 600 ms window.
const CONTINUES_WITHIN_US: u64 = 1_000_000;

/// A turn the lamp ended, for speech that may go on with it (#782).
#[derive(Clone, Copy, Debug)]
struct EndedTurn {
    turn: u64,
    /// When the block that ended it was read.
    at_us: u64,
    session: u64,
    microphone_generation: u64,
}

impl EndedTurn {
    /// Whether speech set off at `trigger_read_at_us` and admitted over
    /// `current` goes on with this turn: `current` is this turn's request,
    /// its input ended and nothing of its answer heard, on the same
    /// connection and microphone, and the speech began within
    /// [`CONTINUES_WITHIN_US`] of the end. A second voice inside the pause
    /// counts too, as it does after an end sent early.
    fn goes_on_in(
        self,
        current: Option<&Reply>,
        trigger_read_at_us: u64,
        session: u64,
        microphone_generation: u64,
    ) -> bool {
        current.is_some_and(|current| {
            current.owner.turn() == self.turn
                && current.local_input_ended
                && !current.had_audio
                && current.playback.is_none()
                && current.cloud.session == self.session
        }) && session == self.session
            && microphone_generation == self.microphone_generation
            && trigger_read_at_us.saturating_sub(self.at_us) <= CONTINUES_WITHIN_US
    }
}

#[cfg(test)]
const MAX_TRACE_EVENTS: usize = crate::session_state::MAX_FINITE_EVENTS;
const PROVIDER_PACKETS_PER_SLICE: usize = 2;
const PROVIDER_SLICE_US: u64 = 2_000;

/// A cloud burst must yield to capture/cancellation and the next speaker chunk.
/// This is cooperative: one bounded datagram decode can exceed the time target.
struct ProviderSlice {
    started_at_us: u64,
    received: usize,
}
impl ProviderSlice {
    fn new(started_at_us: u64) -> Self {
        Self {
            started_at_us,
            received: 0,
        }
    }

    fn receive(&mut self, channel: &mut Channel, at_us: u64) -> io::Result<Option<ProviderOutput>> {
        let elapsed = at_us
            .checked_sub(self.started_at_us)
            .ok_or_else(|| io::Error::other("provider service clock moved backwards"))?;
        if self.received >= PROVIDER_PACKETS_PER_SLICE || elapsed >= PROVIDER_SLICE_US {
            return Ok(None);
        }
        let event = channel.receive()?;
        if event.is_some() {
            self.received += 1;
        }
        Ok(event)
    }
}

/// A sleep the person asked aloud (#3748): its turn, and the latest it is
/// asked while its line is still sounding. It is asked only once that turn
/// is heard whole, or at the cap with its line still sounding; a turn cut by
/// the person, or one that ends with no audio, drops it (the reviewer-lead
/// on #3835).
#[derive(Debug, Default)]
struct SleepOwed {
    owed: Option<(u64, u64)>,
    due: Option<u64>,
}

impl SleepOwed {
    fn asked(&mut self, request: u64, now_us: u64) {
        self.owed = Some((request, now_us + crate::sleep_asked::LINE_PLAYED_WITHIN_US));
    }

    /// As a turn ends: its sleep is due if its line was heard whole.
    fn turn_ended(
        &mut self,
        current: &Reply,
        speaker_pending: &Option<PendingSpeaker>,
        snapshot: Snapshot,
    ) {
        let turn = current.owner.turn();
        if self.owed.is_none_or(|(request, _)| request != turn) {
            return;
        }
        self.owed = None;
        if current.heard_whole(speaker_pending, snapshot) {
            self.due = Some(turn);
        } else {
            eprintln!(
                "lamp-voice: request {turn}: its line was not heard whole ({}): no sleep",
                current.delivery_outcome()
            );
        }
    }

    /// At a poll, with the reply in hand: the request to sleep now, if
    /// any. Its turn gone without being heard whole drops it; at the cap it
    /// is asked only if its line has sounded.
    fn due(&mut self, reply: Option<&Reply>, now_us: u64) -> Option<u64> {
        if let Some((request, latest_us)) = self.owed {
            match reply.filter(|current| current.owner.turn() == request) {
                None => {
                    self.owed = None;
                    eprintln!(
                        "lamp-voice: request {request}: its line was not heard whole: no sleep"
                    );
                }
                Some(current) if now_us >= latest_us => {
                    self.owed = None;
                    if current.sounded {
                        self.due = Some(request);
                    } else {
                        eprintln!(
                            "lamp-voice: request {request}: its line never sounded: no sleep"
                        );
                    }
                }
                Some(_) => {}
            }
        }
        self.due.take()
    }
}

struct Reply {
    owner: TurnOwner,
    cloud: CloudRequest,
    local_input_ended: bool,
    plan: OutputOwner,
    playback: Option<PlaybackToken>,
    pcm: VecDeque<i16>,
    /// Its samples handed to the speaker so far: how much of it was played
    /// when a line or a Stop cut it (`LAMP_SAID_AS_HEARD`).
    handed: u64,
    /// The speaker has accepted some of it (its playback started), now or
    /// before a pause: it has sounded. Never reset.
    sounded: bool,
    generation_done: bool,
    provider_idle: bool,
    input_sequence: u64,
    had_audio: bool,
    final_chunk: Option<u64>,
    playback_gaps: u64,
    last_provider_audio_at_us: Option<u64>,
    start: StartGate,
    need: StartNeed,
    notice: NoticeDelivery,
    media: RequestedMedia,
    /// Taken while the internet could not be reached (#830): never sent.
    local_only: Option<notices::LocalOnly>,
    reminder: Option<crate::reminders::Receipt>,
    /// A say asked of the voice ([`crate::say`], #939), answered when the
    /// turn ends.
    say: Option<crate::say::Ticket>,
    /// How loud the block last handed to the speaker is, for the ring's face.
    voice_level: u8,
    /// The Stop this request tells the model of, if it does: given back if
    /// it is thrown out before its End goes (`not_speech::StopToTell`).
    told_stop: Option<(u64, u64)>,
    /// The news this turn tells (its key, and those told with it): handed
    /// to the tasks part only once the turn is heard whole (#1390).
    news: Vec<String>,
}
impl Reply {
    fn settled(&self, speaker_pending: &Option<PendingSpeaker>) -> bool {
        self.output_settled(speaker_pending) && !self.requested_audio_waiting()
    }

    fn output_settled(&self, speaker_pending: &Option<PendingSpeaker>) -> bool {
        self.provider_idle
            && self.cloud.outcome.is_some()
            && self.local_input_ended
            && self.pcm.is_empty()
            && speaker_pending.is_none()
            && self.playback.is_none()
            && self.notice.settled()
    }

    /// The output portion of whole-turn delivery. Current ownership and
    /// microphone permission still have to be checked at completion.
    fn whole_provider_output(&self, speaker_pending: &Option<PendingSpeaker>) -> bool {
        self.provider_output_played(speaker_pending) && self.playback_gaps == 0
    }

    /// [`Self::whole_provider_output`], whatever gaps its playback had.
    fn provider_output_played(&self, speaker_pending: &Option<PendingSpeaker>) -> bool {
        self.settled(speaker_pending)
            && matches!(self.cloud.outcome, Some(RequestOutcome::Complete))
            && self.had_audio
            && self.notice.origin() == SpeechOrigin::Provider
    }

    /// Whether this turn, as it ends, was the model's and heard whole:
    /// settled and complete, its audio played with no gap, nobody else
    /// owning the turn, and the mic allowed.
    fn heard_whole(&self, speaker_pending: &Option<PendingSpeaker>, snapshot: Snapshot) -> bool {
        self.heard_through(speaker_pending, snapshot) && self.playback_gaps == 0
    }

    /// [`Self::heard_whole`], whatever gaps its playback had: every word
    /// played and nobody's turn taken over it, as after a yield that resumed
    /// (#4035).
    fn heard_through(&self, speaker_pending: &Option<PendingSpeaker>, snapshot: Snapshot) -> bool {
        self.provider_output_played(speaker_pending)
            && snapshot.owner().is_none()
            && snapshot.microphone_permission() == Permission::Allowed
    }

    /// How an onboarding invitation's turn ended, as it completes. Nothing
    /// of it played is no ask at all (#3754); played through with a gap is
    /// no ask either, but said (#4035).
    fn invitation_delivery(
        &self,
        speaker_pending: &Option<PendingSpeaker>,
        snapshot: Snapshot,
        sounded: bool,
    ) -> crate::onboarding::Delivery {
        use crate::onboarding::Delivery;
        if self.heard_whole(speaker_pending, snapshot) {
            Delivery::Whole
        } else if self.heard_through(speaker_pending, snapshot) {
            Delivery::Gapped
        } else if !self.reached_speaker() && !sounded {
            Delivery::Unsaid
        } else {
            Delivery::Cut
        }
    }

    fn complete_reminder(&mut self, speaker_pending: &Option<PendingSpeaker>, snapshot: Snapshot) {
        if self.heard_whole(speaker_pending, snapshot)
            && let Some(reminder) = &mut self.reminder
        {
            reminder.completed();
        }
    }

    /// The news this turn told when it played through but not whole: every
    /// word played, with a gap, and nobody's turn taken over it
    /// ([`Self::heard_through`], #4035). That is no news heard, so its
    /// harness stays open; but a made skill's question in it was said
    /// aloud, so a yes to it counts (fourteen-P10 on #2814: a yes refused
    /// after a question said whole with a gap). The gaps are the lamp's own
    /// word tails, nothing the person did. Call before [`Self::heard_news`],
    /// which takes the news.
    fn news_played_through(
        &self,
        speaker_pending: &Option<PendingSpeaker>,
        snapshot: Snapshot,
    ) -> Vec<String> {
        if self.news.is_empty()
            || self.heard_whole(speaker_pending, snapshot)
            || self.requested_audio_heard_whole(speaker_pending, snapshot)
            || !self.heard_through(speaker_pending, snapshot)
        {
            return Vec::new();
        }
        self.news.clone()
    }

    /// The news this turn told, as it ends: all of it when the turn was
    /// heard whole ([`Self::heard_whole`], the proof a say's `spoken`
    /// takes), including a requested recording that also retired whole.
    /// Otherwise its harness stays open. A Stop, a reply cut off or with no
    /// audio, or a muted mic is no news heard (the
    /// reviewer on #1425).
    fn heard_news(
        &mut self,
        speaker_pending: &Option<PendingSpeaker>,
        snapshot: Snapshot,
    ) -> Vec<String> {
        let news = std::mem::take(&mut self.news);
        if news.is_empty()
            || self.heard_whole(speaker_pending, snapshot)
            || self.requested_audio_heard_whole(speaker_pending, snapshot)
        {
            return news;
        }
        eprintln!(
            "lamp-voice: the news ({}) was not heard whole ({}): its harness stays open",
            news.join(", "),
            self.delivery_outcome()
        );
        Vec::new()
    }

    /// A say this turn was ([`crate::say`]) is answered as it ends:
    /// `spoken` on the proof a reminder's acknowledgement takes, otherwise
    /// `dropped` with what became of the turn.
    fn answer_say(&mut self, speaker_pending: &Option<PendingSpeaker>, snapshot: Snapshot) {
        let Some(say) = self.say.take() else {
            return;
        };
        // A skill's sound is `spoken` when it was played whole, with no gap
        // in its playback.
        let sound = self.notice.origin() == SpeechOrigin::SkillSound
            && self.settled(speaker_pending)
            && self.notice.retired()
            && self.playback_gaps == 0;
        let played_whole = sound || self.heard_whole(speaker_pending, snapshot);
        // complete_reminder runs first. Read its resulting settlement, not the
        // pre-completion eligibility that successful completion consumes.
        let reminder_complete = self
            .reminder
            .as_ref()
            .is_none_or(crate::reminders::Receipt::completion_recorded);
        say.answer(if played_whole && reminder_complete {
            crate::say::Outcome::Spoken
        } else {
            let why = if played_whole && !reminder_complete {
                "reminder_completion_unconfirmed"
            } else {
                self.delivery_outcome()
            };
            crate::say::Outcome::Dropped(why.to_owned())
        });
    }

    fn append_provider_audio(&mut self, samples: Vec<i16>, received_at_us: u64) {
        self.media.confirmation_retired_at_us = None;
        if !samples.is_empty()
            && let Some(reminder) = &mut self.reminder
        {
            reminder.audio();
        }
        // The gate counts frames to measure the service's lead. A plain gate
        // only records it; a continuity gate starts a new hold with the first
        // audio after a generation's final block left.
        self.start.arrival(received_at_us, samples.len());
        self.need.arrival(received_at_us, samples.len());
        // Generation completion is not turn completion. A new generation may
        // arrive while the previous generation's accepted final PCM is still
        // playing. Preserve that receipt identity until it actually retires.
        self.generation_done = false;
        self.pcm.extend(samples);
        self.last_provider_audio_at_us = Some(received_at_us);
    }

    fn retire_playback(&mut self, sequence: u64, final_frame: u64) -> io::Result<PlaybackToken> {
        if self.final_chunk != Some(sequence) || final_frame == 0 {
            return Err(io::Error::other(
                "speech retirement has no matching final chunk",
            ));
        }
        let token = self
            .playback
            .take()
            .ok_or_else(|| io::Error::other("speech retirement has no playback occurrence"))?;
        self.final_chunk = None;
        Ok(token)
    }

    fn may_start(&mut self, at_us: u64) -> bool {
        // The gate receipt describes a dispatchable first block. A lone held
        // tail, empty queue, or prior unretired final cursor is not a start.
        self.has_speaker_chunk() && self.start.open(at_us, self.pcm.len(), self.generation_done)
    }

    /// It has reached the speaker: accepted, now or before a pause.
    fn reached_speaker(&self) -> bool {
        self.sounded || self.playback.is_some()
    }

    fn has_speaker_chunk(&self) -> bool {
        self.final_chunk.is_none()
            && !self.pcm.is_empty()
            && (self.generation_done || self.pcm.len() > 240)
    }

    fn start_trace(&self) -> Value {
        let opened = self.start.opened();
        json!({
            "horizon_us":self.start.horizon_us(),
            "continuity":self.start.continuity().map(|policy| json!({"margin_us":policy.margin_us,"hold_us":policy.hold_us,"cover_us":policy.cover_us})),
            "measured_lead_us":self.start.lead_us(),
            "opened_by":opened.map(|event| match event.by {
                lamp_audio::playout::OpenedBy::Covered => "covered",
                lamp_audio::playout::OpenedBy::Complete => "complete",
                lamp_audio::playout::OpenedBy::Margin => "margin",
                lamp_audio::playout::OpenedBy::HoldBound => "hold_bound",
            }),
            "scope":if self.start.continuity().is_some() {
                "first_dispatchable_chunk_of_each_generation; a later generation is held again once the previous final block left"
            } else {
                "first_dispatchable_chunk_of_reply; later generations do not rebuffer"
            },
            "opened_at_us":opened.map(|event| event.at_us),
            "age_since_first_audio_us":opened.map(|event| event.held_us),
            "queued_frames_at_open":opened.map(|event| event.buffered_frames),
            "opened_by_complete_flag":opened.map(|event| event.by == lamp_audio::playout::OpenedBy::Complete),
            "included_final_holdback_frames":opened.map(|event| if event.by == lamp_audio::playout::OpenedBy::Complete {0} else {event.buffered_frames.min(240)}),
            "added_latency_us":null,
            "boundary":"coordinator queue condition; not accepted PCM or acoustic onset"
        })
    }

    // Retain at most one full block until generation completion identifies the
    // final block. The trace exposes this holdback separately from provider delay.
    fn next_speaker_chunk(&mut self) -> Option<(Vec<i16>, bool)> {
        // The speaker keeps one final cursor. Serialize playback occurrences
        // through its real retirement receipt instead of overwriting that
        // cursor when the same turn receives another Gemini generation.
        if !self.has_speaker_chunk() {
            return None;
        }
        let final_chunk = self.generation_done && self.pcm.len() <= 240;
        if final_chunk {
            self.start.generation_ended();
        }
        let mut samples = Vec::with_capacity(240);
        for _ in 0..240 {
            match self.pcm.pop_front() {
                Some(sample) => samples.push(sample),
                None => break,
            }
        }
        self.handed += samples.len() as u64;
        samples.resize(240, 0);
        self.voice_level = crate::ring_face::level_of(&samples);
        Some((samples, final_chunk))
    }
    fn held_final_samples(&self) -> usize {
        if self.generation_done {
            0
        } else {
            self.pcm.len().min(240)
        }
    }
    fn delivery_outcome(&self) -> &'static str {
        if matches!(self.cloud.outcome, Some(RequestOutcome::Failed { .. })) {
            "provider_failed"
        } else if !self.had_audio {
            "no_audio_answer"
        } else if self.playback_gaps > 0 {
            "audio_written_with_playback_gaps_unscored"
        } else {
            "audio_written_unscored"
        }
    }
}
struct PendingSpeaker {
    origin: SpeechOrigin,
    sequence: u64,
    permit: OutputPermit,
    accepted: usize,
    final_chunk: bool,
    /// See [`check_speaker_progress`].
    report_due_us: u64,
}

/// The output lease requested for each speaker chunk. Its permit may expire
/// sooner (it never outlives the input lease), never later.
const SPEAKER_PERMIT_LEASE_US: u64 = 100_000;
/// Every worker channel refuses a packet older than this (the age bound in
/// `transport.rs`), so a speaker's report reaches the coordinator within it of
/// being produced.
const SPEAKER_REPORT_AGE_US: u64 = 100_000;

/// A chunk handed to the speaker is settled (written in full, refused or
/// discarded) within its permit's lease, after which the speaker may no
/// longer write it, and the speaker's report then arrives within the channel
/// packet age. A speaker that stays connected but has reported nothing that
/// settles the chunk by then has stopped making progress. This holds under
/// every turn-taking policy; it is checked after each capture block, once
/// that block's policy checks have run, so a stricter policy bound keeps
/// reporting first.
fn check_speaker_progress(pending: Option<&PendingSpeaker>, now_us: u64) -> io::Result<()> {
    match pending {
        Some(pending) if now_us > pending.report_due_us => {
            Err(io::Error::other("speaker acknowledgment expired"))
        }
        _ => Ok(()),
    }
}
struct Rig {
    provider: Worker,
    capture: Worker,
    speaker: Worker,
    privacy: Worker,
    /// The privacy worker reads Controls' switch (`LAMP_PRIVACY_CONTROLS=on`),
    /// or Controls' word that there is none (`no-slide`): a switch it cannot
    /// read closes the cycle as a mute does, for the service to wait and read
    /// again, never ending it (#795).
    privacy_controls: bool,
    /// While a gap in the switch's readings is ridden
    /// (`LAMP_PRIVACY_GAP_RIDE=on`, #2602): the capture's and the camera's
    /// authority aren't renewed, so their leases lapse and capture closes
    /// the microphone as on a stall.
    capture_held: bool,
    /// The Controls route reads a slide (`LAMP_PRIVACY_CONTROLS=on`), so its
    /// observations carry a physical receipt for onboarding's privacy step;
    /// `no-slide` has none to give (#2155's P3).
    privacy_receipts: bool,
    ring: OptionalRing,
    camera: Option<OptionalCamera>,
    motor: Option<OptionalMotor>,
    ring_channel_ceiling: Option<u16>,
    diagnostic_directory: Option<PathBuf>,
    noise_suppression: NoiseSuppression,
    turn_taking: TurnTaking,
    end_silence_blocks: Option<u16>,
    /// The end sent early, after this many blocks of quiet (`--early-end-ms`).
    early_end_blocks: Option<u16>,
    /// A line's words asked for at a pause (`LAMP_SECOND_EARLY_FINALIZE`,
    /// [`early_finalize`]).
    early_finalize: bool,
    /// A sound just after a line that ended before its name goes on from it
    /// (`LAMP_SECOND_OPEN_NAME`, [`open_name`]).
    open_name: bool,
    /// A sound just after a line that was only the lamp's name takes the turn
    /// at once (`LAMP_CALL_TAKES_NEXT_LINE`, [`open_name::called`]).
    call_takes_next: bool,
    /// A due end of the line in hand held while the second route's words so
    /// far end unfinished (`LAMP_SECOND_UNFINISHED_WAIT`, [`crate::unfinished_words`]).
    unfinished_wait: bool,
    faint_level: Option<u8>,
    playout_start: PlayoutStart,
    /// `directed-fixture --fixture-at-ms`: the fixture's own turn, this long
    /// after `listening_ready`, with no admitted turn needed. Diagnostic only.
    fixture_at_ms: Option<u64>,
    /// Which microphone the capture worker reads.
    microphone: crate::options::Microphone,
    /// Opt-in end-of-turn worker client; its process is not a `Rig` child.
    endpoint: Option<EndpointRig>,
    /// `LAMP_CUT_KEEPS_NAME=on` ([`crate::attention::cut_keeps_name`]): a
    /// request that goes on with the one the lamp just ended says so in its
    /// Start (#782).
    cut_keeps_name: bool,
    /// `LAMP_ASKER_FIRST=on` ([`crate::asker_first`]): while an answer is
    /// awaited, speech well under the asker's level takes no turn (#829);
    /// `record` judges it without acting.
    asker_first: crate::asker_first::Mode,
    /// `LAMP_FOLLOWS_ON=on` ([`crate::follows_on`]): a line begun soon after
    /// the lamp's words finished says so in its Start (#829).
    follows_on: bool,
    /// How long an utterance that is only the lamp's name waits for the
    /// rest (`--call-wait-ms`, #782).
    call_wait_blocks: Option<u16>,
    /// Whether the keeper has paused the lamp ([`keeper_pause`]).
    paused: KeeperPause,
    /// The standby voice route, when one is configured (`second.rs`).
    second: Option<second::Route>,
    /// What the voice says of its timing for Expression, given rather than
    /// taken from the environment (`LAMP_EXPRESSION_OUT`): a test's own bus.
    /// `None` makes it from the environment, as the lamp does.
    expression_out: Option<crate::expression_out::ExpressionOut>,
}

/// Whether the keeper has paused the lamp, asked once a loop.
type KeeperPause = Box<dyn FnMut() -> bool>;

/// The voice's one look at `lamp/pause` (`docs/keeper.md`, "Named states
/// and the pause"), read as the body reads it: from the kept value, at most
/// once a second, never waiting longer than a file read. Nothing kept, or a
/// bus it cannot reach, is not paused. Paused, the voice takes no new turn
/// and begins none itself; a turn under way is finished.
fn keeper_pause(bus: io::Result<lamp_bus::Bus>) -> KeeperPause {
    let mut pause = bus.ok().map(lamp_body::pause_in::PauseIn::on);
    Box::new(move || pause.as_mut().is_some_and(|pause| pause.paused()))
}

/// The provider session a turn nobody asked for may begin on: a skill's
/// say, the news, a reminder, the look the lamp said it would take, and the
/// second look. None while the keeper has paused the lamp; otherwise
/// exactly the ready session.
fn unasked_session(cloud: &CloudAvailability, paused: bool) -> Option<u64> {
    cloud.ready_session().filter(|_| !paused)
}

/// The conversation's privacy worker as started: on the Controls route
/// (`controls`), one that ended cleanly before its start was read is an
/// unreadable switch ([`service_privacy::unavailable_io`]), as the wait's
/// is (`service_privacy_controls::started`, #1396). On the direct GPIO
/// route, for a worker that crashed and for any other start failure, the
/// error is as it was.
fn privacy_started(worker: io::Result<Worker>, controls: bool) -> io::Result<Worker> {
    worker.map_err(|error| {
        if controls && Worker::ended_cleanly(&error) {
            service_privacy::unavailable_io(error)
        } else {
            error
        }
    })
}

/// Whether the Controls privacy route's wait owns the ring: with a slide
/// (`on`), whose wait paints the physical-mute indication. A lamp with no
/// slide (`no-slide`) has no LOW to paint, so its ring is as with the route
/// off: the asleep and tap faces, and no indicator worker (#795).
fn privacy_ring(route: Option<crate::privacy_bus::Route>) -> bool {
    route == Some(crate::privacy_bus::Route::Slide)
}

/// Whether the asleep face (#712) may show while the service waits: with its
/// switch on, and never while the Controls privacy route's wait has the ring
/// ([`privacy_ring`]), whose wait paints the physical-mute indication; the
/// ring has one writer (#795).
fn asleep_face_allowed(switch_on: bool, privacy_controls: bool) -> bool {
    switch_on && !privacy_controls
}

/// Whether a tap on the head while the lamp waits out a mute shows the
/// asleep face for about a second (#711, First on #711, 17 Oct 10:18):
/// with head touch, where the asleep face is the muted look (not while the
/// Controls privacy route's wait has the ring, [`privacy_ring`], whose ring
/// is the switch's own indication and has one writer, #755), and not
/// already shown all the time by
/// `LAMP_RING_ASLEEP=on`.
fn tap_face_allowed(head_touch: bool, asleep_on: bool, privacy_controls: bool) -> bool {
    head_touch && !asleep_on && !privacy_controls
}

/// Whether a hand's sleep shows the asleep face while the service waits
/// (#1362, the first builder on #1362, 21 Oct 09:21), and if so whether it
/// yields to a closing slide: with head touch, where the face is not
/// already shown all along by `LAMP_RING_ASLEEP=on`, and on the Controls
/// privacy route too, while its slide is open, giving the ring to the
/// slide's indication when it closes ([`crate::ring_asleep::SleepFace`]).
/// A dark ring reads as idle.
fn sleep_face_allowed(head_touch: bool, asleep_on: bool, privacy_controls: bool) -> Option<bool> {
    (head_touch && (privacy_controls || !asleep_on)).then_some(privacy_controls)
}

/// What the journal says, once, when `LAMP_REMINDER_PULSE=on` has no
/// asleep face to draw on (#1545, the first builder, 18 Oct 18:03): the
/// pulse is drawn over that face, so rather than pulse nothing, it says
/// why there is no pulse.
fn pulse_without_face(
    pulse_on: bool,
    ring: bool,
    asleep_on: bool,
    privacy_controls: bool,
) -> Option<String> {
    let why = match (pulse_on, ring, asleep_on, privacy_controls) {
        (false, ..) | (true, true, true, false) => return None,
        (true, false, ..) => "there is no ring",
        (true, true, false, _) => "LAMP_RING_ASLEEP is off",
        (true, true, true, true) => "the Controls privacy route has the ring while muted",
    };
    Some(format!(
        "lamp-voice: LAMP_REMINDER_PULSE is on but {why}, so an owed reminder does not pulse"
    ))
}

/// Whether an owed reminder's pulse may show on this look of the muted wait
/// (#1545): something `owed`, and the wait the privacy switch's or the
/// person's mute (`Waiting::pulses`: not the restart guard or a switch that
/// can't be read, Thirteen on #1545), and not the sleep a hand held on the
/// head chose, even with the switch closed too, which `Waiting::of` reads
/// first (the reviewer on #1581).
fn pulse_shown(owed: Option<bool>, waiting: crate::bus_out::Waiting, asleep: bool) -> bool {
    owed.is_some() && waiting.pulses() && !asleep
}

/// The privacy wait (`wait`) of one muted cycle, with what the service does
/// meanwhile: each state into the trace and the slide's last state kept;
/// at each look, the reminders, the controls and a tap's face (#711). The
/// face is stopped, black and closed, before this returns. The wait is a
/// parameter so a test can run these very closures without a worker (the
/// reviewer on #1288).
#[cfg(test)]
fn muted_wait<F: FnMut() -> Option<crate::ring_asleep::Asleep>>(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    controls: &RefCell<Option<HandControls>>,
    reminders: &RefCell<Option<crate::reminders::Reminders>>,
    tap_face: Option<crate::ring_asleep::TapFace<F>>,
    health: Option<crate::bus_out::WaitHealth>,
    wait: impl FnOnce(
        &mut dyn FnMut(crate::privacy::PrivacyState, BootId, u64) -> Result<()>,
        &mut dyn FnMut() -> Result<bool>,
    ) -> Result<crate::service_privacy::WaitOutcome>,
) -> Result<crate::service_privacy::WaitOutcome> {
    muted_wait_pulsing(
        trace,
        cues,
        controls,
        reminders,
        tap_face,
        None::<crate::ring_asleep::SleepFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        health,
        None,
        None,
        wait,
    )
}

/// The muted wait, and with `pulse` (the asleep face's [`Asleep::owed`],
/// `LAMP_REMINDER_PULSE=on`) on each look since when a reminder has been
/// owed, while its pulse may show ([`pulse_shown`]). That time is kept for
/// the attempt, from the look that first finds it owed, so a pulse held
/// back for a while neither restarts the half-hour nor comes at once when
/// it may show again (Thirteen and the reviewer on #1581).
///
/// With `light` (`LAMP_LIGHT_WHILE_MUTED=on`, #1816), the person's light is
/// said and taken at each look, with no session's ring worker to do it; it
/// draws nothing, and is let go before this returns, so the next session's
/// ring worker owns it again.
///
/// [`Asleep::owed`]: crate::ring_asleep::Asleep::owed
#[allow(clippy::too_many_arguments)]
fn muted_wait_pulsing<
    F: FnMut() -> Option<crate::ring_asleep::Asleep>,
    G: FnMut() -> Option<crate::ring_asleep::Asleep>,
>(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    controls: &RefCell<Option<HandControls>>,
    reminders: &RefCell<Option<crate::reminders::Reminders>>,
    tap_face: Option<crate::ring_asleep::TapFace<F>>,
    sleep_face: Option<crate::ring_asleep::SleepFace<G>>,
    mut health: Option<crate::bus_out::WaitHealth>,
    pulse: Option<std::sync::Arc<crate::ring_asleep::Owed>>,
    light: Option<crate::ring_light::RingLight>,
    wait: impl FnOnce(
        &mut dyn FnMut(crate::privacy::PrivacyState, BootId, u64) -> Result<()>,
        &mut dyn FnMut() -> Result<bool>,
    ) -> Result<crate::service_privacy::WaitOutcome>,
) -> Result<crate::service_privacy::WaitOutcome> {
    let trace = RefCell::new(trace);
    // The slide's last state from the wait: closed is muted too, whatever
    // the software mute says (the reviewer on #1288).
    let slide = std::cell::Cell::new(None);
    // Since when the switch has read unavailable, for the health's
    // "cannot be read" (#1376): only past the wait's own grace.
    let unread_since = std::cell::Cell::new(None::<u64>);
    let tap_face = RefCell::new(tap_face);
    let sleep_face = RefCell::new(sleep_face);
    let owed_since = std::cell::Cell::new(None::<Instant>);
    let light = RefCell::new(light);
    let outcome = wait(
        &mut |state, boot, at_us| {
            slide.set(Some(state));
            // The slide closed: a hand's sleep's face gives the ring to the
            // slide's indication before the wait starts it (#1362, #795).
            if let Some(face) = sleep_face.borrow_mut().as_mut() {
                face.slide(state);
            }
            unread_since.set(match state {
                crate::privacy::PrivacyState::Unavailable => {
                    Some(unread_since.get().unwrap_or(at_us))
                }
                _ => None,
            });
            let mut trace = trace.borrow_mut();
            record(
                &mut trace,
                json!({"kind":"service_privacy_state", "at_us":at_us,"privacy_worker_session":boot,"state":format!("{state:?}"),"audio_workers_open":false}),
            )?;
            flush_cues(&mut trace, cues);
            Ok(())
        },
        &mut || {
            if let Some(reminders) = reminders.borrow_mut().as_mut() {
                reminders.poll(monotonic_us());
            }
            // The say socket answers through the wait; nothing is said
            // while the microphones are closed (#3697).
            crate::say::Says::refuse_parked("the lamp is muted");
            // The person's light, said and taken; nothing is drawn (#1816).
            if let Some(light) = light.borrow_mut().as_mut() {
                light.tick(monotonic_us());
            }
            let (allowed, asleep, restored) = match &mut *controls.borrow_mut() {
                None => (true, false, false),
                Some(controls) => {
                    let (allowed, observation) = controls.quiescent_observation()?;
                    record_hand_pats(&mut trace.borrow_mut(), observation.pets)?;
                    // A hand's sleep shows the asleep face (#1362), and a
                    // tap's face is not drawn over it.
                    let sleeping = match sleep_face.borrow_mut().as_mut() {
                        Some(face) => {
                            if face.poll(observation.asleep, allowed) {
                                record(
                                    &mut trace.borrow_mut(),
                                    json!({"kind":"head_sleep_face","at_us":monotonic_us()}),
                                )?;
                            }
                            face.showing()
                        }
                        None => false,
                    };
                    answer_muted_tap(
                        tap_face.borrow_mut().as_mut(),
                        &observation,
                        may_hear(allowed, slide.get()),
                        sleeping,
                        &mut trace.borrow_mut(),
                    )?;
                    (allowed, observation.asleep, controls.restored_after_fault())
                }
            };
            let now = monotonic_us();
            let unreadable = unread_since.get().is_some_and(|since| {
                now.saturating_sub(since) >= crate::bus_out::WAIT_UNREADABLE_AFTER_US
            });
            let waiting =
                crate::bus_out::Waiting::of(slide.get(), unreadable, allowed, asleep, restored);
            // A due reminder is kept offered while it waits, and said once
            // the microphones open; meanwhile the journal, the trace and the
            // health say it is owed (#1545). Only while the wait is closed:
            // it also runs at every start while the switch is first read,
            // and a reminder pending then is no mute's (the reviewer on
            // #1551).
            let owed = match reminders.borrow_mut().as_mut() {
                Some(reminders) if waiting.closed() => reminders.owed_while_muted(now),
                _ => None,
            };
            if owed == Some(true) {
                record(
                    &mut trace.borrow_mut(),
                    json!({"kind":"reminder_owed_while_muted", "at_us":now}),
                )?;
            }
            if let Some(pulse) = &pulse {
                owed_since.set(match owed {
                    // Its attempt's first look owed: its due time.
                    Some(true) => Some(Instant::now()),
                    // Still owed: kept, or from now in a new muted cycle.
                    Some(false) => owed_since.get().or_else(|| Some(Instant::now())),
                    // Nothing offered: kept through a lapse of the offer,
                    // and a new attempt's first look starts it again.
                    None => owed_since.get(),
                });
                pulse.tell(
                    owed_since
                        .get()
                        .filter(|_| pulse_shown(owed, waiting, asleep)),
                );
            }
            // No session runs while it waits, so the wait says how the
            // voice is: its microphones off, and why (#1371).
            if let Some(health) = &mut health {
                health.report(now, waiting, owed.is_some());
            }
            Ok(allowed)
        },
    );
    if let Some(face) = tap_face.into_inner() {
        face.stop();
    }
    // A wake leaves the person's light on for the conversation's ring
    // (#1362); anything else ends black.
    if let Some(face) = sleep_face.into_inner() {
        match &outcome {
            Ok(crate::service_privacy::WaitOutcome::Allowed) => face.release(),
            _ => face.stop(),
        }
    }
    // Let go before the next session's ring worker takes the light up.
    drop(light.into_inner());
    // The wait ends on the look that read the switch open, with no look
    // after it: say the voice is opening, not still off, until the
    // session's own report (#1376).
    if let (Ok(crate::service_privacy::WaitOutcome::Allowed), Some(health)) =
        (&outcome, &mut health)
    {
        health.report(monotonic_us(), crate::bus_out::Waiting::Opening, false);
    }
    outcome
}

/// One look of the muted wait at the head (#711): a tap taken while the
/// lamp still may not hear gets the asleep face for about a second, said in
/// the trace as `head_tap_muted_face`. Not while a hand's sleep's face is
/// `sleeping` (#1362): it is shown already, and the ring has one writer.
fn answer_muted_tap<F: FnMut() -> Option<crate::ring_asleep::Asleep>>(
    face: Option<&mut crate::ring_asleep::TapFace<F>>,
    observation: &crate::hand_controls::Observation,
    allowed: bool,
    sleeping: bool,
    trace: &mut SessionState,
) -> Result<()> {
    if !sleeping
        && let Some(face) = face
        && face.poll(observation.head_tap_at_us.is_some(), allowed)
    {
        record(
            trace,
            json!({"kind":"head_tap_muted_face","at_us":monotonic_us()}),
        )?;
    }
    Ok(())
}

/// Whether the lamp may hear again, for the tap's face (#711): the software
/// mute open (`allowed`), and the privacy slide, as the wait last read it,
/// not closed. With the Controls route off the voice reads the slide itself
/// and keeps it apart from the software mute, so a closed slide with the
/// mute open is still a muted lamp (the reviewer on #1288).
fn may_hear(allowed: bool, slide: Option<crate::privacy::PrivacyState>) -> bool {
    allowed && slide != Some(crate::privacy::PrivacyState::Muted)
}

/// The person's light a hand's sleep keeps on, from the bus if there is
/// one (#1362): none means the asleep face. Read on Linux, where the ring is.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn sleep_light(bus: Option<&lamp_bus::Bus>) -> Option<crate::ring_face::Lit> {
    bus.and_then(crate::ring_asleep::persons_light)
}

/// What a hand's sleep shows on the lamp's own ring (Linux): the person's
/// own light, if their ring said one was on, and the asleep face otherwise
/// (#1362, the first builder on #1362, 21 Oct 15:1x).
fn sleeping_face(ceiling: u16) -> Option<crate::ring_asleep::Asleep> {
    #[cfg(target_os = "linux")]
    {
        let bus = lamp_bus::Bus::from_environment().ok();
        crate::ring_asleep::Asleep::sleeping(ceiling, sleep_light(bus.as_ref()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ceiling;
        None
    }
}

/// The asleep face for a tap, on the lamp's own ring (Linux).
fn tapped_face(ceiling: u16) -> Option<crate::ring_asleep::Asleep> {
    #[cfg(target_os = "linux")]
    {
        crate::ring_asleep::Asleep::tapped(ceiling)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ceiling;
        None
    }
}

/// Whether a worker's failure ends the voice. Only the second route's is
/// optional, and only while it stands beside Gemini's (`standby`:
/// `Rig::second` is set), in whichever slot it is (`rig.provider` while it
/// is the active route): one that exited is dropped by
/// `second::dropped_if_gone` (#960). Gemini's worker stays mandatory when
/// it waits in `Route::worker`. The second route alone
/// (`LAMP_VOICE_ROUTE=second`) is the lamp's only voice: its exit ends the
/// voice, as Gemini's does.
fn mandatory(worker: &Worker, standby: bool) -> bool {
    worker.role != "second" || !standby
}

/// A worker's refused authority send, as the session ends on it. On the
/// Controls route a privacy worker that ended cleanly is its switch
/// unreadable, as at its start (`privacy_started`): the microphones stay
/// closed and the wait reads it again, never an exit (lamp-75c9, 21 Oct
/// 22:32:43: the session's new worker had ended on `physical privacy source
/// expired` before the first publish, and `privacy.c.p: Connection refused`
/// ended the voice). Its end is waited for at most [`ENDED_WITHIN`]; one that
/// crashed ends the voice as before. One that hasn't ended by then is still
/// the switch unreadable: it closed its socket as it ends, and on a busy
/// host (a reboot, Controls stopping first: lamp-52e6, 27 Oct 16:37, #3697)
/// its exit comes later. The run's shutdown still verifies that exit within
/// its 300 ms, and a worker it has to force is a failed cleanup that ends
/// the voice.
fn authority_refused(worker: &mut Worker, privacy_controls: bool, error: io::Error) -> io::Error {
    if !privacy_controls || worker.role != "privacy" {
        return error;
    }
    let until = std::time::Instant::now() + ENDED_WITHIN;
    loop {
        match worker.try_exit() {
            Ok(Some(status)) if status.success() => return service_privacy::unavailable_io(error),
            Ok(None) if std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => return service_privacy::unavailable_io(error),
            _ => return error,
        }
    }
}

/// How long a privacy worker whose socket refused is waited for to end
/// ([`authority_refused`]): it closes its socket as it exits.
const ENDED_WITHIN: Duration = Duration::from_millis(100);

impl Rig {
    /// The start horizon of a reply begun now ([`playout_for`]): the route
    /// answering is the provider's slot's.
    fn playout_start_now(&self) -> PlayoutStart {
        playout_for(
            self.playout_start,
            second_playout_start(),
            &self.provider.role,
        )
    }

    /// A hand cancellation has already reached the speaker. Deliver revocation
    /// to every remaining child even if one local channel has gone away.
    fn publish_hand_revocation(&mut self, snapshot: Snapshot) -> io::Result<()> {
        let mut failure = None;
        let standby = self.second.is_some();
        let held = self.capture_held;
        let privacy_controls = self.privacy_controls;
        for worker in [&mut self.capture, &mut self.privacy, &mut self.provider] {
            // A ridden gap's lapse holds here too (#2602, the first
            // builder's P1 on #2623): no fresh lease for capture.
            if held && worker.role == "capture" {
                continue;
            }
            if let Err(error) = worker
                .channels
                .control
                .send(Control::Authority { snapshot })
                && mandatory(worker, standby)
            {
                failure.get_or_insert(authority_refused(worker, privacy_controls, error));
            }
        }
        if let Some(route) = &mut self.second
            && let Err(error) = route.publish(snapshot)
            && mandatory(route.worker_mut(), standby)
        {
            failure.get_or_insert(error);
        }
        if let Some(motor) = &mut self.motor {
            motor.publish(snapshot);
        }
        self.ring.publish(snapshot);
        if let Some(camera) = &mut self.camera
            && !held
        {
            let _ = camera
                .worker
                .channels
                .control
                .send(Control::Authority { snapshot });
        }
        failure.map_or(Ok(()), Err)
    }

    fn publish(&mut self, snapshot: Snapshot) -> io::Result<()> {
        // Motor revocation precedes mandatory audio sends, including their failure.
        if let Some(motor) = &mut self.motor {
            motor.publish(snapshot);
        }
        let standby = self.second.is_some();
        let held = self.capture_held;
        let privacy_controls = self.privacy_controls;
        for worker in [
            &mut self.speaker,
            &mut self.capture,
            &mut self.privacy,
            &mut self.provider,
        ] {
            // A ridden gap lets the capture's lease lapse (#2602).
            if held && worker.role == "capture" {
                continue;
            }
            if let Err(error) = worker
                .channels
                .control
                .send(Control::Authority { snapshot })
                && mandatory(worker, standby)
            {
                return Err(authority_refused(worker, privacy_controls, error));
            }
        }
        if let Some(route) = &mut self.second
            && let Err(error) = route.publish(snapshot)
            && mandatory(route.worker_mut(), standby)
        {
            return Err(error);
        }
        self.ring.publish(snapshot);
        if let Some(camera) = self.camera.as_mut()
            && !held
            && !camera.failed
            && camera
                .worker
                .channels
                .control
                .send(Control::Authority { snapshot })
                .is_err()
        {
            camera.pending_failure = Some("camera authority transport failed");
        }
        Ok(())
    }
    fn running(&mut self) -> io::Result<()> {
        if self.privacy_controls {
            self.privacy
                .check_running()
                .map_err(service_privacy::unavailable_io)?;
        }
        let standby = self.second.is_some();
        for worker in [
            &mut self.provider,
            &mut self.capture,
            &mut self.speaker,
            &mut self.privacy,
        ]
        .into_iter()
        .chain(self.second.as_mut().map(second::Route::worker_mut))
        {
            if mandatory(worker, standby) {
                worker.check_running()?;
            }
        }
        self.ring.check_running();
        Ok(())
    }
    fn shutdown(&mut self, trace: &mut SessionState) -> io::Result<()> {
        // A ring being started again is stopped like any other (#1620).
        let _ = self.ring.settle_restart(trace);
        let grace = if self.diagnostic_directory.is_some() {
            DIAGNOSTIC_SHUTDOWN_GRACE
        } else {
            Duration::from_millis(300)
        };
        let deadline = Instant::now() + grace;
        let stop_requested_at_us = monotonic_us();
        if let Some(motor) = &mut self.motor {
            motor.begin_shutdown();
        }
        let mut camera_watch = self.camera.as_mut().map(|camera| {
            camera.begin_shutdown(stop_requested_at_us);
            for event in camera.shutdown_events.iter().flatten() {
                let _ = record(trace, json!({"kind":"camera_shutdown_receipt", "at_us":monotonic_us(), "details":event,
                    "boundary":"local revocation and driver teardown are separate; physical sensor shutdown unmeasured"}));
            }
            camera.shutdown_watch.take().expect("camera shutdown watch")
        });
        // Send urgent stop to every child before waiting on any one child.
        for worker in [
            &mut self.speaker,
            &mut self.capture,
            &mut self.provider,
            &mut self.privacy,
        ]
        .into_iter()
        .chain(self.ring.iter_mut())
        .chain(self.camera.iter_mut().map(|camera| &mut camera.worker))
        .chain(self.second.as_mut().map(second::Route::worker_mut))
        {
            if let Err(error) = worker.channels.control.send(Control::Stop) {
                let _ = record(
                    trace,
                    json!({"kind":"stop_delivery_error", "worker":worker.role,
                    "at_us":monotonic_us(), "error":error.to_string()}),
                );
            }
        }
        // A test fixture ends its child once every Stop is sent, as
        // `Worker::shutdown` runs it (#1249, #1569): each on a thread of its
        // own, so their children end together, as real workers do.
        #[cfg(test)]
        {
            let ends: Vec<_> = [
                &mut self.speaker,
                &mut self.capture,
                &mut self.provider,
                &mut self.privacy,
            ]
            .into_iter()
            .chain(self.ring.iter_mut())
            .chain(self.camera.iter_mut().map(|camera| &mut camera.worker))
            .chain(self.second.as_mut().map(second::Route::worker_mut))
            .filter_map(Worker::take_on_stop)
            .map(std::thread::spawn)
            .collect();
            for end in ends {
                let _ = end.join();
            }
        }
        let mut failure = ShutdownFailures::default();
        if let Some(error) = self.ring.cleanup_error() {
            failure.push(CleanupStage::RingCleanup, "ring", error);
        }
        let ring_stop_requested_at_us = self.ring.stop_requested_at(stop_requested_at_us);
        let mut exited = vec![
            false;
            4 + usize::from(self.ring.is_some())
                + usize::from(self.camera.is_some())
                + usize::from(self.second.is_some())
        ];
        let mut receipts: [Option<DiagnosticFinished>; 2] = [None, None];
        let mut ring_blank_confirmed = self.ring.blank_confirmed();
        // A ring that ended mid-session was disabled then; its cached exit is
        // seen again here, at once. Say so, or it reads as the stop ending it
        // (lamp-52e6, 19 Oct 21:47, #1620). It still fails the stop below.
        let ring_exited_before_stop = self.ring.exit_observed();
        // On the Controls route the privacy worker ends itself, cleanly, on a
        // switch it cannot read, saying so first: that is why the cycle ended,
        // not a failed shutdown, and its exit status is still checked below
        // (the reviewer on #795, 15:50).
        let privacy_controls = self.privacy_controls;
        let mut final_drain = false;
        loop {
            #[cfg(test)]
            if let Err(error) =
                crate::scenario_clock::step(crate::scenario_clock::Phase::Shutdown { deadline })
            {
                // Fixture failure never bypasses the real supervisor's cleanup.
                failure.push(CleanupStage::Fixture, "session", error);
            }
            if let Some(motor) = &mut self.motor {
                motor.poll(|event| { let _ = record(trace, json!({"kind":"motor_shutdown_receipt","at_us":monotonic_us(),"details":event,"physical_stop_verified":false})); });
            }
            // Keep the 100 ms control channel fresh while independent diagnostic
            // writers finish. A serial wait could age a valid finish receipt.
            for (index, worker) in [
                &mut self.speaker,
                &mut self.capture,
                &mut self.provider,
                &mut self.privacy,
            ]
            .into_iter()
            .chain(self.ring.iter_mut())
            .chain(self.camera.iter_mut().map(|camera| &mut camera.worker))
            .chain(self.second.as_mut().map(second::Route::worker_mut))
            .enumerate()
            {
                for _ in 0..32 {
                    match worker.channels.control.receive::<WorkerEvent>() {
                        Ok(Some(WorkerEvent::DiagnosticsFinished { receipt })) if index < 2 => {
                            if receipts[index].replace(receipt).is_some() {
                                failure.push(
                                    CleanupStage::DiagnosticReceipt,
                                    &worker.role,
                                    io::Error::other("duplicate diagnostic finish receipt"),
                                );
                            }
                        }
                        Ok(Some(WorkerEvent::Ring {
                            report:
                                report @ RingFeedback::Blanked {
                                    reason: RingBlankReason::Shutdown,
                                    ..
                                },
                        })) => {
                            if worker.role != "ring"
                                || ring_blank_confirmed
                                || !report
                                    .confirms_shutdown(ring_stop_requested_at_us, monotonic_us())
                            {
                                failure.push(
                                    CleanupStage::RingReceipt,
                                    &worker.role,
                                    io::Error::other("invalid ring shutdown write receipt"),
                                );
                            } else {
                                ring_blank_confirmed = true;
                            }
                            let _ = record(
                                trace,
                                json!({"kind":"ring_shutdown_write","at_us":monotonic_us(),
                                "details":report,"boundary":"SPI write return; optical darkness unmeasured"}),
                            );
                        }
                        Ok(Some(event)) => {
                            let unreadable = privacy_controls
                                && worker.role == "privacy"
                                && matches!(&event, WorkerEvent::Fault { code }
                                    if code.starts_with(crate::privacy_bus::UNREADABLE));
                            if !unreadable
                                && matches!(
                                    event,
                                    WorkerEvent::Fault { .. } | WorkerEvent::ReferenceFault { .. }
                                )
                            {
                                let stage = if matches!(event, WorkerEvent::ReferenceFault { .. }) {
                                    CleanupStage::ReferenceFault
                                } else {
                                    CleanupStage::WorkerFault
                                };
                                failure.push(
                                    stage,
                                    &worker.role,
                                    io::Error::other(format!(
                                        "{} failed during shutdown: {event:?}",
                                        worker.role
                                    )),
                                );
                            }
                            let _ = record(
                                trace,
                                json!({"kind":"worker_shutdown_event", "worker":worker.role, "at_us":monotonic_us(), "details":event}),
                            );
                        }
                        Ok(None) => break,
                        Err(error) => {
                            // What it queued before ending cleanly may have
                            // aged out on the way; its exit, read below
                            // within the grace, is the check. Its socket
                            // closes before that exit can be read, so the
                            // receive error alone is no failure (lamp-52e6's
                            // reboot, 27 Oct 16:37: `Transport endpoint is
                            // not connected`, #3697).
                            let ends_itself = privacy_controls && worker.role == "privacy";
                            if !ends_itself {
                                failure.push(CleanupStage::ControlReceive, &worker.role, error);
                            }
                            break;
                        }
                    }
                }
                if worker.role == "camera" {
                    for _ in 0..32 {
                        match worker.channels.data.receive::<CameraEvent>() {
                            Ok(Some(event)) => {
                                if let Err(error) = camera_watch
                                    .as_mut()
                                    .expect("camera watch")
                                    .observe(&event, monotonic_us())
                                {
                                    failure.push(CleanupStage::CameraObserve, &worker.role, error);
                                }
                                if matches!(
                                    event,
                                    CameraEvent::Revoked { .. } | CameraEvent::Stopped { .. }
                                ) {
                                    let _ = record(
                                        trace,
                                        json!({"kind":"camera_shutdown_receipt", "at_us":monotonic_us(), "details":event}),
                                    );
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                let _ = record(
                                    trace,
                                    json!({"kind":"camera_shutdown_channel_failed", "at_us":monotonic_us(), "error":error.to_string()}),
                                );
                                failure.push(CleanupStage::CameraReceive, &worker.role, error);
                                break;
                            }
                        }
                    }
                }
                if !exited[index] {
                    match worker.try_exit() {
                        Ok(Some(status)) => {
                            exited[index] = true;
                            if let Err(error) = worker.exit_result(status) {
                                let _ = record(
                                    trace,
                                    json!({"kind":"worker_exit_failed", "worker":worker.role,
                                    "at_us":monotonic_us(), "error":error.to_string(),
                                    "exited_before_stop":worker.role == "ring" && ring_exited_before_stop}),
                                );
                                failure.push(CleanupStage::WorkerExit, &worker.role, error);
                            }
                        }
                        Ok(None)
                            if worker.role == "camera"
                                && monotonic_us()
                                    >= camera_watch
                                        .as_ref()
                                        .expect("camera watch")
                                        .deadline_us() =>
                        {
                            exited[index] = true;
                            let error = worker.terminate_and_reap(
                                io::ErrorKind::TimedOut,
                                "camera local-revoke/driver-teardown deadline expired".into(),
                            );
                            failure.push(CleanupStage::ForcedStop, &worker.role, error);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            exited[index] = true;
                            let error = worker.terminate_and_reap(
                                io::ErrorKind::Other,
                                format!("shutdown status failed: {error}"),
                            );
                            failure.push(CleanupStage::ForcedStop, &worker.role, error);
                        }
                    }
                }
            }
            if exited.iter().all(|done| *done)
                && self.motor.as_ref().is_none_or(OptionalMotor::reaped)
            {
                // A final receipt may arrive between the earlier receive and
                // waitpid. Re-drain once after every sender has definitely exited.
                if final_drain {
                    break;
                }
                final_drain = true;
                continue;
            }
            if Instant::now() >= deadline {
                for (index, worker) in [
                    &mut self.speaker,
                    &mut self.capture,
                    &mut self.provider,
                    &mut self.privacy,
                ]
                .into_iter()
                .chain(self.ring.iter_mut())
                .chain(self.camera.iter_mut().map(|camera| &mut camera.worker))
                .chain(self.second.as_mut().map(second::Route::worker_mut))
                .enumerate()
                {
                    if !exited[index] {
                        // Reaping an earlier worker can take time. A later
                        // worker may have exited since this pass last polled it.
                        let error = match worker.try_exit() {
                            Ok(Some(status)) => {
                                if let Err(error) = worker.exit_result(status) {
                                    let _ = record(
                                        trace,
                                        json!({"kind":"worker_exit_failed", "worker":worker.role,
                                        "at_us":monotonic_us(), "error":error.to_string()}),
                                    );
                                    failure.push(CleanupStage::WorkerExit, &worker.role, error);
                                }
                                continue;
                            }
                            Ok(None) => worker.terminate_and_reap(io::ErrorKind::TimedOut,
                                format!("did not stop within shared {} ms deadline; forced termination required", grace.as_millis())),
                            Err(error) => worker.terminate_and_reap(
                                io::ErrorKind::Other,
                                format!("shutdown status failed at deadline: {error}"),
                            ),
                        };
                        let _ = record(
                            trace,
                            json!({"kind":"worker_forced_shutdown", "worker":worker.role,
                            "at_us":monotonic_us(), "error":error.to_string()}),
                        );
                        failure.push(CleanupStage::ForcedStop, &worker.role, error);
                    }
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if let Some(motor) = &self.motor {
            let cleanup = motor.finish();
            let _ = record(
                trace,
                json!({"kind":"motor_cleanup","at_us":monotonic_us(),"worker":motor.identity().map(|i|i.worker),"started":motor.started(),"reaped":motor.reaped(),"confirmed":cleanup.is_ok(),"error":cleanup.as_ref().err().map(ToString::to_string),"physical_stop_verified":false}),
            );
            if let Err(error) = cleanup {
                failure.push(CleanupStage::Motor, "session", error);
            }
        }
        // Mandatory hardware cleanup preceded bounded metadata reads. Native kill/wait
        // has no userspace hard deadline; its result is never hidden as success.
        if !ring_blank_confirmed {
            failure.push(
                CleanupStage::RingBlank,
                "ring",
                io::Error::other("ring shutdown has no verified black-write receipt"),
            );
        }
        if let Some(watch) = camera_watch
            && let Err(error) = watch.finish()
        {
            failure.push(CleanupStage::CameraFinish, "camera", error);
        }
        // The endpoint client closed when the session's detector dropped. Its
        // loss while running only meant the silence fallback, but a worker
        // whose exit is not confirmed must keep this cleanup from succeeding,
        // so no later runtime can start beside it.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if let Some((event, teardown)) = self
            .endpoint
            .take()
            .and_then(|endpoint| endpoint.finish(remaining.max(Duration::from_millis(500))))
        {
            let _ = record(trace, event);
            if let Err(error) = teardown {
                failure.push(CleanupStage::Endpoint, "session", error);
            }
        }
        if let Some(directory) = &self.diagnostic_directory {
            for (index, worker, leaf) in [
                (0, &self.speaker, "render-audio"),
                (1, &self.capture, "capture-audio"),
            ] {
                let certification = worker
                    .diagnostic_start
                    .ok_or_else(|| io::Error::other("missing diagnostic start identity"))
                    .and_then(|started| {
                        diagnostic_control::certify(
                            &directory.join(leaf),
                            started,
                            receipts[index].as_ref(),
                        )
                    });
                let _ = record(
                    trace,
                    json!({"kind":"audio_diagnostics_certified", "worker":worker.role,
                    "worker_boot":worker.boot, "at_us":monotonic_us(), "valid":certification.is_ok(),
                    "receipt":receipts[index], "error":certification.as_ref().err().map(ToString::to_string)}),
                );
                if let Err(error) = certification {
                    // Certification adds evidence; it never replaces worker teardown causes.
                    failure.push(CleanupStage::Certification, &worker.role, error);
                }
            }
        } else if receipts.iter().any(Option::is_some) {
            failure.push(
                CleanupStage::Unsolicited,
                "session",
                io::Error::other("unsolicited audio diagnostics receipt"),
            );
        }
        failure.finish()
    }
}

#[derive(Clone, Copy)]
enum ProviderSource<'a> {
    Gemini(&'a Path),
    /// `at_ms`: `--fixture-at-ms`, the diagnostic timed start.
    Fixture {
        wav: &'a Path,
        sha256: &'a str,
        at_ms: Option<u64>,
    },
}
impl ProviderSource<'_> {
    fn arguments(self) -> io::Result<Vec<String>> {
        let text = |path: &Path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("provider source path must be UTF-8"))
        };
        match self {
            Self::Gemini(config) => Ok(vec![text(config)?]),
            Self::Fixture { wav, sha256, at_ms } => {
                let mut arguments = vec!["--fixture".into(), text(wav)?, sha256.into()];
                if at_ms.is_some() {
                    arguments.push(crate::fixture_provider::TIMED_FLAG.into());
                }
                Ok(arguments)
            }
        }
    }
}

/// One real admitted input may receive one immutable cached reply. This finite
/// experiment uses normal privacy, ownership, speaker permits and AEC reference.
pub fn run_fixture(
    wav: &Path,
    sha256: &str,
    seconds: u64,
    output: &Path,
    options: FixtureOptions,
) -> Result<()> {
    let termination = Termination::install()?;
    if !options.directed.diagnostics {
        return Err(io::Error::other("fixture experiments require explicit --diagnostics").into());
    }
    let info = Fixture::load(wav, sha256)?.info;
    let cues = options
        .cue_socket
        .as_deref()
        .map(CueSink::connect)
        .transpose()?;
    run_source(
        ProviderSource::Fixture {
            wav,
            sha256,
            at_ms: options.at_ms,
        },
        Lifetime::Finite(seconds),
        output,
        options.directed,
        None,
        None,
        None,
        Some(info),
        &ControlsOptions::default(),
        cues,
        &termination,
    )
}

pub fn run_directed(config: &Path, seconds: u64, output: &Path) -> Result<()> {
    run_directed_with_diagnostics(config, seconds, output, false)
}

pub fn run_directed_with_diagnostics(
    config: &Path,
    seconds: u64,
    output: &Path,
    diagnostics: bool,
) -> Result<()> {
    run_directed_with_options(
        config,
        seconds,
        output,
        DirectedOptions {
            diagnostics,
            ..DirectedOptions::default()
        },
    )
}

pub fn run_directed_with_options(
    config: &Path,
    seconds: u64,
    output: &Path,
    options: DirectedOptions,
) -> Result<()> {
    run_directed_session(
        config,
        seconds,
        output,
        SessionOptions {
            directed: options,
            cue_socket: None,
            camera_config: None,
            motor: None,
            failure_notices: None,
            controls: ControlsOptions::default(),
        },
    )
}

/// Observe the same directed conversation without changing its admission,
/// ownership or audio policy. The optional sink never waits for its consumer.
pub fn run_directed_session(
    config: &Path,
    seconds: u64,
    output: &Path,
    options: SessionOptions,
) -> Result<()> {
    let termination = Termination::install()?;
    let cues = options
        .cue_socket
        .as_deref()
        .map(CueSink::connect)
        .transpose()?;
    run_source(
        ProviderSource::Gemini(config),
        Lifetime::Finite(seconds),
        output,
        options.directed,
        options.camera_config.as_deref(),
        options.motor.as_ref(),
        options.failure_notices.as_ref(),
        None,
        &options.controls,
        cues,
        &termination,
    )
}

/// Continuous conversation, with bounded best-effort metadata. It does not
/// relax privacy, worker, provider or per-turn permission/watchdog checks.
/// The provider refuses for now: its quota or its rate limit, or the lamp's
/// key (#2231). The run ended in order like any other terminal end.
#[derive(Debug)]
pub struct ProviderRefusesForNow(pub crate::provider_worker::Fault);
impl std::fmt::Display for ProviderRefusesForNow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "terminal provider output settled; the provider refuses for now (quota, rate limit or key); service stopped without retry",
        )
    }
}
impl std::error::Error for ProviderRefusesForNow {}

/// How long the service stays before it leaves after such a refusal. Whoever
/// restarts it would ask the provider again at once: on lamp-4ace on
/// 2026-10-13 systemd did, every five seconds, about 380 times in half an
/// hour of a spent quota.
const REFUSAL_WAIT: Duration = Duration::from_secs(120);
/// Beside the sessions: the last run ended on a refused key (#2231). A
/// second such end within [`KEY_REFUSED_AGAIN_WITHIN`] waits; any other end
/// removes it.
const KEY_REFUSED_STAMP: &str = ".key-refused";
const KEY_REFUSED_AGAIN_WITHIN: Duration = Duration::from_secs(600);

pub fn run_serve(config: &Path, output: &Path, options: SessionOptions) -> Result<()> {
    options.validate_serve()?;
    // Which route is the voice: refused here, before anything starts (the
    // signal handler, the hand controls, the session folder, any worker),
    // when `LAMP_VOICE_ROUTE` is wrong or `second` lacks its configuration,
    // and so is a wrong `LAMP_SECOND_*` switch. Each run reads them again as
    // it starts its workers (`spawn_rig`), after the session folder: this
    // early refusal is held by `run_serve_refuses_a_wrong_route_before_anything_starts`.
    // An update round brings this binary and the second route's unit but
    // never `/etc/lampOS` (docs/updates.md), so the product's second.json is
    // placed here when the unit asks for it at the product's path and it is
    // not there (#1940). A failure is said, and the start goes on to refuse
    // or run as before.
    match crate::second_config::place_product_if_missing(
        std::env::var_os(second::ROUTE_ENV).as_deref(),
        std::env::var_os(crate::second_config::SECOND_CONFIG_ENV).as_deref(),
    ) {
        Ok(true) => eprintln!(
            "lamp-voice: the second route's configuration was missing: placed the product's at {} (install's own)",
            crate::second_config::PRODUCT_PATH
        ),
        Ok(false) => {}
        Err(error) => eprintln!(
            "lamp-voice: the product's second route configuration could not be placed at {}: {error}",
            crate::second_config::PRODUCT_PATH
        ),
    }
    if let second::Routes::SecondOnly(_) = second::routes_from_environment(true)? {
        eprintln!("{}", second::SECOND_ONLY_LINE);
    }
    second::route_flags_from_environment()?;
    let termination = Termination::install()?;
    let cues = options
        .cue_socket
        .as_deref()
        .map(CueSink::connect)
        .transpose()?;
    let result = run_source(
        ProviderSource::Gemini(config),
        Lifetime::Serve,
        output,
        options.directed,
        options.camera_config.as_deref(),
        options.motor.as_ref(),
        options.failure_notices.as_ref(),
        None,
        &options.controls,
        cues,
        &termination,
    );
    // Everything is shut down and recorded by now. What is left is not to
    // come straight back; a request to stop ends the wait at once.
    let refused = result
        .as_ref()
        .err()
        .and_then(|error| error.downcast_ref::<ProviderRefusesForNow>())
        .map(|refused| refused.0);
    let stamp = output.parent().map(|folder| folder.join(KEY_REFUSED_STAMP));
    let wait = refusal_waits(refused, stamp.as_deref());
    if wait {
        eprintln!(
            "lamp-voice: the provider refuses for now (quota, rate limit or key); staying {} s before leaving, less if the voice's settings change",
            REFUSAL_WAIT.as_secs()
        );
        // A key put right meanwhile (setup, the app's own keys) ends the
        // wait: the settings folder changes when a file in it is replaced.
        let settings = config.parent().map(Path::to_owned);
        let changed_at = |folder: &Option<PathBuf>| {
            folder.as_deref().and_then(|folder| {
                std::fs::metadata(folder)
                    .and_then(|found| found.modified())
                    .ok()
            })
        };
        let before = changed_at(&settings);
        let until = Instant::now() + REFUSAL_WAIT;
        while Instant::now() < until
            && termination.signal_name().is_none()
            && changed_at(&settings) == before
        {
            std::thread::sleep(Duration::from_millis(200));
        }
    } else if refused.is_some() {
        eprintln!(
            "lamp-voice: the provider refused this lamp's key; leaving now to ask once more, and waiting {} s if it refuses again",
            REFUSAL_WAIT.as_secs()
        );
    }
    result
}

/// Whether a run that ended so stays [`REFUSAL_WAIT`] before it leaves, with
/// the refused-key stamp kept beside the sessions. A spent quota or a rate
/// limit waits at once. A refused key waits only the second time in a row
/// within [`KEY_REFUSED_AGAIN_WITHIN`] (the reviewer-lead on #2459): one
/// 401/403 from a proxy, or a key being changed, is asked again after the
/// usual restart. Any other end removes the stamp.
fn refusal_waits(refused: Option<crate::provider_worker::Fault>, stamp: Option<&Path>) -> bool {
    use crate::provider_worker::Fault;
    match refused {
        Some(Fault::Authentication) => {
            let again = stamp.is_some_and(|stamp| {
                std::fs::metadata(stamp)
                    .and_then(|found| found.modified())
                    .ok()
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age < KEY_REFUSED_AGAIN_WITHIN)
            });
            if !again && let Some(stamp) = stamp {
                let _ = std::fs::write(stamp, b"");
            }
            again
        }
        Some(_) => true,
        None => {
            if let Some(stamp) = stamp {
                let _ = std::fs::remove_file(stamp);
            }
            false
        }
    }
}

/// The first record of a run: what it was asked to do.
fn run_start_record(
    options: &DirectedOptions,
    lifetime: Lifetime,
    camera_requested: bool,
    motor_requested: bool,
    fixture: Option<&FixtureInfo>,
    fixture_at_ms: Option<u64>,
    cue_requested: bool,
) -> Value {
    let mut run_start = json!({"kind":"run_start","at_us":monotonic_us(),"scope":if options.ring_channel_ceiling.is_some(){"directed_voice_with_ring"}else{"directed_voice_only"},"seconds":lifetime.seconds(),"mode":if lifetime.continuous(){"serve"}else{"finite"},"acoustic_score":null,"audio_diagnostics_requested":options.diagnostics,
        "ring_channel_ceiling_requested":options.ring_channel_ceiling,
        "camera_context_requested":camera_requested,
        "motor_worker_requested":motor_requested,
        "microphone":options.microphone,
        "capture_device_requested":options.microphone.device(),
        "software_processing_requested":options.noise_suppression.software_processing(),
        "playout_start_horizon_us":options.playout_start.as_micros(),
        "playout_continuity":options.playout_start.policy().map(|policy| json!({"margin_us":policy.margin_us,"hold_us":policy.hold_us,"cover_us":policy.cover_us})),
        "turn_taking":options.turn_taking,"turn_taking_tail_us":if matches!(options.turn_taking, TurnTaking::V1Baseline | TurnTaking::Duplex) { Some(crate::turn_taking::TAIL_US) } else { None },
        "reference_clock":options.reference_clock,
        "reference_recovery":options.reference_recovery,
        "echo_reference":options.echo_reference,
        "provider_continuation":options.provider_continuation,
        "provider_kind":if fixture.is_some(){"one_cached_reply"}else{"gemini"},
        "fixture":fixture,"cue_requested":cue_requested});
    // Without the option the record keeps exactly its existing fields.
    if let Some(blocks) = options.early_end_blocks {
        run_start["early_end_ms"] = json!(u64::from(blocks) * 10);
    }
    // `--fixture-at-ms`: a take whose time never came still says it was timed.
    if let Some(ms) = fixture_at_ms {
        run_start["fixture_at_ms"] = json!(ms);
    }
    run_start
}

#[allow(clippy::too_many_arguments)]
fn run_source(
    source: ProviderSource<'_>,
    lifetime: Lifetime,
    output: &Path,
    options: DirectedOptions,
    camera_config: Option<&Path>,
    motor_options: Option<&crate::options::MotorOptions>,
    notice_options: Option<&crate::options::NoticeOptions>,
    fixture: Option<FixtureInfo>,
    controls_options: &ControlsOptions,
    mut cues: Option<CueSink>,
    termination: &Termination,
) -> Result<()> {
    // With no slide the software mute alone closes the microphones: refused
    // before anything starts when it has no persisted record.
    crate::privacy_bus::refuse_no_slide_without_mute(
        crate::privacy_bus::from_environment()?.map(|(_, route)| route),
        controls_options.mute_record.is_some(),
    )?;
    lifetime.validate()?;
    let session_reader = crate::session_reader::startup(lifetime.continuous(), output);
    // Restore the software latch before any worker, including metadata/GPIO.
    let controls = RefCell::new(HandControls::restore(controls_options)?);
    // One optional storage/IPC process across every privacy cycle. Its failure
    // only withdraws reminder authority; healthy capture and conversation stay.
    let reminders = RefCell::new(match crate::reminders::Reminders::from_environment() {
        Ok(reminders) => reminders,
        Err(_) => {
            eprintln!("lamp-voice: Schedule consumer unavailable; reminders remain pending");
            None
        }
    });
    if lifetime.continuous() && options.diagnostics {
        return Err(io::Error::other("serve does not allow raw audio diagnostics").into());
    }
    if let Some(ceiling) = options.ring_channel_ceiling {
        lamp_ring::ChannelCeiling::new(ceiling).map_err(io::Error::other)?;
    }
    termination.check()?;
    refuse_legacy_owners()?;
    fs::create_dir(output)?;
    fs::set_permissions(output, fs::Permissions::from_mode(0o700))?;
    if lifetime.continuous() {
        crate::marked::session_opened(output);
    }
    let mut trace = if lifetime.continuous() {
        match session_reader {
            Some(reader) => SessionState::service_with_reader(output, reader),
            None => SessionState::service(output),
        }
    } else {
        SessionState::finite()
    };
    let mut run_start = run_start_record(
        &options,
        lifetime,
        camera_config.is_some(),
        motor_options.is_some(),
        fixture.as_ref(),
        match source {
            ProviderSource::Fixture { at_ms, .. } => at_ms,
            ProviderSource::Gemini(_) => None,
        },
        cues.is_some(),
    );
    // Which route answers, when it is the second alone; a wrong value is
    // refused when the workers start (`spawn_rig`).
    if let Ok(routes) = second::routes_from_environment(
        lifetime.continuous() && matches!(source, ProviderSource::Gemini(_)),
    ) {
        routes.mark(&mut run_start);
    }
    record(&mut trace, run_start)?;
    // Bounded local reads happen once before any capture/speaker worker.
    let notices = FailureNotices::load(notice_options);
    // The say service listens from the start, through the privacy wait,
    // not only once a run has opened the microphones (#3697).
    if lifetime.continuous() {
        crate::say::Says::start(notices.recordings());
    }
    let mut motor_rearm = true;
    record(
        &mut trace,
        json!({"kind":"failure_notice_capability","at_us":monotonic_us(),
        "capability":notices.capability(),"automatic_synthesis":false}),
    )?;
    if controls_options.enabled() {
        record(
            &mut trace,
            json!({"kind":"hand_controls_policy","at_us":monotonic_us(),
            "stop":controls_options.stop,"software_mute":controls_options.mute_record.is_some(),
            "clear":"fresh second local hold, durably saved before physical privacy checks",
            "observation_to_speaker_target_us":2000,"acoustic_stop_measured":false}),
        )?;
        let restored = controls
            .borrow()
            .as_ref()
            .is_some_and(HandControls::restored_after_fault);
        if restored {
            // #960: the mute is the restart guard's, after a fault, not the
            // person's choice. Said once here; the wait shows nothing yet.
            eprintln!("lamp-voice: muted by the restart guard after a fault, not by choice");
            record(
                &mut trace,
                json!({"kind":"mute_restored_after_fault","at_us":monotonic_us()}),
            )?;
        }
    }
    let result = (|| {
        let diagnostic_directory = options
            .diagnostics
            .then(|| fs::canonicalize(output))
            .transpose()?;
        if lifetime.continuous() {
            let executable = std::env::current_exe()?;
            // While it waits out a mute or a closed privacy switch, a lamp
            // with a ring shows the asleep face (#712, `LAMP_RING_ASLEEP=on`).
            // Not with the Controls privacy route's slide: its wait paints the
            // physical-mute indication, and the ring has one writer (#795).
            // With no slide there is no indication, and the faces stay.
            let privacy_controls = privacy_ring(
                crate::privacy_bus::from_environment()
                    .ok()
                    .flatten()
                    .map(|(_, route)| route),
            );
            let asleep_ceiling = options.ring_channel_ceiling.filter(|_| {
                asleep_face_allowed(
                    crate::ring_asleep::enabled_from_environment(),
                    privacy_controls,
                )
            });
            // A tap on the head while it waits gets the asleep face for
            // about a second (#711), where that is the muted look and it is
            // not already shown.
            let tap_face_ceiling = options.ring_channel_ceiling.filter(|_| {
                tap_face_allowed(
                    crate::hand_controls::head_touch_from_environment(),
                    crate::ring_asleep::enabled_from_environment(),
                    privacy_controls,
                )
            });
            // A hand's sleep shows the asleep face where it is not shown
            // all along, on the privacy route too while its slide is open
            // (#1362).
            let sleep_face_ceiling = options.ring_channel_ceiling.zip(sleep_face_allowed(
                crate::hand_controls::head_touch_from_environment(),
                crate::ring_asleep::enabled_from_environment(),
                privacy_controls,
            ));
            if let Some(line) = pulse_without_face(
                crate::ring_asleep::pulse_enabled_from_environment(),
                options.ring_channel_ceiling.is_some(),
                crate::ring_asleep::enabled_from_environment(),
                privacy_controls,
            ) {
                eprintln!("{line}");
            }
            service::supervise(
                &mut trace,
                &mut cues,
                termination,
                |trace, cues| {
                    #[cfg(target_os = "linux")]
                    let asleep = asleep_ceiling.and_then(crate::ring_asleep::Asleep::start);
                    #[cfg(not(target_os = "linux"))]
                    let _ = asleep_ceiling;
                    let tap_face = tap_face_ceiling.map(|ceiling| {
                        crate::ring_asleep::TapFace::new(move || tapped_face(ceiling))
                    });
                    let sleep_face = sleep_face_ceiling.map(|(ceiling, yields)| {
                        crate::ring_asleep::SleepFace::new(move || sleeping_face(ceiling), yields)
                    });
                    // A reminder owed while it waits pulses on the asleep
                    // face (#1545, `LAMP_REMINDER_PULSE=on`).
                    #[cfg(target_os = "linux")]
                    let pulse = asleep
                        .as_ref()
                        .filter(|_| crate::ring_asleep::pulse_enabled_from_environment())
                        .map(crate::ring_asleep::Asleep::owed);
                    #[cfg(not(target_os = "linux"))]
                    let pulse = None;
                    let outcome = muted_wait_pulsing(
                        trace,
                        cues,
                        &controls,
                        &reminders,
                        tap_face,
                        sleep_face,
                        crate::bus_out::WaitHealth::from_environment(),
                        pulse,
                        crate::ring_light::while_muted_from_environment(
                            options.ring_channel_ceiling.is_some(),
                        ),
                        |on_state, gate| {
                            crate::service_privacy_controls::wait_selected(
                                &executable,
                                termination,
                                on_state,
                                gate,
                            )
                        },
                    );
                    // Black and the device closed before a cycle's ring
                    // worker can open it.
                    #[cfg(target_os = "linux")]
                    {
                        if let Some(asleep) = asleep {
                            asleep.stop();
                        }
                    }
                    outcome
                },
                |trace, cues| {
                    run_inner(
                        source,
                        lifetime,
                        trace,
                        None,
                        options,
                        camera_config,
                        motor_options,
                        &mut motor_rearm,
                        &notices,
                        cues,
                        termination,
                        &mut controls.borrow_mut(),
                        &mut reminders.borrow_mut(),
                    )
                },
            )
        } else {
            run_inner(
                source,
                lifetime,
                &mut trace,
                diagnostic_directory.as_deref(),
                options,
                camera_config,
                motor_options,
                &mut motor_rearm,
                &notices,
                &mut cues,
                termination,
                &mut controls.borrow_mut(),
                &mut reminders.borrow_mut(),
            )
        }
    })();
    // This runs even after a startup or teardown fault. A previously armed
    // recovery record already inhibits restart if this update cannot be saved.
    let persistence = controls
        .borrow_mut()
        .as_mut()
        .map_or(Ok(()), HandControls::persist_muted);
    let result = service_privacy::finish(result, persistence);
    crate::say::Says::stop();
    finish_run(output, &mut trace, &mut cues, result, termination)
}

fn finish_run(
    output: &Path,
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    result: Result<()>,
    termination: &Termination,
) -> Result<()> {
    if trace.continuous() {
        record(
            trace,
            json!({"kind":"run_end","at_us":monotonic_us(),
            "status":if result.is_ok(){"stopped_unscored"}else{"failed"},
            "error":result.as_ref().err().map(ToString::to_string),
            "termination_signal":termination.signal_name(),"complete_evidence":false}),
        )?;
        flush_cues(trace, cues);
        // Hardware/authority cleanup completed before this bounded optional wait.
        let health = trace.finish_logging();
        println!(
            "{}",
            json!({"kind":"service_metadata_finished","health":health,"output":output,"complete_evidence":false})
        );
        return result;
    }
    let result = result.and_then(|()| termination.check().map_err(Into::into));
    let end_index = trace.len();
    trace.push(json!({"kind":"run_end","at_us":monotonic_us(),"status":if result.is_ok(){"completed_unscored"}else{"failed"},"error":result.as_ref().err().map(ToString::to_string),"termination_signal":termination.signal_name()}));
    flush_cues(trace, cues);
    trace[end_index]["cue_valid"] = cues.as_ref().map(|sink| sink.fault().is_none()).into();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output.join("events.jsonl"))?;
    for event in trace.iter() {
        serde_json::to_writer(&mut file, &event)?;
        writeln!(file)?;
    }
    file.sync_all()?;
    result
}

fn flush_cues(trace: &mut SessionState, cues: &mut Option<CueSink>) {
    trace.flush(cues, monotonic_us());
}

fn refuse_legacy_owners() -> io::Result<()> {
    // This is a coexistence guard for the test board, not a legacy runtime
    // dependency. An absent unit is fine; a running writer forbids this run.
    for unit in [
        "hal.service",
        "os-server.service",
        "led-boot.service",
        "led-shutdown.service",
    ] {
        let status = Command::new("/usr/bin/systemctl")
            .args(["is-active", "--quiet", unit])
            .status()?;
        if status.success() {
            return Err(io::Error::other(format!(
                "{unit} still owns the test device; explicit exclusive cutover required"
            )));
        }
        if !matches!(status.code(), Some(3) | Some(4)) {
            return Err(io::Error::other("cannot verify legacy service ownership"));
        }
    }
    Ok(())
}

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}
/// How the last two seconds of 10 ms blocks were judged, for a record:
/// the detector's score in twentieths, the level on the face's scale, and
/// how many the detector scored as speech and the faint floor turned away.
/// Numbers about the sound, and nothing of the sound itself.
fn last_blocks(judged: &VecDeque<(u8, u8, bool)>) -> Value {
    json!({
        "score_x20": judged.iter().map(|block| block.0).collect::<Vec<_>>(),
        "level": judged.iter().map(|block| block.1).collect::<Vec<_>>(),
        "under_floor": judged.iter().filter(|block| block.2 && block.0 >= 10).count(),
    })
}

/// The onboarding walk's part of a physical mute: its close candidate, and
/// its records (the camera ask among them) written to the trace now, since
/// the mute ends the cycle before the loop would take them. Memory only.
fn mute_onboarding(
    trace: &mut SessionState,
    walk: Option<&mut crate::onboarding::Onboarding>,
    proof: Option<&crate::wire::PrivacyReceipt>,
    at_us: u64,
    eyes: Option<&mut crate::body_link::EyesLink>,
) -> Result<Option<crate::onboarding::privacy::Checkpoint>> {
    let (Some(walk), Some(proof)) = (walk, proof) else {
        return Ok(None);
    };
    let (checkpoint, records) = walk.take_close_candidate_telling(proof, at_us, eyes);
    for line in records {
        record(trace, line)?;
    }
    Ok(checkpoint)
}

fn record(trace: &mut SessionState, event: Value) -> Result<()> {
    trace.record(event).map_err(Into::into)
}

/// Admit and record one provider offer; preparation grants no playback authority.
fn handle_requested_audio(
    reply: &mut Option<Reply>,
    service: Option<&crate::requested_audio::Service>,
    worker: BootId,
    snapshot: Snapshot,
    (request, session): (u64, u64),
    offer: lamp_msgs::RequestedAudioOffer,
    trace: &mut SessionState,
) -> Result<()> {
    // Why a refused offer was refused, in the trace and once
    // in the journal (#2793: a refusal said nothing).
    let current = reply
        .as_mut()
        .filter(|current| current.owner.turn() == request && current.cloud.session == session);
    let refused = match (service, current) {
        (None, _) => Some("requested audio is off or unavailable"),
        (_, None) => Some("not the turn in hand"),
        (Some(service), Some(current)) => {
            match current.requested_audio_refusal(worker, snapshot, &offer) {
                Some(why) => Some(why),
                None => (!current.accept_requested_audio(service, worker, snapshot, offer))
                    .then_some("its recording could not be prepared"),
            }
        }
    };
    let accepted = refused.is_none();
    if let Some(why) = refused {
        eprintln!("lamp-voice: request {request}: requested audio refused: {why}");
    }
    record(
        trace,
        json!({"kind":"requested_audio_offered",
        "turn":request,"provider_session":session,"accepted":accepted,"refused":refused,
        "at_us":monotonic_us(),"boundary":"queued preparation; no playback authority"}),
    )
}

fn link_trace(link: ProviderLink, received_at_us: u64) -> Value {
    let kind = match link.state {
        ProviderLinkState::Ready { .. } => "provider_ready",
        ProviderLinkState::Unavailable { .. } => "provider_unavailable",
        ProviderLinkState::Terminal { .. } => "provider_terminal",
    };
    let mut event = json!({"kind":kind,"at_us":link.observed_at_us,
        "at_us_upper":link.source_clock.map_or(link.observed_at_us, |bounds| bounds.observed_upper_us),
        "at_us_role":if link.source_clock.is_some(){"source_interval_lower"}else{"host_native"},
        "coordinator_observed_at_us":received_at_us,"link":link});
    if let ProviderLinkState::Ready { setup_us, .. } = link.state {
        event["setup_us"] = json!(setup_us);
    }
    event
}

/// One shared finite control budget per coordinator tick. Returning false
/// defers cloud data/submission, but not capture, privacy or owned playback.
fn service_provider_control(
    channel: &mut Channel,
    cloud: &mut CloudAvailability,
    remaining: &mut usize,
    trace: &mut SessionState,
) -> Result<bool> {
    while *remaining > 0 {
        let Some(event) = channel.receive::<WorkerEvent>()? else {
            return Ok(true);
        };
        *remaining -= 1;
        match event {
            WorkerEvent::ProviderLink { link } => {
                let at = monotonic_us();
                cloud.observe(link, at)?;
                record(trace, link_trace(link, at))?;
            }
            WorkerEvent::VisualContext { receipt } => {
                if receipt.at_us > monotonic_us() || receipt.request == 0 || receipt.session == 0 {
                    return Err(io::Error::other("invalid visual context receipt").into());
                }
                record(
                    trace,
                    json!({"kind":"visual_context", "at_us":receipt.at_us, "receipt":receipt,
                    "boundary":"provider image offer; not model consumption or visual correctness"}),
                )?;
            }
            WorkerEvent::Fault { code } => return Err(io::Error::other(code).into()),
            _ => return Err(io::Error::other("unexpected provider control event").into()),
        }
    }
    Ok(false)
}

/// The same priority path is used at tick start and immediately before a
/// candidate can replace the current turn. A full budget defers admission.
trait SpeakerControlIo {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>>;
    fn publish_speaker_snapshot(&mut self, snapshot: Snapshot) -> io::Result<()>;
}
impl SpeakerControlIo for Rig {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
        self.speaker.channels.control.receive()
    }
    fn publish_speaker_snapshot(&mut self, snapshot: Snapshot) -> io::Result<()> {
        self.publish(snapshot)
    }
}
/// Latches that the bound onboarding invitation reached the speaker. It is
/// read right after the speaker's receipts are taken, before anything can
/// drop the reply, so audio that played and was cut before the walk's next
/// poll counts as said. Provider PCM that never reached the speaker does
/// not: that turn stays unsaid and gives the try back (#3754).
fn latch_invitation_sounded(
    reply: &Option<Reply>,
    invitation: Option<(BootId, u64)>,
    audio: &mut bool,
) {
    if let Some(current) = reply.as_ref()
        && invitation == Some((current.owner.boot(), current.owner.turn()))
    {
        *audio |= current.reached_speaker();
    }
}

#[allow(clippy::too_many_arguments)]
fn service_speaker_control(
    io: &mut impl SpeakerControlIo,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    speaker_pending: &mut Option<PendingSpeaker>,
    playback_mask: &mut PlaybackMask,
    snapshot: &mut Snapshot,
    last_publish: &mut u64,
    remaining: &mut usize,
    trace: &mut SessionState,
    lapsed: &mut Option<TurnOwner>,
) -> Result<bool> {
    while *remaining > 0 {
        let Some(event) = io.next_speaker_event()? else {
            return Ok(true);
        };
        *remaining -= 1;
        match event {
            WorkerEvent::Playback {
                owner: written_owner,
                first_sample,
                end_sample,
                queue_observed_at_us,
                turn,
                sequence,
                queued_frames,
                accepted_frames,
                observed_at_us,
            } => {
                if written_owner.turn() != turn {
                    return Err(io::Error::other("speaker owner/turn mismatch").into());
                }
                if accepted_frames > 0 {
                    playback_mask.speaker_took(crate::turn_taking::Taken {
                        turn,
                        frames: accepted_frames,
                        queued_frames,
                        queue_observed_at_us,
                        level: reply
                            .as_ref()
                            .filter(|current| current.owner.turn() == turn)
                            .map_or(0, |current| current.voice_level),
                    });
                }
                playback_mask.accepted(
                    written_owner,
                    sequence,
                    first_sample,
                    end_sample,
                    accepted_frames,
                    observed_at_us,
                    queue_observed_at_us,
                    monotonic_us(),
                )?;
                trace
                    .receipts
                    .accepted(written_owner, sequence, accepted_frames, end_sample)?;
                if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == turn)
                    && accepted_frames > 0
                {
                    let Some(pending) = speaker_pending.as_mut().filter(|p| p.sequence == sequence)
                    else {
                        return Err(
                            io::Error::other("speaker acceptance has no matching chunk").into()
                        );
                    };
                    if pending.origin != current.notice.origin() {
                        return Err(
                            io::Error::other("speaker receipt output origin changed").into()
                        );
                    }
                    pending.accepted += accepted_frames;
                    if pending.accepted > 240 {
                        return Err(io::Error::other("speaker accepted too many frames").into());
                    }
                    if current.playback.is_none() {
                        let token = owner.playback_started(now(), pending.permit)?;
                        current.playback = Some(token);
                        current.sounded = true;
                        *snapshot = owner.snapshot(now())?;
                        io.publish_speaker_snapshot(*snapshot)?;
                        *last_publish = monotonic_us();
                        record(
                            trace,
                            json!({"kind":format!("{}_first_write",pending.origin.trace_name()),"origin":pending.origin,"notice":current.notice.evidence(),"turn":turn,"owner":current.owner,"playback_sequence":token.sequence(),"at_us":observed_at_us,"boundary":"ALSA accepted; acoustic onset unmeasured",
                                "start_gate":current.start_trace()}),
                        )?;
                        if pending.origin == SpeechOrigin::RequestedAudio {
                            let retired =
                                current.media.confirmation_retired_at_us.ok_or_else(|| {
                                    io::Error::other("requested audio has no retired confirmation")
                                })?;
                            let latency = observed_at_us.checked_sub(retired).ok_or_else(|| {
                                io::Error::other("requested audio preceded confirmation retirement")
                            })?;
                            record(
                                trace,
                                json!({"kind":"requested_audio_handoff",
                                "turn":turn,"owner":current.owner,"at_us":observed_at_us,
                                "confirmation_retired_at_us":retired,
                                "confirmation_to_first_write_us":latency,
                                "target_us":250_000,
                                "boundary":"confirmation final retirement to first accepted recording PCM; acoustic onset unmeasured"}),
                            )?;
                        }
                    }
                    if pending.accepted == 240 {
                        if pending.final_chunk {
                            if pending.origin.local() {
                                current.notice.accepted_final(sequence, end_sample)?;
                            }
                            current.final_chunk = Some(sequence);
                        }
                        *speaker_pending = None;
                    }
                }
            }
            WorkerEvent::PlaybackClockReset {
                previous,
                next_epoch,
                observed_at_us,
            } => {
                playback_mask.clock_reset(previous, next_epoch, observed_at_us, monotonic_us())?;
                if let Some(reminder) = reply.as_mut().and_then(|current| current.reminder.as_mut())
                {
                    reminder.invalidate();
                }
                trace.receipts.reset(previous);
                record(
                    trace,
                    json!({"kind":"playback_clock_reset","previous":previous,"next_epoch":next_epoch,"at_us":observed_at_us,"boundary":"successful physical discard/reset; not speech retirement"}),
                )?;
            }
            WorkerEvent::SpeechDrained {
                owner: drained_owner,
                sequence,
                last_sample,
                observed_at_us,
            } => {
                let applied = playback_mask.drained(
                    drained_owner,
                    sequence,
                    last_sample,
                    observed_at_us,
                    monotonic_us(),
                )?;
                trace.receipts.drained(drained_owner, last_sample);
                record(
                    trace,
                    json!({"kind":"speech_queue_drained","owner":drained_owner,"sequence":sequence,"last_sample":last_sample,"at_us":observed_at_us,"mask_updated":applied,"boundary":"ALSA speech cursor retirement; not turn completion or acoustic silence"}),
                )?;
            }
            WorkerEvent::SpeechRetired {
                owner: delivered,
                sequence,
                final_sample,
                observed_at_us,
            } => {
                trace.receipts.drained(delivered, final_sample);
                if let Some(current) = reply.as_mut().filter(|reply| reply.owner == delivered) {
                    let origin = current.notice.origin();
                    if origin.local() {
                        current.notice.validate_retirement(sequence, final_sample)?;
                    }
                    let token = current.retire_playback(sequence, final_sample.frame)?;
                    owner.playback_ended(now(), token)?;
                    if origin == SpeechOrigin::Provider
                        && let Some(reminder) = &mut current.reminder
                    {
                        reminder.retired(token.sequence(), final_sample.epoch, final_sample.frame);
                    }
                    if origin.local() {
                        current.notice.retire(observed_at_us)?;
                    } else {
                        current.media.confirmation_retired_at_us = Some(observed_at_us);
                    }
                    *snapshot = owner.snapshot(now())?;
                    io.publish_speaker_snapshot(*snapshot)?;
                    *last_publish = monotonic_us();
                    record(
                        trace,
                        json!({"kind":if origin == SpeechOrigin::Provider {"speech_final_sample_retired".to_owned()}else{format!("{}_final_sample_retired",origin.trace_name())},"origin":origin,"notice":current.notice.evidence(),"turn":delivered.turn(),"owner":delivered,
                            "playback_sequence":token.sequence(),"at_us":observed_at_us,"final_sample":final_sample,"boundary":"ALSA delay retirement; idle zeros may remain queued; acoustic end unmeasured"}),
                    )?;
                }
            }
            event @ WorkerEvent::CancelledTailRetired {
                owner: old,
                end_sample,
                observed_at_us,
                ..
            } => {
                playback_mask.cancelled_tail(old, end_sample, observed_at_us, monotonic_us())?;
                handle_cancelled_tail_retirement(reply, trace, event, monotonic_us())?;
            }
            event @ WorkerEvent::PlaybackGap { gap, .. } => {
                // Zeros the lamp holds on purpose at a sentence end (#828)
                // are no delivery gap, unless they ran on past the hold.
                let listening = playback_mask.listen_gap_covers(gap.owner, gap.started_at_us);
                let overran_us = playback_mask.listen_gap_overran_us(
                    gap.owner,
                    gap.started_at_us,
                    gap.last_zero_accepted_at_us,
                );
                handle_playback_gap(
                    reply,
                    speaker_pending.as_ref(),
                    trace,
                    event,
                    listening,
                    overran_us,
                )?;
            }
            WorkerEvent::PlaybackRejected {
                turn,
                sequence,
                reason,
                lapsed: refused_on_lapse,
            } => {
                playback_mask.rejected(sequence, monotonic_us())?;
                trace.receipts.rejected(sequence);
                record(
                    trace,
                    json!({"kind":"speaker_rejected","turn":turn,"sequence":sequence,"reason":reason,"lapsed":refused_on_lapse,"at_us":monotonic_us()}),
                )?;
                if let Some(current) = reply.as_ref().filter(|r| r.owner.turn() == turn) {
                    if !refused_on_lapse {
                        return Err(io::Error::other("current reply rejected at speaker").into());
                    }
                    // #1178: refused as the speaker's authority lapsed while
                    // it was held up. The chunk is settled, and the answer is
                    // cut by the caller, never the voice (the reviewer on #1214).
                    if speaker_pending
                        .as_ref()
                        .is_some_and(|pending| pending.sequence == sequence)
                    {
                        *speaker_pending = None;
                    }
                    *lapsed = Some(current.owner);
                }
            }
            event @ WorkerEvent::PlaybackDiscarded {
                owner: old,
                reason,
                observed_at_us,
                ..
            } => {
                playback_mask.discarded(old, observed_at_us, monotonic_us())?;
                let current = reply.as_ref().is_some_and(|r| r.owner == old);
                handle_playback_discard(owner, reply, trace, event)?;
                // #1178: the current answer's speech dropped as the
                // speaker's authority lapsed: cut by the caller.
                if current && reason == PlaybackDiscardReason::ExpiredState {
                    *lapsed = Some(old);
                }
            }
            WorkerEvent::ReferenceFault { fault } => {
                record(
                    trace,
                    json!({"kind":"reference_fault","worker":"speaker","details":fault}),
                )?;
                return Err(fault.into());
            }
            WorkerEvent::Fault { code } => return Err(io::Error::other(code).into()),
            _ => return Err(io::Error::other("unexpected speaker event").into()),
        }
    }
    Ok(false)
}

fn accepted_decision(decision: Decision, trace: &mut SessionState) -> Result<Option<Accepted>> {
    match decision {
        Decision::Accepted(accepted) => Ok(Some(accepted)),
        Decision::Rejected(rejection) => {
            record_input_rejection(trace, rejection)?;
            Ok(None)
        }
        Decision::Ignored => {
            Err(io::Error::other("directed candidate decision lost its identity").into())
        }
    }
}

/// Only a successful local admission decision may interrupt an older answer.
/// Cloud unavailability prevents submission, never that local cancellation.
/// No provider session/request is invented for this never-submitted input.
fn accepted_without_cloud(
    accepted: Accepted,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    admission: &mut InputAdmission,
    trace: &mut SessionState,
) -> Result<(Snapshot, bool)> {
    let displaced = reply.as_ref().map(|current| current.owner);
    cancel_reply(owner, reply, trace, "user_interrupted")?;
    admission.discard_stream_until_end(monotonic_us())?;
    let state = owner.snapshot(now())?;
    record(
        trace,
        json!({"kind":"input_not_submitted","at_us":monotonic_us(),
        "candidate":accepted.candidate.id,"admission_basis":accepted.basis,
        "interrupted_owner":displaced,"reason":"provider_unavailable_or_pending_control",
        "provider_request":null,"provider_session":null,
        "outcome":RequestOutcome::Failed { stage:crate::provider_worker::FailedStage::NotDelivered,
            reason:crate::provider_worker::Fault::Local, local_not_submitted:true },
        "prefix_first_host_read_us":accepted.audio.first().map(|frame| frame.captured_at_us)}),
    )?;
    Ok((state, !accepted.ended))
}

fn discard_request_input(
    current: &mut Reply,
    queue: &mut VecDeque<ProviderInput>,
    admission: &mut InputAdmission,
    draining: &mut bool,
    trace: &mut SessionState,
    reason: &'static str,
) -> Result<()> {
    let discarded_commands = queue.len();
    queue.clear();
    current.cloud.input_abandoned = true;
    if !current.local_input_ended {
        admission.discard_stream_until_end(monotonic_us())?;
        *draining = true;
    }
    record(
        trace,
        json!({"kind":"provider_input_abandoned","at_us":monotonic_us(),
        "turn":current.owner.turn(),"provider_session":current.cloud.session,"reason":reason,
        "start_submitted":current.cloud.start_submitted,"input_end_submitted":current.cloud.input_end_submitted,
        "discarded_commands":discarded_commands}),
    )?;
    Ok(())
}

fn apply_submission_gate(
    cloud: &CloudAvailability,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    admission: &mut InputAdmission,
    draining: &mut bool,
    trace: &mut SessionState,
) -> Result<()> {
    if let Some(current) = reply.as_mut()
        && current.cloud.abandon_if_changed(cloud)
    {
        discard_request_input(
            current,
            queue,
            admission,
            draining,
            trace,
            "provider_link_changed",
        )?;
        if current.cloud.outcome.is_some() {
            current.provider_idle = true;
            current.generation_done = true;
            record(
                trace,
                json!({"kind":"provider_turn_failed","at_us":monotonic_us(),
                "turn":current.owner.turn(),"outcome":current.cloud.outcome}),
            )?;
        }
    }
    Ok(())
}
/// Validate ordered model evidence before it can mutate playback. Availability
/// is solely the priority register: a stale credited Ready has no effect here.
fn observe_provider_output(
    cloud: &mut CloudAvailability,
    reply: &mut Option<Reply>,
    event: &ProviderOutput,
) -> io::Result<()> {
    if cloud.output_ended()
        && !matches!(
            event,
            ProviderOutput::TurnFailed { .. }
                | ProviderOutput::OwnershipAssumed { .. }
                | ProviderOutput::Ready { .. }
        )
    {
        return Err(io::Error::other("model output followed ordered Ended"));
    }
    // Keep match borrowing explicit: only this request's source receipt may
    // change its delivery evidence; cancelled old owners are already terminal.
    match event {
        // Evidence about input of a request that is already retired: it
        // changes no request's delivery state.
        ProviderOutput::Ready { .. }
        | ProviderOutput::Transcript { .. }
        | ProviderOutput::Stages { .. }
        | ProviderOutput::ReminderDeferred { .. }
        | ProviderOutput::InputDiscarded { .. } => {}
        // About the line still heard, for the detector, taken by the loop.
        ProviderOutput::WordsEnd { .. } => {}
        // Only a provider configured to take a silent earlier response as
        // over reports this. It grants nothing here: the request still needs
        // its own Started before any of its output is usable.
        ProviderOutput::OwnershipAssumed { .. } => {}
        // Asked again by the loop once the turn is over (#926); a tool's
        // look is taken by the loop too (#941).
        ProviderOutput::Unclear { .. }
        | ProviderOutput::LookAsked { .. }
        | ProviderOutput::SleepAsked { .. }
        | ProviderOutput::StepCallHeard { .. }
        | ProviderOutput::RequestedAudio { .. } => {}
        // A finished task's news the person asked about, answered in this
        // turn: the tasks part is told so only once the turn is heard whole
        // (#1390), as with news told unasked.
        ProviderOutput::NewsAnswered { request, key } => {
            match reply.as_mut().filter(|r| r.owner.turn() == *request) {
                Some(current) => {
                    if !current.news.contains(key) {
                        current.news.push(key.clone());
                    }
                }
                None => eprintln!(
                    "lamp-voice: the news ({key}) was answered for a turn already over: its harness stays open"
                ),
            }
        }
        ProviderOutput::HoldingAnswer { request, session } => {
            if let Some(current) = reply.as_ref().filter(|r| r.owner.turn() == *request) {
                current.cloud.require_output()?;
                if current.cloud.session != *session {
                    return Err(io::Error::other(
                        "holding answer belongs to another session",
                    ));
                }
            }
        }
        ProviderOutput::LookNoCall { request, session } => {
            if let Some(current) = reply.as_ref().filter(|r| r.owner.turn() == *request) {
                current.cloud.require_output()?;
                if current.cloud.session != *session {
                    return Err(io::Error::other(
                        "a look's answer belongs to another session",
                    ));
                }
            }
        }
        ProviderOutput::Ended { .. } => cloud.end_output()?,
        ProviderOutput::Started {
            request, session, ..
        } => {
            if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == *request) {
                current.cloud.confirm_start(*session)?;
            }
        }
        ProviderOutput::Audio { request, .. } | ProviderOutput::Interrupted { request } => {
            if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == *request) {
                current.cloud.require_output()?;
            }
        }
        ProviderOutput::GenerationComplete { request } => {
            if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == *request) {
                current.cloud.require_output()?;
                current.generation_done = true;
            }
        }
        ProviderOutput::TurnComplete { request, idle } => {
            if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == *request) {
                current.cloud.complete(*idle)?;
                current.provider_idle = *idle;
                if *idle {
                    current.generation_done = true;
                }
            }
        }
        ProviderOutput::TurnFailed {
            request,
            stage,
            reason,
        } => {
            if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == *request) {
                current.cloud.failed(*stage, *reason)?;
                // Prior PCM retains its owner but never becomes a successful
                // answer. Flush even a partial held final block, then fail.
                current.generation_done = true;
                current.provider_idle = true;
            }
        }
    }
    Ok(())
}

/// Sending is bounded independently of answer delivery. No queued Audio/End
/// can be moved to a replacement session, and WouldBlock is not delivery.
fn flush_provider_input(
    cloud: &CloudAvailability,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    channel: &mut Channel,
) -> io::Result<()> {
    flush_provider_input_with(cloud, reply, queue, |command| channel.send(command))
}

fn flush_provider_input_with(
    cloud: &CloudAvailability,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    mut send: impl FnMut(&ProviderInput) -> io::Result<()>,
) -> io::Result<()> {
    for _ in 0..8 {
        let Some(command) = queue.front() else {
            break;
        };
        let current = reply
            .as_mut()
            .ok_or_else(|| io::Error::other("provider input lost its owner"))?;
        if !current.cloud.can_submit(cloud) {
            break;
        }
        let request = match command {
            ProviderInput::Start { request, .. }
            | ProviderInput::Audio { request, .. }
            | ProviderInput::End { request, .. }
            | ProviderInput::Called { request, .. }
            | ProviderInput::Paused { request, .. }
            | ProviderInput::CountOnly { request, .. }
            | ProviderInput::StepCall { request, .. }
            | ProviderInput::Onboarding { request, .. }
            | ProviderInput::Reminder { request, .. }
            | ProviderInput::Written { request, .. } => *request,
        };
        if request != current.owner.turn() {
            return Err(io::Error::other(
                "queued provider input has stale ownership",
            ));
        }
        current.cloud.validate_submission(command)?;
        match send(command) {
            Ok(()) => {
                current.cloud.submitted(command)?;
                queue.pop_front();
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[derive(Default)]
struct StartupEvents {
    cloud: CloudAvailability,
    output_flow: OutputReceiver,
    events: Vec<Value>,
    /// The capture sent a frame before activation (#1567): the microphone
    /// was open and gave audio, so the run heard, whatever it then refused.
    microphone_spoke: bool,
}
impl StartupEvents {
    fn push(&mut self, event: Value) -> io::Result<()> {
        if self.events.len() >= 16 {
            return Err(io::Error::other("startup event capacity exceeded"));
        }
        self.events.push(event);
        Ok(())
    }
    /// `role` is the worker's. In `rig.provider` that is Gemini's
    /// (`provider`) or, with `LAMP_VOICE_ROUTE=second`, the second route's
    /// (`second`), taken alike. A standby second route is never polled here:
    /// it starts through `second::Route::startup_tick`.
    fn poll(&mut self, channels: &mut WorkerChannels, role: &str) -> io::Result<()> {
        let provider = matches!(role, "provider" | "second");
        for _ in 0..16 {
            let Some(event) = channels.control.receive::<WorkerEvent>()? else {
                break;
            };
            match (&event, role) {
                (WorkerEvent::ProviderLink { link }, _) if provider => {
                    let at = monotonic_us();
                    self.cloud.observe(*link, at)?;
                    self.push(link_trace(*link, at))?;
                }
                (WorkerEvent::CaptureDspReset { .. }, "capture") => self.push(
                    json!({"kind":"echo_reference_reset","at_us":monotonic_us(),"details":event}),
                )?,
                (WorkerEvent::ReferenceStatus { status }, "capture") => {
                    self.push(reference_status_trace(status))?
                }
                (
                    WorkerEvent::Ring {
                        report: RingFeedback::Blanked { .. },
                    },
                    "ring",
                ) => self.push(
                    json!({"kind":"ring_startup_event","at_us":monotonic_us(),"details":event}),
                )?,
                (WorkerEvent::Fault { code }, _) => return Err(io::Error::other(code.clone())),
                _ => {
                    return Err(io::Error::other(
                        "unexpected worker event before activation",
                    ));
                }
            }
        }
        for _ in 0..16 {
            match role {
                "provider" | "second" => match channels.data.receive::<ProviderOutput>()? {
                    // A provider can recover while later children are still
                    // starting. Count its repeated Ready against the original
                    // capacity sequence; it does not admit input by itself.
                    Some(ProviderOutput::Ready { setup_us }) => {
                        self.output_flow.received()?;
                        self.push(json!({"kind":"provider_ready_data_ignored","at_us":monotonic_us(),"setup_us":setup_us}))?;
                    }
                    Some(ProviderOutput::Ended {
                        reason,
                        fault,
                        attempts,
                    }) => {
                        self.output_flow.received()?;
                        self.cloud.end_output()?;
                        self.push(json!({"kind":"provider_output_ended","at_us":monotonic_us(),"reason":reason,"fault":fault,"attempts":attempts}))?;
                    }
                    Some(_) => {
                        return Err(io::Error::other("unexpected provider output before input"));
                    }
                    None => break,
                },
                "camera" => match channels.data.receive::<CameraEvent>()? {
                    Some(CameraEvent::Progress { .. } | CameraEvent::FrameLink { .. }) => {}
                    Some(_) => return Err(io::Error::other("camera activated before privacy")),
                    None => break,
                },
                "speaker" | "ring" => break,
                "capture" => {
                    if channels.data.receive::<MicrophoneFrame>()?.is_some() {
                        self.microphone_spoke = true;
                        return Err(io::Error::other("microphone opened before activation"));
                    }
                    break;
                }
                _ => return Err(io::Error::other("unexpected startup worker role")),
            }
        }
        if provider {
            self.output_flow.replenish(0, &mut channels.control)?;
        }
        Ok(())
    }
}

fn pulse(
    owner: &mut Controller,
    workers: &mut [&mut Worker],
    last: &mut u64,
    startup: &mut StartupEvents,
    termination: &Termination,
) -> io::Result<()> {
    termination.check()?;
    // Consume operational notifications while later children start. A startup
    // wait must not age valid packets in the 100 ms runtime channels.
    for worker in workers.iter_mut() {
        startup.poll(&mut worker.channels, &worker.role)?;
    }
    let time = monotonic_us();
    if time.saturating_sub(*last) >= 20_000 {
        let snapshot = owner
            .snapshot(MonoTime::from_micros(time))
            .map_err(io::Error::other)?;
        for worker in workers {
            worker
                .channels
                .control
                .send(Control::Authority { snapshot })?;
            worker.check_running()?;
        }
        *last = time;
    }
    Ok(())
}
/// Startup workers have denied capture authority. A mute observed here is
/// persisted and aborts startup; partial-start teardown is not a resumable cycle.
fn controls_startup(controls: &mut Option<HandControls>) -> io::Result<()> {
    if let Some(controls) = controls.as_mut()
        && controls.poll()?.muted
    {
        controls.persist_muted()?;
        return Err(io::Error::other(
            "software mute during worker startup; restart remains muted",
        ));
    }
    Ok(())
}

fn record_hand_pats(trace: &mut SessionState, count: u64) -> Result<()> {
    if count != 0 {
        record(
            trace,
            json!({"kind":"hand_pat_observed","at_us":monotonic_us(),"count":count,"action":"none"}),
        )?;
    }
    Ok(())
}

/// The provider worker's arguments. Every one comes from the options the
/// worker acts on or from the environment; the coordinator's own settings,
/// such as when it sends the end (`--early-end-ms`), are not among them.
fn provider_arguments(
    source: ProviderSource<'_>,
    options: &DirectedOptions,
    lifetime: Lifetime,
    camera: Option<&PreparedCamera>,
    wait_for_room: bool,
) -> io::Result<Vec<String>> {
    // A service that runs all day never gives its provider link up.
    let keep_link = lifetime.continuous() && matches!(source, ProviderSource::Gemini(_));
    let mut arguments = source.arguments()?;
    // Before the camera context, which the worker reads as the rest.
    arguments.extend(options.provider_continuation.arguments());
    arguments.extend(crate::sight::arguments_from_environment());
    arguments.extend(crate::thread::arguments_from_environment());
    arguments.extend(crate::session_fact::arguments_from_environment());
    arguments.extend(crate::room_told::arguments_from_environment());
    arguments.extend(crate::air_told::arguments_from_environment());
    arguments.extend(crate::seems_told::arguments_from_environment());
    arguments.extend(crate::health_told::arguments_from_environment());
    if keep_link {
        arguments.push(crate::provider_worker::KEEP_LINK_FLAG.to_owned());
    }
    // A second route's held line is replayed faster than speech (#892).
    if wait_for_room {
        arguments.push(crate::provider_worker::WAIT_FOR_ROOM_FLAG.to_owned());
    }
    arguments.extend(crate::tools::arguments_from_environment());
    arguments.extend(crate::attention::arguments_from_environment());
    arguments.extend(crate::attention::passing_arguments_from_environment());
    arguments.extend(crate::attention::spoken_to_arguments_from_environment());
    arguments.extend(crate::same_voice::arguments_from_environment());
    arguments.extend(crate::addressed::arguments_from_environment());
    arguments.extend(crate::attention::name_arguments_from_environment());
    arguments.extend(crate::called::arguments_from_environment());
    arguments.extend(crate::attention::head_touch_arguments_from_environment());
    arguments.extend(crate::bus_out::worker_arguments_from_environment());
    arguments.extend(crate::first_output::arguments_from_environment());
    arguments.extend(crate::config::writes_arguments_from_environment());
    arguments.extend(crate::not_speech::arguments_from_environment());
    arguments.extend(crate::not_speech::unclear_arguments_from_environment());
    arguments.extend(crate::not_speech::transcription_arguments_from_environment());
    arguments.extend(crate::policy_holds::arguments_from_environment());
    arguments.extend(crate::resend::arguments_from_environment());
    arguments.extend(crate::staff_note::arguments_from_environment());
    arguments.extend(crate::config::patience_arguments_from_environment());
    arguments.extend(crate::remarks::arguments_from_environment());
    arguments.extend(crate::going_on_asks::arguments_from_environment());
    arguments.extend(crate::cut_in_told::arguments_from_environment());
    arguments.extend(crate::spoken_stop::arguments_from_environment());
    arguments.extend(crate::misheard_hum::arguments_from_environment());
    arguments.extend(crate::holding_answer::arguments_from_environment());
    arguments.extend(crate::messages::arguments_from_environment());
    arguments.extend(crate::others_answer::arguments_from_environment());
    arguments.extend(crate::going_on_same_one::arguments_from_environment());
    arguments.extend(crate::turned_away::arguments_from_environment());
    arguments.extend(crate::said_as_heard::arguments_from_environment());
    arguments.extend(crate::scoped_session::arguments_from_environment());
    arguments.extend(crate::name_or_follow_up::arguments_from_environment());
    if let Some(camera) = camera {
        arguments.extend(camera.source.arguments());
    }
    Ok(arguments)
}

#[allow(clippy::too_many_arguments)]
fn spawn_rig(
    source: ProviderSource<'_>,
    directory: &Path,
    boot: BootId,
    owner: &mut Controller,
    startup: &mut StartupEvents,
    diagnostic_directory: Option<&Path>,
    options: DirectedOptions,
    camera: Option<PreparedCamera>,
    lifetime: Lifetime,
    termination: &Termination,
    controls: &mut Option<HandControls>,
) -> Result<Rig> {
    controls_startup(controls)?;
    termination.check()?;
    let executable = std::env::current_exe()?;
    // Model loading overlaps the remaining startup; nothing waits for it.
    let endpoint = EndpointRig::start(&options, &executable);
    let provider_boot = new_boot()?;
    let fixture_at_ms = match source {
        ProviderSource::Fixture { at_ms, .. } => at_ms,
        ProviderSource::Gemini(_) => None,
    };
    // A service that runs all day may keep a second voice route standing
    // by, or, with `LAMP_VOICE_ROUTE=second`, have it as its only voice in
    // Gemini's place (`run_serve` refused a wrong value before any of this).
    let routes = second::routes_from_environment(
        lifetime.continuous() && matches!(source, ProviderSource::Gemini(_)),
    )?;
    // Texts are answered by the second route's worker (#1673): without it,
    // `lamp-cloud` refuses each one, and the journal says why, once.
    if crate::messages::on_from_environment() && !routes.has_second() {
        eprintln!(
            "lamp-voice: LAMP_MESSAGES=on, but texts need the second route (LAMP_VOICE_SECOND_CONFIG), which is not set: each text is refused"
        );
    }
    let mut arguments = provider_arguments(
        source,
        &options,
        lifetime,
        camera.as_ref(),
        routes.standing_by(),
    )?;
    // `go_to_sleep` only where a tap on the head can wake it (#3835).
    crate::sleep_asked::only_with_head_sleep(
        &mut arguments,
        controls.as_ref().is_some_and(HandControls::head_sleep),
    );
    let second::Workers {
        role,
        arguments,
        standby: second_arguments,
    } = routes.workers(arguments, second::route_flags_from_environment()?)?;
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let mut provider = Worker::spawn_with_boot_and_tick(
        &executable,
        directory,
        role,
        boot,
        provider_boot,
        &arguments,
        || {
            controls_startup(controls)?;
            termination.check()
        },
    )?;
    let mut last = 0;
    let capture_arguments = options.capture_arguments(
        diagnostic_directory.map(|p| p.join("capture-audio")),
        // Only for stop words that are listened for (#1047).
        if crate::stop_words::listen() == crate::stop_words::Listen::Off {
            crate::stop_words::Source::Processed
        } else {
            crate::stop_words::source()
        },
    )?;
    let speaker_arguments = AudioWorkerOptions {
        diagnostic_directory: diagnostic_directory.map(|p| p.join("render-audio")),
        ..AudioWorkerOptions::default()
    }
    .child_arguments("plug:device_speaker", false)?;
    let capture_arguments: Vec<&str> = capture_arguments.iter().map(String::as_str).collect();
    let speaker_arguments: Vec<&str> = speaker_arguments.iter().map(String::as_str).collect();
    let mut capture = Worker::spawn_with_tick(
        &executable,
        directory,
        "capture",
        boot,
        &capture_arguments,
        || {
            controls_startup(controls)?;
            pulse(owner, &mut [&mut provider], &mut last, startup, termination)
        },
    )?;
    let mut speaker = Worker::spawn_with_tick(
        &executable,
        directory,
        "speaker",
        boot,
        &speaker_arguments,
        || {
            controls_startup(controls)?;
            pulse(
                owner,
                &mut [&mut provider, &mut capture],
                &mut last,
                startup,
                termination,
            )
        },
    )?;
    // A service that runs all day also shows a resting face between turns,
    // under the same ceiling as its cues. A bounded directed run does not.
    let resting_ceiling = options
        .ring_channel_ceiling
        .map(|ceiling| ceiling.to_string());
    let mut ring_arguments: Vec<&str> = match (&resting_ceiling, lifetime.continuous()) {
        (Some(ceiling), true) => vec!["--continuous", "--resting", ceiling],
        (None, true) => vec!["--continuous"],
        (_, false) => Vec::new(),
    };
    // The lamp's own light is asked for and said on the bus, by a ring
    // that runs all day (`ring_light`). A worker is started with no
    // environment, so it is told where the bus is.
    let ring_bus = crate::bus_out::worker_arguments_from_environment();
    // And whether its face is the eye (`LAMP_RING_EYE=on`, task 34), which
    // needs that bus to see where people are.
    let ring_eye = crate::ring_sight::arguments_from_environment();
    // And whether a hand kept on the head dims it, a countdown to sleep
    // (`LAMP_HEAD_TOUCH=on`, #711), which reads Controls on that bus.
    let ring_head = crate::ring_head::arguments_from_environment();
    // And whether lights asked for that lapse are shown over the person's
    // (`LAMP_LIGHT_REQUESTS=on`, #940), on that bus too.
    let ring_requests = crate::ring_light::arguments_from_environment();
    // And whether the one shown tints the Speaking face
    // (`LAMP_SKILL_LIGHT_SPEAKING=on`, El on #1722), after that flag.
    let ring_speaking = crate::ring_light::speaking_arguments_from_environment();
    // And whether the person's light is kept across a restart
    // (`LAMP_LIGHT_KEEP=on`, #1464).
    let ring_keep = crate::ring_light::keep_arguments_from_environment();
    let ring_caps = crate::ring_caps::arguments_from_environment();
    if lifetime.continuous() {
        ring_arguments.extend(ring_bus.iter().map(String::as_str));
        ring_arguments.extend(ring_eye.iter().map(String::as_str));
        ring_arguments.extend(ring_head.iter().map(String::as_str));
        ring_arguments.extend(ring_requests.iter().map(String::as_str));
        ring_arguments.extend(ring_speaking.iter().map(String::as_str));
        ring_arguments.extend(ring_keep.iter().map(String::as_str));
        ring_arguments.extend(ring_caps.iter().map(String::as_str));
    } else if !ring_caps.is_empty() {
        ring_arguments.extend(ring_caps.iter().map(String::as_str));
        ring_arguments.extend(ring_bus.iter().map(String::as_str));
    }
    let mut ring = if options.ring_channel_ceiling.is_some() {
        OptionalRing::spawn(&executable, directory, boot, &ring_arguments, || {
            controls_startup(controls)?;
            pulse(
                owner,
                &mut [&mut provider, &mut capture, &mut speaker],
                &mut last,
                startup,
                termination,
            )
        })?
    } else {
        OptionalRing::from(None)
    };
    // The optional worker is created closed, before privacy activation. Its
    // startup failure is evidence of unavailable sight, not a voice failure.
    let mut camera = if let Some(prepared) = camera {
        let consumer = format!("context={}", boot_text(provider_boot));
        let extra = [
            prepared
                .path
                .to_str()
                .ok_or_else(|| io::Error::other("camera path UTF-8"))?,
            &prepared.digest,
            "--frame-consumer",
            &consumer,
        ];
        match camera::optional_startup(
            |tick| {
                Worker::spawn_with_boot_and_tick(
                    &executable,
                    directory,
                    "camera",
                    boot,
                    prepared.source.camera,
                    &extra,
                    tick,
                )
            },
            || {
                controls_startup(controls)?;
                let previous = last;
                pulse(
                    owner,
                    &mut [&mut provider, &mut capture, &mut speaker],
                    &mut last,
                    startup,
                    termination,
                )?;
                let snapshot = (previous != last)
                    .then(|| owner.snapshot(now()))
                    .transpose()
                    .map_err(io::Error::other)?;
                ring.startup_tick(snapshot);
                Ok(())
            },
        )? {
            Some(worker) => Some(OptionalCamera::new(worker, prepared.source)),
            None => {
                startup.push(json!({"kind":"camera_unavailable", "at_us":monotonic_us(), "stage":"worker_startup", "audio_continues":true}))?;
                None
            }
        }
    } else {
        None
    };
    let privacy_route = crate::privacy_bus::from_environment()?;
    let privacy_arguments = crate::privacy_bus::conversation_arguments(
        privacy_route
            .as_ref()
            .map(|(root, route)| (root.as_path(), *route)),
        lifetime.continuous() && crate::privacy_gap::enabled_from_environment(),
    )?;
    let privacy_arguments: Vec<&str> = privacy_arguments.iter().map(String::as_str).collect();
    let mut later_tick = |second: Option<&mut second::Route>| -> io::Result<()> {
        controls_startup(controls)?;
        let previous = last;
        pulse(
            owner,
            &mut [&mut provider, &mut capture, &mut speaker],
            &mut last,
            startup,
            termination,
        )?;
        let ring_snapshot = (previous != last)
            .then(|| owner.snapshot(now()))
            .transpose()
            .map_err(io::Error::other)?;
        ring.startup_tick(ring_snapshot);
        // A post-handshake camera failure is optional even while a mandatory
        // child starts. It never joins the voice worker failure domain.
        if let Some(camera) = camera.as_mut() {
            let snapshot = (previous != last)
                .then(|| owner.snapshot(now()))
                .transpose()
                .map_err(io::Error::other)?;
            camera.startup_tick(startup, snapshot);
        }
        if let Some(route) = second {
            let snapshot = (previous != last)
                .then(|| owner.snapshot(now()))
                .transpose()
                .map_err(io::Error::other)?;
            route.startup_tick(snapshot, startup)?;
        }
        Ok(())
    };
    let mut second = second::Route::spawn(
        &executable,
        directory,
        boot,
        second_arguments.as_deref(),
        || later_tick(None),
    )?;
    // On the Controls route a privacy worker that ends before its start is
    // read is an unreadable switch, as one that ends while it runs
    // (`running`): the conversation ends closed and the wait reads the
    // switch again (Seventh on #1396).
    let privacy = privacy_started(
        Worker::spawn_with_tick(
            &executable,
            directory,
            "privacy",
            boot,
            &privacy_arguments,
            || later_tick(second.as_mut()),
        ),
        privacy_route.is_some(),
    )?;
    for (worker, expected_stream) in [
        (&capture, DiagnosticStream::Capture),
        (&speaker, DiagnosticStream::Render),
    ] {
        match (diagnostic_directory, worker.diagnostic_start) {
            (Some(_), Some(started))
                if started.boot == boot
                    && started.stream == expected_stream
                    && started.started_at_us > 0
                    && started.started_at_us <= monotonic_us() =>
            {
                startup.push(json!({"kind":"audio_diagnostics_started", "worker":worker.role, "worker_boot":worker.boot, "details":started}))?;
            }
            (None, None) => {}
            _ => return Err(io::Error::other("audio diagnostic startup identity mismatch").into()),
        }
    }
    match (
        std::env::var_os(SECOND_PLAYOUT_START_ENVIRONMENT),
        second_playout_start(),
    ) {
        (Some(_), Some(second)) => eprintln!(
            "lamp-voice: while the second route answers, a reply's first sound waits for {} ms of audio, not the unit's {} ms ({SECOND_PLAYOUT_START_ENVIRONMENT})",
            second.as_micros() / 1_000,
            options.playout_start.as_micros() / 1_000
        ),
        (Some(_), None) => eprintln!(
            "lamp-voice: {SECOND_PLAYOUT_START_ENVIRONMENT} is not 0 to 500: the unit's start horizon stays"
        ),
        (None, _) => {}
    }
    Ok(Rig {
        expression_out: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: privacy_route.is_some(),
        capture_held: false,
        privacy_receipts: matches!(privacy_route, Some((_, crate::privacy_bus::Route::Slide))),
        ring,
        camera,
        motor: None,
        ring_channel_ceiling: options.ring_channel_ceiling,
        diagnostic_directory: diagnostic_directory.map(Path::to_path_buf),
        noise_suppression: options.noise_suppression,
        turn_taking: options.turn_taking,
        end_silence_blocks: options.end_silence_blocks,
        early_end_blocks: options.early_end_blocks,
        early_finalize: early_finalize::from_environment(),
        open_name: open_name::from_environment(),
        call_takes_next: open_name::call_takes_next_from_environment(),
        unfinished_wait: crate::unfinished_words::on(),
        faint_level: options.faint_level,
        playout_start: options.playout_start,
        fixture_at_ms,
        microphone: options.microphone,
        endpoint,
        call_wait_blocks: options.call_wait_blocks,
        cut_keeps_name: crate::attention::cut_keeps_name(),
        asker_first: crate::asker_first::Mode::from_environment(),
        follows_on: crate::follows_on::on(),
        paused: keeper_pause(lamp_bus::Bus::from_environment()),
        second,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    source: ProviderSource<'_>,
    lifetime: Lifetime,
    trace: &mut SessionState,
    diagnostic_directory: Option<&Path>,
    options: DirectedOptions,
    camera_config: Option<&Path>,
    motor_options: Option<&crate::options::MotorOptions>,
    motor_rearm: &mut bool,
    notices: &FailureNotices,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    controls: &mut Option<HandControls>,
    reminders: &mut Option<crate::reminders::Reminders>,
) -> Result<()> {
    let result = run_cycle(
        source,
        lifetime,
        trace,
        diagnostic_directory,
        options,
        camera_config,
        motor_options,
        motor_rearm,
        notices,
        cues,
        termination,
        controls,
        reminders,
    );
    released_if_never_listened(result, trace, controls)
}

/// With `LAMP_GUARD_NEVER_LISTENED=on` (#1567, B): a run that ended in a
/// fault, anywhere from its arming on, before its capture delivered any
/// audio and with no control input in flight, releases the restart guard,
/// the latch written as it was, and says so once. It heard nothing, so the
/// next start comes up as the person left it, not muted with nobody told
/// (lamp-4ace on 18 Oct: 15:32, a capture that gave no block, and 17:48, a
/// microphone busy at its open). A privacy exit, a fault while listening and
/// one with a mute in flight keep the guard; the error is returned as it was.
fn released_if_never_listened(
    result: Result<()>,
    trace: &mut SessionState,
    controls: &mut Option<HandControls>,
) -> Result<()> {
    let Err(error) = result else {
        return result;
    };
    let privacy = error.is::<PhysicalMute>()
        || error.is::<SoftwareMute>()
        || service_privacy::is_unavailable(&*error);
    if privacy {
        return Err(error);
    }
    let Some(controls) = controls.as_mut() else {
        return Err(error);
    };
    match controls.release_never_listened() {
        Ok(true) => {
            eprintln!(
                "lamp-voice: the run failed before it heard anything ({error}); the mute is kept as it was, not restored muted"
            );
            // The run's error is returned as it was, even if this write fails.
            let _ = record(
                trace,
                json!({"kind":"guard_released_never_listened","at_us":monotonic_us(),"could_not_start":controls.could_not_start()}),
            );
        }
        Ok(false) => {}
        Err(release) => {
            eprintln!(
                "lamp-voice: the run failed before it heard anything, and the guard could not be released: {release}"
            );
        }
    }
    Err(error)
}

#[allow(clippy::too_many_arguments)]
fn run_cycle(
    source: ProviderSource<'_>,
    lifetime: Lifetime,
    trace: &mut SessionState,
    diagnostic_directory: Option<&Path>,
    options: DirectedOptions,
    camera_config: Option<&Path>,
    motor_options: Option<&crate::options::MotorOptions>,
    motor_rearm: &mut bool,
    notices: &FailureNotices,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    controls: &mut Option<HandControls>,
    reminders: &mut Option<crate::reminders::Reminders>,
) -> Result<()> {
    termination.check()?;
    if let Some(controls) = controls.as_mut() {
        controls.fence_clear()?;
        let observation = controls.poll()?;
        if observation.muted {
            controls.persist_muted()?;
            return Err(Box::new(SoftwareMute {
                observed_at_us: monotonic_us(),
                asleep: observation.asleep,
            }));
        }
        controls.arm_active()?;
        // Runs before this one could not start: say so as this one tries,
        // before anything can fail again (#1567).
        if controls.could_not_start() > 0
            && let Some(mut health) = crate::bus_out::WaitHealth::from_environment()
        {
            health.report_could_not_start(monotonic_us(), controls.could_not_start());
        }
    }
    let directory = SessionDirectory::create()?;
    let boot = new_boot()?;
    if let Some(sink) = cues.as_mut() {
        sink.set_boot(boot);
    }
    record(
        trace,
        json!({"kind":"session_boot","at_us":monotonic_us(),"boot":boot}),
    )?;
    let mut owner = cycle_controller(boot);
    let mut startup = StartupEvents::default();
    let camera = camera_config.and_then(|path| {
        match PreparedCamera::load(path, &directory.path) {
            Ok(camera) => {
                let _ = record(trace, json!({"kind":"camera_context_configured", "at_us":monotonic_us(),
                    "config_sha256":camera.digest, "camera_worker":camera.source.camera,
                    "source":camera.source.source, "width":camera.source.width,"height":camera.source.height,
                    "capture_buffer_limit":camera.source.max_frame_bytes,"cloud_jpeg_limit":lamp_gemini::MAX_IMAGE_BYTES,
                    "dequeue_age_limit_us":crate::visual_context::MAX_FRAME_AGE_US}));
                Some(camera)
            },
            Err(_) => {
                // Configuration/device absence never silently turns into sight.
                let _ = record(trace, json!({"kind":"camera_unavailable", "at_us":monotonic_us(), "stage":"configuration", "audio_continues":true}));
                None
            }
        }
    });
    let head_options = motor_options.and_then(|m| m.head.as_ref());
    let head = head_options
        .map(|options| {
            let camera = camera
                .as_ref()
                .ok_or_else(|| io::Error::other("head qualification requires prepared camera"))?;
            crate::head_qualification::Driver::load(
                options,
                &camera.digest,
                camera.source.camera,
                boot,
            )
        })
        .transpose()?;
    let head_camera = head
        .as_ref()
        .and(camera.as_ref())
        .map(|c| {
            crate::head_qualification::Frames::new(
                c.source.camera,
                c.config.clone(),
                c.digest.clone(),
            )
        })
        .transpose()?;
    let motor = motor_options.filter(|_| *motor_rearm).and_then(|options| match PreparedMotor::load_bound(options, &directory.path, head.as_ref().map(crate::head_qualification::Driver::binding)) {
        Ok(prepared) => {
            let _ = record(trace, json!({"kind":"motor_configured","at_us":monotonic_us(),"identity":prepared.identity(),"automatic_motion":false}));
            Some(prepared)
        }
        Err(error) => {
            let _ = record(trace, json!({"kind":"motor_unavailable","at_us":monotonic_us(),"stage":"configuration","error":motor::bounded(error),"audio_continues":true}));
            *motor_rearm = false;
            None
        }
    });
    if head.is_some() && motor.is_none() {
        return Err(io::Error::other("head qualification motor provisioning failed").into());
    }
    let mut motor = motor.map(|prepared| OptionalMotor::new(boot, Some(prepared)));
    if let Some(driver) = head {
        record(
            trace,
            json!({"kind":"head_qualification_configured","at_us":monotonic_us(),"plan_sha256":driver.plan_sha256,"placement_binding":driver.binding(),"automatic_clearance_inference":false,"automatic_motion":false}),
        )?;
        motor.as_mut().expect("checked above").attach_head(driver)?;
    }
    let spawned = spawn_rig(
        source,
        &directory.path,
        boot,
        &mut owner,
        &mut startup,
        diagnostic_directory,
        options,
        camera,
        lifetime,
        termination,
        controls,
    );
    heard_if_spoke(&startup, controls);
    let mut rig = spawned?;
    rig.motor = motor;
    if let Some(frames) = head_camera {
        if let Some(camera) = &mut rig.camera {
            camera.retain_head_frames(frames);
        } else if let Some(motor) = &mut rig.motor {
            motor.begin_shutdown();
        }
    }
    let mut reply = None;
    let result = (|| {
        termination.check()?;
        // The trusted supervisor supplies both immutable peer boots. No process
        // learns reference identity from incoming PCM or an unsolicited handshake.
        rig.capture
            .channels
            .control
            .send(Control::ConnectReference {
                peer: rig.speaker.boot,
            })?;
        rig.speaker
            .channels
            .control
            .send(Control::ConnectReference {
                peer: rig.capture.boot,
            })?;
        conversation_with_controls(
            &mut rig,
            &mut owner,
            lifetime,
            trace,
            startup,
            &mut reply,
            cues,
            termination,
            notices,
            controls,
            reminders,
        )
    })();
    let privacy_mute = result
        .as_ref()
        .err()
        .is_some_and(|error| error.is::<PhysicalMute>() || error.is::<SoftwareMute>());
    // A stop systemd asked for (an update, a restart) that the conversation
    // ended cleanly: whatever cleanup then says, it was no crash (#960).
    // A signal may also end it at a `termination.check()`, as Interrupted.
    let interrupted = result.as_ref().err().is_some_and(|error| {
        error
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::Interrupted)
    });
    // #1567, A: read before any cleanup, as the conversation returns its
    // error, so a signal that comes during the cleanup does not count.
    let stop_came_first = released_by_stop_first(
        controls.as_ref(),
        result.is_err() && !privacy_mute,
        termination.signal_name(),
    );
    if stop_came_first {
        eprintln!(
            "lamp-voice: the run's fault came after {}; it is the stop's, and the mute is kept as it was",
            termination.signal_name().unwrap_or("the stop")
        );
    }
    let stop_path =
        guard_released_on_stop(result.is_ok() || interrupted, termination.signal_name());
    let requested_stop = stop_came_first || stop_path;
    // A alone, not the clean stop's own rule: its "nothing in flight" is
    // asked again at the release (the reviewer and Jarvis on #1592).
    let only_by_stop_first = stop_came_first && !stop_path;
    // A person's mute, accepted and revoked: the restart guard's marker goes
    // now, before teardown, so a crash in teardown cannot leave it for a tap
    // to open (#997). A removal that fails ends the cycle as a fault would.
    // Whatever the result: a run can end in a fault after the mute was taken
    // (a failed publish, a speaker socket gone), and `arm_active` refuses to
    // start muted, so a muted live latch here is always the person's (the
    // reviewer's P3 on #997).
    let chosen = controls
        .as_mut()
        .map_or(Ok(()), |controls| controls.mute_chosen());
    let result = finish_active_run(&mut rig, &mut owner, &mut reply, trace, cues, result);
    chosen?;
    if let Some(motor) = &rig.motor {
        *motor_rearm &= motor.may_rearm_after_cycle(privacy_mute);
    }
    // Reopening is allowed only after all child and local endpoint ownership
    // has ended. Never accumulate per-cycle output/IPC directories.
    drop(rig);
    let result = if lifetime.continuous() {
        service_privacy::finish(result, fs::remove_dir(&directory.path))
    } else {
        result
    };
    if (result.is_ok() || requested_stop)
        && let Some(controls) = controls.as_mut()
        && released_after_cleanup(controls, only_by_stop_first)?
        && result.is_err()
    {
        eprintln!(
            "lamp-voice: a requested stop's cleanup reported a fault; the mute is kept as it was, not restored muted"
        );
    }
    result
}

/// The guard released after the run's cleanup, as the latch was (#960).
/// When the stop-first rule (#1567, A) is the only reason, a control input
/// that came during the teardown, queued unread or taken and unsaved, keeps
/// the guard: the person pressed while the voice was going, and an
/// unmuted latch must not be written over it (Jarvis on #1592). Whether it
/// released.
fn released_after_cleanup(
    controls: &mut HandControls,
    only_by_stop_first: bool,
) -> io::Result<bool> {
    if !only_by_stop_first {
        controls.disarm_after_clean_stop()?;
        return Ok(true);
    }
    // From the look through the save, no press can be queued (#1607).
    let released = controls.release_if_still_quiet()?;
    if !released {
        eprintln!(
            "lamp-voice: a control came during the stop's teardown; the guard is kept, so the next start is muted"
        );
    }
    Ok(released)
}

/// A microphone that spoke before activation was open and gave audio: the
/// run heard, so its fault keeps the guard (#1567, the reviewer on #1592).
fn heard_if_spoke(startup: &StartupEvents, controls: &mut Option<HandControls>) {
    if startup.microphone_spoke {
        heard_a_frame(controls);
    }
}

/// The capture gave a frame: from here on this run heard, whatever it then
/// drops or refuses (#1567).
fn heard_a_frame(controls: &mut Option<HandControls>) {
    if let Some(controls) = controls.as_mut() {
        controls.heard_audio();
    }
}

/// Whether the restart guard is released as for a clean stop (#960): the
/// conversation ended because the service was asked to stop (SIGTERM from
/// systemd for an update or a restart, or SIGINT), cleanly or at a
/// termination check, not by a fault or a privacy event. Its cleanup may still report a fault (on lamp-4ace at
/// 09:40 on 2026-10-16, "endpoint worker exit not confirmed within 500 ms"):
/// that is not a crash, and the latch is written as it was: muted only if
/// it was muted. A fault or a privacy exit keeps the conservative guard.
fn guard_released_on_stop(ended_by_the_stop: bool, signal: Option<&str>) -> bool {
    ended_by_the_stop && signal.is_some()
}

/// With `LAMP_GUARD_NEVER_LISTENED=on` (#1567, A): a fault the
/// conversation returned once the stop's signal had come is the stop's, not
/// a crash: the workers go as the service stops, and what they say then is
/// part of it. (Not 08:34 on 18 Oct on lamp-4ace: that run had no signal,
/// the first builder read in its journal; its leases lapsed on their own.)
/// The signal handler sets its
/// flag as the signal comes and the conversation returns its error at once,
/// so a flag set when it returned came before the fault, to within that
/// return (the workspace allows no `unsafe`, so the handler keeps no time).
/// A fault returned before the signal keeps the guard, and a privacy exit
/// is never this.
fn stop_before_fault(faulted: bool, signal: Option<&str>) -> bool {
    faulted && signal.is_some()
}

/// A with its gates (#1567): the switch on, and no control input in flight,
/// as for B, so a press queued and not yet read is never lost to an
/// unmuted latch (the reviewer on #1592). `faulted` is a fault that is not
/// a mute; an unreadable switch is one A may release, since the switch is
/// read again at every start.
fn released_by_stop_first(
    controls: Option<&HandControls>,
    faulted: bool,
    signal: Option<&str>,
) -> bool {
    controls.is_some_and(|controls| controls.never_listened_on() && controls.nothing_in_flight())
        && stop_before_fault(faulted, signal)
}

fn service_head(
    rig: &mut Rig,
    owner: &mut Controller,
    privacy_allowed: bool,
    trace: &mut SessionState,
) -> Result<()> {
    let frame = rig.camera.as_ref().and_then(OptionalCamera::head_frame);
    if let Some(motor) = &mut rig.motor
        && let Some(mut head) = motor.head.take()
    {
        let events = head.tick(owner, motor, frame, privacy_allowed);
        motor.head = Some(head);
        for event in events {
            record(trace, event)?;
        }
    }
    Ok(())
}

fn service_motor(rig: &mut Rig, trace: &mut SessionState) -> Result<()> {
    if let Some(motor) = &mut rig.motor {
        let mut error = None;
        motor.poll(|event| {
            if let Err(e) = record(trace, json!({"kind":"motor_lifecycle","at_us":monotonic_us(),"details":event,"listening_readiness":false,"physical_stop_verified":false})) { error.get_or_insert(e); }
        });
        let worker = motor.identity().map(|identity| identity.worker);
        if let Some(reason) = motor.take_failure() {
            record(
                trace,
                json!({"kind":"motor_degraded","at_us":monotonic_us(),"worker":worker,"reason":reason,"audio_continues":true,"physical_stop_verified":false}),
            )?;
        }
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(())
}

fn finish_active_run(
    rig: &mut Rig,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    result: Result<()>,
) -> Result<()> {
    if result.is_err() {
        // Keep an explicit outcome even when the failing child cannot deliver
        // speech. Privacy revocation and stop below still run if tracing fails.
        let reason = if result
            .as_ref()
            .err()
            .is_some_and(|error| error.downcast_ref::<PhysicalMute>().is_some())
        {
            "privacy_muted"
        } else {
            "runtime_failed"
        };
        let _ = cancel_reply(owner, reply, trace, reason);
    }
    // Also close a yield with no active reply, including any error exit. Lost
    // observation storage still cannot inhibit privacy revocation or cleanup.
    let _ = finish_duplex_yield(
        trace,
        monotonic_us(),
        if result.is_err() {
            "session_failed"
        } else {
            "session_ended"
        },
        None,
        None,
    );
    let _ = owner.set_camera_permission(now(), Permission::Denied);
    let _ = owner.set_microphone_permission(now(), Permission::Denied);
    if let Ok(snapshot) = owner.snapshot(now()) {
        let _ = rig.publish(snapshot);
    }
    flush_cues(trace, cues);
    let shutdown = rig.shutdown(trace);
    service_privacy::finish(result, shutdown)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn conversation(
    rig: &mut Rig,
    owner: &mut Controller,
    lifetime: Lifetime,
    trace: &mut SessionState,
    startup: StartupEvents,
    reply: &mut Option<Reply>,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    notices: &FailureNotices,
) -> Result<()> {
    conversation_with_controls(
        rig,
        owner,
        lifetime,
        trace,
        startup,
        reply,
        cues,
        termination,
        notices,
        &mut None,
        &mut None,
    )
}

#[allow(clippy::too_many_arguments)]
fn conversation_with_controls(
    rig: &mut Rig,
    owner: &mut Controller,
    lifetime: Lifetime,
    trace: &mut SessionState,
    startup: StartupEvents,
    reply: &mut Option<Reply>,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    notices: &FailureNotices,
    controls: &mut Option<HandControls>,
    reminders: &mut Option<crate::reminders::Reminders>,
) -> Result<()> {
    conversation_with_gate(
        rig,
        owner,
        lifetime,
        trace,
        startup,
        reply,
        cues,
        termination,
        notices,
        controls,
        reminders,
        PrivacyGate::default(),
        #[cfg(test)]
        None,
    )
}

/// An explicit policy allowance for a real-worker ordering fixture. The
/// source clock and IPC bounds stay real; the watchdog bounds the host run.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn conversation_with_fixture_privacy(
    rig: &mut Rig,
    owner: &mut Controller,
    lifetime: Lifetime,
    trace: &mut SessionState,
    startup: StartupEvents,
    reply: &mut Option<Reply>,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    notices: &FailureNotices,
    gate: PrivacyGate,
    host_deadline: Instant,
) -> Result<()> {
    conversation_with_gate(
        rig,
        owner,
        lifetime,
        trace,
        startup,
        reply,
        cues,
        termination,
        notices,
        &mut None,
        &mut None,
        gate,
        Some(host_deadline),
    )
}

/// For an utterance said close to the lamp, its loud blocks the detector did
/// not call speech; `None` when it was not. The utterance began at the
/// oldest speech block kept, as the blocks are cleared at each end.
fn said_close(
    speech_levels: &VecDeque<(u64, u8)>,
    loud_blocks: &VecDeque<u64>,
    rise: &crate::room_floor::Rise,
) -> Option<usize> {
    let (began, _) = *speech_levels.front()?;
    rise.close_since(began)
        .then(|| loud_blocks.iter().filter(|at| **at >= began).count())
}

#[cfg(test)]
thread_local! {
    /// A loop test's full look, in place of the environment: the still's
    /// file. Thread-local, so no other test's loop takes it.
    static TEST_FULL_LOOK: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
    /// A loop test's eyes, with no eyes' link: each still asked for is
    /// written at once to `TEST_FULL_LOOK`'s file, with its ask's id.
    static TEST_EYES_ANSWER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_UNCLEAR_HUM=on` (#1421), as `TEST_FULL_LOOK`.
    static TEST_UNCLEAR_HUM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_PRIVACY_GAP_RIDE=on` (#2602), as `TEST_FULL_LOOK`.
    static TEST_PRIVACY_GAP_RIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_MISHEARD_HUM=on` (#828), as `TEST_FULL_LOOK`.
    static TEST_MISHEARD_HUM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_HOLDING_ANSWER=on`, without changing process-wide flags.
    static TEST_HOLDING_ANSWER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_SECOND_LOOK_NOTE=on`, as `TEST_HOLDING_ANSWER`.
    static TEST_LOOK_NOTE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_SECOND_LOOK_WALK=on`, as `TEST_LOOK_NOTE`.
    static TEST_LOOK_WALK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's skills, in place of lamp-skills' socket, for the
    /// walk's calls: taken by the loop as it starts.
    static TEST_WALK_SKILL: std::cell::RefCell<Option<look_note::SkillCaller>> =
        const { std::cell::RefCell::new(None) };
    /// A loop test's task news, in place of the tasks part, as
    /// `TEST_FULL_LOOK`: taken by the loop as it starts.
    static TEST_TASK_NEWS: std::cell::RefCell<Option<crate::task_news::TaskNews>> =
        const { std::cell::RefCell::new(None) };
    /// A loop test's `LAMP_TASK_NEWS_AT_PAUSE=on` (#560), as `TEST_FULL_LOOK`.
    static TEST_NEWS_AT_PAUSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's say service (`LAMP_SAY=on`, #939), as `TEST_TASK_NEWS`.
    static TEST_SAYS: std::cell::RefCell<Option<crate::say::Says>> =
        const { std::cell::RefCell::new(None) };
    /// A loop test's eyes link, as `TEST_TASK_NEWS`, with its
    /// `LAMP_CAMERA_FOLLOWS_EYES=on` (#2233).
    static TEST_EYES: std::cell::RefCell<Option<crate::body_link::EyesLink>> =
        const { std::cell::RefCell::new(None) };
    static TEST_CAMERA_FOLLOWS_EYES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_WAIT_KEEPS_ANSWER=on` (#1123), as
    /// `TEST_CAMERA_FOLLOWS_EYES`.
    static TEST_WAIT_KEEPS_ANSWER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's room busy with other people's talk, for the busy-room
    /// hold, as `TEST_CAMERA_FOLLOWS_EYES`: the fixture plays no three
    /// unanswered lines.
    static TEST_ROOM_BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_OWN_LINE_OVER_TALK=on`, as `TEST_ROOM_BUSY`.
    static TEST_OWN_LINE_OVER_TALK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's `LAMP_VOICE_TONE=on`, as `TEST_CAMERA_FOLLOWS_EYES`.
    static TEST_VOICE_TONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A loop test's known person at the lamp for the voice's tone, by
    /// memory id, in place of the eyes' file.
    static TEST_TONE_PERSON: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    /// A loop test's near-call try (#1803), in place of onboarding's own.
    static TEST_NEAR: std::cell::RefCell<Option<crate::onboarding::near::NearFlow>> =
        const { std::cell::RefCell::new(None) };
    /// How many times a loop test's call lines were told ended to its
    /// near-call try (`near_call_line_ended`).
    static TEST_LINE_ENDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// A loop test's open sentence window (#1803), in the scenario's
    /// monotonic microseconds, in place of onboarding's own.
    static TEST_SENTENCE_WINDOW: std::cell::Cell<Option<(u64, u64)>> =
        const { std::cell::Cell::new(None) };
    /// A loop test's forgets from memory (`memory/forgot`), one for each
    /// End in turn, told just before it: `None`, nothing at that End.
    static TEST_TONE_FORGOT: std::cell::RefCell<VecDeque<Option<String>>> =
        const { std::cell::RefCell::new(VecDeque::new()) };
    /// A loop test's reader of memory's forgets on a bus of its own, in
    /// place of the environment's (`memory/forgot`).
    pub(super) static TEST_FORGOT_IN: std::cell::RefCell<Option<crate::bus_out::ForgotIn>> =
        const { std::cell::RefCell::new(None) };
    /// A loop test's reader of the body's pushes on a bus of its own, in
    /// place of the environment's (`body/hand`).
    pub(super) static TEST_HAND_IN: std::cell::RefCell<Option<crate::search_pushed::HandIn>> =
        const { std::cell::RefCell::new(None) };
}

/// Memory has forgotten everything under `person` (`memory/forgot`), or
/// every person's (`None`): their tone baseline goes, or every one, and
/// memory's "everyone" is the room's. Anything else it forgot (a rules
/// scope, `r-`) is no voice. The record never names the person.
fn tone_forgotten(
    baselines: &mut crate::voice_tone::Baselines,
    person: Option<&str>,
    trace: &mut SessionState,
    at_us: u64,
) -> Result<()> {
    let whose = match person {
        None => {
            baselines.forget_all();
            None
        }
        Some("everyone") => {
            baselines.forget(None);
            Some(crate::voice_tone::Whose::Room)
        }
        Some(person) if lamp_msgs::known::is_person_id(person) => {
            baselines.forget(Some(person));
            Some(crate::voice_tone::Whose::Person)
        }
        Some(_) => return Ok(()),
    };
    record(
        trace,
        json!({"kind":"voice_tone_forgot","all":person.is_none(),"whose":whose,"by":"memory","at_us":at_us}),
    )
}

/// The most of a person's line kept to read its words: far more than a
/// sentence. A piece that would pass it is left out.
const TONE_LINE_BYTES: usize = 4_096;
/// A line's last pieces can come a while after its End, as the recogniser
/// catches up: none for this long after it, and the line is over. The next
/// line beginning ends it at once, whatever the quiet, so a pause the
/// person makes is never what splits a line (the reviewer-lead's P3 on
/// #2600).
const TONE_LINE_QUIET_US: u64 = 1_000_000;

/// The person's line, its pieces end to end, for its tone and a forget
/// (Thirteen's duplex finding on #1722): the turn of the lamp's line it was
/// heard in, and when its last piece came. A piece heard in another turn's
/// line, or none, begins another (the reviewer-lead's P2 on #2600), so two
/// lines never join, nor does a line's forget wait past the next one's End.
/// Words heard with no lamp line yet, just before one begins, are its first
/// (the reviewer-lead's P3 on #2608).
#[derive(Debug, Default)]
struct ToneLine {
    words: String,
    turn: Option<u64>,
    last_us: Option<u64>,
}

impl ToneLine {
    /// A piece heard at `now_us` while `turn` is the lamp's line. The line
    /// before, if it was heard in another turn's, is given back to be read
    /// first.
    fn heard(
        &mut self,
        piece: &str,
        turn: Option<u64>,
        now_us: u64,
    ) -> Option<(String, Option<u64>)> {
        let begun = self.begins(turn, now_us);
        let before = (self.last_us.is_some() && self.turn != turn && !begun).then(|| self.take());
        if self.words.len() + piece.len() <= TONE_LINE_BYTES {
            self.words.push_str(piece);
        }
        self.turn = turn;
        self.last_us = Some(now_us);
        before
    }

    /// Whether this line, heard while there was no lamp line, is the first
    /// of `turn`'s, begun at `now_us`: its last piece came within
    /// [`TONE_LINE_QUIET_US`].
    fn begins(&self, turn: Option<u64>, now_us: u64) -> bool {
        self.turn.is_none()
            && turn.is_some()
            && self
                .last_us
                .is_some_and(|last| now_us.saturating_sub(last) < TONE_LINE_QUIET_US)
    }

    /// Whether the line is over at `now_us`, with `turn` the lamp's line
    /// now and `ended` whether its End has come: the turn it was heard in is
    /// no longer the lamp's (another began, or it was let go), or no piece
    /// has come for [`TONE_LINE_QUIET_US`] once its End has. Pieces heard
    /// while a line is still being said wait for its End. A line heard with
    /// no lamp line that has just begun one becomes that line's
    /// ([`ToneLine::begins`]).
    fn over(&mut self, now_us: u64, turn: Option<u64>, ended: bool) -> bool {
        if self.begins(turn, now_us) {
            self.turn = turn;
        }
        self.last_us.is_some_and(|last| {
            self.turn != turn || (ended && now_us.saturating_sub(last) >= TONE_LINE_QUIET_US)
        })
    }

    /// The line's words and the turn they were heard in; a new line begun.
    fn take(&mut self) -> (String, Option<u64>) {
        self.last_us = None;
        (std::mem::take(&mut self.words), self.turn.take())
    }
}

/// The person's line is over. The reading waiting for its words is told
/// with them when it is the reading of the line they were heard in
/// (`heard_in`): words heard while another line is the lamp's are that
/// line's, so the waiting one is told without them. And asked to forget
/// their face, or everything, their own baseline goes (the room's for
/// someone not known), or every one. The words decide, before any function
/// runs. Never the words or the person.
fn tone_line_over(
    line: &str,
    heard_in: Option<u64>,
    held: &mut ToneHeld,
    baselines: &mut crate::voice_tone::Baselines,
    person: Option<&str>,
    trace: &mut SessionState,
) -> Result<()> {
    held.heard(line, heard_in);
    let Some(forget) = crate::voice_tone::forgets(line) else {
        return Ok(());
    };
    // A forget is kept: a line that goes on puts back no baseline from
    // before it.
    if let Some(held) = held.reading.as_mut() {
        held.before = None;
    }
    let all = forget == crate::voice_tone::Forget::All;
    if all {
        baselines.forget_all();
    } else {
        baselines.forget(person);
    }
    record(
        trace,
        json!({"kind":"voice_tone_forgot","all":all,"by":"words",
            "whose":(!all).then_some(if person.is_some() {
                crate::voice_tone::Whose::Person
            } else {
                crate::voice_tone::Whose::Room
            }),"at_us":monotonic_us()}),
    )
}

/// What the voice holds of the person's lines for their tone: the reading
/// waiting to be told, and a later line's words heard before its End.
#[derive(Debug, Default)]
struct ToneHeld {
    reading: Option<TonePending>,
    later: Option<(u64, String)>,
}

impl ToneHeld {
    /// The person's line `line`, heard while `heard_in` was the lamp's line:
    /// the waiting reading's own words, a later line's kept for its
    /// reading, or another's, after which its own are taken as not coming.
    /// Nothing is told here.
    fn heard(&mut self, line: &str, heard_in: Option<u64>) {
        let ToneHeld {
            reading: pending,
            later,
        } = self;
        let held_turn = pending.as_ref().map(TonePending::turn);
        if let Some(held) = pending.as_mut()
            && heard_in.is_some_and(|turn| held.turns.contains(&turn))
        {
            held.heard(line, heard_in == Some(held.turn()));
        } else if let (Some(turn), Some(held_turn)) = (heard_in, held_turn)
            && turn > held_turn
        {
            // A later line's words, before its End: kept for its reading,
            // which may yet go on from the one waiting, so that one isn't
            // told for them.
            let words = later.get_or_insert_with(|| (turn, String::new()));
            if words.0 == turn && words.1.len() + line.len() < TONE_LINE_BYTES {
                words.1.push_str(line);
            }
        } else if let Some(held) = pending.as_mut() {
            // Words of no lamp line, or an older one's: not its own, so its
            // own are taken as not coming. It is told as it falls due, never
            // here, so a line that still goes on from it is taken in once
            // (the reviewer-lead's P3 on #2670).
            held.worded = true;
        }
    }
}

/// A line's reading, waiting to be told: until its words come, and for
/// [`crate::voice_tone::GOES_ON_US`] after its End, since a line that goes
/// on from it is read over both and told once, under the last one's turn
/// (the first builder on #2602, 21 Oct 21:49). In memory only.
#[derive(Debug)]
struct TonePending {
    /// The turns its line was heard in, the first first: one, or more when
    /// a line went on from the one before.
    turns: Vec<u64>,
    reading: crate::voice_tone::Reading,
    ended_us: u64,
    /// Its words so far, from any of its turns, and whether its last turn's
    /// have come.
    words: Option<String>,
    worded: bool,
    /// The baselines before its line was taken in, put back when the next
    /// line goes on from it; none once a forget came, which is kept.
    before: Option<crate::voice_tone::Baselines>,
}

impl TonePending {
    /// The turn it is told under: its last.
    fn turn(&self) -> u64 {
        self.turns.last().copied().unwrap_or_default()
    }

    /// Words heard in one of its turns, its last turn's if `last`.
    fn heard(&mut self, line: &str, last: bool) {
        let words = self.words.get_or_insert_with(String::new);
        if words.len() + line.len() < TONE_LINE_BYTES {
            if !words.is_empty() {
                words.push(' ');
            }
            words.push_str(line);
        }
        self.worded |= last;
    }

    /// Whether it is told now, at `now_us`: its words have come, no line is
    /// going on from it, and none can any more (the lamp answered, or the
    /// line was let go) or the window for one has passed.
    fn due(&self, now_us: u64, meter: Option<&crate::voice_tone::ToneMeter>) -> bool {
        let going_on = meter.is_some_and(crate::voice_tone::ToneMeter::going_on);
        let may_go_on = meter.is_some_and(crate::voice_tone::ToneMeter::may_go_on);
        self.worded
            && !going_on
            && (!may_go_on || now_us.saturating_sub(self.ended_us) >= crate::voice_tone::GOES_ON_US)
    }

    fn tell(
        self,
        trace: &mut SessionState,
        out: Option<&mut crate::bus_out::ToneOut>,
    ) -> Result<()> {
        let turn = self.turn();
        tell_tone(trace, out, turn, self.reading, self.words.as_deref())
    }
}

/// A line's tone told, in the session record and on the bus: with its
/// words' valence, or `words` `None` when they never came. Never the words.
fn tell_tone(
    trace: &mut SessionState,
    out: Option<&mut crate::bus_out::ToneOut>,
    turn: u64,
    reading: crate::voice_tone::Reading,
    words: Option<&str>,
) -> Result<()> {
    let told =
        crate::voice_tone::told(turn, reading, words.and_then(crate::voice_tone::valence_of));
    record(
        trace,
        json!({"kind":"voice_tone","turn":turn,"arousal":told.arousal,"sure":told.sure,
            "valence":told.valence,"words":words.is_some(),"at_us":monotonic_us()}),
    )?;
    if let Some(out) = out {
        out.tell(monotonic_us(), &told);
    }
    Ok(())
}

/// Whether onboarding's near-call step holds the reply to the utterance
/// whose speech began at `began_us`, at `at_us` (#1803): its try window is
/// open and the lamp's name was heard as it began, so it is the person
/// calling the lamp for the measurement, and the lamp's answer would spoil
/// the try. Nothing outside the step is held.
fn near_call_window(
    onboarding: Option<&crate::onboarding::Onboarding>,
    began_us: Option<u64>,
    at_us: u64,
) -> bool {
    let Some(began_us) = began_us else {
        return false;
    };
    #[cfg(test)]
    if let Some(held) = TEST_NEAR.with(|near| {
        near.borrow()
            .as_ref()
            .map(|flow| flow.holds_reply(began_us, at_us))
    }) {
        return held;
    }
    onboarding.is_some_and(|walk| walk.near_holds_reply(began_us, at_us))
}

/// Whether onboarding's `how_you_sound` waits for a call now (#3504), from
/// its name prompt to its try's judgement: a line begun now is told the
/// second route as `StepCall`, so only the lamp's name is never answered.
fn step_call(onboarding: Option<&crate::onboarding::Onboarding>) -> bool {
    #[cfg(test)]
    if let Some(waits) = TEST_NEAR.with(|near| {
        near.borrow()
            .as_ref()
            .map(crate::onboarding::near::NearFlow::waits_for_call)
    }) {
        return waits;
    }
    onboarding.is_some_and(crate::onboarding::Onboarding::near_waits_for_call)
}

/// A call's line begun at `began_us` whose words were the lamp's name alone
/// (#3923): the step's call heard by speech to text. Its Unix start when the
/// step took it; why not, otherwise.
fn near_text_call(
    onboarding: Option<&mut crate::onboarding::Onboarding>,
    (began_us, ended_us): (u64, u64),
) -> std::result::Result<u64, &'static str> {
    #[cfg(test)]
    if let Some(taken) = TEST_NEAR.with(|near| {
        near.borrow_mut()
            .as_mut()
            .map(|flow| flow.text_call(began_us, ended_us))
    }) {
        return taken;
    }
    onboarding.map_or(Err("no walk"), |walk| {
        walk.near_text_call(began_us, ended_us)
    })
}

/// The step's call lines kept at once: the words of a line begun before the
/// one before it had its words back still find their line.
const STEP_CALL_LINES: usize = 4;

/// A call's line begun at `began_us`, whose turn ended with its words more
/// than the name, or none (#3923): its try waits no longer for them.
fn near_call_words_back(onboarding: Option<&mut crate::onboarding::Onboarding>, began_us: u64) {
    #[cfg(test)]
    if TEST_NEAR.with(|near| {
        near.borrow_mut()
            .as_mut()
            .map(|flow| flow.call_words_back(began_us))
            .is_some()
    }) {
        return;
    }
    if let Some(walk) = onboarding {
        walk.near_call_words_back(began_us);
    }
}

/// A call's line begun at `began_us` ended (#3923): the step waits a moment
/// for its words before judging its try.
fn near_call_line_ended(onboarding: Option<&mut crate::onboarding::Onboarding>, began_us: u64) {
    #[cfg(test)]
    TEST_LINE_ENDS.with(|ends| ends.set(ends.get() + 1));
    #[cfg(test)]
    if TEST_NEAR.with(|near| {
        near.borrow_mut()
            .as_mut()
            .map(|flow| flow.call_line_ended(began_us))
            .is_some()
    }) {
        return;
    }
    if let Some(walk) = onboarding {
        walk.near_call_line_ended(began_us);
    }
}

/// Whether the sentence part's window of onboarding's `how_you_sound` is
/// open at `at_us` (#1803): a line that ends in it is the person's sentence.
/// Unlike a name in the near-call window, its End goes on, so the words
/// come back; its answer is held (the turn ends as its words arrive, or as
/// its first answer audio does), and the words are counted, never kept.
fn sentence_window(onboarding: Option<&crate::onboarding::Onboarding>, at_us: u64) -> bool {
    #[cfg(test)]
    if let Some((from, to)) = TEST_SENTENCE_WINDOW.with(std::cell::Cell::get) {
        return (from..to).contains(&at_us);
    }
    onboarding.is_some_and(|walk| walk.sentence_window(at_us))
}

#[allow(clippy::too_many_arguments)]
fn conversation_with_gate(
    rig: &mut Rig,
    owner: &mut Controller,
    lifetime: Lifetime,
    trace: &mut SessionState,
    startup: StartupEvents,
    reply: &mut Option<Reply>,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    notices: &FailureNotices,
    controls: &mut Option<HandControls>,
    reminders: &mut Option<crate::reminders::Reminders>,
    gate: PrivacyGate,
    #[cfg(test)] host_deadline: Option<Instant>,
) -> Result<()> {
    // Keep the pending cue outside the conversation body so ordinary and
    // error returns can record why a consumed wake was cancelled.
    let mut waking = None;
    let result = conversation_with_gate_inner(
        rig,
        owner,
        lifetime,
        trace,
        startup,
        reply,
        cues,
        termination,
        notices,
        controls,
        reminders,
        gate,
        #[cfg(test)]
        host_deadline,
        &mut waking,
    );
    if let Some((woken_us, _)) = waking {
        let recorded = trace.record(json!({
            "kind":"head_wake_cue","acquired_at_us":woken_us,"at_us":monotonic_us(),
            "plays":false,"why":"conversation_ended",
            "boundary":"the waking input's acquisition to this decision; no sound was committed"
        }));
        // Preserve the original result if the recorder accepts the row
        // (including its existing bounded-drop policy). A recorder error
        // must not retain a physical-mute restart token.
        return service_privacy::finish(result, recorded);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn conversation_with_gate_inner(
    rig: &mut Rig,
    owner: &mut Controller,
    lifetime: Lifetime,
    trace: &mut SessionState,
    startup: StartupEvents,
    reply: &mut Option<Reply>,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    notices: &FailureNotices,
    controls: &mut Option<HandControls>,
    reminders: &mut Option<crate::reminders::Reminders>,
    mut gate: PrivacyGate,
    #[cfg(test)] host_deadline: Option<Instant>,
    waking: &mut Option<(u64, u64)>,
) -> Result<()> {
    let mut cloud = startup.cloud;
    let mut output_flow = startup.output_flow;
    let requested_audio =
        crate::requested_audio::Service::from_environment().unwrap_or_else(|error| {
            eprintln!("lamp-voice: requested audio unavailable: {error}");
            None
        });
    for event in startup.events {
        record(trace, event)?;
    }
    let mut session_clock =
        SessionClock::new(lifetime, session_instant(), INPUT_READINESS_TIMEOUT)?
            .with_start_offline(crate::session_lifetime::start_offline());
    // Said once when a service started with no link goes on listening.
    let mut said_waiting_for_link = false;
    // A service that speaks for itself when the internet cannot be reached
    // (#830): the offline line once an outage, a local-only turn for each
    // try, and the ring's offline breath while listening.
    let offline_voice = lifetime.continuous()
        && std::env::var(OFFLINE_VOICE_ENVIRONMENT).ok().as_deref() == Some("on");
    let mut outage = Outage::default();
    // With `LAMP_REACH_PROBE=on`: a link down while the internet answers is
    // not the home's outage, and the voice waits for the probe's verdict
    // before it says one (#830).
    let reach_on =
        offline_voice && std::env::var(REACH_PROBE_ENVIRONMENT).ok().as_deref() == Some("on");
    let mut reach = reach_verdict(reach_on, lamp_bus::Bus::from_environment());
    // With `LAMP_SERVICE_DOWN_LINE=on` too: a link down while the internet
    // answers says the service's line once instead of nothing (#830).
    let service_down_line =
        reach_on && std::env::var(SERVICE_DOWN_LINE_ENVIRONMENT).ok().as_deref() == Some("on");
    // Since when the link has been down, for the grace the bus also gives it.
    let mut link_down_since: Option<u64> = None;
    // Since when "I'm back online." is owed, if it is.
    let mut back_owed_since: Option<u64> = None;
    let privacy_started = session_instant();
    let mut privacy_sequence = 0u64;
    let mut onboarding_privacy_receipt: Option<crate::wire::PrivacyReceipt> = None;
    let mut permitted = false;
    // A gap in the switch's readings ridden with the capture's lease let
    // lapse (`LAMP_PRIVACY_GAP_RIDE=on`, #2602), on the Controls route only;
    // and from when frames are taken again after one (none read before).
    #[cfg(test)]
    let gap_ride_on = crate::privacy_gap::enabled_from_environment()
        || TEST_PRIVACY_GAP_RIDE.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let gap_ride_on = crate::privacy_gap::enabled_from_environment();
    let mut gap_ride = crate::privacy_gap::GapRide::new(
        gap_ride_on && rig.privacy_controls && lifetime.continuous(),
    );
    let mut frames_from_us = 0u64;
    // A privacy observation read stale before the microphone was first
    // allowed: said once (lamp-4ace, 19 Oct 01:19).
    let mut said_stale_at_start = false;
    let mut capture_requested = false;
    let mut capture_epoch = None;
    let mut last_capture = None;
    let mut capture_gap = CaptureGap::default();
    // The epoch a stall closed: its frames still in flight are dropped.
    let mut closed_epoch = None;
    // A gap cut into someone's utterance: it is abandoned as a hand Stop
    // abandons it, never glued across the gap.
    let mut gap_cut_utterance = false;
    // An utterance a gap cut was abandoned: its rest is dropped after it.
    let mut kept_through_gap = false;
    // This gap has already cancelled what it cut: once per gap.
    let mut gap_cancelled = false;
    let mut last_publish = 0;
    let mut last_health = 0;
    let mut last_hand_health = 0;
    let mut choreography = if rig.ring.is_active() {
        rig.ring_channel_ceiling
            .map(RingChoreographer::new)
            .transpose()?
    } else {
        None
    };
    let mut ring_trace = RingTrace::default();
    // How loud the person's last captured block was, for the ring's face.
    let mut heard_level = 0u8;
    let mut speech_levels: VecDeque<(u64, u8)> = VecDeque::new();
    // When loud blocks the detector did not call speech came, for an
    // utterance said close to the lamp (`not_speech::scrap`).
    let mut loud_blocks: VecDeque<u64> = VecDeque::new();
    // When the detector last called a block speech.
    let mut last_speech_us = 0u64;
    let reminder_cycle_started_us = monotonic_us();
    // `--fixture-at-ms` (diagnostic only): when the fixture's own turn is
    // due, N ms after `listening_ready`; set once, when that is recorded.
    let mut fixture_at_ms = rig.fixture_at_ms;
    let mut fixture_due_us = None;
    // A service that runs all day may also tell a body what it is doing.
    let mut body_link = if lifetime.continuous() {
        crate::body_link::BodyLink::from_environment()
    } else {
        None
    };
    // And its eyes that they may be open, while the microphone is.
    let mut eyes_link = if lifetime.continuous() {
        crate::body_link::EyesLink::from_environment()
    } else {
        None
    };
    #[cfg(test)]
    if let Some(given) = TEST_EYES.with(|eyes| eyes.borrow_mut().take()) {
        eyes_link = Some(given);
    }
    #[cfg(not(test))]
    let follows_eyes = camera_follows_eyes();
    #[cfg(test)]
    let follows_eyes = camera_follows_eyes() || TEST_CAMERA_FOLLOWS_EYES.with(std::cell::Cell::get);
    let mut attention = crate::body_link::Attention::default();
    // The same count kept whether or not there is a body to tell: whether
    // the talk in the room has been for the lamp lately.
    let mut room = crate::body_link::Attention::default();
    // Short utterances in a room that is not busy go to the model after all
    // (`LAMP_SHORT_SOUNDS=quiet-room`, off unless set).
    let short_sounds = crate::not_speech::short_sounds_from_environment();
    // A new sound waits while an answer is awaited unheard
    // (`LAMP_WAIT_KEEPS_ANSWER=on`, #1123), and the candidate it was last
    // recorded for, so that the record says so once. With `words`, its
    // words are heard too.
    let (wait_keeps_answer, wait_hears_words) = wait_keeps_answer_from_environment();
    #[cfg(test)]
    let wait_keeps_answer = wait_keeps_answer || TEST_WAIT_KEEPS_ANSWER.with(std::cell::Cell::get);
    let mut answer_kept_for: Option<crate::admission::CandidateId> = None;
    // A new sound waits while the lamp's own line has not sounded
    // (`LAMP_OWN_LINE_KEEPS=on`, #2220), and the candidate it was last
    // recorded for.
    let own_line_keeps = own_line_keeps_from_environment();
    let mut own_kept_for: Option<crate::admission::CandidateId> = None;
    // The sound last let go while the own line waited, at its deadline or as
    // it sounded, while it goes on (#2602).
    let mut own_let_go: Option<LetGoSound> = None;
    // The newest turn the lamp began itself with no ticket or news on it:
    // a line of the setup walk.
    let mut lamp_began: Option<u64> = None;
    // The eyes' faces, for the person an own line is for (#2287's
    // follow-up): read every 100 ms off the audio path, only with either
    // switch; passing ids and booleans, nothing kept past 30 s.
    // Talk over the lamp's own line in a busy room is let go
    // (`LAMP_OWN_LINE_OVER_TALK=on`), and whether the room was busy as the
    // line under way began: its turn, and that.
    let own_line_over_talk = own_line_over_talk_from_environment();
    #[cfg(test)]
    let own_line_over_talk =
        own_line_over_talk || TEST_OWN_LINE_OVER_TALK.with(std::cell::Cell::get);
    let mut own_line_busy: Option<(u64, bool)> = None;
    // The line whose talk let go as a busy room's the journal was told of.
    let mut busy_room_told: Option<u64> = None;
    // When `lamp-name` last heard a stop word beginning, for that.
    let mut stop_began_us: Option<u64> = None;
    let own_line_eyes = (own_line_keeps || own_line_over_talk)
        .then(|| std::sync::Arc::new(crate::speaking_faces::SpeakingFaces::default()));
    let _own_line_reader = own_line_eyes.as_ref().and_then(|eyes| {
        let root = lamp_bus::Bus::from_environment().ok()?.root().to_path_buf();
        crate::speaking_faces::read_from(std::sync::Arc::clone(eyes), &root)
    });
    // The candidate last decided as the own line's person, recorded once.
    let mut own_theirs_for: Option<crate::admission::CandidateId> = None;
    // When the lamp's name was last heard beginning, and when the sound
    // last let go had stopped, for the wait.
    let mut name_began_us: Option<u64> = None;
    let mut let_go_ended_us: Option<u64> = None;
    // The candidate decided at once when the answer began to sound.
    let mut answer_sounded_for: Option<crate::admission::CandidateId> = None;
    // The turn whose speech last ended and when, and the turn for which the
    // floor was last held, so that the record says so once.
    let mut speech_ended: Option<(u64, u64)> = None;
    let mut floor_held_for: Option<u64> = None;
    // The turn the lamp last ended, for speech that goes on with it
    // (`LAMP_CUT_KEEPS_NAME`, #782). Never kept without the switch.
    let mut ended_turn: Option<EndedTurn> = None;
    // The turn whose words ended before a name, and when its speech ended,
    // for the name said next (`LAMP_SECOND_OPEN_NAME`, [`open_name`]).
    let mut open_name_turn: Option<(u64, u64)> = None;
    // The turn in hand that was only the lamp's name, and when its speech
    // ended (`LAMP_CALL_TAKES_NEXT_LINE`).
    let mut call_turn: Option<(u64, u64)> = None;
    // The turn that was asked while the room was busy, and when the lamp
    // began to be heard answering it.
    let mut asked_in_busy_room: Option<u64> = None;
    let mut busy_answer_since: Option<(u64, u64)> = None;
    let mut key_refusals = key_refusals::KeyRefusals::default();
    // With `LAMP_SECOND_REFUSED_HOLD=on`: the model service refusing every
    // turn is said once and held, not failed turn by turn (#4265).
    let refused_hold = std::env::var(model_refusing::ENVIRONMENT).ok().as_deref() == Some("on");
    let mut model_refusing = model_refusing::ModelRefusing::default();
    // When the lamp was last speaking, and the turn that took the place of
    // another: neither a word after its own nor one that cut in is a sound.
    let mut last_spoke_us: Option<u64> = None;
    // When a hand or the head last stopped the lamp's speech: a short sound
    // just after is the tap's own, not an answer (#829, 16:33).
    let mut stopped_at_us: Option<u64> = None;
    // A Stop the model has not been told of yet: the next request on the
    // session it cut tells it (`LAMP_STOP_TOLD=on`, off until it is played).
    let stop_told = crate::not_speech::stop_told_from_environment();
    let mut stop_untold = crate::not_speech::StopToTell::default();
    // What was played of a reply that a line or a Stop cut is told to that
    // line, or to the next on the session the Stop cut
    // (`LAMP_SAID_AS_HEARD=on`, `crate::said_as_heard`).
    let said_as_heard = crate::said_as_heard::from_environment();
    let mut stop_cut: Option<(crate::said_as_heard::HeardOfCut, u64)> = None;
    let mut spoke_in_turn: Option<u64> = None;
    // The head tap's "hm?" (#711): its sound, made once for the cycle when
    // the head is read, and when it last played, so it plays at most once
    // in 30 s.
    // Made for an empty "Sorry?" too (`LAMP_UNCLEAR_HUM=on`, #1417).
    #[cfg(test)]
    let unclear_hum_on =
        crate::head_cue::unclear_from_environment() || TEST_UNCLEAR_HUM.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let unclear_hum_on = crate::head_cue::unclear_from_environment();
    // A misheard cut-in's "hm?" in place of the model's "Sorry?"
    // (`LAMP_MISHEARD_HUM=on`, #828).
    #[cfg(test)]
    let misheard_hum_on = crate::misheard_hum::enabled_from_environment()
        || TEST_MISHEARD_HUM.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let misheard_hum_on = crate::misheard_hum::enabled_from_environment();
    let head_cue = if controls.as_ref().is_some_and(HandControls::head_touch)
        || unclear_hum_on
        || misheard_hum_on
    {
        let level = crate::head_cue::level_from_environment().map_err(io::Error::other)?;
        Some(crate::head_cue::hum(level))
    } else {
        None
    };
    // A hand's sleep (#1362): the falling breath in a hold's last second,
    // where a hold may put the lamp to sleep, at the hum's level; the hold
    // it last sighed for; and a wake the person made, for its "hm?" once
    // this conversation can say it.
    let sleep_breath = if controls.as_ref().is_some_and(HandControls::head_sleep) {
        let level = crate::head_cue::level_from_environment().map_err(io::Error::other)?;
        Some(crate::head_cue::breath(level))
    } else {
        None
    };
    let mut sighed_for: Option<u64> = None;
    *waking = controls
        .as_mut()
        .and_then(|controls| controls.take_woken(monotonic_us()))
        // Consume the marker even with no hum configured, but keep no cue
        // whose cancellation the outer conversation would otherwise record.
        .filter(|_| head_cue.is_some())
        .map(|woken_us| (woken_us, monotonic_us()));
    let mut last_hum_us: Option<u64> = None;
    let mut took_over_turn: Option<u64> = None;
    // The turn whose speech last ended as something said, not a sound, and
    // when: someone was there then.
    let mut said_turn: Option<u64> = None;
    let mut said_at_us: Option<u64> = None;
    // The overdue reminder last said to be waiting for someone.
    let mut overdue_wait = crate::reminders::OverdueWait::default();
    // The turn last judged a sound and sent on only for the model to judge
    // (`LAMP_SHORT_SOUNDS`): until it is answered the lamp does not ponder
    // it, and the room and the provider do not count it as talk.
    let mut sound_turn: Option<u64> = None;
    // A call heard during the near-call step's try window (#1803): it ends
    // unanswered, however long it runs past the window, if it is no more
    // than a call.
    let mut near_call_turn: Option<u64> = None;
    // The turn whose line began in the sentence window (#1803): marked
    // count-only to the provider, its words are counted for onboarding and
    // kept nowhere, and its answer is neither played, published nor kept.
    let mut sentence_turn: Option<u64> = None;
    // The turns told the second route as onboarding's call (`StepCall`,
    // #3504), and, once each line ended, when its speech began and ended
    // (#3923): what the step takes if its words come back as the lamp's
    // name alone. A few, newest last: a line begun before an earlier one's
    // words are back leaves that one's place to it (c0a4, 28 Oct).
    let mut step_call_lines: VecDeque<(u64, Option<(u64, u64)>)> = VecDeque::new();
    // The turn in hand and when its speech set the detector off (its
    // candidate's trigger), the start a call's line is measured from
    // (#3923). Not the oldest speech block kept: blocks are kept from
    // outside any turn too, and on c0a4 that start was 2.5 s early.
    let mut line_began: Option<(u64, u64)> = None;
    // The newest turn told the second route as the step's call: a turn is
    // told once, even after its words took its place in `step_call_lines`.
    let mut step_call_told: u64 = 0;
    // Its words as they come, in memory only, until the last piece: on
    // Live they come in pieces, all of which are counted.
    let mut sentence_heard: Option<String> = None;
    // What the person is saying, until its last piece, for the onboarding
    // steps that end on a heard answer (#1722 row 10). Memory only.
    let mut onboarding_heard = String::new();
    // The answer whose words are arriving, and whether any of them has been
    // a word yet.
    let mut answer_words: Option<(u64, bool)> = None;
    // The turn whose first answer audio is recorded (`answer_first_audio`),
    // and on the second route the turn whose words have begun: its answer's
    // audio is what follows them, a wait cue or the down line before them
    // being no answer (`AnswerAudio`).
    let mut answer_audio = AnswerAudio::default();
    // A face added from a photo, asked about once at the lamp
    // (`LAMP_PHOTO_FACES=on`, #1968): the eyes' socket, and when they were
    // last asked whether one is here.
    let photo_faces = (lifetime.continuous() && crate::photo_faces::on())
        .then(|| std::env::var_os(crate::known_faces::ENVIRONMENT))
        .flatten()
        .map(|socket| crate::photo_faces::Watch::start(PathBuf::from(socket)));
    // Whether someone is at the lamp, when the eyes keep a presence file
    // for it: for a reminder or news that waits for a person, and who a
    // skill's note is told is there. The hello on an arrival is Welcome
    // back's alone (#2825).
    let mut presence = if lifetime.continuous() {
        crate::speak_up::Presence::from_environment()
    } else {
        None
    };
    // When anything was last said to the lamp or by it.
    let mut last_turn_us = monotonic_us();
    // When the lamp's own words last finished playing: only a reply that
    // played to its end, never one cut off (`LAMP_FOLLOWS_ON=on`, #829, the
    // first builder at 09:01 on 17 October).
    let mut lamp_words_ended_us: Option<u64> = None;
    // The first five minutes with a new person, led by the lamp
    // (`docs/onboarding.md`). Off unless `LAMP_ONBOARDING=on`.
    let mut onboarding = if lifetime.continuous() {
        trace.onboarding_storage.as_ref().and_then(|storage| {
            #[cfg(test)]
            if let Some(file) = trace.onboarding_test_file.as_ref() {
                return Some(crate::onboarding::Onboarding::with_storage(
                    file.clone(),
                    storage.client(),
                ));
            }
            crate::onboarding::Onboarding::from_storage(
                storage.client(),
                trace.onboarding_definition.as_ref()?.clone(),
            )
        })
    } else {
        None
    };
    // Its progress read at once, where storage has it, so the walk can say
    // from the first pass whether it is under way (#3747).
    if let Some(walk) = onboarding.as_mut() {
        walk.poll_storage(monotonic_us());
    }
    let onboarding_epoch = onboarding.as_ref().and_then(|_| {
        lamp_controls::runtime::clock_id(None).ok().map(|clock_id| {
            crate::onboarding::privacy::Epoch {
                device_boot: clock_id.clone(),
                clock_id,
            }
        })
    });
    let mut onboarding_invitation_turn: Option<(BootId, u64)> = None;
    // Whether any of the bound invitation's audio played, kept past its
    // reply: a turn that ends unsettled with none is no ask (#3754).
    let mut onboarding_invitation_audio = false;
    // The near-call step's evidence (#1803): `name/heard`, `ears/level` and
    // `voice/state`, drained without waiting. Without the bus the step is
    // unavailable.
    let mut onboarding_ears = onboarding
        .as_ref()
        .and_then(|_| crate::onboarding::near::Ears::from_environment());
    // The giving-way step's evidence (#1800): `body/hand` and
    // `body/joint_states`, drained without waiting.
    let mut onboarding_feel = onboarding
        .as_ref()
        .and_then(|_| crate::onboarding::giving_way::Feel::from_environment());
    // The inputs the walk's steps need, last recorded (`onboarding_inputs`,
    // row 10 on lamp-52e6): says, bus, ears, body, feel, storage.
    let mut onboarding_inputs: Option<(bool, bool, bool, bool, bool, bool)> = None;
    // News from the work handed to agents on other computers, told
    // unasked at a good moment (`docs/tasks.md`).
    #[cfg(test)]
    let given_news = TEST_TASK_NEWS.with(|news| news.borrow_mut().take());
    #[cfg(not(test))]
    let given_news = None;
    let mut task_news = if lifetime.continuous() {
        given_news.or_else(crate::task_news::TaskNews::from_environment)
    } else {
        None
    };
    // Told at the first pause of a second, not after 15 s with no turn
    // (#560, `LAMP_TASK_NEWS_AT_PAUSE=on`).
    let news_at_pause = crate::task_news::at_pause(
        std::env::var(crate::task_news::AT_PAUSE_ENVIRONMENT)
            .ok()
            .as_deref(),
    );
    #[cfg(test)]
    let news_at_pause = news_at_pause || TEST_NEWS_AT_PAUSE.with(std::cell::Cell::get);
    // What other parts ask the voice to say, skills first (#939,
    // `LAMP_SAY=on`).
    #[cfg(test)]
    let given_says = TEST_SAYS.with(|says| says.borrow_mut().take());
    #[cfg(not(test))]
    let given_says = None;
    let mut says = if lifetime.continuous() {
        given_says.or_else(|| crate::say::Says::for_run(notices.recordings()))
    } else {
        None
    };
    // The body's ask when it lets go for its pose, said through the say
    // service from a thread of its own (#2938, `LAMP_BODY_ASK=on`).
    #[cfg(not(test))]
    if let Some(says) = says.as_ref() {
        crate::body_ask::start_from_environment(says.own_asks());
    }
    // A skill typed in the app, handed over and told aloud as the voice's
    // own (#2813, `LAMP_APP_MAKE_SKILL=on`): a socket for lamp-cloud.
    #[cfg(not(test))]
    if lifetime.continuous() {
        crate::app_make::start_from_environment();
    }
    // The level under which sound is not speech, which may follow the room;
    // the speech blocks of the turn in hand and of the moment before it,
    // until it is known whether they were for the lamp; and the turn they
    // belong to.
    let mut room_floor = rig
        .faint_level
        .map(|lowest| crate::room_floor::from_environment(lowest, monotonic_us()));
    let mut floor_recorded: Option<(u8, Option<u8>)> = None;
    let mut near = crate::room_floor::Near::from_environment();
    // Whether an utterance was said close to the lamp, for the scrap gate.
    let mut rise = crate::room_floor::Rise::from_environment();
    // The last two seconds of blocks as the detector and the floor judged
    // them, for the record of an utterance thrown out as not speech:
    // twentieths of the detector's score, the level, and whether the floor
    // turned the block away. Numbers only.
    let mut judged_blocks: VecDeque<(u8, u8, bool)> = VecDeque::new();
    let mut turn_blocks: Vec<u8> = Vec::new();
    let mut loose_blocks: VecDeque<(u64, u8)> = VecDeque::new();
    let mut floor_turn: Option<u64> = None;
    // Taking a look when asked: the lamp's own "Let me look" is how it asks
    // for its eyes, and the look is the turn after it. Only where the eyes
    // keep a frame for it.
    #[cfg(test)]
    let test_look = TEST_FULL_LOOK.with(|still| still.borrow().clone());
    #[cfg(not(test))]
    let test_look: Option<std::path::PathBuf> = None;
    let looks = test_look.is_some()
        || lifetime.continuous()
            && std::env::var_os(crate::sight::LOOK_ENVIRONMENT)
                .is_some_and(|path| !path.is_empty());
    // A full look (#836, `LAMP_LOOK_FULL=on`): the eyes are asked for one
    // still as the lamp says it will look, the look waits for it at most
    // `STILL_WAIT`, and the model is told to say what it cannot make out.
    let mut look = match test_look {
        Some(still) => crate::sight::Look::new(Some(still)),
        None if looks => crate::sight::Look::from_environment(),
        None => crate::sight::Look::new(None),
    };
    let mut answer_opening: Option<(u64, String)> = None;
    let mut look_wanted: Option<(u64, u64)> = None;
    // The turn whose look a skill's tool asked for (#941): the eyes are
    // asked only once its first audio plays or its reply ends, the moment
    // the lamp would have said it will look, so the head lamp-skills sent
    // down before answering is moving and the eyes wait for it to settle.
    // With the session that made the call, and whether the second route
    // was answering when it was made: the look's turn goes to that session
    // on that route, or the look is dropped.
    let mut look_held: Option<(u64, u64, bool)> = None;
    // A sleep the person asked aloud (#3748), asked once its line is heard.
    let mut sleep_owed = SleepOwed::default();
    // The last turn told the provider it began while onboarding was under
    // way, so a `go_to_sleep` it calls is answered not now.
    let mut onboarding_marked: Option<u64> = None;
    // A turn held as unclear in a conversation, and when (#926): asked again
    // with a written turn once it is over.
    let mut asking = AskAgainState::default();
    // The lamp's own "Sorry?" that came back empty gets its "hm?" instead
    // (`LAMP_UNCLEAR_HUM=on`, #1417).
    let mut unclear_hum = UnclearHum::default();
    // A due reminder's quoted words cannot authorize a later camera turn.
    // Turn IDs increase within this conversation; fence older late words too.
    let mut last_reminder_turn: Option<u64> = None;
    // Due things a skill may say (#939, (c)): the reminder or news held for
    // a skill's ask, and since when.
    let news_by_skill = crate::task_news::by_skill();
    let mut reminder_held: Option<(String, u64)> = None;
    let mut news_held: Option<(String, u64)> = None;
    let mut gesture_cue = crate::body_link::Cue::default();
    // What the voice is doing, heard and said, for whoever listens on the
    // lamp's bus. Nothing here waits for a listener.
    let mut bus_out = crate::bus_out::BusOut::from_environment();
    // How each line sounded, against the person's own way of speaking
    // (`LAMP_VOICE_TONE`, [`crate::voice_tone`]): measured on its speech
    // blocks, read at its End, told with its words' valence.
    let tone_on = crate::voice_tone::on(
        std::env::var(crate::voice_tone::ENVIRONMENT)
            .ok()
            .as_deref(),
    );
    #[cfg(test)]
    let tone_on = tone_on || TEST_VOICE_TONE.with(std::cell::Cell::get);
    let mut tone_meter = tone_on.then(crate::voice_tone::ToneMeter::default);
    // Each known person's own way of speaking, and the room's; and whose the
    // last line was, the eyes' known person at its End by memory id.
    let mut tone_baselines = crate::voice_tone::Baselines::default();
    let mut tone_person: Option<String> = None;
    let mut tone_out = tone_on
        .then(crate::bus_out::ToneOut::from_environment)
        .flatten();
    // Memory's forgets, for the tone baselines (`memory/forgot`).
    let mut forgot_in = tone_on
        .then(crate::bus_out::ForgotIn::from_environment)
        .flatten();
    #[cfg(test)]
    if tone_on && let Some(given) = TEST_FORGOT_IN.with(|slot| slot.borrow_mut().take()) {
        forgot_in = Some(given);
    }
    // The body's pushes (`body/hand`): one ends a skill's search for the
    // voice as lamp-skills ends it for the body (#2904).
    let mut hand_in = crate::search_pushed::HandIn::from_environment();
    #[cfg(test)]
    if let Some(given) = TEST_HAND_IN.with(|slot| slot.borrow_mut().take()) {
        hand_in = Some(given);
    }
    // A line's reading, waiting for its words and for a line that goes on
    // from it ([`TonePending`]).
    let mut tone_held = ToneHeld::default();
    // The person's line so far: the duplex route hears a line in pieces and
    // marks none finished (Thirteen on #1722, 21 Oct), so a line's words
    // are put together to be read whole.
    let mut tone_line = ToneLine::default();
    // The voice's own timing, for the part that plans what the lamp shows
    // (docs/expression.md). Only if that is switched on.
    let mut expression_out = rig
        .expression_out
        .take()
        .or_else(crate::expression_out::ExpressionOut::from_environment);
    // And, only if someone switched it on, what the microphone hears, for
    // a part of the lamp that listens for itself.
    let mut mic_out = crate::bus_out::MicOut::from_environment(rig.microphone.as_str());
    // The self-test's speaker check, behind its switch (`speaker_tone`).
    let mut speaker_echo = speaker_tone::SpeakerEcho::from_environment();
    // The lamp's name as its own part hears it in the room's sound, for a
    // second look at something said to it by name that got nothing. Only
    // if that is switched on.
    let mut name_in = crate::second_look::on()
        .then(crate::bus_out::NameIn::from_environment)
        .flatten();
    // A stop word heard by `lamp-name`'s own listener (`stop/heard`, #828):
    // with `LAMP_STOP_HEARD=on`, it yields the reply while it sounds.
    let mut stop_in = crate::bus_out::stop_heard_on()
        .then(|| crate::bus_out::NameIn::from_environment_on(lamp_msgs::StopHeard::TOPIC))
        .flatten();
    // The first sign of the lamp's name as it is said, for a call's wait
    // (`--call-wait-ms`, #782): only with that option.
    let mut rising_in = rig
        .call_wait_blocks
        .is_some()
        .then(|| crate::bus_out::NameIn::from_environment_on(lamp_msgs::NameHeard::RISING_TOPIC))
        .flatten();
    // While an answer is awaited, the person it is for comes first
    // (`LAMP_ASKER_FIRST=on`, #829); the lamp's name as it is said, by
    // anyone, still takes the turn. With `record` it is judged, not done.
    let asker_mode = rig.asker_first;
    let mut asker_first = asker_mode
        .applies()
        .then(crate::asker_first::AskerFirst::default);
    // The line the asker rule waits on: its candidate, the turn whose
    // answer is awaited, where its own speech began and its last chance.
    // The answer's first block waits for it until then.
    let mut asker_waited: Option<(crate::admission::CandidateId, u64, u64, u64)> = None;
    // The line the answer was last held for, so that `asker_first_answer_held`
    // is recorded once a line.
    let mut asker_held_for: Option<crate::admission::CandidateId> = None;
    // Recorded only: a candidate taken at once, followed as `on` would
    // follow it, `(turn, own start, last chance)`, until `on` would have
    // decided it, so that `record` gives `on`'s judgement (the reviewer on
    // #1092).
    let mut asker_shadow: Option<(u64, u64, u64)> = None;
    let mut asker_named = asker_first
        .is_some()
        .then(|| crate::bus_out::NameIn::from_environment_on(lamp_msgs::NameHeard::RISING_TOPIC))
        .flatten();
    // #1712's B: the body's joints, so that a sound its motors make while
    // an answer is awaited takes no turn (`LAMP_MOTION_KEEPS_ANSWER=on`).
    let mut motion_in = crate::body_motion::MotionIn::from_environment();
    if motion_in.is_some() {
        eprintln!(
            "lamp-voice: motion keeps the answer: while an answer is awaited, a sound that begins as the body moves a joint (4 servo counts or more a message) is the lamp's own, unless its name is heard"
        );
    }
    // Such a sound about as loud as the asker is theirs (`LAMP_MOTION_VOICE`).
    let motion_voice = crate::motion_voice::Mode::from_environment();
    // The sound the motion rule last found the asker's: decided once, so a
    // later rule that holds it doesn't have it judged again on more blocks,
    // and perhaps let go then (the reviewer's P3 1 on #2371).
    let mut motion_kept_for: Option<crate::admission::CandidateId> = None;
    if motion_in.is_some() && motion_voice != crate::motion_voice::Mode::Off {
        eprintln!(
            "lamp-voice: motion voice{}: a sound begun as the body moves is the asker's when its own speech is no more than {:.0} dB under theirs{}",
            if motion_voice == crate::motion_voice::Mode::Record {
                ", recorded only"
            } else {
                ""
            },
            f32::from(crate::motion_voice::MARGIN) / 7.5,
            if asker_mode.applies() {
                ""
            } else {
                "; LAMP_ASKER_FIRST is off, so no asker's level is known and nothing changes"
            }
        );
    }
    if asker_first.is_some() {
        eprintln!(
            "lamp-voice: asker first{}: for {} s while an answer is awaited, speech more than {:.0} dB under the asker's takes no turn; {}",
            if asker_mode == crate::asker_first::Mode::Record {
                ", recorded only"
            } else {
                ""
            },
            crate::asker_first::KEEPS_FOR_US / 1_000_000,
            f32::from(crate::asker_first::MARGIN) / 7.5,
            if asker_named.is_some() {
                "the lamp's name said in it still does"
            } else {
                "the lamp's name is not heard here (no name/rising on the bus)"
            }
        );
    }
    let mut second_look =
        crate::second_look::SecondLook::default().with_the_note(crate::called::on());
    #[cfg(test)]
    let holding_on = crate::holding_answer::on() || TEST_HOLDING_ANSWER.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let holding_on = crate::holding_answer::on();
    let mut holding_answer = holding::Continuation::new(holding_on);
    // A skill's search on the second route that the model left after a look
    // is noted once to go on (`LAMP_SECOND_LOOK_NOTE=on`).
    let look_note_on = std::env::var(crate::second_provider::LOOK_NOTE_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    #[cfg(test)]
    let look_note_on = look_note_on || TEST_LOOK_NOTE.with(std::cell::Cell::get);
    // And, left again after it, walked on by the lamp to the next place
    // (`LAMP_SECOND_LOOK_WALK=on`), calling the search's skill itself.
    let look_walk_on = std::env::var(crate::second_provider::LOOK_WALK_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    #[cfg(test)]
    let look_walk_on = look_walk_on || TEST_LOOK_WALK.with(std::cell::Cell::get);
    // A search's note and walk are its skill's file's, as the skills'
    // tools file gives them; a loop test's are look-for-things' own file's.
    let search_walks = look_note::skills_walks();
    #[cfg(test)]
    let search_walks = {
        drop(search_walks);
        look_note::look_for_things_walks()
    };
    let mut look_note = look_note::LookNote::new(look_note_on)
        .with_walk(look_walk_on)
        .with_walks(search_walks);
    let walk_skill = look_note::skills_caller();
    #[cfg(test)]
    let walk_skill = TEST_WALK_SKILL
        .with(|skill| skill.borrow().clone())
        .unwrap_or(walk_skill);
    let policy_detector = match rig.turn_taking {
        // Duplex gates the scores this detector sees; its rule is unchanged.
        TurnTaking::V1Baseline | TurnTaking::Duplex => TurnDetector::v1_baseline(),
        TurnTaking::VadOnly => TurnDetector::default(),
    };
    let (mut detector, endpoint_policy) = EndpointRig::attach(
        rig.endpoint.as_mut(),
        rig.turn_taking,
        match rig.end_silence_blocks {
            Some(blocks) => policy_detector
                .with_end_silence(blocks)
                .map_err(io::Error::other)?,
            None => policy_detector,
        },
    )?;
    record(trace, endpoint_policy)?;
    // A bare call ends nothing (#782): with `--call-wait-ms`, an utterance
    // that is only the lamp's name waits that long for the rest.
    if let Some(blocks) = rig.call_wait_blocks {
        detector = detector.with_call_wait(blocks).map_err(io::Error::other)?;
        record(
            trace,
            json!({"kind":"call_wait_policy","at_us":monotonic_us(),
                "wait_ms":u64::from(blocks) * 10,
                "most_speech_ms":u64::from(crate::activity::CALL_SPEECH_BLOCKS) * 10,
                "basis":"the first sign of the lamp's name from lamp-name on the bus (name/rising) in an utterance that is only that so far"}),
        )?;
    }
    // The end sent early, if that is switched on: the request is ended to
    // the provider after a short quiet, and its answer held until the
    // detector's own end of speech confirms it (`early_end`).
    let mut early = rig.early_end_blocks.map(EarlyEnd::new);
    // The words asked for at a pause, the stream kept open
    // (`LAMP_SECOND_EARLY_FINALIZE`): off unless set.
    let mut early_finalize = rig
        .early_finalize
        .then(early_finalize::EarlyFinalize::default);
    // A due end held while the second route's words so far end unfinished
    // (`LAMP_SECOND_UNFINISHED_WAIT`): off unless set.
    let unfinished_wait = rig.unfinished_wait;
    if let Some(early) = early.as_ref() {
        record(
            trace,
            json!({"kind":"early_end_policy","at_us":monotonic_us(),
                "early_end_ms":u64::from(early.blocks()) * 10,
                "confirmed_by":"the endpoint policy: its silence wait, or the end-of-turn verdict",
                "speaking_again_after_blocks":crate::activity::START_BLOCKS,
                "speaking_again_score":crate::activity::START_PROBABILITY,
                "opening_kept_blocks":crate::activity::PRE_ROLL_BLOCKS,
                "given_up_as":"user_interrupted, continued by the next request"}),
        )?;
    }
    let mut endpoint_watch = endpoint::Watch::default();
    // Duplex is the conservative mask with ears. A candidate that began over
    // the lamp is held until the lamp's own sound has gone, so its decision
    // budget is longer; everything else about admission is unchanged.
    let duplex = rig.turn_taking == TurnTaking::Duplex;
    // A yield's request begins at the person's first words, said under the
    // lamp's voice, as far back as 1.6 s (#828). Off unless set.
    let first_words = duplex && crate::activity::first_words_kept();
    // And a line begun as the lamp's sound ends, at the onset that sound
    // hid (#1722, `LAMP_REPLY_END_REACH=on`). Off unless set.
    let reply_end_reach = first_words && crate::activity::reply_end_reach();
    // Listening gaps (#828, `LAMP_LISTEN_GAPS=on`): a long reply's
    // sentence ends held quiet, and a person heard in the lamp's own
    // silence. Off unless set; only with ears.
    let listen_gaps = duplex && crate::listen_gaps::on_from_environment();
    // A person who tries again over a reply fallen back is still heard
    // (#2602, `LAMP_CUT_IN_KEEPS_LISTENING=on`). Off unless set.
    let keeps_listening = duplex && crate::turn_taking::cut_in_keeps_listening();
    // A resume counts as fruitless only when its yield's evidence window
    // points at the lamp's own voice (#2602, `LAMP_CUT_IN_COUNTS_REAL=on`).
    // Off unless set.
    let counts_real = duplex && crate::turn_taking::cut_in_counts_real();
    detector.keep_first_words(first_words);
    // A line that begins softly keeps its first word (`LAMP_SOFT_START=on`):
    // an ordinary start, the lamp silent, may begin at it. Off unless set;
    // only with first words kept.
    detector.keep_soft_starts(first_words && crate::activity::soft_start());
    let hold = match (duplex, first_words) {
        (true, true) => Hold::DUPLEX.keeping_first_words(),
        (true, false) => Hold::DUPLEX,
        (false, _) => Hold::STANDARD,
    };
    // A yield's words, listened for with the second voice's speech-to-text
    // (#828, `LAMP_STOP_WORDS=heard`): a stop word ends the reply. Or all
    // that is heard while the lamp speaks (`heard-always`): a stop word
    // stops it as a hand Stop does. Off unless set, and without the second
    // voice's configuration.
    let listen = crate::stop_words::listen();
    let always = listen == crate::stop_words::Listen::Always;
    if listen != crate::stop_words::Listen::Off && !duplex {
        eprintln!(
            "lamp-voice: LAMP_STOP_WORDS is set, but stop words are heard only with --turn-taking duplex: not heard"
        );
    }
    let mut stop_words = (duplex && listen != crate::stop_words::Listen::Off)
        .then(|| match crate::second_config::load_from_env() {
            Ok(Some(config)) => {
            // Its backchannels are the speech-to-text's language's (#2939).
            let language = config.stt_language.clone();
            crate::stop_words::RelayEars::start(
                {
                    let keyterm = crate::stop_words::key_terms_from_environment();
                    let config = crate::stop_words::with_key_terms(config, keyterm);
                    if keyterm {
                        eprintln!(
                            "lamp-voice: stop words: the stop stream listens for the stop words as key terms ({})",
                            crate::stop_words::KEYTERM_ENVIRONMENT
                        );
                    }
                    if let Some(gain) = crate::stop_words::gain_from_environment()
                        && crate::stop_words::source() == crate::stop_words::Source::Processed
                    {
                        eprintln!(
                            "lamp-voice: stop words: the canceller's output is raised by {gain} dB before it is heard ({})",
                            crate::stop_words::GAIN_ENVIRONMENT
                        );
                    }
                    eprintln!(
                        "lamp-voice: stop words: {}, from the {} blocks, through the second voice's speech-to-text at {}",
                        if always { "heard-always, all the lamp's speech" } else { "heard, in a yield" },
                        crate::stop_words::source().as_str(),
                        crate::stop_words::where_heard(&config)
                    );
                    config
                },
                always,
            )
                .map(|ears| {
                    if always {
                        crate::stop_words::StopWords::always(Box::new(ears))
                            .with_source(crate::stop_words::source())
                            .with_hold(crate::stop_words::holds_for_own_words())
                            .with_gain(crate::stop_words::gain_from_environment())
                            .with_language(language.as_deref())
                            .with_backchannel_holds(crate::stop_words::holds_backchannels())
                            .with_backchannel_debug(crate::stop_words::debugs_backchannels())
                            .with_asides(crate::stop_words::holds_asides())
                    } else {
                        // Heard in a yield the source stays the default
                        // (`Processed`), so the gain is passed only when the
                        // blocks handed on are that source (El's P2 on #2690).
                        crate::stop_words::StopWords::new(Box::new(ears))
                            .with_gain(crate::stop_words::yield_gain(
                                crate::stop_words::gain_from_environment(),
                                crate::stop_words::source(),
                            ))
                            .with_language(language.as_deref())
                            .with_backchannel_debug(crate::stop_words::debugs_backchannels())
                            .with_asides(crate::stop_words::holds_asides())
                    }
                })
                .map_err(|error| eprintln!("lamp-voice: stop words are not heard: {error}"))
                .ok()
            }
            Ok(None) => {
                eprintln!(
                    "lamp-voice: LAMP_STOP_WORDS has no speech-to-text without the second voice's configuration: stop words are not heard"
                );
                None
            }
            Err(error) => {
                eprintln!("lamp-voice: stop words are not heard: {error}");
                None
            }
        })
        .flatten();
    // A waited sound's words (`LAMP_WAIT_KEEPS_ANSWER=words`, #1123 step
    // 2), from a stream of their own through the second voice's
    // speech-to-text. Without its configuration the wait is `on`'s alone.
    // A waited sound whose words only said to stop stops the answer it
    // waited for on the next pass, as a spoken stop does.
    let mut waited_stop = false;
    let mut wait_words = wait_hears_words
        .then(|| match crate::second_config::load_from_env() {
            Ok(Some(config)) => {
                eprintln!(
                    "lamp-voice: wait words: a sound while an answer waits is judged by its words, through the second voice's speech-to-text at {}",
                    crate::stop_words::where_heard(&config)
                );
                crate::stop_words::RelayEars::for_waits(config)
                    .map(|ears| crate::wait_words::WaitWords::new(Box::new(ears)))
                    .map_err(|error| {
                        eprintln!("lamp-voice: wait words are not heard: {error}");
                    })
                    .ok()
            }
            Ok(None) => {
                eprintln!(
                    "lamp-voice: LAMP_WAIT_KEEPS_ANSWER=words has no speech-to-text without the second voice's configuration: the wait is on, without words"
                );
                None
            }
            Err(error) => {
                eprintln!("lamp-voice: wait words are not heard: {error}");
                None
            }
        })
        .flatten();
    if wait_keeps_answer {
        record(
            trace,
            json!({"kind":"wait_keeps_answer","at_us":monotonic_us(),
                "words":wait_words.is_some(),
                "keeps_for_us":WAIT_KEEPS_FOR_US,
                "quiet_blocks":WAIT_QUIET_BLOCKS,
                "words_asked_after_quiet_blocks":wait_words.is_some().then_some(crate::wait_words::ASK_AFTER_QUIET_BLOCKS)}),
        )?;
    }
    // A backchannel held as a yield (#2939, `LAMP_BACKCHANNEL_HOLDS=on`):
    // only with a yield's words heard (`LAMP_STOP_WORDS=heard` or
    // `heard-always`).
    let backchannel_holds = crate::stop_words::holds_backchannels()
        && if stop_words.is_some() {
            eprintln!(
                "lamp-voice: backchannel holds: a yield's commit waits for its words, up to {} ms after they are asked for, and a backchannel resumes the answer",
                u16::from(lamp_audio::duplex::BACKCHANNEL_WORDS_WAIT_BLOCKS) * 10
            );
            if crate::stop_words::holds_asides() {
                eprintln!(
                    "lamp-voice: asides hold: \"hmm\", \"interesting\", \"wow\" and \"nice\", alone or two together, are held as backchannels"
                );
            }
            true
        } else {
            eprintln!(
                "lamp-voice: {} is set, but backchannels are held only with {}=heard or heard-always: not held",
                crate::stop_words::BACKCHANNEL_ENVIRONMENT,
                crate::stop_words::ENVIRONMENT
            );
            false
        };
    let mut admission = InputAdmission::with_hold(hold);
    let mut playback_mask = if duplex {
        record(
            trace,
            json!({"kind":"duplex_policy","at_us":monotonic_us(),
            "speech_score":Hearing::EVALUATED.speech,
            "explained_percent":Hearing::EVALUATED.explained_percent,
            "floor_percent":Hearing::EVALUATED.floor_percent,
            "clear_after_blocks":Hearing::EVALUATED.clear_after_blocks,
            "yield_after_blocks":Confirmation::EVALUATED.onset_blocks,
            "commit_after_clear_blocks":Confirmation::EVALUATED.speech_blocks,
            "resume_after_still_blocks":Confirmation::EVALUATED.silence_blocks,
            "resume_deadline_blocks":Confirmation::EVALUATED.deadline_blocks,
            "plain_percent":Hearing::EVALUATED.plain_percent,
            "commit_on_stillness_after_plain_blocks":Confirmation::EVALUATED.plain_blocks,
            "fruitless_yields_before_fallback":FRUITLESS_YIELDS,
            "fruitless_yield_forgiven_after_ms":crate::turn_taking::FORGIVE_FRUITLESS_US / 1_000,
            "keeps_listening":keeps_listening.then_some(json!({"over_blocks":crate::turn_taking::KEEPS_TRYING_BLOCKS,"more_fruitless_yields":crate::turn_taking::KEEPS_TRYING_YIELDS})),
            "fruitless_counts_own_voice_only":counts_real.then_some(json!({"window":"onset run through the settling block","own":"own, or over not plain","person":"plain over, or clear"})),
            "decision_budget_us":hold.decision_budget_us,
            "first_words_kept_blocks":first_words.then_some(crate::activity::REACH_BACK_BLOCKS),
            "reply_end_reach_within_blocks":reply_end_reach.then_some(lamp_audio::evidence::QUIET_BLOCKS_CEILING),
            "listen_gaps":listen_gaps.then_some(json!({"after_ms":crate::listen_gaps::AFTER_FRAMES * 1_000 / u64::from(lamp_audio::RENDER_RATE),"quiet_run_ms":crate::listen_gaps::LOW_RUN_CHUNKS * 10,"gap_ms":crate::listen_gaps::GAP_US / 1_000,"silence_heard":true})),
            "stop_words_after_still_blocks":(stop_words.is_some() && !always).then_some(crate::stop_words::FINALIZE_AFTER_STILL_BLOCKS),
            "stop_words_always":(stop_words.is_some() && always).then_some(true),
            "backchannel_holds":backchannel_holds.then_some(json!({"words_wait_ms_after_ask":u64::from(lamp_audio::duplex::BACKCHANNEL_WORDS_WAIT_BLOCKS) * 10,"sentence_ms":u64::from(lamp_audio::duplex::BACKCHANNEL_SPEECH_BLOCKS) * 10})),
            "provider_queue_bound":provider_queue_bound(hold),
            "provider_total_queue_bound":provider_queue_bound(hold) + 1,
            "provider_terminal_allowance":1,
            "fallback":"v1-baseline mask for a block without echo evidence and for the rest of a reply after its fruitless yields",
            "basis":"echo evidence with the detector score; not level or score alone",
            "addressee_inference":false}),
        )?;
        let mut mask = PlaybackMask::duplex();
        if first_words {
            mask.keep_first_words();
        }
        if listen_gaps {
            mask.listen_in_gaps();
        }
        if keeps_listening {
            mask.keep_listening();
        }
        if counts_real {
            mask.count_real_cut_ins();
        }
        if backchannel_holds {
            mask.hold_backchannels();
        }
        // A sound that waits holds its pre-roll and up to the hold's 1.2 s:
        // more than the mask's usual memory, which would reject it as
        // evicted (the reviewer and the first builder on #1130).
        if wait_keeps_answer {
            mask.keep_waited_sound();
        }
        mask
    } else {
        PlaybackMask::new(rig.turn_taking)
    };
    let mut heard_reply: Option<TurnOwner> = None;
    let mut policy_available = None;
    let mut capture_lineage: Option<CaptureLineage> = None;
    let provider_queue_bound = provider_queue_bound(hold);
    let mut provider_queue = VecDeque::with_capacity(provider_queue_bound);
    let mut submission_draining = false;
    let mut speaker_pending: Option<PendingSpeaker> = None;
    // The answer whose speech the speaker dropped as its authority lapsed
    // (#1178), to be cut right after the speaker's reports are read.
    let mut speaker_lapsed: Option<TurnOwner> = None;
    let mut speaker_sequence = 0u64;
    let mut hand_cancelled_through: Option<u64> = None;
    let mut snapshot = owner.snapshot(now())?;
    let mut highest_known_turn = snapshot.owner().map(|owner| owner.turn());
    rig.publish(snapshot)?;
    // Paused by the keeper as of the last loop, for the record of a change.
    let mut was_paused = false;
    // Whether a sound not taken for the pause has been recorded during it:
    // once a pause, not once a block.
    let mut refused_while_paused = false;
    // With LAMP_STALLS=on, a pass or a gap between passes of 50 ms or more
    // is recorded with what the lamp was doing (#1178).
    let mut stalls = crate::stalls::Stalls::from_environment("coordinator");
    // With LAMP_QUIET_PACE=on, a pass every 5 ms rather than 2 while nothing
    // is going on (#705, 08:37: the quiet voice's CPU).
    let pace = crate::quiet_pace::Pace::from_environment();
    loop {
        #[cfg(test)]
        if host_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "real-provider fixture exceeded its host watchdog",
            )
            .into());
        }
        #[cfg(test)]
        crate::scenario_clock::step(crate::scenario_clock::Phase::Run)?;
        let loop_start = Instant::now();
        let stall_pass = stalls.begin();
        // A second route whose worker exited is dropped before anything
        // else of this tick touches it, and a turn it held ends (#960).
        if second::dropped_if_gone(rig, &mut cloud, &mut output_flow, trace)? {
            // As a turn the provider interrupted: its unsent input and
            // unwritten speech go with it, and the rest of a line still
            // being said is drained, never taken for a new request.
            let input_active = reply
                .as_ref()
                .is_some_and(|current| !current.local_input_ended);
            cancel_reply(owner, reply, trace, "second_route_failed")?;
            provider_queue.clear();
            speaker_pending = None;
            if input_active {
                admission.discard_stream_until_end(monotonic_us())?;
                submission_draining = true;
            }
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
        }
        if let Some(current) = reply.as_ref() {
            let turn = current.owner.turn();
            highest_known_turn = Some(highest_known_turn.map_or(turn, |old| old.max(turn)));
        }
        if lifetime.continuous() && termination.signal_name().is_some() {
            record(
                trace,
                json!({"kind":"service_stop_requested","at_us":monotonic_us(),"signal":termination.signal_name()}),
            )?;
            cancel_reply(owner, reply, trace, "service_stopped")?;
            return Ok(());
        }
        termination.check()?;
        let clock = monotonic_us();
        if cloud.available() {
            if link_down_since.take().is_some() {
                // Not after the service's line: the internet never left.
                if offline_voice && outage.said && !outage.service {
                    back_owed_since = Some(clock);
                }
                outage = Outage::default();
            }
        } else {
            link_down_since.get_or_insert(clock);
            back_owed_since = None;
        }
        let verdict = if link_down_since.is_some() {
            reach(clock)
        } else {
            None
        };
        let offline = offline_now(offline_voice, link_down_since, clock, verdict);
        let service_down = service_down_now(service_down_line, link_down_since, clock, verdict);
        let paused = (rig.paused)();
        if paused != was_paused {
            was_paused = paused;
            refused_while_paused = false;
            eprintln!(
                "lamp-voice: {}",
                if paused {
                    "paused by the keeper: no new turn; one under way is finished"
                } else {
                    "resumed by the keeper"
                }
            );
            record(
                trace,
                json!({"kind":"keeper_pause","paused":paused,"at_us":clock}),
            )?;
        }
        // A privacy gap ridden holds as the keeper's pause does: no new
        // turn begins, so the lamp says nothing in it (#2602).
        let mut paused = paused || gap_ride.lasted(clock).is_some();
        if session_clock.startup_expired(session_instant()) {
            return Err(io::Error::other("voice input never became ready").into());
        }
        if !said_waiting_for_link && session_clock.waiting_for_link(session_instant()) {
            said_waiting_for_link = true;
            eprintln!("lamp-voice: listening with no link to the speech service; it keeps trying");
            record(trace, json!({"kind":"waiting_for_link","at_us":clock}))?;
        }
        // Privacy always precedes input, model output, or actuator work.
        // A later fresh mute cannot relabel an already-lost sensor lease as a
        // healthy user-mute lifecycle that permits automatic reopening.
        // On the Controls route a switch that cannot be read is closed, not
        // a fault: the cycle ends as a mute does and the service reads again
        // (#795). The direct GPIO route keeps ending on it.
        let privacy_controls = rig.privacy_controls;
        let lost = |why: String| -> Box<dyn std::error::Error + Send + Sync> {
            if privacy_controls {
                service_privacy::unavailable(why)
            } else {
                io::Error::other(why).into()
            }
        };
        // Before the microphone is first allowed, and within its first
        // second, a privacy observation read stale (it waited in the queue
        // while the voice started its workers) closes the gate and is waited
        // past for a fresh one, instead of ending the voice (lamp-4ace, 19
        // Oct 01:19: 0.7 s after a start, and back muted by the restart
        // guard). The gate stays closed meanwhile: nothing is heard. A
        // worker that never says anything still ends it at its freshness,
        // and once the microphone has been allowed nothing changes.
        let starting = !permitted
            && session_instant().saturating_duration_since(privacy_started)
                < Duration::from_micros(crate::privacy::PRIVACY_START_STALE_US);
        // With `LAMP_PRIVACY_GAP_RIDE=on`, a lapse after the microphone was
        // allowed is ridden for up to a second below (#2602): not the end
        // here.
        if lifetime.continuous()
            && privacy_sequence != 0
            && !starting
            && gate.state(now()) == PrivacyState::Unavailable
            && !gap_ride.rides(permitted, monotonic_us())
        {
            gap_ended(&gap_ride, trace, "no fresh reading")?;
            return Err(lost(
                "service privacy freshness lost before next observation".into(),
            ));
        }
        let mut worker_gap_since = None;
        for _ in 0..16 {
            let received = rig.privacy.channels.control.receive::<WorkerEvent>();
            let received = match received {
                Err(error) if privacy_controls => {
                    gap_ended(&gap_ride, trace, "privacy worker failed")?;
                    return Err(service_privacy::unavailable(error));
                }
                received => received?,
            };
            let Some(event) = received else {
                break;
            };
            match event {
                WorkerEvent::PrivacyUnavailable { since_us } => {
                    let at_us = monotonic_us();
                    if !privacy_controls || since_us > at_us || !gap_ride.rides(permitted, at_us) {
                        gap_ended(&gap_ride, trace, "no fresh reading")?;
                        return Err(lost("physical privacy source expired".into()));
                    }
                    gate.close();
                    gap_ride.noticed_since(since_us);
                    worker_gap_since =
                        Some(worker_gap_since.map_or(since_us, |old: u64| old.min(since_us)));
                }
                WorkerEvent::Privacy {
                    acquired_at_us,
                    muted,
                    receipt,
                } => {
                    // This metadata proves only the onboarding step. The
                    // existing privacy gate remains the authority owner.
                    onboarding_privacy_receipt = receipt.filter(|proof| {
                        proof.physical.stamp.at_us == acquired_at_us
                            && proof.physical.muted == Some(muted)
                    });
                    privacy_sequence = privacy_sequence
                        .checked_add(1u64)
                        .ok_or_else(|| io::Error::other("privacy sequence overflow"))?;
                    let read_at = now();
                    gate.observe(
                        privacy_sequence,
                        MonoTime::from_micros(acquired_at_us),
                        read_at,
                        muted,
                    );
                    let stale = read_at
                        .as_micros()
                        .checked_sub(acquired_at_us)
                        .is_some_and(|age| age >= gate.freshness_us());
                    if lifetime.continuous() {
                        match gate.state(now()) {
                            PrivacyState::Unavailable if starting && stale => {
                                if !said_stale_at_start {
                                    said_stale_at_start = true;
                                    eprintln!(
                                        "lamp-voice: a privacy observation {} ms old was read as the voice started; the gate stays closed until a fresh one",
                                        read_at.as_micros().saturating_sub(acquired_at_us) / 1_000
                                    );
                                    record(
                                        trace,
                                        json!({"kind":"privacy_stale_at_start","at_us":read_at.as_micros(),"acquired_at_us":acquired_at_us}),
                                    )?;
                                }
                            }
                            // Ridden as any lapse (#2602): nothing is taken
                            // meanwhile, and it ends at its second.
                            PrivacyState::Unavailable
                                if gap_ride.rides(permitted, monotonic_us()) => {}
                            PrivacyState::Unavailable => {
                                gap_ended(&gap_ride, trace, "no fresh reading")?;
                                return Err(lost(
                                    "invalid or stale service privacy observation".into(),
                                ));
                            }
                            PrivacyState::Muted => {
                                let observed_at_us = monotonic_us();
                                // The camera is told no first, before any
                                // record (#1802).
                                tell_eyes_no(eyes_link.as_mut(), "physical mute (privacy event)");
                                gap_ended(&gap_ride, trace, "read closed")?;
                                record(
                                    trace,
                                    json!({"kind":"physical_mute", "at_us":observed_at_us,"acquired_at_us":acquired_at_us,"fresh_observation":true}),
                                )?;
                                let onboarding = mute_onboarding(
                                    trace,
                                    onboarding.as_mut(),
                                    onboarding_privacy_receipt.as_ref(),
                                    observed_at_us,
                                    eyes_link.as_mut(),
                                )?;
                                return Err(Box::new(PhysicalMute {
                                    observed_at_us,
                                    onboarding,
                                }));
                            }
                            _ => {}
                        }
                    }
                }
                WorkerEvent::Fault { code } => {
                    gap_ended(&gap_ride, trace, "privacy worker failed")?;
                    return Err(lost(code));
                }
                _ => {
                    gap_ended(&gap_ride, trace, "privacy worker failed")?;
                    return Err(io::Error::other("unexpected privacy event").into());
                }
            }
        }
        // Provider loss closes cloud submissions, never the microphone authority
        // needed to retain interruptions or finish already-owned playback.
        // A recovered reading may be in the same batch as the worker's
        // expiry. Still perform the cut/hold once before considering reopen.
        let privacy_state = if worker_gap_since.is_some() {
            PrivacyState::Unavailable
        } else {
            gate.state(now())
        };
        if lifetime.continuous() {
            match privacy_state {
                PrivacyState::Muted => {
                    let at_us = monotonic_us();
                    tell_eyes_no(eyes_link.as_mut(), "physical mute (privacy gate)");
                    gap_ended(&gap_ride, trace, "read closed")?;
                    record(
                        trace,
                        json!({"kind":"physical_mute", "at_us":at_us,"fresh_observation":true}),
                    )?;
                    let onboarding = mute_onboarding(
                        trace,
                        onboarding.as_mut(),
                        onboarding_privacy_receipt.as_ref(),
                        at_us,
                        eyes_link.as_mut(),
                    )?;
                    return Err(Box::new(PhysicalMute {
                        observed_at_us: at_us,
                        onboarding,
                    }));
                }
                PrivacyState::Unavailable
                    if ((privacy_sequence != 0 && !starting)
                        || (privacy_sequence == 0
                            && session_instant().saturating_duration_since(privacy_started)
                                >= Duration::from_micros(gate.freshness_us())))
                        && !gap_ride.rides(permitted, monotonic_us()) =>
                {
                    gap_ended(&gap_ride, trace, "no fresh reading")?;
                    return Err(lost("physical privacy unavailable; not a user mute".into()));
                }
                _ => {}
            }
        }
        service::poll_storage(trace);
        let onboarding_location = trace
            .onboarding_storage
            .as_ref()
            .and_then(|storage| storage.client().snapshot().ok())
            .and_then(|snapshot| snapshot.hardware_location);
        if let (Some(walk), Some(epoch)) = (onboarding.as_mut(), onboarding_epoch.as_ref()) {
            walk.set_privacy_context(
                epoch.clone(),
                rig.privacy_receipts,
                onboarding_privacy_receipt.clone(),
                onboarding_location.as_deref().filter(|_| {
                    says.is_some() && bus_out.is_some() && !trace.onboarding_privacy_disabled
                }),
            );
        }
        if let Some(walk) = onboarding.as_mut() {
            // Which input keeps a step not_available, once per change:
            // booleans only, no paths (row 10 on lamp-52e6).
            let inputs = (
                says.is_some(),
                bus_out.is_some(),
                onboarding_ears.is_some(),
                body_link.is_some(),
                onboarding_feel.is_some(),
                !trace.onboarding_privacy_disabled,
            );
            if onboarding_inputs != Some(inputs) {
                record(
                    trace,
                    json!({"kind":"onboarding_inputs","says":inputs.0,"bus":inputs.1,"ears":inputs.2,
                        "body":inputs.3,"feel":inputs.4,"storage":inputs.5,"at_us":monotonic_us()}),
                )?;
                onboarding_inputs = Some(inputs);
            }
            walk.set_near_available(
                says.is_some()
                    && bus_out.is_some()
                    && onboarding_ears.is_some()
                    && !trace.onboarding_privacy_disabled,
            );
            if let Some(ears) = onboarding_ears.as_mut() {
                for seen in ears.take(monotonic_us()) {
                    walk.near_seen(seen);
                }
            }
            // No body to give way: the step is not available.
            // PARKED (#3702): giving way is out of the onboarding walk (the owner's call, 28 Oct): the step is
            // never offered and is kept `not_available`. Its code, words and tests stay. To put it back,
            // restore the line below in place of `false`.
            // says.is_some() && bus_out.is_some() && body_link.is_some() && onboarding_feel.is_some() && !trace.onboarding_privacy_disabled
            walk.set_giving_way_available(false);
            if let Some(feel) = onboarding_feel.as_mut() {
                let now_us = monotonic_us();
                for felt in feel.take(now_us) {
                    walk.giving_way_felt(felt, now_us);
                }
            }
        }
        // No sleep from the head while the walk is under way, its gaps
        // between steps too (#3747, the lead on #1711): giving way tells the
        // person to hold the head. Told before each poll; kept while the
        // walk hasn't read its progress yet, and between conversations,
        // where there is no walk to ask.
        if let Some(controls) = controls.as_mut()
            && let Some(under_way) = onboarding
                .as_ref()
                .map_or(Some(false), crate::onboarding::Onboarding::under_way)
        {
            controls.set_onboarding(under_way);
        }
        let walk_under_way = controls.as_ref().is_some_and(HandControls::onboarding);
        // Every turn in hand while the walk is under way, whichever way it
        // began, is told so before its `go_to_sleep` can be answered: never
        // the line, then a sleep refused (#3835's P3).
        if sleep_not_now(
            crate::sleep_asked::switched_on(),
            controls.as_ref(),
            onboarding.as_ref(),
        ) && let Some(current) = reply.as_ref()
            && onboarding_marked != Some(current.owner.turn())
        {
            onboarding_marked = Some(current.owner.turn());
            provider_queue.push_back(ProviderInput::Onboarding {
                request: current.owner.turn(),
                privacy_generation: snapshot.microphone_generation(),
            });
        }
        // A sleep asked aloud whose turn is gone without being heard whole
        // (cut by the person, or failed), or whose cap has passed: at the
        // cap it is asked only if its line is still sounding.
        // A sleep asked aloud, its line heard whole or still sounding at the
        // cap (#3748): the hand controls ask it at this poll, refusing it
        // if the walk is under way, and the mute path below takes it.
        if let Some(request) = sleep_owed.due(reply.as_ref(), monotonic_us()) {
            match controls.as_mut() {
                Some(controls) => controls.ask_sleep(),
                None => eprintln!(
                    "lamp-voice: request {request}: asked to sleep, but there are no hand controls to sleep with"
                ),
            }
        }
        let observation = match controls.as_mut() {
            Some(controls) => Some(controls.poll()?),
            None => None,
        };
        // A stall that has lapsed the capture lease inside someone's
        // utterance (#784) abandons it here, as a hand Stop does, before
        // admission sees the lineage break further down this pass.
        // A gap is a turn boundary, whether someone was speaking or the lamp
        // was answering: what it cut is abandoned, never carried over it
        // (#784). Once per gap: the explicit close, or the lease lapsing.
        let gap_cut = (std::mem::take(&mut gap_cut_utterance)
            || last_capture.is_some_and(|at| {
                (INPUT_LEASE_US..CAPTURE_GAP_US).contains(&monotonic_us().saturating_sub(at))
            }))
            && !gap_cancelled;
        let cut_question = !admission.idle_for_reminder();
        let muted = observation.as_ref().is_some_and(|seen| seen.muted);
        // Muted because a hand held on the head put it to sleep (#711).
        let asleep = observation.as_ref().is_some_and(|seen| seen.asleep);
        let stop = observation.as_ref().is_some_and(|seen| seen.stop);
        // A head touch (#711) stops the lamp while it says its answer, as
        // the button does; while it is quiet it asks nothing.
        let head_touch_at_us = observation.as_ref().and_then(|seen| seen.head_touch_at_us);
        // A tap on the head (#711): when it came and when its hand arrived.
        let head_tap = observation
            .as_ref()
            .and_then(|seen| seen.head_tap_at_us.zip(seen.head_tap_began_us));
        // A hand on the head now, and how long (#1362).
        let head_held = observation.as_ref().and_then(|seen| seen.head_held);
        let speaking = reply
            .as_ref()
            .is_some_and(|current| current.had_audio || current.playback.is_some());
        let head_stop = speaking && head_touch_at_us.is_some();
        // A stop phrase said over the lamp and heard while it speaks
        // (#828, `LAMP_STOP_WORDS=heard-always`) stops it as the hand does,
        // and so does one said before its answer began, heard as a waited
        // sound's words (`LAMP_WAIT_KEEPS_ANSWER=words`, the reviewer's P3
        // on #1277).
        // Both taken on every pass, so that one heard on the same pass as
        // the other does not leave it to cancel again on the next (the
        // reviewer's P3 on #1352).
        let waited = std::mem::take(&mut waited_stop);
        let heard_over = stop_words.as_mut().is_some_and(|words| words.spoken_stop()) && speaking;
        // A stop heard by `lamp-name` whose forced yield committed
        // (`LAMP_STOP_HEARD`, #828): it stops the reply as the hand does,
        // though nothing said in the yield was admitted (the reviewer on
        // #1555).
        let stop_heard = playback_mask.take_stop_committed() && speaking;
        let spoken_stop = waited || heard_over || stop_heard;
        // A push on the body ends a skill's search for the voice too, as
        // lamp-skills ends it for the body (#2904): its held look's picture
        // never goes to the model, and no note or walk step follows. The
        // search's turn in hand (the answer to a look's picture, or "Let me
        // look on the left") is cut as a spoken stop cuts it, and what its
        // request still sends is fenced, so no answer to the pushed look
        // plays and no second round speaks (El's P2 on #2990). The second
        // route holds its further looks itself (`search_pushed`).
        let mut body_pushed = false;
        if let Some(hand) = hand_in.as_mut()
            && hand.pushed(clock)
            && let Some((held, its_turn)) =
                look_note.pushed(reply.as_ref().map(|current| current.owner.turn()))
        {
            let dropped = look_wanted
                .map(|(turn, _)| turn)
                .filter(|turn| held == Some(*turn));
            if let Some(turn) = dropped {
                look_wanted = None;
                look_held = None;
                record(
                    trace,
                    json!({"kind":"look_dropped","turn":turn,"by":"tool","why":crate::search_pushed::WHY,"at_us":clock}),
                )?;
            }
            body_pushed = its_turn;
            record(
                trace,
                json!({"kind":"search_pushed","look_dropped":dropped,"turn_cut":its_turn,"at_us":clock}),
            )?;
            eprintln!(
                "lamp-voice: {}: the search ended{}{}",
                crate::search_pushed::WHY,
                if dropped.is_some() {
                    "; its look's picture is dropped"
                } else {
                    ""
                },
                if its_turn {
                    "; its turn in hand is cut"
                } else {
                    ""
                }
            );
        }
        {
            if stop || muted || gap_cut || head_stop || spoken_stop || body_pushed {
                let observed_at_us = monotonic_us();
                let cancellation = hand::cancel(
                    owner,
                    reply,
                    &mut rig.speaker.channels.control,
                    &mut speaker_pending,
                    &mut provider_queue,
                    &mut admission,
                    &mut detector,
                    muted,
                    observed_at_us,
                    monotonic_us,
                )?;
                snapshot = cancellation.snapshot;
                // The session of the answer the Stop cut, if it cut one.
                let stopped_session = cancellation
                    .reply
                    .as_ref()
                    .map(|current| current.cloud.session);
                // How much of that answer was played, for the next line on
                // its session (`LAMP_SAID_AS_HEARD`).
                if let Some(cut) = stop_cut_of(said_as_heard, cancellation.reply.as_ref()) {
                    stop_cut = Some(cut);
                }
                // A Stop, a head touch, the mute or a gap ends the unclear
                // turn's asking too: nothing is asked again after it (#926).
                let hum = asking.hum;
                if let Some(turn) = asking.cancelled() {
                    record(
                        trace,
                        json!({"kind":"ask_again_dropped","turn":turn,"at_us":monotonic_us(),"why":"cancelled with its turn"}),
                    )?;
                    if hum {
                        eprintln!(
                            "lamp-voice: request {turn}: a misheard cut-in: no \"hm?\" (cancelled with its turn)"
                        );
                    }
                }
                // And a "hm?" owed for an empty "Sorry?" (#1417, the
                // Seventh on #1421): a Stop is the person's whole answer.
                let misheard = unclear_hum.misheard();
                if let Some(owed_us) = unclear_hum.cancelled() {
                    record(
                        trace,
                        json!({"kind":"unclear_hum","owed_at_us":owed_us,"at_us":monotonic_us(),"plays":false,"why":"cancelled",
                            "misheard_turn":misheard.map(|(turn, _)| turn)}),
                    )?;
                    // As the other drops of a misheard cut-in's "hm?" are
                    // said (the reviewer on #1659).
                    if let Some((turn, _)) = misheard {
                        eprintln!(
                            "lamp-voice: request {turn}: a misheard cut-in: its \"hm?\" was not played (cancelled)"
                        );
                    }
                }
                // What a yield heard is dropped with the reply (#828).
                if let Some(words) = stop_words.as_mut() {
                    words.close();
                }
                // So is what a waited sound heard (#1123).
                if let Some(words) = wait_words.as_mut() {
                    words.close();
                }
                if gap_cut && !stop && !head_stop && !spoken_stop && !body_pushed && !muted {
                    // Admission keeps nothing of it: its capture lineage
                    // ends with the gap. After the gap, what is still said
                    // of it is dropped until a pause, not a new question.
                    admission.reset();
                    kept_through_gap = cut_question;
                    gap_cancelled = true;
                }
                // A reply may already be complete while its independent text
                // socket still holds a "let me look" tail. Idle Stop also
                // fences that last known request, not just a currently owned one.
                if let Some(turn) = cancellation.cancelled_turn.or(highest_known_turn) {
                    hand_cancelled_through =
                        Some(hand_cancelled_through.map_or(turn, |old| old.max(turn)));
                }
                last_publish = monotonic_us();
                // No provider, optional presentation, filesystem or trace call
                // precedes speaker revocation. Capture is next for mute.
                let publish = rig.publish_hand_revocation(snapshot);
                // The camera is next after the speaker and before any
                // record (#1802).
                if muted {
                    let exit = if asleep {
                        "head sleep"
                    } else {
                        "software mute"
                    };
                    tell_eyes_no(eyes_link.as_mut(), exit);
                }
                submission_draining = admission.draining();
                look_wanted = None;
                answer_opening = None;
                second_look =
                    crate::second_look::SecondLook::default().with_the_note(crate::called::on());
                holding_answer.clear();
                look_note.clear();
                last_turn_us = monotonic_us();
                speech_ended = None;
                ended_turn = None;
                floor_held_for = None;
                asked_in_busy_room = None;
                busy_answer_since = None;
                use crate::receipt_history::{
                    BODY_PUSHED, CAPTURE_GAP, HAND_STOP, HEAD_SLEEP, HEAD_STOP, SOFTWARE_MUTED,
                    SPOKEN_STOP,
                };
                // Only a Stop that cut the lamp's speech: after an idle
                // Stop, "Lamp?" or "Hi." is taken as always (Second on
                // #1107).
                if (head_stop || stop && speaking) && !muted {
                    stopped_at_us = Some(observed_at_us);
                    if stop_told && let Some(session) = stopped_session {
                        stop_untold.stopped(observed_at_us, session);
                    }
                    // The lamp's words are over: nothing said next follows on
                    // from an earlier answer (`LAMP_FOLLOWS_ON`, the reviewer
                    // on #1278), with `LAMP_STOP_TOLD` or without.
                    lamp_words_ended_us = None;
                }
                // A stop word said over the lamp's speech ends its words too
                // (`LAMP_STOP_WORDS`, the reviewer's optional P3 on #1278).
                // So does a push that cut the search's turn (#2904).
                if (spoken_stop || body_pushed) && !muted {
                    lamp_words_ended_us = None;
                }
                let reason = if asleep {
                    HEAD_SLEEP
                } else if muted {
                    SOFTWARE_MUTED
                } else if stop {
                    HAND_STOP
                } else if head_stop {
                    HEAD_STOP
                } else if spoken_stop {
                    SPOKEN_STOP
                } else if body_pushed {
                    BODY_PUSHED
                } else {
                    CAPTURE_GAP
                };
                record(
                    trace,
                    json!({"kind":"hand_control_cancel", "reason":reason,
                    "at_us":observed_at_us,
                    "acquired_at_us":observation.as_ref().and_then(|seen| seen.acquired_at_us.or(seen.head_touch_at_us)),
                    "speaker_told_at_us":cancellation.speaker_told_at_us,
                    "observation_to_speaker_dispatch_us":cancellation.speaker_told_at_us.saturating_sub(observed_at_us),
                    "boundary":"coordinator observation to local speaker control send; excludes bus delivery and acoustic stop"}),
                )?;
                if let Some(current) = cancellation.reply {
                    record_cancelled_reply(
                        current,
                        trace,
                        reason,
                        cancellation.speaker_told_at_us,
                    )?;
                }
                publish?;
                if muted {
                    if let Some(walk) = onboarding.as_mut() {
                        walk.invalidate_privacy(
                            crate::onboarding::privacy::Invalidation::SoftwareMute,
                        );
                    }
                    return Err(Box::new(SoftwareMute {
                        observed_at_us,
                        asleep,
                    }));
                }
            } else if let Some(acquired_at_us) = head_touch_at_us {
                record(
                    trace,
                    json!({"kind":"head_touch_noticed", "acquired_at_us":acquired_at_us,
                    "at_us":observation.as_ref().and_then(|seen| seen.observed_at_us),
                    "why":"quiet: a head touch asks nothing"}),
                )?;
            }
        }
        if let Some(observation) = observation {
            record_hand_pats(trace, observation.pets)?;
        }
        if reply
            .as_ref()
            .and_then(|current| current.say.as_ref())
            .is_some_and(|ticket| !ticket.authority_valid(monotonic_us()))
        {
            cancel_reply(owner, reply, trace, "unplug_authority_lost")?;
            provider_queue.clear();
            speaker_pending = None;
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
        }
        let can_open = privacy_state == PrivacyState::Allowed;
        // A fresh Allowed arriving at the deadline cannot bypass the bound.
        if gap_ride.lasted(monotonic_us()).is_some() && !gap_ride.rides(permitted, monotonic_us()) {
            gap_ended(&gap_ride, trace, "no fresh reading")?;
            return Err(lost(
                "service privacy freshness lost before next observation".into(),
            ));
        }
        if let Some(reminders) = reminders.as_mut() {
            reminders.poll(monotonic_us());
            if reply.as_ref().is_some_and(|current| {
                current.reminder.is_some() && !reminders.active_valid(current.owner, monotonic_us())
            }) {
                cancel_reply(owner, reply, trace, "reminder_authority_lost")?;
                provider_queue.clear();
                speaker_pending = None;
                snapshot = owner.snapshot(now())?;
                rig.publish(snapshot)?;
                last_publish = monotonic_us();
            }
        }
        if permitted && !can_open {
            // A lapse, not a closed switch (that was the mute above): with
            // `LAMP_PRIVACY_GAP_RIDE=on` the conversation stays for up to a
            // second (#2602). From this moment no frame is taken, the turn
            // in flight is cut, nothing new begins and the capture's and
            // the camera's authority aren't renewed, so their leases lapse
            // and capture closes the microphone as on a stall.
            let rides = match privacy_state {
                PrivacyState::Unavailable => gap_ride.rides(permitted, monotonic_us()),
                // The readings are back and open but not yet steady: a gap
                // ridden waits for the gate's 60 ms, as the first grant does.
                PrivacyState::Stabilizing => {
                    gap_ride.lasted(monotonic_us()).is_some()
                        && gap_ride.rides(permitted, monotonic_us())
                }
                _ => false,
            };
            if !rides {
                gap_ended(&gap_ride, trace, "no fresh reading")?;
                return Err(lost("physical privacy closed or lost freshness".into()));
            }
            if gap_ride.begin(permitted, worker_gap_since.unwrap_or_else(monotonic_us)) {
                if !gap_ride.rides(permitted, monotonic_us()) {
                    gap_ended(&gap_ride, trace, "no fresh reading")?;
                    return Err(lost("physical privacy source expired".into()));
                }
                rig.capture_held = true;
                paused = true;
                tell_eyes_no(eyes_link.as_mut(), "privacy gap");
                // The reply is cut here whatever `gap_cancelled` says: the
                // next pass's cancel (admission's reset) is skipped only when
                // a capture gap still open has already run it.
                cancel_reply(owner, reply, trace, "privacy_gap")?;
                provider_queue.clear();
                speaker_pending = None;
                gap_cut_utterance = true;
                // The speaker hears of the cut at once, not at the next
                // renewal; capture and the camera are held from here.
                snapshot = owner.snapshot(now())?;
                rig.publish(snapshot)?;
                last_publish = monotonic_us();
                eprintln!(
                    "lamp-voice: the privacy switch's readings lapsed: nothing is heard, and the conversation waits up to {} ms for a fresh one",
                    crate::privacy_gap::RIDE_US / 1_000
                );
                record(
                    trace,
                    json!({"kind":"privacy_gap","state":"began","at_us":monotonic_us()}),
                )?;
            }
        }
        // The gap ridden is over: a fresh reading said open (#2602). The
        // capture's authority is renewed again; capture reopens in the same
        // generation with a new epoch, and no frame read before now is taken.
        let reopen_at_us = monotonic_us();
        if permitted && can_open && gap_ride.lasted(reopen_at_us).is_some() {
            let Some(lasted_us) = gap_ride.reopened(reopen_at_us) else {
                gap_ended(&gap_ride, trace, "no fresh reading")?;
                return Err(lost("physical privacy source expired".into()));
            };
            rig.capture_held = false;
            frames_from_us = monotonic_us();
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
            eprintln!(
                "lamp-voice: the privacy switch reads open again after {} ms: the conversation goes on",
                lasted_us / 1_000
            );
            record(
                trace,
                json!({"kind":"privacy_gap","state":"reopened","at_us":monotonic_us(),"lasted_us":lasted_us}),
            )?;
        }
        if can_open && !permitted {
            owner.set_microphone_permission(now(), Permission::Allowed)?;
            // The eyes, a process of their own, may look exactly while the
            // microphone is permitted (`EyesLink`): the authority says so,
            // so what is read from them (whose a turn is) counts. With no
            // camera of the voice's own it was never said, and every second
            // route turn was the room's (#1895, lamp-52e6 20 Oct). A mute
            // denies both (`hand.rs`).
            eyes_look_with_the_microphone(
                owner,
                eyes_link.is_some() && follows_eyes,
                rig.camera.is_some(),
            )?;
            permitted = true;
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
        }
        service_camera(rig, owner, can_open, trace)?;
        // One pre-capture spawn; no initial denied authority is suppressed.
        if can_open
            && !capture_requested
            && let Some(motor) = &mut rig.motor
        {
            motor.start_before_capture(
                owner.snapshot(now())?,
                false,
                rig.camera.as_ref().is_some_and(|camera| !camera.failed),
            );
        }
        service_motor(rig, trace)?;
        // The original physical sample was drained immediately above. Recheck
        // its age before the optional operator channel; no new permission.
        service_head(
            rig,
            owner,
            gate.state(now()) == PrivacyState::Allowed,
            trace,
        )?;
        let mut provider_control_budget = 16;
        let mut provider_control_empty = service_provider_control(
            &mut rig.provider.channels.control,
            &mut cloud,
            &mut provider_control_budget,
            trace,
        )?;
        apply_submission_gate(
            &cloud,
            reply,
            &mut provider_queue,
            &mut admission,
            &mut submission_draining,
            trace,
        )?;
        second::service(
            rig,
            &mut cloud,
            &mut output_flow,
            reply,
            &mut provider_queue,
            submission_draining,
            trace,
        )?;
        if let Some(gone) = drain_capture_control(
            &mut rig.capture.channels.control,
            &mut capture_epoch,
            rig.noise_suppression,
            trace,
        )? {
            closed_epoch = Some(gone);
            capture_gap.closed = true;
            // The worker closed the microphone: a turn boundary now, before
            // the reopened epoch's frames or any more of a reply.
            gap_cut_utterance = true;
        }
        let mut speaker_control_budget = 32;
        service_speaker_control(
            rig,
            owner,
            reply,
            &mut speaker_pending,
            &mut playback_mask,
            &mut snapshot,
            &mut last_publish,
            &mut speaker_control_budget,
            trace,
            &mut speaker_lapsed,
        )?;
        latch_invitation_sounded(
            reply,
            onboarding_invitation_turn,
            &mut onboarding_invitation_audio,
        );
        if let Some(lapsed) = speaker_lapsed.take()
            && cut_after_speaker_lapse(
                lapsed,
                owner,
                reply,
                &mut provider_queue,
                &mut speaker_pending,
                trace,
            )?
        {
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
        }
        // Return through input/cancellation and speaker dispatch after at most
        // two cloud packets, even when more PCM is immediately available.
        // Render reference still goes directly between the audio workers.
        let mut provider_slice = ProviderSlice::new(monotonic_us());
        while provider_control_empty {
            provider_control_empty = service_provider_control(
                &mut rig.provider.channels.control,
                &mut cloud,
                &mut provider_control_budget,
                trace,
            )?;
            apply_submission_gate(
                &cloud,
                reply,
                &mut provider_queue,
                &mut admission,
                &mut submission_draining,
                trace,
            )?;
            if !provider_control_empty {
                break;
            }
            let Some(event) =
                provider_slice.receive(&mut rig.provider.channels.data, monotonic_us())?
            else {
                break;
            };
            output_flow.received()?;
            if hand::cancelled_output(&event, hand_cancelled_through) {
                continue;
            }
            if second::takes_failed_turn(
                rig,
                &mut cloud,
                &mut output_flow,
                reply,
                &mut provider_queue,
                &event,
                trace,
            )? {
                continue;
            }
            observe_provider_output(&mut cloud, reply, &event)?;
            key_refusals.observe(&event, monotonic_us());
            // Answered again: the next refusal is a new outage, its line owed.
            // An onboarding line's own words, said on a failed chat (#4263),
            // are no answer of the model's.
            let fixed_line = matches!(
                &event,
                ProviderOutput::GenerationComplete { request }
                    if onboarding_invitation_turn.is_some_and(|(_, turn)| turn == *request)
            );
            if refused_hold && model_refusing.observe(&event, monotonic_us(), fixed_line) {
                outage = Outage::default();
            }
            if words_heard(&event) {
                last_turn_us = monotonic_us();
            }
            // A call's line whose own turn ended without `StepCallHeard`
            // (#3923): its words were more than the name, or none, and the
            // step's try waits for nothing more. Only its own turn's end: a
            // finished transcript names no line, and an earlier line's late
            // words would empty this one's slot (the Reviewer-lead on #4006).
            if let ProviderOutput::TurnComplete { request, .. } = &event
                && let Some(at) = step_call_lines
                    .iter()
                    .position(|(turn, spoken)| turn == request && spoken.is_some())
                && let Some((_, Some((began, _)))) = step_call_lines.remove(at)
            {
                near_call_words_back(onboarding.as_mut(), began);
            }
            match event {
                ProviderOutput::ReminderDeferred {
                    request,
                    session,
                    reason,
                } => {
                    if reply.as_ref().is_some_and(|current| {
                        current.reminder.is_some()
                            && current.owner.turn() == request
                            && current.cloud.session == session
                    }) {
                        // Keep the receipt out of Reply's destructor until the
                        // real owner has been revoked and published. This
                        // deferral retries only before any PCM; admitted user
                        // interruption has its separate qualified boundary.
                        let mut reminder =
                            reply.as_mut().and_then(|current| current.reminder.take());
                        cancel_reply(owner, reply, trace, "reminder_deferred")?;
                        provider_queue.clear();
                        speaker_pending = None;
                        snapshot = owner.snapshot(now())?;
                        rig.publish(snapshot)?;
                        if let Some(reminder) = &mut reminder {
                            reminder.deferred_before_audio();
                        }
                        last_publish = monotonic_us();
                    }
                    record(
                        trace,
                        json!({"kind":"reminder_deferred", "at_us":monotonic_us(),
                        "request":request,"provider_session":session,"reason":reason,
                        "acknowledged":false}),
                    )?;
                }
                ProviderOutput::Ready { setup_us } => {
                    record(
                        trace,
                        json!({"kind":"provider_ready_data_ignored","at_us":monotonic_us(),"setup_us":setup_us}),
                    )?;
                }
                ProviderOutput::Audio {
                    request,
                    sequence,
                    provider_event_at_us,
                    source_frames,
                    source_offset,
                    samples,
                } => {
                    let received_at_us = monotonic_us();
                    if samples.is_empty()
                        || samples.len() > PACKET_SAMPLES
                        || source_frames == 0
                        || source_frames > lamp_gemini::MAX_OUTPUT_SAMPLES
                        || source_offset
                            .checked_add(samples.len())
                            .is_none_or(|end| end > source_frames)
                        || provider_event_at_us == 0
                        || provider_event_at_us > received_at_us
                    {
                        return Err(io::Error::other("invalid provider audio frame").into());
                    }
                    // An answer to the sentence whose words have not come
                    // back first (Gemini Live) ends before it is played.
                    if sentence_turn == Some(request)
                        && reply
                            .as_ref()
                            .is_some_and(|current| current.owner.turn() == request)
                    {
                        cancel_reply(owner, reply, trace, "onboarding_sentence")?;
                        speaker_pending = None;
                        provider_queue.clear();
                        snapshot = owner.snapshot(now())?;
                        rig.publish(snapshot)?;
                        last_publish = monotonic_us();
                    }
                    if reply
                        .as_ref()
                        .and_then(|current| current.say.as_ref())
                        .is_some_and(|ticket| !ticket.authority_valid(monotonic_us()))
                    {
                        cancel_reply(owner, reply, trace, "unplug_authority_lost")?;
                        provider_queue.clear();
                        speaker_pending = None;
                        snapshot = owner.snapshot(now())?;
                        rig.publish(snapshot)?;
                        last_publish = monotonic_us();
                    }
                    if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == request) {
                        if current.pcm.len() + samples.len() > MAX_REPLY_SAMPLES {
                            return Err(io::Error::other(
                                "provider violated reserved reply capacity",
                            )
                            .into());
                        }
                        if !current.had_audio {
                            record(
                                trace,
                                json!({"kind":"provider_first_audio","turn":request,"at_us":monotonic_us()}),
                            )?;
                            current.had_audio = true;
                        }
                        let now_us = monotonic_us();
                        if let Some(after) =
                            answer_audio.audio(request, rig.provider.role == "second", now_us)
                        {
                            record(
                                trace,
                                json!({"kind":"answer_first_audio","turn":request,"at_us":now_us,
                                "after_decision_ms":after}),
                            )?;
                        }
                        let received_frames = samples.len();
                        current.append_provider_audio(samples, received_at_us);
                        record(
                            trace,
                            json!({"kind":"provider_audio_enqueued","turn":request,
                            "at_us":received_at_us,"sequence":sequence,"provider_event_at_us":provider_event_at_us,
                            "provider_event_to_enqueue_us":received_at_us-provider_event_at_us,
                            "source_frames":source_frames,"source_offset":source_offset,"frames":received_frames,
                            "reply_queued_frames":current.pcm.len(),"held_final_frames":current.held_final_samples(),
                            "speaker_pending_sequence":speaker_pending.as_ref().map(|pending| pending.sequence)}),
                        )?;
                    }
                }
                ProviderOutput::GenerationComplete { .. } => {}
                ProviderOutput::Unclear {
                    request,
                    why,
                    cut_in,
                } => {
                    // Only for the turn in hand: one cancelled first (a hand's
                    // Stop, a capture gap) is never asked about. A misheard
                    // cut-in gets the lamp's "hm?" instead, with the switch.
                    let hum = misheard_hum_on && cut_in;
                    let current = asking.unclear(
                        request,
                        reply.as_ref().map(|current| current.owner.turn()),
                        monotonic_us(),
                        hum,
                    );
                    record(
                        trace,
                        json!({"kind":"provider_turn_unclear","turn":request,"why":why,"at_us":monotonic_us(),"asked_again":current && !hum,"hum":current && hum}),
                    )?;
                }
                // Put on its turn by `observe_provider_output`.
                ProviderOutput::NewsAnswered { .. } => {}
                // The second route's times for the request, recorded with the
                // end of speech decision's time, for an A/B scored on each
                // request's first answer audio (19 Oct).
                ProviderOutput::Stages {
                    request,
                    words_ms,
                    cue_ms,
                    chat_ms,
                    audio_ms,
                } => {
                    record(
                        trace,
                        json!({"kind":"answer_stages","turn":request,"at_us":monotonic_us(),
                        "from":"line_end","decided_at_us":answer_audio.decided_at(request),
                        "words_ms":words_ms,"cue_ms":cue_ms,"chat_first_ms":chat_ms,
                        "first_answer_audio_ms":audio_ms}),
                    )?;
                }
                // The second route's words so far end unfinished, or no
                // longer do: a due end of the line in hand waits a little
                // longer while they do (`LAMP_SECOND_UNFINISHED_WAIT`).
                ProviderOutput::WordsEnd {
                    request,
                    unfinished,
                } => {
                    if unfinished_wait
                        && reply
                            .as_ref()
                            .is_some_and(|current| current.owner.turn() == request)
                    {
                        detector.words_end(unfinished);
                    }
                }
                ProviderOutput::RequestedAudio {
                    request,
                    session,
                    offer,
                } => {
                    handle_requested_audio(
                        reply,
                        requested_audio.as_deref(),
                        rig.provider.boot,
                        snapshot,
                        (request, session),
                        offer,
                        trace,
                    )?;
                }
                ProviderOutput::LookAsked {
                    request,
                    session,
                    dropped,
                    search,
                    call,
                } => match dropped {
                    Some(why) if look.full() => {
                        // Dropped in a search's last look's turn: the
                        // search's bound, not only the lamp having
                        // written the turn.
                        let why = if look_note.took_its_looks(request) {
                            "the search took its looks"
                        } else {
                            why.as_str()
                        };
                        record(
                            trace,
                            json!({"kind":"look_dropped","turn":request,"by":"tool","why":why,"at_us":monotonic_us()}),
                        )?;
                    }
                    Some(_) => {}
                    None => {
                        if record_tool_look(
                            look.full(),
                            request,
                            reply.as_ref().map(|current| current.owner.turn()),
                            last_reminder_turn,
                            &mut look_wanted,
                            trace,
                        )? {
                            // The route answering now made the call: the
                            // second route's sessions are numbered apart
                            // from Gemini's, so the number alone is not
                            // enough.
                            let by_second = rig
                                .second
                                .as_ref()
                                .is_some_and(second::Route::second_active);
                            look_held = Some((request, session, by_second));
                            // A skill's look on the second route is a
                            // search's: a new one if the person asked it,
                            // or one going on in its own written turn.
                            if search {
                                let heard = reply.as_ref().is_some_and(|current| {
                                    current.owner.turn() == request && current.input_sequence > 0
                                });
                                look_note.asked(request, heard, call);
                            }
                        }
                    }
                },
                ProviderOutput::TurnComplete { request, idle } => {
                    record(
                        trace,
                        json!({"kind":"provider_turn_complete","turn":request,"idle":idle,"at_us":monotonic_us()}),
                    )?;
                }
                ProviderOutput::HoldingAnswer { request, session } => {
                    holding_answer.observed(request, session, rig.provider.boot, reply.as_ref());
                }
                // A call's line whose words were the lamp's name alone
                // (#3923): its speech's start goes to the step, which counts
                // it only while its name part listens for a call.
                ProviderOutput::StepCallHeard { request } => {
                    // Only this line's, wherever it is among them.
                    let line = step_call_lines
                        .iter()
                        .position(|(turn, _)| *turn == request)
                        .and_then(|at| step_call_lines.remove(at));
                    let taken = match line {
                        None => Err("not a call's line in hand"),
                        Some((_, None)) => Err("its line's start was not known"),
                        Some((_, Some(spoken))) => near_text_call(onboarding.as_mut(), spoken),
                    };
                    let mut row = json!({"kind":"onboarding_text_call","turn":request,
                        "taken":taken.is_ok(),"at_us":monotonic_us()});
                    match taken {
                        Ok(began_ms) => row["began_unix_ms"] = json!(began_ms),
                        Err(why) => row["why"] = json!(why),
                    }
                    record(trace, row)?;
                }
                ProviderOutput::SleepAsked { request } => {
                    // Asked once its line has been heard whole, or at the
                    // cap while it is still sounding.
                    sleep_owed.asked(request, monotonic_us());
                    eprintln!(
                        "lamp-voice: request {request}: asked to sleep; once its line has played"
                    );
                }
                ProviderOutput::LookNoCall { request, session } => {
                    // Only the look's own turn, still in hand on its session.
                    let current = reply.as_ref().is_some_and(|current| {
                        current.owner.turn() == request && current.cloud.session == session
                    });
                    let owed = if current {
                        look_note.answered(request, session, rig.provider.boot)
                    } else {
                        Err("its turn is not the one in hand")
                    };
                    if let Err(why) = owed {
                        record(
                            trace,
                            json!({"kind":"look_note_dropped","turn":request,"why":why,"at_us":monotonic_us()}),
                        )?;
                    }
                }
                // Onboarding's sentence (#1803): every piece of its words is
                // gathered in memory and only their count goes to onboarding,
                // once the last piece is in. None of them, and nothing of the
                // model's answer to it, is published, put in the trace or
                // played: the answer ends as soon as it shows, or as the
                // words are in.
                ProviderOutput::Transcript {
                    request,
                    text,
                    finished,
                } if (request.is_none() && sentence_heard.is_some())
                    || (request.is_some() && request == sentence_turn) =>
                {
                    let done = match request {
                        Some(_) => true,
                        None => {
                            let heard = sentence_heard.as_mut().expect("guarded");
                            // ponytail: past 4 KiB a line's words are not
                            // counted; a sentence is far shorter.
                            if heard.len() + text.len() <= 4_096 {
                                heard.push_str(&text);
                            }
                            finished
                        }
                    };
                    if request.is_none()
                        && finished
                        && let Some(heard) = sentence_heard.take()
                    {
                        let words = crate::wait_words::words_in(&heard);
                        if let Some(walk) = onboarding.as_mut() {
                            walk.sentence_words(words);
                        }
                        record(
                            trace,
                            json!({"kind":"onboarding_sentence_words","turn":sentence_turn,
                                "words":words,"at_us":monotonic_us()}),
                        )?;
                    }
                    if done
                        && let Some(turn) = sentence_turn
                        && reply
                            .as_ref()
                            .is_some_and(|current| current.owner.turn() == turn)
                    {
                        cancel_reply(owner, reply, trace, "onboarding_sentence")?;
                        speaker_pending = None;
                        provider_queue.clear();
                        snapshot = owner.snapshot(now())?;
                        rig.publish(snapshot)?;
                        last_publish = monotonic_us();
                    }
                }
                ProviderOutput::Transcript {
                    request,
                    text,
                    finished,
                } => {
                    if let Some(turn) = request {
                        answer_audio.words(turn);
                    } else if let Some(walk) = onboarding.as_mut() {
                        // ponytail: past 4 KiB a line is not read; an
                        // answer is far shorter.
                        if onboarding_heard.len() + text.len() <= 4_096 {
                            onboarding_heard.push_str(&text);
                        }
                        if finished {
                            walk.heard(&onboarding_heard, monotonic_us());
                            onboarding_heard.clear();
                        }
                    }
                    // Words for an answer whose end was sent early are not
                    // the lamp's until it is sure the person has finished:
                    // held, and said then, or dropped with the request.
                    let held = match (early.as_mut(), request) {
                        (Some(early), Some(turn)) => {
                            early.keep_current(reply.as_ref().map(|current| current.owner.turn()));
                            early.hold_words(turn, &text, finished)
                        }
                        _ => false,
                    };
                    if !held && let Some(bus) = bus_out.as_mut() {
                        match request {
                            Some(turn) => bus.said(turn, &text, finished, monotonic_us()),
                            None => bus.heard(&text, finished, monotonic_us()),
                        }
                    }
                    // A line the person finished: an answer to a step the
                    // model ends (`docs/onboarding.md`, LAMP_ONBOARDING_NUDGE).
                    if request.is_none()
                        && finished
                        && let Some(walk) = onboarding.as_mut()
                    {
                        walk.heard_line(&text);
                    }
                    // The person's line, its pieces end to end: read for
                    // its tone and a forget once it is over, at a piece
                    // marked finished (the second route's whole line) or
                    // once its pieces stop (below, `TONE_LINE_QUIET_US`).
                    if request.is_none() && tone_meter.is_some() {
                        let turn = reply.as_ref().map(|current| current.owner.turn());
                        if let Some((words, heard_in)) =
                            tone_line.heard(&text, turn, monotonic_us())
                        {
                            tone_line_over(
                                &words,
                                heard_in,
                                &mut tone_held,
                                &mut tone_baselines,
                                tone_person.as_deref(),
                                trace,
                            )?;
                        }
                        if finished {
                            let (words, heard_in) = tone_line.take();
                            tone_line_over(
                                &words,
                                heard_in,
                                &mut tone_held,
                                &mut tone_baselines,
                                tone_person.as_deref(),
                                trace,
                            )?;
                        }
                    }
                    // A line whose words end before a name, its answer
                    // unheard: a sound begun just after its end goes on
                    // from it (`LAMP_SECOND_OPEN_NAME`). The second route
                    // holds the answer meanwhile. Never the words.
                    if request.is_none()
                        && finished
                        && let Some((turn, ended_us)) = open_name::opened(
                            rig.open_name && rig.provider.role == "second",
                            &text,
                            reply
                                .as_ref()
                                .map(|current| (current.owner.turn(), answer_unheard(current))),
                            speech_ended,
                        )
                    {
                        open_name_turn = Some((turn, ended_us));
                        eprintln!(
                            "lamp-voice: request {turn}: its words end before a name: a sound begun within 2.5 s of its end goes on from it (LAMP_SECOND_OPEN_NAME)"
                        );
                        record(
                            trace,
                            json!({"kind":"open_name","turn":turn,"ended_at_us":ended_us,"at_us":monotonic_us()}),
                        )?;
                    }
                    // A line that was only the lamp's name, its answer
                    // unheard and held by the second route for the line
                    // after it: a sound begun within 2.5 s of its end takes
                    // the turn at once (`LAMP_CALL_TAKES_NEXT_LINE`). Never
                    // the words.
                    if request.is_none()
                        && finished
                        && let Some((turn, ended_us)) = open_name::called(
                            rig.call_takes_next && rig.provider.role == "second",
                            &text,
                            reply
                                .as_ref()
                                .map(|current| (current.owner.turn(), answer_unheard(current))),
                            speech_ended,
                        )
                    {
                        call_turn = Some((turn, ended_us));
                        eprintln!(
                            "lamp-voice: request {turn}: only the lamp's name: a sound begun within 2.5 s of its end takes the turn at once (LAMP_CALL_TAKES_NEXT_LINE)"
                        );
                        record(
                            trace,
                            json!({"kind":"call_takes_next","turn":turn,"ended_at_us":ended_us,"at_us":monotonic_us()}),
                        )?;
                    }
                    if request.is_some()
                        && let Some(walk) = onboarding.as_mut()
                    {
                        walk.lamp_said();
                    }
                    // What the lamp says is its own, not a stop said over it
                    // (#828, `heard-always`), stamped with when it will be
                    // heard: the reply's audio queued ahead of it plays
                    // first, up to 30 s of it. Stamped as it came, a "wait"
                    // in a long story had left the list before it was played
                    // (the reviewer on #1047).
                    if request.is_some()
                        && let Some(words) = stop_words.as_mut()
                    {
                        let queued = reply
                            .as_ref()
                            .filter(|current| Some(current.owner.turn()) == request)
                            .map_or(0, |current| current.pcm.len());
                        words.lamp_said(
                            &text,
                            crate::stop_words::plays_at_us(monotonic_us(), queued),
                            queued > 0,
                        );
                    }
                    // The model keeping quiet aloud: an answer that opens
                    // with no word in it ("...") while none of it has been
                    // played is not an answer. It ends here, unheard, and
                    // counts as a turn the lamp did not answer.
                    if let Some(turn) = request {
                        if answer_words.map(|(of, _)| of) != Some(turn) {
                            answer_words = Some((turn, false));
                        }
                        let worded = text.chars().any(char::is_alphanumeric);
                        let had_word = answer_words.is_some_and(|(_, word)| word);
                        answer_words = Some((turn, had_word || worded));
                        // Nor is the name of a function said aloud where it
                        // should have been called.
                        let named = !had_word && crate::not_speech::names_a_function(&text);
                        if !had_word
                            && (named || (!worded && !text.trim().is_empty()))
                            && reply.as_ref().is_some_and(|current| {
                                current.owner.turn() == turn
                                    && (current.local_input_ended || held)
                                    && current.playback.is_none()
                            })
                            // Not yet sure the person has finished: noted,
                            // and it ends unheard at that moment unless they
                            // go on first.
                            && !(held
                                && early
                                    .as_mut()
                                    .is_some_and(|early| early.nothing_said(turn, named)))
                        {
                            if named {
                                record(
                                    trace,
                                    json!({"kind":"function_named","turn":turn,"at_us":monotonic_us()}),
                                )?;
                            }
                            let heard_on = reply.as_ref().map(|current| current.cloud.session);
                            cancel_reply(owner, reply, trace, "nothing_said")?;
                            drop_unsaid_look(
                                &mut look_wanted,
                                look_held,
                                turn,
                                "its answer said nothing",
                                trace,
                            )?;
                            // An answer with no word in it is nothing said:
                            // someone who called the lamp by name is owed
                            // the second look all the same.
                            if let Some(score) = second_look.finished(turn, true, monotonic_us()) {
                                if let Some(session) = heard_on {
                                    second_look.heard_on(session);
                                }
                                record(
                                    trace,
                                    json!({"kind":"second_look_due","turn":turn,"at_us":monotonic_us(),"name_score":score,"after":"nothing_said"}),
                                )?;
                            }
                            // The lamp's own "Sorry?" with no word in it is
                            // as silent as one with no audio (#1417).
                            if unclear_hum.finished(turn, true, monotonic_us()) {
                                record(
                                    trace,
                                    json!({"kind":"unclear_hum_owed","turn":turn,"at_us":monotonic_us(),"after":"nothing_said"}),
                                )?;
                            }
                            speaker_pending = None;
                            provider_queue.clear();
                            snapshot = owner.snapshot(now())?;
                            rig.publish(snapshot)?;
                            last_publish = monotonic_us();
                        }
                    }
                    record_look_intent(
                        looks,
                        request,
                        &text,
                        last_reminder_turn,
                        &mut answer_opening,
                        &mut look_wanted,
                        trace,
                    )?;
                    // The lamp's own words, as its answer begins: a nod
                    // with a yes, a shake with a no, when it has a body.
                    // Held words are not its own yet.
                    if !held
                        && let (Some(turn), Some(body)) = (request, body_link.as_mut())
                        && let Some(gesture) = gesture_cue.heard(turn, &text, finished)
                    {
                        body.gesture(gesture);
                        // The face's accent with the move (#940).
                        if let Some(choreography) = choreography.as_mut() {
                            choreography
                                .set_accent(crate::ring_face::accent_code(gesture), monotonic_us());
                        }
                        record(
                            trace,
                            json!({"kind":"body_gesture_asked","turn":turn,"gesture":gesture,"at_us":monotonic_us()}),
                        )?;
                    }
                    record(
                        trace,
                        json!({"kind":"transcript","turn":request,"text":text,"finished":finished,"at_us":monotonic_us(),"input_correlation":if request.is_none(){"unreliable; session scoped"}else{"output lineage"}}),
                    )?
                }
                ProviderOutput::Interrupted { request } => {
                    record(
                        trace,
                        json!({"kind":"provider_interrupted","turn":request,"at_us":monotonic_us()}),
                    )?;
                    if reply
                        .as_ref()
                        .is_some_and(|current| current.owner.turn() == request)
                    {
                        let input_active = reply
                            .as_ref()
                            .is_some_and(|current| !current.local_input_ended);
                        cancel_reply(owner, reply, trace, "provider_interrupted")?;
                        speaker_pending = None;
                        provider_queue.clear();
                        // Keep the detector's utterance boundary. Reconnecting or
                        // cancellation must never reinterpret its tail as a new input.
                        if input_active {
                            admission.discard_stream_until_end(monotonic_us())?;
                            submission_draining = true;
                        }
                        snapshot = owner.snapshot(now())?;
                        rig.publish(snapshot)?;
                        last_publish = monotonic_us();
                    }
                }
                ProviderOutput::Started {
                    request,
                    session,
                    waiting_for_barrier,
                } => {
                    // What is heard from here on is told as this turn's
                    // (#800, `crate::policy_holds`).
                    if let Some(bus) = bus_out.as_mut() {
                        bus.input_started(request);
                    }
                    record(
                        trace,
                        json!({"kind":"provider_input_started","turn":request,
                        "provider_session":session,"waiting_for_barrier":waiting_for_barrier,"at_us":monotonic_us()}),
                    )?;
                }
                ProviderOutput::TurnFailed {
                    request,
                    stage,
                    reason,
                } => {
                    if let Some(current) = reply.as_mut().filter(|r| r.owner.turn() == request) {
                        discard_request_input(
                            current,
                            &mut provider_queue,
                            &mut admission,
                            &mut submission_draining,
                            trace,
                            "provider_turn_failed",
                        )?;
                    }
                    record(
                        trace,
                        json!({"kind":"provider_turn_failed","turn":request,
                        "stage":stage,"reason":reason,"at_us":monotonic_us()}),
                    )?;
                }
                ProviderOutput::OwnershipAssumed {
                    request,
                    superseded,
                } => {
                    record(
                        trace,
                        json!({"kind":"provider_ownership_assumed","at_us":monotonic_us(),
                        "turn":request,"superseded_turn":superseded,
                        "basis":"earlier response silent after it was asked to stop; not confirmed by the service"}),
                    )?;
                }
                ProviderOutput::InputDiscarded {
                    request,
                    dropped_samples,
                    sent_samples,
                } => {
                    record(
                        trace,
                        json!({"kind":"provider_input_discarded","at_us":monotonic_us(),
                        "through_turn":request,"dropped_unsent_samples":dropped_samples,
                        "already_sent_samples":sent_samples,
                        "boundary":"transport bookkeeping; samples already sent were not discarded and precede the next request in its activity"}),
                    )?;
                }
                ProviderOutput::Ended {
                    reason,
                    fault,
                    attempts,
                } => {
                    record(
                        trace,
                        json!({"kind":"provider_output_ended","at_us":monotonic_us(),
                        "reason":reason,"fault":fault,"attempts":attempts}),
                    )?;
                    apply_submission_gate(
                        &cloud,
                        reply,
                        &mut provider_queue,
                        &mut admission,
                        &mut submission_draining,
                        trace,
                    )?;
                }
            }
        }
        if session_clock.expired(session_instant()) {
            // Drain fault channels and inspect children before accepting the
            // deadline. A crashed or forcibly killed worker is never success.
            rig.running()?;
            record(
                trace,
                json!({"kind":"turn_taking_summary","at_us":monotonic_us(),"policy":rig.turn_taking,"suppressed_blocks":playback_mask.suppressed_blocks(),"tail_us":crate::turn_taking::TAIL_US,"acoustic_tail_measured":false}),
            )?;
            if let Some(((yields, commits, resumes), fallen_back)) = playback_mask.duplex_summary()
            {
                record(
                    trace,
                    json!({"kind":"duplex_summary","at_us":monotonic_us(),"yields":yields,"commits":commits,"resumes":resumes,
                    "fallen_back_at_end":fallen_back,"fruitless_forgiven":playback_mask.fruitless_forgiven(),
                    "hearing_margin_db":playback_mask.hearing_margin_db(),
                    "margin_scope":"person in the clear against own voice, both before cancellation; not addressee evidence"}),
                )?;
            }
            cancel_reply(owner, reply, trace, "deadline_cancelled")?;
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            return Ok(());
        }
        if let Some(capture) = capture_lineage
            && let Some(rejection) = admission.maintain(
                AdmissionContext {
                    capture,
                    authority: owner.snapshot(now())?,
                },
                monotonic_us(),
            )?
        {
            record_input_rejection(trace, rejection)?;
        }
        if permitted && !capture_requested {
            rig.capture.channels.control.send(Control::StartCapture)?;
            capture_requested = true;
        }
        for _ in 0..8 {
            let Some(mut frame) = rig.capture.channels.data.receive::<MicrophoneFrame>()? else {
                break;
            };
            heard_a_frame(controls);
            // A privacy gap ridden (#2602): from its lapse no frame is taken,
            // including those capture still reads inside its lease, and after
            // it none read before the reopen.
            if !gap_ride.takes_frame(frame.read_completed_at_us, frames_from_us, monotonic_us()) {
                capture_gap.dropped += 1;
                continue;
            }
            // A turn cut by a gap is cancelled on the next pass: until then
            // nothing reaches admission or the provider across the cut.
            if closed_epoch == Some(frame.epoch) || gap_cut_utterance {
                capture_gap.dropped += 1;
                continue;
            }
            if let Some(gone) = validate_capture_frame(
                &mut rig.capture.channels.control,
                &mut capture_epoch,
                snapshot.microphone_generation(),
                &frame,
                rig.noise_suppression,
                trace,
            )? {
                // Closed and maybe already reopened: either way this pass
                // takes no frame across the cut.
                closed_epoch = Some(gone);
                capture_gap.closed = true;
                gap_cut_utterance = true;
                capture_gap.dropped += 1;
                continue;
            }
            if let Some(check) = speaker_echo.as_mut() {
                check.observe(frame.echo.as_ref(), frame.read_completed_at_us);
            }
            let at = monotonic_us();
            let Some(age) = at.checked_sub(frame.read_completed_at_us) else {
                return Err(io::Error::other("capture became stale").into());
            };
            if age >= INPUT_LEASE_US {
                if age >= CAPTURE_GAP_US {
                    return Err(io::Error::other("capture became stale").into());
                }
                // Read before a stall and delivered after it: too old to
                // retain or admit, so it is dropped as part of the gap.
                capture_gap.dropped += 1;
                continue;
            }
            if let Some(gap) = capture_gap.ended(last_capture, frame.read_completed_at_us) {
                // An utterance the gap cut into is abandoned on the next
                // pass, as a hand Stop abandons one: the model is never
                // answered over a hole in what it heard.
                gap_cut_utterance |= !admission.idle_for_reminder() && !gap_cancelled;
                gap_cancelled = false;
                // Start listening afresh after it, unless an utterance it cut
                // is still waiting for its end.
                if std::mem::take(&mut kept_through_gap) {
                    detector.resume_after_cut();
                } else {
                    detector.reset();
                }
                playback_mask.clear_read_lineage();
                eprintln!("lamp-voice: {gap}");
                record(
                    trace,
                    json!({"kind":"capture_gap","stalled_us":gap.stalled_us,"dropped_frames":gap.dropped,"at_us":at}),
                )?;
            }
            last_capture = Some(frame.read_completed_at_us);
            holding_answer.observe_capture(&frame);
            look_note.observe_capture(&frame);
            session_clock.capture_retained();
            owner.set_capture(
                now(),
                CaptureState::RetainingUntil(MonoTime::from_micros(
                    frame.read_completed_at_us + INPUT_LEASE_US,
                )),
            )?;
            owner.set_admission(
                now(),
                AdmissionState::OpenUntil(MonoTime::from_micros(at + INPUT_LEASE_US)),
            )?;
            if !session_clock.ready()
                && cloud.available()
                && provider_control_empty
                && !submission_draining
            {
                session_clock.activate(session_instant())?;
                if let Some(ms) = fixture_at_ms.take() {
                    fixture_due_us = Some(at + ms * 1_000);
                }
                // The link state readiness was just validated against, as a
                // current-state publication. Retained startup link records can
                // be older than the cue bound; they stay history, never re-stamped.
                record(
                    trace,
                    json!({"kind":"provider_link_current","at_us":at,"state":"ready",
                    "session":cloud.ready_session(),"basis":"coordinator link register at listening readiness"}),
                )?;
                record(
                    trace,
                    json!({"kind":"listening_ready","at_us":at,"scope":"explicit directed session; addressee inference absent"}),
                )?;
                println!(
                    "{}",
                    json!({"status":"listening_ready","seconds":lifetime.seconds()})
                );
            }
            let lineage = CaptureLineage {
                worker: rig.capture.boot,
                epoch: frame.epoch,
                dsp_epoch: frame.dsp_epoch,
                privacy_generation: frame.privacy_generation,
            };
            if capture_lineage.is_some_and(|old| old != lineage) {
                clear_capture_lineage(
                    &mut detector,
                    &mut playback_mask,
                    trace,
                    at,
                    frame.frame_sequence,
                    frame.read_completed_at_us,
                )?;
            }
            capture_lineage = Some(lineage);
            let context = AdmissionContext {
                capture: lineage,
                authority: owner.snapshot(now())?,
            };
            heard_level = crate::ring_face::level_of(&frame.samples);
            if let Some(mic) = mic_out.as_mut() {
                mic.block(&frame.samples, at);
            }
            // What is too faint to be someone at the lamp is not speech,
            // whatever the detector makes of it: on lamp-4ace a run of
            // "utterances" at the bottom of the scale, voices across the
            // office or its own motors, were sent to the provider, came
            // back as "¿Qué?", and were taken for the person at the lamp
            // going on or cutting in.
            // Nor is a voice over the first seconds of an answer the lamp
            // is giving to something asked in a busy room: it finishes what
            // it was asked, as a person among people talking would.
            let keeping_answer = reply.as_ref().is_some_and(|current| {
                busy_answer_since.is_some_and(|(turn, since)| {
                    turn == current.owner.turn()
                        && frame.read_completed_at_us.saturating_sub(since) < KEEP_ANSWER_US
                })
            });
            let faint = room_floor
                .as_mut()
                .map(|floor| floor.at(frame.read_completed_at_us));
            // Recorded when it moves a decibel, and when the level the
            // room alone would ask for does: with a fixed floor that second
            // number is what following the room would have done.
            let unbounded = room_floor.as_ref().and_then(|floor| floor.unbounded());
            if let Some(faint) = faint
                && floor_recorded.is_none_or(|(last, last_unbounded)| {
                    last.abs_diff(faint) >= 8
                        || match (last_unbounded, unbounded) {
                            (Some(before), Some(now)) => before.abs_diff(now) >= 8,
                            (None, None) => false,
                            _ => true,
                        }
                })
            {
                floor_recorded = Some((faint, unbounded));
                let dbfs = |level: u8| -48.0 + f32::from(level) * 34.0 / 255.0;
                record(
                    trace,
                    json!({"kind":"room_floor","at_us":frame.read_completed_at_us,
                        "dbfs":dbfs(faint),"room_alone_dbfs":unbounded.map(dbfs)}),
                )?;
            }
            // What the detector takes for speech while the lamp is silent,
            // kept until it is known whether it was for the lamp: a turn's
            // own blocks with the turn, the rest for a moment in case a
            // turn is about to begin with them.
            if room_floor.is_some()
                && frame.probability.is_finite()
                && frame.probability >= 0.5
                && reply
                    .as_ref()
                    .is_none_or(|current| current.playback.is_none())
            {
                if reply
                    .as_ref()
                    .is_some_and(|current| !current.local_input_ended)
                {
                    if turn_blocks.len() < 6_000 {
                        turn_blocks.push(heard_level);
                    }
                } else {
                    loose_blocks.push_back((frame.read_completed_at_us, heard_level));
                }
            }
            while let Some((at, level)) = loose_blocks.front().copied() {
                if frame.read_completed_at_us.saturating_sub(at) < 400_000 {
                    break;
                }
                loose_blocks.pop_front();
                if let Some(floor) = room_floor.as_mut() {
                    floor.not_for_the_lamp(level);
                }
            }
            let under_floor = faint
                .is_some_and(|faint| !near.hears(heard_level, faint, frame.read_completed_at_us));
            if judged_blocks.len() == 200 {
                judged_blocks.pop_front();
            }
            judged_blocks.push_back((
                (frame.probability.clamp(0.0, 1.0) * 20.0).round() as u8,
                heard_level,
                under_floor,
            ));
            let probability = if keeping_answer || under_floor {
                0.0
            } else {
                frame.probability
            };
            #[cfg(test)]
            if let Some(accepted) = trace.onboarding_test_capture_accepted.as_ref() {
                // Counts only validated, fresh frames reaching the hearing path.
                accepted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            // Talk over the lamp's own line in a busy room doesn't pause it
            // (`LAMP_OWN_LINE_OVER_TALK`): withheld for a yield, the line's
            // audio left the speaker playing zeros, and the say ended
            // `dropped` with playback gaps though its talk was let go
            // (lamp-52e6, 22 Oct).
            playback_mask.room_talk(
                own_line_over_talk
                    && reply.as_ref().is_some_and(|current| {
                        busy_room_line(
                            current,
                            own_line_busy == Some((current.owner.turn(), true)),
                            lamp_began,
                        )
                    }),
            );
            let (heard, yielded) = hear_capture_block(
                &mut playback_mask,
                reply,
                &mut heard_reply,
                frame.frame_sequence,
                frame.read_completed_at_us,
                probability,
                frame.echo.as_ref(),
                monotonic_us(),
                trace,
            )?;
            rise.sample(frame.read_completed_at_us);
            // A sound let go while the own line waited goes on with each
            // block of speech, and ends after a gap of quiet (#2602).
            own_let_go = own_let_go.and_then(|sound| {
                sound.heard(
                    frame.read_completed_at_us,
                    probability.is_finite() && probability >= 0.5,
                )
            });
            // How loud what the detector calls speech has been lately, for
            // the record of each utterance: its last eight seconds.
            // Once the lamp answers, nothing after goes on from the line
            // before for its tone.
            if let Some(meter) = tone_meter.as_mut()
                && reply
                    .as_ref()
                    .is_some_and(|current| current.playback.is_some())
            {
                meter.answered();
            }
            if probability.is_finite() && probability >= 0.5 {
                // A line's tone takes only speech while the lamp is silent:
                // the line's own, and what came just before it began. Never
                // the lamp's answer, or anyone talking over it.
                if let Some(meter) = tone_meter.as_mut()
                    && reply
                        .as_ref()
                        .is_none_or(|current| current.playback.is_none())
                {
                    meter.speech_block(
                        frame.read_completed_at_us,
                        &frame.samples,
                        reply
                            .as_ref()
                            .is_some_and(|current| !current.local_input_ended),
                    );
                }
                speech_levels.push_back((frame.read_completed_at_us, heard_level));
                last_speech_us = frame.read_completed_at_us;
                // For the asker-first rule: the asker's own speech while
                // they say their request, away from the lamp's sound; and
                // speech while its answer is awaited with the lamp silent.
                if let Some(rule) = asker_first.as_mut()
                    && let Some(current) = reply.as_ref()
                {
                    if !current.local_input_ended
                        && last_spoke_us.is_none_or(|spoke| {
                            frame.read_completed_at_us
                                > spoke.saturating_add(crate::asker_first::ECHO_TAIL_US)
                        })
                    {
                        rule.asking(current.owner.turn(), heard_level);
                    } else if current.local_input_ended
                        && (current.playback.is_none()
                            || asker_waited.is_some()
                                && frame.echo.as_ref().is_some_and(|evidence| {
                                    lamp_audio::duplex::Hearing::EVALUATED
                                        .heard(frame.probability, evidence)
                                        .person()
                                }))
                    {
                        // Over the answer too, while the rule waits on a
                        // line begun before it: its own blocks decide it,
                        // those the render does not explain, as the duplex
                        // rule hears a person over the lamp. The lamp's own
                        // voice let through by the canceller is as loud as
                        // a person on a fifth of its blocks on lamp-4ace,
                        // and judged on it a line from across the room took
                        // the turn from the answer (the reviewer on #1197).
                        rule.heard(frame.read_completed_at_us, heard_level);
                    }
                    // Recorded only, what `on` would still be judging.
                    if asker_shadow.is_some() && !current.local_input_ended {
                        rule.heard(frame.read_completed_at_us, heard_level);
                    }
                }
            } else if !keeping_answer && heard_level >= crate::not_speech::LOUD_LEVEL {
                loud_blocks.push_back(frame.read_completed_at_us);
            }
            // Recorded only: the candidate taken at once, judged as `on`
            // would judge it, at the point `on` would decide.
            if let Some((turn, start, due)) = asker_shadow
                && let Some(rule) = asker_first.as_mut()
            {
                let now = monotonic_us();
                match rule.judge(turn, start, now, now >= due) {
                    crate::asker_first::Judgement::Wait => {}
                    crate::asker_first::Judgement::Take => asker_shadow = None,
                    crate::asker_first::Judgement::Keep(kept) => {
                        asker_shadow = None;
                        rule.kept_line(now);
                        record_asker_first(
                            rule,
                            trace,
                            turn,
                            now,
                            kept,
                            false,
                            "at_its_last_chance",
                        )?;
                    }
                }
            }
            while speech_levels
                .front()
                .is_some_and(|(at, _)| frame.read_completed_at_us.saturating_sub(*at) > 8_000_000)
            {
                speech_levels.pop_front();
            }
            while loud_blocks
                .front()
                .is_some_and(|at| frame.read_completed_at_us.saturating_sub(*at) > 8_000_000)
            {
                loud_blocks.pop_front();
            }
            // Before the canceller, or its linear output, for the stop
            // words, when they asked; otherwise they hear `samples`.
            let words_block = stop_words_block(&mut frame);
            let observation = ObservedAudio {
                sequence: frame.frame_sequence,
                captured_at_us: frame.read_completed_at_us,
                samples: frame
                    .samples
                    .try_into()
                    .map_err(|_| io::Error::other("invalid capture block"))?,
            };
            // What was kept for an end sent early goes with its request,
            // whatever ended that request.
            if let Some(early) = early.as_mut() {
                early.keep_current(reply.as_ref().map(|current| current.owner.turn()));
            }
            let observed_at_us = monotonic_us();
            // A yield's start may reach back over what was heard with echo
            // evidence, by each block's own score (#828).
            if yielded {
                detector.reach_back();
            } else if reaches_past_reply(reply_end_reach, frame.echo.as_ref()) {
                detector.reach_back_to_onset();
            }
            let samples = observation.samples;
            let captured_at_us = observation.captured_at_us;
            // The lamp's sound still reaches the microphone (#828,
            // `heard-always`).
            let sounding = frame.echo.as_ref().is_some_and(|echo| {
                echo.render_quiet_blocks < Hearing::EVALUATED.clear_after_blocks
            });
            detector.lamp_silent(lamp_silent(
                sounding,
                keeping_answer,
                reply_sounding(&playback_mask, reply, frame.read_completed_at_us),
            ));
            let (update, input_ended) = observe_prospective_heard(
                &mut playback_mask,
                &mut detector,
                &mut admission,
                context,
                observation,
                frame.timing.first_read_started_at_us,
                heard,
                frame.echo.is_some().then_some(probability),
                observed_at_us,
            )?;
            // An end held for unfinished words, let go: counts only.
            if let Some(waited) = detector.take_unfinished_wait() {
                eprintln!(
                    "{}",
                    waited.told(reply.as_ref().map(|current| current.owner.turn()))
                );
            }
            check_speaker_progress(speaker_pending.as_ref(), observed_at_us)?;
            let discarded_endpoint = submission_draining && input_ended;
            if discarded_endpoint {
                submission_draining = false;
                // The utterance whose rest was discarded has ended; nothing
                // kept for an end sent early in it is wanted any more.
                if let Some(early) = early.as_mut() {
                    early.forget();
                }
                if let Some(current) = reply.as_mut()
                    && current.cloud.input_abandoned
                    && !current.local_input_ended
                {
                    current.local_input_ended = true;
                    owner.input_ended(now(), current.owner)?;
                    snapshot = owner.snapshot(now())?;
                    rig.publish(snapshot)?;
                    last_publish = monotonic_us();
                }
                record(
                    trace,
                    json!({"kind":"input_discarded_endpoint","at_us":at,
                    "last_block_host_read_us":frame.read_completed_at_us}),
                )?;
            }
            if let Some(rejection) = update.rejection {
                record_input_rejection(trace, rejection)?;
            }
            if let AdmissionStep::Candidate(candidate) = &update.step {
                record(
                    trace,
                    json!({"kind":"input_candidate","candidate":candidate,"at_us":monotonic_us()}),
                )?;
            }
            // The words of a yield (#828): its held audio when it begins,
            // each block while it is in force, and the rule told when they
            // come. Nothing but what is said in a yield is heard.
            if let Some(words) = stop_words.as_mut() {
                // Listening to everything, a reply counts from its first
                // sound from the provider: one held, or never answered,
                // opens no stream (the first builder at 15:29, eleven in two
                // minutes of office noise). The 0.6 to 0.8 s the stream
                // takes to connect overlaps the start of playout, and
                // what is heard meanwhile is kept for it.
                let replying = reply.as_ref().is_some_and(|current| {
                    !words.hears_always() || current.had_audio || current.playback.is_some()
                });
                words.heard_speech(probability.is_finite() && probability >= 0.5);
                hear_stop_words(
                    words,
                    &mut playback_mask,
                    &admission,
                    &update.step,
                    replying,
                    sounding,
                    captured_at_us,
                    words_block.as_deref().unwrap_or(&samples),
                    trace,
                )?;
            }
            // A stream for a waited sound's words is kept ready while a
            // sound may still wait and the microphone is allowed, and what a
            // sound no longer pending heard is dropped (#1123 step 2).
            if let Some(words) = wait_words.as_mut() {
                let still_pending = words
                    .holds()
                    .is_some_and(|candidate| admission.pending_audio(candidate).is_some());
                let asked_until_us = speech_ended
                    .filter(|(turn, _)| {
                        reply.as_ref().map(|current| current.owner.turn()) == Some(*turn)
                    })
                    .map(|(_, at)| at);
                let awaited = snapshot.microphone_permission() == Permission::Allowed
                    && (words_awaited(reply.as_ref(), asked_until_us, monotonic_us())
                        || own_line_awaited(reply.as_ref(), own_line_keeps, lamp_began));
                words.follow(awaited, still_pending);
            }
            let step = match update.step {
                AdmissionStep::None => admission
                    .pending_evidence(monotonic_us(), AcceptanceBasis::DirectedSessionVadOnly)
                    .map_or(AdmissionStep::None, |(candidate, _)| {
                        AdmissionStep::Candidate(candidate)
                    }),
                step => step,
            };
            // An utterance ending in the near-call window is held too, even
            // if no block of it was (#1803); but only a call: one with more
            // said than the name ("can we skip this?") goes on as any other.
            if matches!(step, AdmissionStep::End(_)) {
                if near_call_window(
                    onboarding.as_ref(),
                    speech_levels.front().map(|(at, _)| *at),
                    monotonic_us(),
                ) {
                    near_call_turn = reply.as_ref().map(|current| current.owner.turn());
                }
                if speech_levels.len() > usize::from(crate::activity::CALL_SPEECH_BLOCKS) {
                    near_call_turn = None;
                }
            }
            // On the second route a call told as the step's (`StepCall`) is
            // not held (#3923): the route itself never answers the name
            // alone, and its End must go on for its words to come back as
            // the step's call by text. Held, the line had no words (c0a4,
            // requests 3 and 5), and its kept sound was carried into the
            // next line, which then was no bare name and was answered
            // (request 7, "Hey Lamp"). On Live the hold stands.
            // A line still going on, never told as the step's call because
            // the step did not wait for one as it began (its Start came
            // before the prompt's playback was known whole, or the walk was
            // between ticks): told now, before its End, once the step waits
            // (c0a4, 28 Oct: "Hey Lamp." answered as a turn). The route
            // reads the mark only once the line's words are in.
            if matches!(step, AdmissionStep::Audio(_) | AdmissionStep::End(_))
                && let Some(current) = reply.as_ref()
                && !current.local_input_ended
                && current.owner.turn() > step_call_told
                && sentence_turn != Some(current.owner.turn())
                // Its End sent early: nothing more goes for it.
                && early
                    .as_ref()
                    .is_none_or(|early| !early.holds(current.owner.turn()))
                && step_call(onboarding.as_ref())
            {
                let turn = current.owner.turn();
                provider_queue.push_back(ProviderInput::StepCall {
                    request: turn,
                    privacy_generation: snapshot.microphone_generation(),
                });
                step_call_lines.push_back((turn, None));
                step_call_told = turn;
                if step_call_lines.len() > STEP_CALL_LINES {
                    step_call_lines.pop_front();
                }
                record(
                    trace,
                    json!({"kind":"onboarding_step_call","turn":turn,"late":true,"at_us":monotonic_us()}),
                )?;
            }
            if near_call_turn
                .is_some_and(|held| step_call_lines.iter().any(|(turn, _)| *turn == held))
                && (rig.provider.role == "second"
                    || rig
                        .second
                        .as_ref()
                        .is_some_and(second::Route::second_active))
            {
                near_call_turn = None;
            }
            match step {
                AdmissionStep::None => {}
                AdmissionStep::Rejected(rejection) => record_input_rejection(trace, rejection)?,
                AdmissionStep::Candidate(candidate) => {
                    // Control and PCM travel independently. Recheck the original
                    // write receipts after reading this candidate, with the same
                    // finite tick budget; exhaustion keeps the input pending.
                    if !service_speaker_control(
                        rig,
                        owner,
                        reply,
                        &mut speaker_pending,
                        &mut playback_mask,
                        &mut snapshot,
                        &mut last_publish,
                        &mut speaker_control_budget,
                        trace,
                        &mut speaker_lapsed,
                    )? {
                        latch_invitation_sounded(
                            reply,
                            onboarding_invitation_turn,
                            &mut onboarding_invitation_audio,
                        );
                        continue;
                    }
                    latch_invitation_sounded(
                        reply,
                        onboarding_invitation_turn,
                        &mut onboarding_invitation_audio,
                    );
                    if let Some(lapsed) = speaker_lapsed.take()
                        && cut_after_speaker_lapse(
                            lapsed,
                            owner,
                            reply,
                            &mut provider_queue,
                            &mut speaker_pending,
                            trace,
                        )?
                    {
                        snapshot = owner.snapshot(now())?;
                        rig.publish(snapshot)?;
                        last_publish = monotonic_us();
                    }
                    // Finish an already qualified reminder before this
                    // candidate can cancel its receipt. Keep the candidate's
                    // audio pending while the existing completion path below
                    // retires the owner and updates admission. The next capture
                    // frame, including quiet, can then admit its kept prefix.
                    // Failed, partial or reset delivery keeps the interruption
                    // path; this check never acknowledges a reminder itself.
                    if let Some(current) = reply.as_ref()
                        && current.whole_provider_output(&speaker_pending)
                        && current
                            .reminder
                            .as_ref()
                            .is_some_and(crate::reminders::Receipt::eligible_for_completion)
                    {
                        let completion_authority = owner.snapshot(now())?;
                        if completion_authority.owner() == Some(current.owner)
                            && completion_authority.microphone_permission() == Permission::Allowed
                        {
                            break;
                        }
                    }
                    // In a room busy with other people's talk, what the
                    // lamp has just been asked is not dropped for the next
                    // voice: until its answer begins, or 1.8 s have gone
                    // without one, a new sound does not take the turn.
                    let at_us = monotonic_us();
                    #[cfg(not(test))]
                    let busy = room.busy();
                    #[cfg(test)]
                    let busy = room.busy() || TEST_ROOM_BUSY.with(std::cell::Cell::get);
                    let holding = busy
                        && reply.as_ref().is_some_and(|current| {
                            let turn = current.owner.turn();
                            current.local_input_ended
                                && current.playback.is_none()
                                && if current.had_audio {
                                    current
                                        .last_provider_audio_at_us
                                        .is_some_and(|at| at_us.saturating_sub(at) < HOLD_FLOOR_US)
                                } else {
                                    speech_ended.is_some_and(|(ended, at)| {
                                        ended == turn && at_us.saturating_sub(at) < HOLD_FLOOR_US
                                    })
                                }
                        });
                    // A sound begun just after a line that ended before its
                    // name goes on from it (`LAMP_SECOND_OPEN_NAME`): the
                    // answer wait and the body's motors don't let it go. The
                    // keeper's pause, the busy-room hold and asker-first,
                    // which keep the room's next voice off an awaited answer,
                    // still do (the reviewer's P3 on #2355).
                    let goes_on_from_name = open_name::goes_on(
                        open_name_turn,
                        reply
                            .as_ref()
                            .map(|current| (current.owner.turn(), answer_unheard(current))),
                        candidate.first_read_at_us,
                    );
                    // The line after a bare name: decided at once, as the
                    // name's carry waits for it; it is not marked as going on
                    // from that line, which the second route joins itself.
                    let after_the_call = open_name::goes_on(
                        call_turn,
                        reply
                            .as_ref()
                            .map(|current| (current.owner.turn(), answer_unheard(current))),
                        candidate.first_read_at_us,
                    );
                    if paused {
                        // Paused by the keeper: a new sound takes no turn,
                        // whether or not one is under way; that one goes on.
                        if let Some(rejection) = admission.release_prospective(monotonic_us())? {
                            record_input_rejection(trace, rejection)?;
                        }
                        detector.clear_prospective();
                        if !refused_while_paused {
                            refused_while_paused = true;
                            record(
                                trace,
                                json!({"kind":"input_not_taken_paused","at_us":at_us}),
                            )?;
                        }
                        continue;
                    }
                    if holding {
                        // Read the clock again: the admission's own must
                        // not be handed a time before its last.
                        if let Some(rejection) = admission.release_prospective(monotonic_us())? {
                            record_input_rejection(trace, rejection)?;
                        }
                        detector.clear_prospective();
                        let turn = reply.as_ref().map(|current| current.owner.turn());
                        if floor_held_for != turn {
                            floor_held_for = turn;
                            record(
                                trace,
                                json!({"kind":"floor_held","turn":turn,"at_us":at_us,
                                    "boundary":"a new sound did not take the turn while an answer was awaited in a busy room"}),
                            )?;
                        }
                        continue;
                    }
                    // #2220: while a line the lamp started on its own has
                    // not sounded, a sound the room makes doesn't end it.
                    let own_wait = OwnLineWait {
                        on: own_line_keeps,
                        lamp_began,
                        waited: own_kept_for == Some(candidate.id),
                        heard: !playback_mask.is_duplex() || frame.echo.is_some(),
                        name_began_us,
                        // A face seen speaking through the sound, turned to
                        // the lamp: from its first read to its last speech
                        // and the eyes' lag after it, so the looks after a
                        // short "Not now." don't count against it (the
                        // reviewer's P3 on #2308).
                        theirs: own_line_eyes.as_ref().is_some_and(|eyes| {
                            let (now, now_us) = (Instant::now(), monotonic_us());
                            let back = |at_us: u64| {
                                now.checked_sub(Duration::from_micros(now_us.saturating_sub(at_us)))
                            };
                            let until_us = if last_speech_us >= candidate.first_read_at_us {
                                last_speech_us
                                    .saturating_add(OWN_LINE_EYES_LAG_US)
                                    .min(now_us)
                            } else {
                                now_us
                            };
                            back(candidate.first_read_at_us)
                                .zip(back(until_us))
                                .is_some_and(|(from, to)| {
                                    !eyes.turned_speakers(from, to).is_empty()
                                })
                        }),
                        let_go_goes_on: own_let_go.zip(reply.as_ref()).is_some_and(
                            |(sound, current)| sound.goes_on(current.owner.turn(), &candidate),
                        ),
                        busy: own_line_over_talk
                            && own_line_busy.zip(reply.as_ref()).is_some_and(
                                |((turn, busy), current)| busy && turn == current.owner.turn(),
                            ),
                        stop_began_us,
                    };
                    let own = while_own_line_waits(own_wait, reply.as_ref(), &candidate, at_us);
                    // Decided as the line's person's: no rule after it takes
                    // it for the body's motors, as the lamp's name isn't.
                    let mut own_theirs_now = false;
                    // With `words`, its words too, as the answer's wait
                    // hears them (#2287's follow-up).
                    let own = match wait_words.as_mut() {
                        Some(words) => {
                            let (own, told) = own_line_with_words(
                                words,
                                own,
                                &candidate,
                                admission.pending_audio(candidate.id).unwrap_or_default(),
                                admission.pending_ended(),
                                detector.pause_blocks(),
                                last_speech_us,
                                room.busy(),
                                at_us,
                            );
                            if let Some(told) = told {
                                // The count of its words only, never the words.
                                record(
                                    trace,
                                    json!({"kind":"wait_words","turn":reply.as_ref().map(|current| current.owner.turn()),
                                        "candidate":candidate.id,"words":told.words,"took":told.took,
                                        "stop":told.stop,"asked_at_us":told.asked_at_us,"at_us":at_us,
                                        "for":"own_line"}),
                                )?;
                            }
                            own
                        }
                        None => own,
                    };
                    match own {
                        WhileOwnLineWaits::AsBefore | WhileOwnLineWaits::Decide => {}
                        WhileOwnLineWaits::Theirs => {
                            own_theirs_now = true;
                            if own_theirs_for != Some(candidate.id) {
                                own_theirs_for = Some(candidate.id);
                                eprintln!(
                                    "lamp-voice: a sound while the lamp's own line waited to sound was its person's, turned to the lamp and speaking: decided as before"
                                );
                                record(
                                    trace,
                                    json!({"kind":"own_line_theirs","turn":reply.as_ref().map(|current| current.owner.turn()),
                                        "candidate":candidate.id,"at_us":at_us}),
                                )?;
                            }
                        }
                        WhileOwnLineWaits::Stop => {
                            // Only a stop, said before the line sounded:
                            // never asked of the model, and the line is
                            // stopped on the next pass, as a stop said before
                            // an answer is.
                            if let Some(rejection) = admission.release_prospective(monotonic_us())?
                            {
                                record_input_rejection(trace, rejection)?;
                            }
                            detector.clear_prospective();
                            own_kept_for = None;
                            waited_stop = true;
                            record(
                                trace,
                                json!({"kind":"input_let_go_own_line","turn":reply.as_ref().map(|current| current.owner.turn()),
                                    "candidate":candidate.id,"why":"stop_said","at_us":at_us}),
                            )?;
                            continue;
                        }
                        // Pending for the name, the line playing on: not
                        // `own_kept_for`, which would let it go as `sounded`.
                        WhileOwnLineWaits::ForName => continue,
                        WhileOwnLineWaits::Wait => {
                            if own_kept_for != Some(candidate.id) {
                                own_kept_for = Some(candidate.id);
                                record(
                                    trace,
                                    json!({"kind":"own_line_waits","turn":reply.as_ref().map(|current| current.owner.turn()),
                                        "candidate":candidate.id,"at_us":at_us}),
                                )?;
                            }
                            continue;
                        }
                        WhileOwnLineWaits::LetGo(why) => {
                            if let Some(rejection) = admission.release_prospective(monotonic_us())?
                            {
                                record_input_rejection(trace, rejection)?;
                            }
                            detector.clear_prospective();
                            own_kept_for = None;
                            // Let go at its deadline, as the line sounded, or
                            // going on from one that was: kept while it goes
                            // on (#2602).
                            if let Some(current) = reply.as_ref() {
                                own_let_go =
                                    LetGoSound::let_go(own, current.owner.turn(), last_speech_us)
                                        .or(own_let_go);
                            }
                            if why == "busy_room" {
                                // Talk goes on as many candidates, each its
                                // own record: the journal says it once a line.
                                let turn = reply.as_ref().map(|current| current.owner.turn());
                                if busy_room_told != turn {
                                    busy_room_told = turn;
                                    eprintln!(
                                        "lamp-voice: talk over the lamp's own line in a busy room, with no name or face turned to it, is let go; the line goes on"
                                    );
                                }
                            } else {
                                eprintln!(
                                    "lamp-voice: a sound while the lamp's own line waited to sound was let go ({why}); the line goes on"
                                );
                            }
                            record(
                                trace,
                                json!({"kind":"input_let_go_own_line","turn":reply.as_ref().map(|current| current.owner.turn()),
                                "candidate":candidate.id,"why":why,"at_us":at_us,
                                "boundary":if why == "goes_on" {
                                    "a sound let go while a line the lamp started on its own waited, going on past its first audio: the room's"
                                } else if why == "busy_room" {
                                    "talk over a line the lamp started on its own, sounding, in a room busy as it began, with no name, stop word or face turned to the lamp: the room's"
                                } else {
                                    "a sound while a line the lamp started on its own had not sounded: the room's"
                                }}),
                            )?;
                            continue;
                        }
                    }
                    // #1712's B: a new sound that began as the body moved a
                    // joint, while an answer is awaited, is the lamp's own
                    // motors: let go, and the answer goes on. Until the
                    // body has said where it was then, it waits.
                    if let Some(motion) = motion_in.as_mut().filter(|_| {
                        !own_theirs_now
                            && !goes_on_from_name
                            && !after_the_call
                            && motion_kept_for != Some(candidate.id)
                    }) {
                        let waits_until_us = candidate
                            .trigger_read_at_us
                            .saturating_add(crate::body_motion::WAITS_US)
                            .min(candidate.decision_deadline_us.saturating_sub(20_000));
                        match body_heard(
                            || {
                                motion.judge(
                                    candidate.first_read_at_us,
                                    candidate.trigger_read_at_us,
                                    at_us,
                                    crate::privacy_bus::wall_ms(),
                                    waits_until_us,
                                )
                            },
                            reply.as_ref(),
                            reply_sounding(&playback_mask, reply, at_us),
                            name_began_us,
                            &candidate,
                        ) {
                            crate::body_motion::Judged::Moved { began, ended } => {
                                let (decision, levels) = motion_voice_heard(
                                    motion_voice,
                                    asker_first.as_ref(),
                                    reply.as_ref(),
                                    &candidate,
                                    at_us,
                                );
                                let mut moving = json!({"at_us":at_us,
                                    "turn":reply.as_ref().map(|current| current.owner.turn()),
                                    "onset_from_us":candidate.first_read_at_us,
                                    "onset_to_us":candidate.trigger_read_at_us,
                                    "moved_from_us":began,"moved_until_us":ended});
                                if motion_voice != crate::motion_voice::Mode::Off {
                                    moving["candidate"] = json!(candidate.id);
                                    moving["motion_voice"] = json!(motion_voice.as_str());
                                    moving["margin"] = json!(crate::motion_voice::MARGIN);
                                    moving["asker_level"] =
                                        json!(levels.map(|levels| levels.asker));
                                    moving["heard_level"] =
                                        json!(levels.map(|levels| levels.heard));
                                    moving["heard_upper_tenth"] =
                                        json!(levels.map(|levels| levels.upper));
                                    moving["blocks"] =
                                        json!(levels.map_or(0, |levels| levels.blocks));
                                }
                                if let crate::motion_voice::Decision::LetGo { would_keep } =
                                    decision
                                {
                                    if let Some(rejection) =
                                        admission.release_prospective(monotonic_us())?
                                    {
                                        record_input_rejection(trace, rejection)?;
                                    }
                                    detector.clear_prospective();
                                    eprintln!(
                                        "lamp-voice: a sound that began as the body moved a joint was let go; the answer goes on"
                                    );
                                    moving["kind"] = json!("input_let_go_body_moving");
                                    moving["boundary"] = json!(
                                        "a new sound began as the body moved a joint while an answer was awaited: the lamp's own motors"
                                    );
                                    if motion_voice != crate::motion_voice::Mode::Off {
                                        moving["would_keep"] = json!(would_keep);
                                        moving["boundary"] = json!(
                                            "a new sound began as the body moved a joint while an answer was awaited: the lamp's own motors; levels on the face's scale, 7.5 to a decibel; would_keep: LAMP_MOTION_VOICE=on would have taken it for the asker's"
                                        );
                                    }
                                    record(trace, moving)?;
                                    continue;
                                }
                                // About as loud as the asker: theirs. It goes
                                // on to the rules after this one, as if the
                                // body were still.
                                eprintln!(
                                    "lamp-voice: a sound that began as the body moved a joint is about as loud as the asker: theirs, not the motors"
                                );
                                motion_kept_for = Some(candidate.id);
                                moving["kind"] = json!("input_kept_body_moving");
                                moving["boundary"] = json!(
                                    "a new sound began as the body moved a joint while an answer was awaited, its own speech no more than the margin under the asker's: the asker's, judged on as if the body were still; levels on the face's scale, 7.5 to a decibel"
                                );
                                record(trace, moving)?;
                            }
                            crate::body_motion::Judged::Waits => continue,
                            crate::body_motion::Judged::Still => {}
                        }
                    }
                    // #1123: while an answer is awaited and has not
                    // sounded, a new sound takes the turn only once it has
                    // gone on; one that ends or goes quiet first is let go,
                    // and the answer goes on.
                    let wait = AnswerWait {
                        on: wait_keeps_answer,
                        waited: answer_kept_for == Some(candidate.id),
                        asked_until_us: speech_ended
                            .filter(|(turn, _)| {
                                reply.as_ref().map(|current| current.owner.turn()) == Some(*turn)
                            })
                            .map(|(_, at)| at),
                        name_began_us,
                        let_go_ended_us,
                        sounding: reply_sounding(&playback_mask, reply, at_us),
                        decided: answer_sounded_for == Some(candidate.id),
                        heard: !playback_mask.is_duplex() || frame.echo.is_some(),
                    };
                    let mut waits = if goes_on_from_name || after_the_call {
                        WhileAnswerWaits::Decide
                    } else {
                        while_answer_waits(
                            wait,
                            reply.as_ref(),
                            &candidate,
                            admission.pending_ended(),
                            detector.pause_blocks(),
                            at_us,
                        )
                    };
                    // With `words`, its words too (step 2).
                    if let Some(words) = wait_words
                        .as_mut()
                        .filter(|_| !goes_on_from_name && !after_the_call)
                    {
                        let told;
                        (waits, told) = with_waited_words(
                            words,
                            waits,
                            &candidate,
                            admission.pending_audio(candidate.id).unwrap_or_default(),
                            admission.pending_ended(),
                            detector.pause_blocks(),
                            last_speech_us,
                            may_hold_once_decided(wait),
                            at_us,
                        );
                        if let Some(told) = told {
                            // The count of its words only, never the words.
                            record(
                                trace,
                                json!({"kind":"wait_words","turn":reply.as_ref().map(|current| current.owner.turn()),
                                    "candidate":candidate.id,"words":told.words,"took":told.took,
                                    "stop":told.stop,"asked_at_us":told.asked_at_us,"at_us":at_us}),
                            )?;
                        }
                    }
                    match waits {
                        WhileAnswerWaits::Decide => {
                            if wait.waited && wait.sounding {
                                answer_sounded_for = Some(candidate.id);
                            }
                        }
                        WhileAnswerWaits::Wait => {
                            if answer_kept_for != Some(candidate.id) {
                                answer_kept_for = Some(candidate.id);
                                record(
                                    trace,
                                    json!({"kind":"answer_waits","turn":reply.as_ref().map(|current| current.owner.turn()),
                                        "candidate":candidate.id,"at_us":at_us}),
                                )?;
                            }
                            continue;
                        }
                        WhileAnswerWaits::LetGo(why) => {
                            if let Some(rejection) = admission.release_prospective(monotonic_us())?
                            {
                                record_input_rejection(trace, rejection)?;
                            }
                            // The last block the detector heard as speech:
                            // when the sound let go stopped.
                            let_go_ended_us = Some(last_speech_us);
                            detector.clear_prospective();
                            record(
                                trace,
                                json!({"kind":"input_let_go_answer_waits","turn":reply.as_ref().map(|current| current.owner.turn()),
                                    "candidate":candidate.id,"why":why,"at_us":at_us}),
                            )?;
                            continue;
                        }
                        WhileAnswerWaits::Stop => {
                            // "Stop." said before the answer began: never
                            // asked of the model, and the answer it waits
                            // for is stopped on the next pass, as a stop
                            // said over the lamp's speech stops it (the
                            // reviewer's P3 on #1277).
                            if let Some(rejection) = admission.release_prospective(monotonic_us())?
                            {
                                record_input_rejection(trace, rejection)?;
                            }
                            detector.clear_prospective();
                            waited_stop = true;
                            record(
                                trace,
                                json!({"kind":"input_let_go_answer_waits","turn":reply.as_ref().map(|current| current.owner.turn()),
                                    "candidate":candidate.id,"why":"stop_said","at_us":at_us}),
                            )?;
                            continue;
                        }
                    }
                    // While an answer is awaited, speech well under the
                    // asker's is someone else's talk: it takes no turn, and
                    // the lamp keeps the answer (#829). It is judged on its
                    // own blocks, from its own start, and waits, kept as it
                    // is, until enough of them are heard.
                    let own_start_us = candidate
                        .trigger_read_at_us
                        .saturating_sub(u64::from(crate::activity::START_BLOCKS - 1) * BLOCK_US);
                    // Its last chance: before its deadline, or its oldest
                    // block's read history, runs out.
                    let last_chance_at_us = candidate
                        .decision_deadline_us
                        .min(
                            candidate
                                .first_read_at_us
                                .saturating_add(crate::turn_taking::HISTORY_US),
                        )
                        .saturating_sub(ASKER_LAST_CHANCE_US);
                    // A line the rule waits on is judged on its own blocks
                    // until its last chance, though the answer begins over
                    // it: the asker going on after a soft "um" still takes
                    // the turn (the reviewer on #1197). Judged at the
                    // answer's first block, it was let go on its soft start.
                    let waited = asker_waited.is_some_and(|(id, ..)| id == candidate.id);
                    if !waited {
                        asker_waited = None;
                    }
                    let over_answer = reply
                        .as_ref()
                        .is_some_and(|current| current.playback.is_some());
                    let judged = reply
                        .as_ref()
                        .filter(|current| {
                            current.local_input_ended && (current.playback.is_none() || waited)
                        })
                        .and_then(|current| {
                            let turn = current.owner.turn();
                            // Nor does one whose utterance has ended, which
                            // would refuse the next one's Start while it
                            // waited (the first builder on #1092).
                            let last_chance =
                                admission.pending_ended() || at_us >= last_chance_at_us;
                            Some((
                                turn,
                                asker_first
                                    .as_ref()?
                                    .judge(turn, own_start_us, at_us, last_chance),
                            ))
                        });
                    match judged {
                        // Neither let go nor admitted: on a later block.
                        Some((turn, crate::asker_first::Judgement::Wait))
                            if asker_mode == crate::asker_first::Mode::On =>
                        {
                            asker_waited =
                                Some((candidate.id, turn, own_start_us, last_chance_at_us));
                            continue;
                        }
                        // Recorded only, it never waits: it is taken now,
                        // and followed as `on` would follow it.
                        Some((turn, crate::asker_first::Judgement::Wait)) => {
                            asker_shadow = Some((turn, own_start_us, last_chance_at_us));
                        }
                        // Well under the asker at its last chance, it is let
                        // go, so the answer plays, if it has begun: left to
                        // the duplex rule then, it was admitted a block into
                        // the answer and replaced it.
                        Some((turn, crate::asker_first::Judgement::Keep(kept))) => {
                            asker_waited = None;
                            let acts = asker_mode == crate::asker_first::Mode::On;
                            if acts {
                                if let Some(rejection) =
                                    admission.release_prospective(monotonic_us())?
                                {
                                    record_input_rejection(trace, rejection)?;
                                }
                                detector.clear_prospective();
                            }
                            if let Some(rule) = asker_first.as_mut() {
                                rule.kept_line(at_us);
                                record_asker_first(
                                    rule,
                                    trace,
                                    turn,
                                    at_us,
                                    kept,
                                    acts,
                                    if over_answer {
                                        "after_its_answer_began"
                                    } else {
                                        "at_its_last_chance"
                                    },
                                )?;
                            }
                            if acts {
                                continue;
                            }
                        }
                        _ => asker_waited = None,
                    }
                    let Some(accepted) = decide_prospective_input(
                        &playback_mask,
                        &mut detector,
                        &mut admission,
                        candidate,
                        AdmissionContext {
                            capture: lineage,
                            authority: owner.snapshot(now())?,
                        },
                        monotonic_us(),
                        trace,
                    )?
                    else {
                        continue;
                    };
                    // Recheck priority immediately before admitting a new owner.
                    // A coalesced recovery is a session change even when its outage
                    // observation never fitted in the priority socket.
                    provider_control_empty = service_provider_control(
                        &mut rig.provider.channels.control,
                        &mut cloud,
                        &mut provider_control_budget,
                        trace,
                    )?;
                    apply_submission_gate(
                        &cloud,
                        reply,
                        &mut provider_queue,
                        &mut admission,
                        &mut submission_draining,
                        trace,
                    )?;
                    let held_refused = refused_hold && model_refusing.holds(monotonic_us());
                    let Some(provider_session) = cloud.ready_session().filter(|_| {
                        provider_control_empty && !submission_draining && !held_refused
                    }) else {
                        if ((offline || service_down) && !cloud.available()) || held_refused {
                            // Heard while offline, or while only the voice
                            // service is down: a turn of its own, never sent.
                            snapshot = commit_offline_input(
                                owner,
                                reply,
                                &mut speaker_pending,
                                &mut provider_queue,
                                accepted,
                                rig.playout_start_now(),
                                (service_down && !offline) || held_refused,
                                trace,
                                |state| rig.publish(state),
                            )?;
                            last_publish = monotonic_us();
                            turn_blocks.extend(loose_blocks.drain(..).map(|(_, level)| level));
                            continue;
                        }
                        let mut interrupted = take_interrupted_reminder(reply);
                        let (state, draining) =
                            accepted_without_cloud(accepted, owner, reply, &mut admission, trace)?;
                        provider_queue.clear();
                        speaker_pending = None;
                        submission_draining = draining;
                        snapshot = state;
                        rig.publish(snapshot)?;
                        if let Some(receipt) = &mut interrupted {
                            receipt.interrupted_after_owner_fenced();
                        }
                        last_publish = monotonic_us();
                        continue;
                    };
                    // Someone is answered: the lamp has shown it is back.
                    back_owed_since = None;
                    // Sent while refusing: the service is tried, and the
                    // turns after it held for the time again (#4265).
                    model_refusing.tried(monotonic_us());
                    // Taking over from the lamp's own answer is cutting in,
                    // and taking over from something that was said is going
                    // on with it. Taking over from a sound is neither.
                    let took_over = reply.as_ref().is_some_and(|current| {
                        crate::not_speech::takes_over(
                            lamps_voice_heard(Some(current)),
                            said_turn == Some(current.owner.turn()),
                        )
                    });
                    // Speech back soon after the lamp ended a turn, before
                    // anything of its answer was heard: the same utterance
                    // going on, its pause longer than the end waited (#782).
                    let continues = ended_turn.take().is_some_and(|ended| {
                        ended.goes_on_in(
                            reply.as_ref(),
                            accepted.candidate.trigger_read_at_us,
                            provider_session,
                            snapshot.microphone_generation(),
                        )
                    });
                    // Over the lamp's sound or a line said is over the lamp;
                    // over a sound sent on for the model to judge, with the
                    // lamp silent, is not (the reviewer on #1278).
                    let follows_on = crate::follows_on::due(
                        rig.follows_on,
                        took_over,
                        lamp_words_ended_us,
                        accepted.candidate.trigger_read_at_us,
                    );
                    let heard_of_cut = heard_of_cut(
                        said_as_heard,
                        reply.as_ref(),
                        &mut stop_cut,
                        provider_session,
                    );
                    // A line begun in onboarding's sentence window is the
                    // sentence (#1803): count-only from its Start.
                    let count_only = sentence_window(onboarding.as_ref(), monotonic_us());
                    // Begun while the walk is under way (#3748): a
                    // `go_to_sleep` it calls is answered not now.
                    let not_now = sleep_not_now(
                        crate::sleep_asked::switched_on(),
                        controls.as_ref(),
                        onboarding.as_ref(),
                    );
                    let trigger_us = accepted.candidate.trigger_read_at_us;
                    snapshot = commit_admitted_input(
                        owner,
                        reply,
                        &mut speaker_pending,
                        &mut provider_queue,
                        accepted,
                        provider_session,
                        continues,
                        goes_on_from_name,
                        took_over,
                        stop_untold.take_for(provider_session, monotonic_us()),
                        heard_of_cut,
                        follows_on,
                        count_only,
                        not_now,
                        rig.playout_start_now(),
                        trace,
                        |state| rig.publish(state),
                    )?;
                    if not_now {
                        onboarding_marked = reply.as_ref().map(|current| current.owner.turn());
                    }
                    line_began = reply
                        .as_ref()
                        .map(|current| (current.owner.turn(), trigger_us));
                    last_publish = monotonic_us();
                    // Begun while how_you_sound waits for a call (#3504):
                    // the second route leaves a line that is only the name
                    // to the step. Its Start and opening are queued already.
                    if !count_only
                        && step_call(onboarding.as_ref())
                        && let Some(current) = reply.as_ref()
                    {
                        provider_queue.push_back(ProviderInput::StepCall {
                            request: current.owner.turn(),
                            privacy_generation: snapshot.microphone_generation(),
                        });
                        step_call_lines.push_back((current.owner.turn(), None));
                        step_call_told = current.owner.turn();
                        if step_call_lines.len() > STEP_CALL_LINES {
                            step_call_lines.pop_front();
                        }
                        record(
                            trace,
                            json!({"kind":"onboarding_step_call","turn":current.owner.turn(),"at_us":monotonic_us()}),
                        )?;
                    }
                    if goes_on_from_name {
                        open_name_turn = None;
                        eprintln!(
                            "lamp-voice: request {}: goes on from the line that ended before its name (LAMP_SECOND_OPEN_NAME)",
                            reply.as_ref().map_or(0, |current| current.owner.turn())
                        );
                    }
                    if after_the_call {
                        call_turn = None;
                        eprintln!(
                            "lamp-voice: request {}: the line after the bare name, taken at once (LAMP_CALL_TAKES_NEXT_LINE)",
                            reply.as_ref().map_or(0, |current| current.owner.turn())
                        );
                    }
                    if took_over {
                        took_over_turn = reply.as_ref().map(|current| current.owner.turn());
                    }
                    if let Some(current) = reply.as_ref() {
                        second_look.request_began(current.owner.turn(), monotonic_us());
                    }
                    // Words heard from now are this line's: counted only
                    // when it is the sentence.
                    if count_only {
                        sentence_turn = reply.as_ref().map(|current| current.owner.turn());
                    }
                    sentence_heard = count_only.then(String::new);
                    // The sound just before a turn began is the turn's.
                    turn_blocks.extend(loose_blocks.drain(..).map(|(_, level)| level));
                }
                AdmissionStep::Audio(audio) => {
                    let Some(current) = reply.as_mut() else {
                        return Err(io::Error::other("speech continuation lost its owner").into());
                    };
                    let turn = current.owner.turn();
                    if near_call_window(
                        onboarding.as_ref(),
                        speech_levels.front().map(|(at, _)| *at),
                        monotonic_us(),
                    ) {
                        near_call_turn = Some(turn);
                    }
                    let mut resumed = None;
                    if early.as_ref().is_some_and(|early| early.holds(turn)) {
                        // The end was sent early: nothing more goes to the
                        // provider for this request. Someone speaking again
                        // gives it up, and what they go on to say is its
                        // successor.
                        if let Some(early) = early.as_mut()
                            && let HeardAfterEnd::Resumed { given_up, opening } =
                                early.hear(audio, heard)
                        {
                            resumed = Some((given_up, opening));
                        }
                    } else {
                        let captured_at_us = audio.captured_at_us;
                        queue_input(
                            &mut provider_queue,
                            current,
                            snapshot.microphone_generation(),
                            audio,
                            provider_queue_bound,
                        )?;
                        if let Some(early_finalize) = early_finalize.as_mut()
                            && early_finalize.heard(turn, detector.pause_blocks())
                        {
                            queue_input_paused(
                                &mut provider_queue,
                                current,
                                snapshot.microphone_generation(),
                                provider_queue_bound,
                            );
                        }
                        // The end sent early, after a short quiet. Not for
                        // what would be thrown out as a sound if the quiet
                        // went on: that is judged when the lamp is sure, as
                        // it always was, so that a lone "Lamp," is never
                        // thrown out for a pause before its question.
                        if let Some(early) = early.as_mut()
                            && current.local_only.is_none()
                            && early.due(turn, detector.pause_blocks())
                            && !crate::not_speech::was_a_sound(
                                speech_levels.len(),
                                took_over_turn == Some(turn),
                                crate::not_speech::own_words_ago(
                                    last_spoke_us,
                                    stopped_at_us,
                                    monotonic_us(),
                                ),
                            )
                        {
                            let sent_at = monotonic_us();
                            queue_input_end(
                                &mut provider_queue,
                                current,
                                snapshot.microphone_generation(),
                                provider_queue_bound,
                                false,
                                took_over_turn == Some(turn),
                            )?;
                            early.send(turn, sent_at, captured_at_us);
                            // A call's line ended to the route here: its
                            // words may come back before the lamp is sure
                            // (#3923, a slow "Hey, … Lamp").
                            if let Some((_, spoken)) =
                                step_call_lines.iter_mut().find(|(told, _)| *told == turn)
                                && let Some((_, began)) = line_began.filter(|(of, _)| *of == turn)
                            {
                                *spoken = Some((began, captured_at_us.max(began)));
                                near_call_line_ended(onboarding.as_mut(), began);
                            }
                            // The request's sound has ended, for the floor,
                            // the second look and the room. The lamp's own
                            // authority still says it is listening.
                            speech_ended = Some((turn, sent_at));
                            second_look.speech_ended(turn, sent_at);
                            if speech_levels.len() >= crate::not_speech::SCRAP_BLOCKS {
                                said_turn = Some(turn);
                                // Someone was there, for the news: the
                                // confirmation later skips the usual place
                                // (found by the third builder on #709).
                                said_at_us = Some(sent_at);
                            }
                            asked_in_busy_room = room.busy().then_some(turn);
                            record(
                                trace,
                                json!({"kind":"early_end","owner":current.owner,"turn":turn,"at_us":sent_at,
                                    "last_block_host_read_us":captured_at_us,
                                    "pause_ms":u64::from(early.blocks()) * 10,
                                    "speech_blocks":speech_levels.len(),
                                    "boundary":"request ended to the provider queue; not yet the lamp's end of speech, which still decides playback"}),
                            )?;
                        }
                    }
                    if let Some((given_up, opening)) = resumed {
                        // Whether the provider can take the successor now,
                        // checked as for any new input.
                        provider_control_empty = service_provider_control(
                            &mut rig.provider.channels.control,
                            &mut cloud,
                            &mut provider_control_budget,
                            trace,
                        )?;
                        apply_submission_gate(
                            &cloud,
                            reply,
                            &mut provider_queue,
                            &mut admission,
                            &mut submission_draining,
                            trace,
                        )?;
                        let took_over = reply.as_ref().is_some_and(|current| {
                            crate::not_speech::takes_over(
                                lamps_voice_heard(Some(current)),
                                said_turn == Some(current.owner.turn()),
                            )
                        });
                        let resumed_at = monotonic_us();
                        let opening_blocks = opening.len();
                        match cloud
                            .ready_session()
                            .filter(|_| provider_control_empty && !submission_draining)
                        {
                            Some(provider_session) => {
                                let not_now = sleep_not_now(
                                    crate::sleep_asked::switched_on(),
                                    controls.as_ref(),
                                    onboarding.as_ref(),
                                );
                                // The same line going on: its start is the
                                // line given up's (a slow "Hey, … Lamp"), or
                                // the opening's first block (#3923).
                                let line_start = line_began
                                    .map(|(_, at)| at)
                                    .or_else(|| opening.first().map(|block| block.captured_at_us));
                                snapshot = continue_after_early_end(
                                    owner,
                                    reply,
                                    &mut speaker_pending,
                                    &mut provider_queue,
                                    opening,
                                    provider_session,
                                    rig.cut_keeps_name,
                                    sentence_heard.is_some(),
                                    not_now,
                                    rig.playout_start_now(),
                                    trace,
                                    |state| rig.publish(state),
                                )?;
                                line_began = reply
                                    .as_ref()
                                    .zip(line_start)
                                    .map(|(current, at)| (current.owner.turn(), at));
                                if not_now {
                                    onboarding_marked =
                                        reply.as_ref().map(|current| current.owner.turn());
                                }
                                if took_over {
                                    took_over_turn =
                                        reply.as_ref().map(|current| current.owner.turn());
                                }
                                if let Some(current) = reply.as_ref() {
                                    second_look.request_began(current.owner.turn(), monotonic_us());
                                }
                                if sentence_heard.is_some() {
                                    sentence_turn =
                                        reply.as_ref().map(|current| current.owner.turn());
                                }
                            }
                            None => {
                                // No provider to take it: given up all the
                                // same, and the rest of the utterance is
                                // drained, as for any input it cannot take.
                                let displaced = reply.as_ref().map(|current| current.owner);
                                let mut interrupted = take_interrupted_reminder(reply);
                                cancel_reply(owner, reply, trace, "user_interrupted")?;
                                admission.discard_stream_until_end(monotonic_us())?;
                                submission_draining = true;
                                provider_queue.clear();
                                speaker_pending = None;
                                record(
                                    trace,
                                    json!({"kind":"input_not_submitted","at_us":monotonic_us(),
                                    "candidate":null,"admission_basis":"continued_after_early_end",
                                    "interrupted_owner":displaced,"reason":"provider_unavailable_or_pending_control",
                                    "provider_request":null,"provider_session":null,
                                    "outcome":RequestOutcome::Failed { stage:crate::provider_worker::FailedStage::NotDelivered,
                                        reason:crate::provider_worker::Fault::Local, local_not_submitted:true },
                                    "prefix_first_host_read_us":null}),
                                )?;
                                snapshot = owner.snapshot(now())?;
                                rig.publish(snapshot)?;
                                if let Some(receipt) = &mut interrupted {
                                    receipt.interrupted_after_owner_fenced();
                                }
                            }
                        }
                        last_publish = monotonic_us();
                        record(
                            trace,
                            json!({"kind":"early_end_given_up","turn":given_up.turn,"at_us":resumed_at,
                                "early_end_at_us":given_up.at_us,"after_early_end_ms":given_up.ago_us(resumed_at) / 1_000,
                                "successor":reply.as_ref().map(|current| current.owner.turn()),
                                "opening_blocks":opening_blocks,
                                "boundary":"someone spoke again before the lamp was sure: given up as a continued utterance"}),
                        )?;
                    }
                }
                AdmissionStep::End(audio)
                    if reply.as_ref().is_some_and(|current| {
                        near_call_turn == Some(current.owner.turn())
                            || crate::not_speech::dropped(
                                speech_levels.len(),
                                took_over_turn == Some(current.owner.turn()),
                                crate::not_speech::own_words_ago(
                                    last_spoke_us,
                                    stopped_at_us,
                                    monotonic_us(),
                                ),
                                said_close(&speech_levels, &loud_blocks, &rise),
                                short_sounds,
                                room.busy(),
                                crate::not_speech::just_stopped(
                                    stopped_at_us,
                                    last_spoke_us,
                                    monotonic_us(),
                                ),
                            )
                    }) =>
                {
                    // A sound, not something said, or the person calling the
                    // lamp's name for the near-call step (#1803). It is not
                    // sent on as a finished question: the turn ends here,
                    // unanswered.
                    let near_call = reply
                        .as_ref()
                        .is_some_and(|current| near_call_turn == Some(current.owner.turn()));
                    if let Some(current) = reply.as_ref() {
                        let mut endpoint_record = json!({"kind":"local_endpoint","owner":current.owner,"turn":current.owner.turn(),"at_us":at,
                                "last_block_host_read_us":audio.captured_at_us,
                                "includes_silence_wait_ms":detector.last_endpoint().map_or(detector.endpoint_silence_ms(), EndpointDecision::quiet_ms),
                                "acoustic_speech_end":null});
                        if near_call {
                            endpoint_record["onboarding_near_call"] = json!(
                                "held: the name alone, heard in the near-call step's try window, so the lamp does not answer it"
                            );
                        } else {
                            endpoint_record["not_speech"] = json!({"speech_blocks":speech_levels.len(),"fewer_than":crate::not_speech::SCRAP_BLOCKS,
                                "said_close":said_close(&speech_levels, &loud_blocks, &rise),
                                "last_blocks":last_blocks(&judged_blocks)});
                        }
                        if !near_call
                            && crate::not_speech::just_stopped(
                                stopped_at_us,
                                last_spoke_us,
                                monotonic_us(),
                            )
                        {
                            endpoint_record["not_speech"]["after_stop"] = json!(true);
                            eprintln!(
                                "lamp-voice: a short sound just after the lamp was stopped is not taken as a request"
                            );
                        }
                        // Its end went early, while it was not yet a sound
                        // (the lamp's own words were still near); its
                        // answer is never played.
                        if let Some(sent) = early
                            .as_mut()
                            .and_then(|early| early.confirm(current.owner.turn()))
                        {
                            endpoint_record["early_end"] = early_end_json(&sent, at);
                        }
                        record(trace, endpoint_record)?;
                    }
                    speech_levels.clear();
                    loud_blocks.clear();
                    if let Some(meter) = tone_meter.as_mut() {
                        meter.clear();
                    }
                    // Its End never went: the Stop it was to tell of goes to
                    // the next request (the first builder on #1148).
                    if let Some(stop) = reply.as_ref().and_then(|current| current.told_stop) {
                        stop_untold.given_back(stop);
                    }
                    cancel_reply(
                        owner,
                        reply,
                        trace,
                        if near_call {
                            "onboarding_near_call"
                        } else {
                            "not_speech"
                        },
                    )?;
                    speaker_pending = None;
                    provider_queue.clear();
                    snapshot = owner.snapshot(now())?;
                    rig.publish(snapshot)?;
                    last_publish = monotonic_us();
                }
                AdmissionStep::End(audio) => {
                    let current = reply
                        .as_mut()
                        .ok_or_else(|| io::Error::other("speech end lost its owner"))?;
                    let input_end_us = audio.captured_at_us;
                    // A call's line ending (#3923): when its speech began,
                    // for its words, and the step waits a moment for them.
                    // A line whose end went early was told then, not again
                    // (the Reviewer-lead on #4006); its span still reaches
                    // this End, so a hearing in between stays inside it.
                    if let Some((_, spoken)) = step_call_lines
                        .iter_mut()
                        .find(|(turn, _)| *turn == current.owner.turn())
                        && let Some((_, began)) =
                            line_began.filter(|(turn, _)| *turn == current.owner.turn())
                    {
                        let told = spoken.is_some();
                        *spoken = Some((began, input_end_us.max(began)));
                        if !told {
                            near_call_line_ended(onboarding.as_mut(), began);
                        }
                    }
                    // The sentence's line ending (begun in its window,
                    // count-only): its speech and the room microphone's
                    // largest rise during it, numbers only. Onboarding takes it if it ends in the
                    // window too.
                    if sentence_turn == Some(current.owner.turn()) {
                        let turn = current.owner.turn();
                        let speech_ms =
                            speech_levels.len() as u64 * u64::from(lamp_audio::BLOCK_MS);
                        let rise_db = speech_levels
                            .front()
                            .and_then(|(began, _)| rise.most_since(*began));
                        if let Some(walk) = onboarding.as_mut() {
                            walk.sentence_line(speech_ms, rise_db, monotonic_us());
                        }
                        record(
                            trace,
                            json!({"kind":"onboarding_sentence_line","turn":turn,
                                "speech_ms":speech_ms,"rise_db":rise_db,"at_us":monotonic_us()}),
                        )?;
                    }
                    // The lamp is sure the person has finished. A request
                    // whose end went early sends nothing more: what it
                    // waited for was this moment, and from here its answer
                    // may be played.
                    let sent = early
                        .as_mut()
                        .and_then(|early| early.confirm(current.owner.turn()));
                    current.local_input_ended = true;
                    if rig.cut_keeps_name {
                        ended_turn = Some(EndedTurn {
                            turn: current.owner.turn(),
                            at_us: input_end_us,
                            session: current.cloud.session,
                            microphone_generation: snapshot.microphone_generation(),
                        });
                    }
                    if sent.is_none() {
                        speech_ended = Some((current.owner.turn(), monotonic_us()));
                        second_look.speech_ended(current.owner.turn(), monotonic_us());
                        if speech_levels.len() >= crate::not_speech::SCRAP_BLOCKS {
                            said_turn = Some(current.owner.turn());
                            said_at_us = Some(monotonic_us());
                        } else if crate::not_speech::was_a_sound(
                            speech_levels.len(),
                            took_over_turn == Some(current.owner.turn()),
                            crate::not_speech::own_words_ago(
                                last_spoke_us,
                                stopped_at_us,
                                monotonic_us(),
                            ),
                        ) {
                            // Let through only for the model to judge
                            // (`LAMP_SHORT_SOUNDS`): unanswered, it is no one
                            // else's talk.
                            room.not_counted(current.owner.turn());
                            sound_turn = Some(current.owner.turn());
                        }
                        asked_in_busy_room = room.busy().then(|| current.owner.turn());
                        queue_input(
                            &mut provider_queue,
                            current,
                            snapshot.microphone_generation(),
                            audio,
                            provider_queue_bound,
                        )?;
                    }
                    owner.input_ended(now(), current.owner)?;
                    snapshot = owner.snapshot(now())?;
                    rig.publish(snapshot)?;
                    last_publish = monotonic_us();
                    let as_sound = sound_turn == Some(current.owner.turn());
                    // Offline, only speech gets the line: never a sound let
                    // through, nor a scrap too short to be a request.
                    if let Some(local) = current.local_only.as_mut() {
                        local.speech = !as_sound && said_turn == Some(current.owner.turn());
                    }
                    if sent.is_none() {
                        // Only the lamp's name, and nothing more in its
                        // wait: the model is told so before the end (#782).
                        if let Some(called) = call_note(
                            detector.last_endpoint(),
                            current.owner.turn(),
                            current.local_only.is_some(),
                            snapshot.microphone_generation(),
                            provider_queue.len(),
                            provider_queue_bound,
                        ) {
                            provider_queue.push_back(called);
                        }
                        queue_input_end(
                            &mut provider_queue,
                            current,
                            snapshot.microphone_generation(),
                            provider_queue_bound,
                            as_sound,
                            took_over_turn == Some(current.owner.turn()),
                        )?;
                    }
                    answer_audio.decided(current.owner.turn(), at);
                    let mut endpoint_record = json!({"kind":"local_endpoint","owner":current.owner,"turn":current.owner.turn(),"at_us":at,"last_block_host_read_us":input_end_us,
                        "includes_silence_wait_ms":detector.last_endpoint().map_or(detector.endpoint_silence_ms(), EndpointDecision::quiet_ms),
                        "acoustic_speech_end":null});
                    if as_sound {
                        endpoint_record["let_through_as_sound"] = json!(true);
                    }
                    // Baseline records keep exactly their existing fields.
                    if detector.confirmation_policy().is_some() {
                        endpoint_record["endpoint"] = json!(detector.last_endpoint());
                    }
                    if let Some(sent) = sent.as_ref() {
                        endpoint_record["early_end"] = early_end_json(sent, at);
                    }
                    let confirmed = sent.map(|sent| (current.owner.turn(), sent));
                    // How loud this utterance was at the microphone after
                    // cancellation, on the face's scale (0 is -48 dBFS or
                    // under, 255 is -14 or over): the middle and the upper
                    // tenth of its speech blocks. For telling someone at
                    // the lamp from someone across the room.
                    if !speech_levels.is_empty() {
                        let mut levels: Vec<u8> =
                            speech_levels.iter().map(|(_, level)| *level).collect();
                        levels.sort_unstable();
                        endpoint_record["speech_level"] = json!({
                            "blocks": levels.len(),
                            "median": levels[levels.len() / 2],
                            "upper_tenth": levels[levels.len() * 9 / 10],
                            // Its loud blocks, when it was said close: what
                            // let a short one through the gate (Ten on #1137).
                            "said_close": said_close(&speech_levels, &loud_blocks, &rise),
                        });
                        // A short utterance that was taken as said carries
                        // how its blocks were judged, as one thrown out as
                        // a sound does: what tells a short word from a
                        // sound is to be read from the two side by side.
                        if levels.len() < crate::not_speech::SHORT_BLOCKS {
                            endpoint_record["last_blocks"] = last_blocks(&judged_blocks);
                        }
                        speech_levels.clear();
                    }
                    loud_blocks.clear();
                    // How it sounded against the person's own way: read now,
                    // told with its words. A line before it whose words
                    // never came is told without them. A sound let through
                    // is not a line.
                    if let Some(meter) = tone_meter.as_mut() {
                        let went_on = meter.going_on() && !as_sound;
                        let tone = meter.finish(input_end_us).filter(|_| !as_sound);
                        // A line that went on from the one before is read
                        // over both and taken in once: the one before is
                        // not told, and the baselines are as they were
                        // before it. Any other line before it is told now,
                        // with its words or without.
                        let mut before_line = tone_held.reading.take();
                        let mut gone_on_from = None;
                        if went_on && tone.is_some() {
                            gone_on_from = before_line.take();
                            if let Some(before) =
                                gone_on_from.as_mut().and_then(|held| held.before.take())
                            {
                                tone_baselines = before;
                            }
                        }
                        if let Some(held) = before_line {
                            held.tell(trace, tone_out.as_mut())?;
                        }
                        #[cfg(test)]
                        if let Some(person) = TEST_TONE_FORGOT
                            .with(|told| told.borrow_mut().pop_front())
                            .flatten()
                        {
                            tone_forgotten(
                                &mut tone_baselines,
                                Some(&person),
                                trace,
                                monotonic_us(),
                            )?;
                        }
                        // Whose it is: the known person at the lamp, read
                        // from the eyes' small file as a skill's note reads it.
                        tone_person = crate::known_faces::known_from_environment()
                            .map(|name| crate::known_faces::memory_person(&name));
                        #[cfg(test)]
                        if let Some(person) =
                            TEST_TONE_PERSON.with(|person| person.borrow().clone())
                        {
                            tone_person = Some(person);
                        }
                        if let Some(tone) = tone {
                            let before = tone_baselines.clone();
                            let (reading, whose, lines) =
                                tone_baselines.hear(tone_person.as_deref(), &tone);
                            endpoint_record["tone"] = json!({"tone": tone, "reading": reading,
                                "whose": whose, "baseline_lines": lines});
                            let mut held = TonePending {
                                turns: vec![current.owner.turn()],
                                reading,
                                // When the End was handled, not the
                                // capture time the meter's window counts
                                // from: a little later, so this waits at
                                // least as long as a line could still go
                                // on, the safe side (the reviewer-lead's P3
                                // on #2670).
                                ended_us: monotonic_us(),
                                words: None,
                                worded: false,
                                before: Some(before),
                            };
                            if let Some(from) = gone_on_from {
                                endpoint_record["tone"]["went_on_from"] = json!(from.turns[0]);
                                held.turns.splice(0..0, from.turns);
                                held.words = from.words;
                            }
                            // This line's words, if they came before its End.
                            if let Some((turn, words)) = tone_held.later.take()
                                && turn == current.owner.turn()
                            {
                                held.heard(&words, true);
                            }
                            tone_held.reading = Some(held);
                        }
                    }
                    // The asker's level, for what is said while its answer
                    // is awaited: its own blocks, away from the lamp's
                    // sound. A sound let through has none.
                    if let Some(rule) = asker_first.as_mut().filter(|_| !as_sound)
                        && let Some(level) = rule.asked(current.owner.turn(), input_end_us)
                    {
                        endpoint_record["asker_level"] = json!(level);
                    }
                    record(trace, endpoint_record)?;
                    if let Some((turn, mut sent)) = confirmed {
                        // The words held for its answer are the lamp's now.
                        for (text, finished) in sent.take_words() {
                            if let Some(bus) = bus_out.as_mut() {
                                bus.said(turn, &text, finished, monotonic_us());
                            }
                            if let Some(body) = body_link.as_mut()
                                && let Some(gesture) = gesture_cue.heard(turn, &text, finished)
                            {
                                body.gesture(gesture);
                                // The face's accent with the move (#940).
                                if let Some(choreography) = choreography.as_mut() {
                                    choreography.set_accent(
                                        crate::ring_face::accent_code(gesture),
                                        monotonic_us(),
                                    );
                                }
                                record(
                                    trace,
                                    json!({"kind":"body_gesture_asked","turn":turn,"gesture":gesture,"at_us":monotonic_us()}),
                                )?;
                            }
                        }
                        // An answer that opened with nothing said ends
                        // unheard now, as it would have when it came.
                        if let Some(named) = sent.nothing_said()
                            && reply.as_ref().is_some_and(|current| {
                                current.owner.turn() == turn && current.playback.is_none()
                            })
                        {
                            if named {
                                record(
                                    trace,
                                    json!({"kind":"function_named","turn":turn,"at_us":monotonic_us()}),
                                )?;
                            }
                            let heard_on = reply.as_ref().map(|current| current.cloud.session);
                            cancel_reply(owner, reply, trace, "nothing_said")?;
                            drop_unsaid_look(
                                &mut look_wanted,
                                look_held,
                                turn,
                                "its answer said nothing",
                                trace,
                            )?;
                            if let Some(score) = second_look.finished(turn, true, monotonic_us()) {
                                if let Some(session) = heard_on {
                                    second_look.heard_on(session);
                                }
                                record(
                                    trace,
                                    json!({"kind":"second_look_due","turn":turn,"at_us":monotonic_us(),"name_score":score,"after":"nothing_said"}),
                                )?;
                            }
                            // As at the other `nothing_said` cut (#1417). A
                            // written "Sorry?" never comes here today: only a
                            // spoken turn's End is sent early and its words
                            // held for this confirmation, and the lamp's own
                            // turn has no End of the person's to send early.
                            if unclear_hum.finished(turn, true, monotonic_us()) {
                                record(
                                    trace,
                                    json!({"kind":"unclear_hum_owed","turn":turn,"at_us":monotonic_us(),"after":"nothing_said"}),
                                )?;
                            }
                            speaker_pending = None;
                            provider_queue.clear();
                            snapshot = owner.snapshot(now())?;
                            rig.publish(snapshot)?;
                            last_publish = monotonic_us();
                        }
                    }
                }
            }
        }
        if retention_expired(last_capture, frames_from_us, monotonic_us()) {
            return Err(io::Error::other("microphone retention heartbeat expired").into());
        }
        let monitor = rig.endpoint.as_ref().and_then(|e| e.monitor.as_ref());
        if let Some(event) = endpoint_watch.observe(&detector, monitor) {
            record(trace, event)?;
        }
        if clock.saturating_sub(last_publish) >= 20_000 {
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = clock;
        }
        provider_control_empty = service_provider_control(
            &mut rig.provider.channels.control,
            &mut cloud,
            &mut provider_control_budget,
            trace,
        )?;
        apply_submission_gate(
            &cloud,
            reply,
            &mut provider_queue,
            &mut admission,
            &mut submission_draining,
            trace,
        )?;
        if let Some(reminders) = reminders.as_mut()
            && reply.as_ref().is_some_and(|current| {
                current.reminder.is_some() && !reminders.active_valid(current.owner, monotonic_us())
            })
        {
            cancel_reply(owner, reply, trace, "reminder_lease_expired")?;
            provider_queue.clear();
            speaker_pending = None;
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
        }
        if provider_control_empty {
            second::flush(
                rig,
                &mut cloud,
                &mut output_flow,
                reply,
                &mut provider_queue,
                trace,
            )?;
        }
        outage.note_delivery(reply);
        select_failure_notice_in(notices, reply, &speaker_pending, &mut outage, trace)?;
        if let Some((request, why)) = reply.as_ref().and_then(|current| {
            current
                .requested_audio_invalidity(rig.provider.boot, snapshot, monotonic_us())
                .map(|why| (current.owner.turn(), why))
        }) {
            eprintln!("lamp-voice: request {request}: requested audio revoked: {why}");
            cancel_reply(owner, reply, trace, "requested_audio_revoked")?;
            provider_queue.clear();
            speaker_pending = None;
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
        }
        if let Some(current) = reply.as_mut()
            && current.prepare_requested_audio(&speaker_pending, monotonic_us())
        {
            record(
                trace,
                json!({"kind":"requested_audio_prepared","turn":current.owner.turn(),
                "confirmation_retired_at_us":current.media.confirmation_retired_at_us,
                "at_us":monotonic_us(),"boundary":"immutable samples ready after actual confirmation retirement"}),
            )?;
        }
        let prior_speaker_sequence = speaker_sequence;
        settle_duplex_yield_before_dispatch(&playback_mask, reply, monotonic_us(), trace)?;
        // While the asker rule waits on a line and the answer has not begun,
        // the answer's first block waits for the line's judgement, no later
        // than its last chance (about 0.26 s after it began): the line is
        // judged in the clear. Over the reply's first 0.2 to 0.45 s the
        // evidence cannot tell a person from the lamp, and the asker's own
        // follow-up after a soft "um" was let go on its soft start alone
        // (the reviewer on #1197).
        let hold_for_line = asker_waited
            .filter(|(.., last_chance_at_us)| monotonic_us() < *last_chance_at_us + BLOCK_US)
            .filter(|_| {
                reply.as_ref().is_some_and(|current| {
                    current.playback.is_none() && current.notice.origin() == SpeechOrigin::Provider
                })
            });
        if let Some((id, turn, ..)) = hold_for_line
            && asker_held_for != Some(id)
            && reply
                .as_ref()
                .is_some_and(|current| current.cloud.started && current.has_speaker_chunk())
        {
            asker_held_for = Some(id);
            record(
                trace,
                json!({"kind":"asker_first_answer_held","turn":turn,"candidate":id,"at_us":monotonic_us(),
                    "boundary":"the answer's first block waits for the line the asker rule waits on, until its last chance"}),
            )?;
        }
        if reply
            .as_ref()
            .and_then(|current| current.say.as_ref())
            .is_some_and(|ticket| !ticket.authority_valid(monotonic_us()))
        {
            cancel_reply(owner, reply, trace, "unplug_authority_lost")?;
            provider_queue.clear();
            speaker_pending = None;
            snapshot = owner.snapshot(now())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
        }
        let notice_attempt = if hold_for_line.is_some() {
            None
        } else {
            dispatch_speaker_chunk(
                owner,
                reply,
                &mut rig.speaker.channels.data,
                &mut speaker_pending,
                &mut speaker_sequence,
                &mut snapshot,
                &mut playback_mask,
            )?
        };
        if let Some(news) = playback_mask
            .listen_gaps()
            .and_then(|gaps| gaps.take_news())
        {
            record(trace, news)?;
        }
        if let Some((notice_owner, evidence)) = notice_attempt {
            // Named for what plays: a failure notice or the head tap's "hm?".
            let origin = reply
                .as_ref()
                .map_or(SpeechOrigin::FailureNotice, |current| {
                    current.notice.origin()
                });
            record(
                trace,
                json!({"kind":format!("{}_attempted",origin.trace_name()),"at_us":monotonic_us(),
                "owner":notice_owner,"turn":notice_owner.turn(),"notice":evidence,
                "boundary":"reserved before speaker transport; not audible delivery"}),
            )?;
        }
        if speaker_sequence != prior_speaker_sequence {
            let pending = speaker_pending
                .as_ref()
                .ok_or_else(|| io::Error::other("dispatched speech has no pending receipt"))?;
            trace
                .receipts
                .dispatched(pending.permit.owner(), pending.sequence)?;
        }
        let available = playback_mask.available(monotonic_us());
        if policy_available != Some(available) {
            record(
                trace,
                json!({"kind":"turn_taking_availability","at_us":monotonic_us(),"policy":rig.turn_taking,"prospective_input_available":available,"hard_capture_authority_unchanged":true}),
            )?;
            policy_available = Some(available);
        }
        output_flow.replenish(
            reply.as_ref().map_or(0, |current| current.pcm.len()),
            &mut rig.provider.channels.control,
        )?;
        if let Some(current) = reply.as_mut()
            && current.settled(&speaker_pending)
        {
            let turn = current.owner.turn();
            if current.had_audio {
                lamp_words_ended_us = Some(monotonic_us());
            }
            owner.complete_turn(now(), current.owner)?;
            current.complete_requested_audio(monotonic_us());
            trace.receipts.completed(current.owner)?;
            snapshot = owner.snapshot(now())?;
            admission.owner_completed(current.owner, snapshot, monotonic_us())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
            current.complete_reminder(&speaker_pending, snapshot);
            current.answer_say(&speaker_pending, snapshot);
            sleep_owed.turn_ended(current, &speaker_pending, snapshot);
            if onboarding_invitation_turn == Some((current.owner.boot(), turn)) {
                if let Some(walk) = onboarding.as_mut() {
                    let delivery = current.invitation_delivery(
                        &speaker_pending,
                        snapshot,
                        onboarding_invitation_audio,
                    );
                    walk.invitation_finished(current.owner.boot(), turn, delivery, monotonic_us());
                }
                onboarding_invitation_turn = None;
            }
            let through = current.news_played_through(&speaker_pending, snapshot);
            if !through.is_empty()
                && let Some(news) = task_news.as_ref()
            {
                news.played_through(through);
            }
            let heard = current.heard_news(&speaker_pending, snapshot);
            if !heard.is_empty()
                && let Some(news) = task_news.as_ref()
            {
                news.heard(turn, heard);
            }
            record(
                trace,
                json!({"kind":"turn_finished","turn":turn,"generation":current.owner.generation(),"at_us":monotonic_us(),"outcome":current.delivery_outcome(),"playback_gaps":current.playback_gaps,
                    "provider_session":current.cloud.session,"provider_outcome":current.cloud.outcome,"failure_notice":current.notice.evidence(),
                "start_gate":current.start_trace(),"start_deficit_us":current.need.deficit_us(),
                "start_horizon_needed_us":current.need.horizon_us(),
                "start_need_scope":"all retained reply arrivals including later generations; excludes local handoff",
                "start_need_censored":!matches!(current.cloud.outcome,Some(RequestOutcome::Complete))}),
            )?;
            finish_duplex_yield(trace, monotonic_us(), "reply_completed", None, None)?;
            // Said to the lamp by name, and nothing came of it: the model
            // is told what the lamp's own ears heard, once.
            let said_nothing = current.delivery_outcome() == "no_audio_answer"
                && matches!(current.cloud.outcome, Some(RequestOutcome::Complete));
            // The lamp's own "Sorry?" with nothing said leaves the person in
            // silence (#1417, lamp-4ace 17 Oct 16:26): its "hm?" is owed.
            if unclear_hum.finished(
                turn,
                current.delivery_outcome() == "no_audio_answer",
                monotonic_us(),
            ) {
                record(
                    trace,
                    json!({"kind":"unclear_hum_owed","turn":turn,"at_us":monotonic_us(),
                        "provider_outcome":current.cloud.outcome}),
                )?;
            }
            let sound_finished = current.notice.origin().unshown();
            let played_whole = matches!(current.cloud.outcome, Some(RequestOutcome::Complete))
                && current.delivery_outcome() == "audio_written_unscored"
                && !sound_finished;
            holding_answer.settled(turn, played_whole, monotonic_us());
            if let Some((kind, why)) = look_note.settled(turn, played_whole, monotonic_us()) {
                record(
                    trace,
                    json!({"kind":kind,"turn":turn,"why":why,"at_us":monotonic_us()}),
                )?;
            }
            if !sound_finished
                && let Some(score) = second_look.finished(turn, said_nothing, monotonic_us())
            {
                second_look.heard_on(current.cloud.session);
                record(
                    trace,
                    json!({"kind":"second_look_due","turn":turn,"at_us":monotonic_us(),"name_score":score}),
                )?;
            }
            *reply = None;
        }
        // Terminal priority status can overtake valid PCM/outcomes. Only the
        // ordered Ended closes that stream; a submitted Start may still receive
        // its local NotDelivered afterwards. Keep serving capture and speaker
        // receipts until the owned input/output obligations actually settle.
        if lifetime.continuous()
            && cloud.output_ended()
            && reply.is_none()
            && provider_queue.is_empty()
            && !submission_draining
            && speaker_pending.is_none()
            && trace.receipts.is_settled()
        {
            record(
                trace,
                json!({"kind":"service_provider_ended","at_us":monotonic_us(),
                    "ordered_output_ended":true,"retained_work_settled":true,
                    "automatic_retry":false}),
            )?;
            flush_cues(trace, cues);
            if let Some(fault) = cloud.refused_for_now() {
                // Said before the run leaves, whenever it last reported.
                if let Some(bus) = bus_out.as_mut() {
                    bus.report_now(
                        cloud.available(),
                        cloud.refusal(),
                        monotonic_us(),
                        trace.mic_at_start,
                    );
                }
                return Err(ProviderRefusesForNow(fault).into());
            }
            return Err(io::Error::other(
                "terminal provider output settled; service stopped without retry",
            )
            .into());
        }
        // A short sound of the lamp's own is no turn for the head, the room
        // or the voice's state (`speaker_tone`); what the ring shows of it
        // is `tell_ring_of_turn`'s.
        let sound_now = reply
            .as_ref()
            .is_some_and(|current| current.notice.origin().unshown());
        // A lamp that has stopped speaking for someone looks at them: the
        // listening face and pose at once, whatever becomes of the yield.
        let attending = playback_mask.yielded(clock);
        // The lamp's own voice while it speaks, the person's otherwise.
        let voice_level = match reply.as_ref() {
            Some(current) if current.notice.origin() == SpeechOrigin::RequestedAudio => 0,
            Some(current) if current.playback.is_some() && !attending => current.voice_level,
            _ => heard_level,
        };
        if let Some(choreography) = choreography.as_mut() {
            choreography.set_level(voice_level);
            choreography.set_attending(attending);
            tell_ring_of_turn(
                choreography,
                reply.as_ref().map(|current| current.notice.origin()),
            );
            choreography.set_offline(offline || service_down || model_refusing.refusing());
        }
        if let Some(current) = reply
            .as_ref()
            .filter(|current| current.playback.is_some() && !sound_now)
        {
            let turn = current.owner.turn();
            if asked_in_busy_room == Some(turn) && busy_answer_since.map(|(of, _)| of) != Some(turn)
            {
                busy_answer_since = Some((turn, monotonic_us()));
                record(
                    trace,
                    json!({"kind":"answer_kept","turn":turn,"at_us":monotonic_us(),"for_us":KEEP_ANSWER_US,
                        "boundary":"asked in a busy room: a voice over the start of this answer does not stop it"}),
                )?;
            }
            last_spoke_us = Some(monotonic_us());
            spoke_in_turn = Some(turn);
        }
        // A turn that ended by itself with the lamp saying nothing was the
        // room's talk, and tells the floor how loud the room is. One the
        // lamp answered does not, nor one replaced by more talk: that may
        // be a person pausing in what they are saying to the lamp.
        let turn_now = reply
            .as_ref()
            .filter(|_| !sound_now)
            .map(|current| current.owner.turn());
        if floor_turn != turn_now {
            if let (Some(ended), Some(floor)) = (floor_turn, room_floor.as_mut())
                && spoke_in_turn != Some(ended)
                && turn_now.is_none()
            {
                for level in turn_blocks.drain(..) {
                    floor.not_for_the_lamp(level);
                }
            }
            if turn_now.is_none() {
                turn_blocks.clear();
            }
            floor_turn = turn_now;
        }
        // Whether the room was busy as the line under way began, taken before
        // its own audio counts as an answer.
        if let Some(current) = reply.as_ref()
            && own_line_busy.map(|(turn, _)| turn) != Some(current.owner.turn())
        {
            #[cfg(not(test))]
            let busy = room.busy_lately(clock);
            #[cfg(test)]
            let busy = room.busy_lately(clock) || TEST_ROOM_BUSY.with(std::cell::Cell::get);
            own_line_busy = Some((current.owner.turn(), busy));
        }
        // For the room, a turn is answered when the lamp was heard in it:
        // an answer that arrived and was never played is not one.
        room.observe(
            reply.as_ref().filter(|_| !sound_now).map(|current| {
                let turn = current.owner.turn();
                (turn, spoke_in_turn == Some(turn))
            }),
            clock,
        );
        if let Some(bus) = bus_out.as_mut() {
            tell_voice_of_turn(
                bus,
                snapshot,
                reply.as_ref().map(|current| current.notice.origin()),
                clock,
            );
            bus.set_could_not_start(controls.as_ref().map_or(0, HandControls::could_not_start));
            bus.set_ring_off(rig.ring.off_for_good());
            bus.set_chat_key_refused(key_refusals.said(clock));
            bus.set_model_refusing(model_refusing.health(clock));
            bus.report(
                cloud.available(),
                cloud.refusal(),
                clock,
                trace.mic_at_start,
            );
        }
        if let Some(mic) = mic_out.as_mut() {
            mic.tick(clock);
        }
        let taken = playback_mask.take_taken();
        if let Some(out) = expression_out.as_mut() {
            // The first sound of an answer that has come and is not yet
            // playing: when its start will open, and the speaker's queue
            // after that.
            let first_sound_us = reply
                .as_ref()
                .filter(|current| current.playback.is_none() && current.local_input_ended)
                .and_then(|current| {
                    current
                        .start
                        .opens_by(clock, current.pcm.len(), current.generation_done)
                })
                .map(|opens| opens.saturating_add(out.ahead_us()));
            out.tick(crate::expression_out::Seen {
                now_us: clock,
                turn: reply
                    .as_ref()
                    .filter(|_| !sound_now)
                    .map(|current| current.owner.turn()),
                phase: crate::ring_wire::RingPhase::from_snapshot(snapshot).filter(|_| !sound_now),
                accepting_input: snapshot.listening_ready(now()),
                yielding: attending,
                person_speaking: clock.saturating_sub(last_speech_us)
                    < crate::expression_out::SPEECH_LASTS_US,
                first_sound_us,
            });
            for piece in taken.into_iter().filter(|_| !sound_now) {
                out.played(
                    piece.turn,
                    piece.frames,
                    piece.queued_frames,
                    piece.queue_observed_at_us,
                    piece.level,
                    clock,
                );
            }
            // What is held before the speaker plays next: said ahead, so
            // that what the lamp shows can move with it.
            if let Some(current) = reply.as_ref().filter(|current| {
                current.playback.is_some() && !attending && !sound_now && out.foretell_due(clock)
            }) {
                let ahead: Vec<i16> = current
                    .pcm
                    .iter()
                    .take(crate::expression_out::foretell_samples())
                    .copied()
                    .collect();
                out.foretell(current.owner.turn(), &ahead, clock);
            }
        }
        if let Some(body) = body_link.as_mut() {
            use crate::ring_wire::RingPhase;
            let phase = RingPhase::from_snapshot(snapshot).filter(|_| !sound_now);
            // The answer has arrived and is about to be spoken: the head
            // comes back to the person before the first word, not during it.
            let about_to_speak = reply
                .as_ref()
                .is_some_and(|current| current.had_audio && current.local_input_ended);
            attention.observe(
                reply
                    .as_ref()
                    .filter(|_| !sound_now)
                    .map(|current| (current.owner.turn(), current.had_audio)),
                clock,
            );
            body.tell(
                match phase {
                    // "Where you sit": between turns the lamp looks at the
                    // person, its gaze turning the head to them, before it
                    // asks to be moved (#1722 row 10, rule 8).
                    None if onboarding.as_ref().is_some_and(|walk| walk.looking()) => {
                        Some(RingPhase::Listening)
                    }
                    Some(RingPhase::Speaking) if attending => Some(RingPhase::Listening),
                    Some(RingPhase::Waiting) if about_to_speak => Some(RingPhase::Speaking),
                    // A sound sent on for the model to judge is not something
                    // to think about: the head stays as it was while the
                    // sound was heard, and an answer brings it back.
                    Some(RingPhase::Waiting)
                        if sound_turn.is_some()
                            && sound_turn == reply.as_ref().map(|current| current.owner.turn()) =>
                    {
                        None
                    }
                    // The talk around it has not been for it: the head
                    // stays where it is until the lamp has something to say.
                    Some(RingPhase::Listening | RingPhase::Waiting) if attention.settled() => None,
                    phase => phase,
                },
                voice_level,
                clock,
            );
        }
        // A ridden gap tells the eyes no as it begins, and no yes follows
        // until it is over (#2602, the first builder's P1 on #2623).
        if let Some(eyes) = eyes_link.as_mut() {
            eyes.permit(
                snapshot.microphone_permission() == Permission::Allowed && !rig.capture_held,
                clock,
            );
        }
        // A yield's words are never heard without the microphone (#828).
        if snapshot.microphone_permission() != Permission::Allowed
            && let Some(words) = stop_words.as_mut()
        {
            words.close();
        }
        // Nor a waited sound's (#1123).
        if snapshot.microphone_permission() != Permission::Allowed
            && let Some(words) = wait_words.as_mut()
        {
            words.close();
        }
        // The one thing the lamp says unasked. Only from rest: nothing
        // being heard, nothing owed, a provider ready to take the turn
        // (each consumer checks that itself). The quiet they wait out is
        // talk's: the lamp speaking, the person's words heard
        // (`words_heard`), or any voice the detector calls speech
        // (`last_speech_us`), words or none: a line in a language the route
        // takes no words from is still someone talking. The rest of a
        // request, its end's silence and the wait for an answer that never
        // comes aloud, does not hold it: on lamp-52e6 on 21 October a fan's
        // sound opened 11 wordless requests in 2 minutes, and the 3 s the
        // onboarding walk waits for never came.
        if reply
            .as_ref()
            .is_some_and(|current| current.playback.is_some())
        {
            last_turn_us = clock;
        }
        last_turn_us = last_turn_us.max(last_speech_us);
        // The look the lamp said it would take, as soon as it has finished
        // saying so. Someone speaking first takes the turn and the look is
        // dropped; so is one that could not begin within fifteen seconds of being asked for.
        if let Some((turn, mut asked_us)) = look_wanted {
            // A tool's look waits for its turn's first audio or its end; the
            // still's wait counts from the eyes' ask.
            let by_tool = look_held
                .filter(|(of, ..)| *of == turn)
                .map(|(_, session, by_second)| (session, by_second));
            let asked_again = asking.wanted().is_some_and(|(of, _)| of == turn);
            // A tool's look goes only to the session that made the call, on
            // the route that made it: one renewed meanwhile never saw it,
            // nor the other route, whose sessions are numbered apart (a
            // Gemini call whose turn the second route took, or the second
            // route's once Gemini is back). While a route answers, `cloud`
            // is its link. Worked out before the ask, so none is made.
            let gone = by_tool.is_some_and(|(session, by_second)| {
                by_second
                    != rig
                        .second
                        .as_ref()
                        .is_some_and(second::Route::second_active)
                    || unasked_session(&cloud, paused).is_some_and(|ready| ready != session)
            });
            if (by_tool.is_none() || tool_look_due(reply.as_ref(), turn))
                && !asked_again
                && !gone
                && ask_for_still(&mut look, eyes_link.as_mut(), turn, clock, trace)?
                && by_tool.is_some()
            {
                asked_us = clock;
                look_wanted = Some((turn, asked_us));
            }
            let waited = Duration::from_micros(clock.saturating_sub(asked_us));
            let dropped = |why: &str, trace: &mut SessionState| {
                record(
                    trace,
                    json!({"kind":"look_dropped","turn":turn,"by":if by_tool.is_some() {"tool"} else {"words"},"why":why,"at_us":clock}),
                )
            };
            match reply.as_ref().map(|current| current.owner.turn()) {
                _ if gone => {
                    look_wanted = None;
                    // The tool's look is over: the second route's own "Let
                    // me take a look." on this turn asks for its own.
                    look_held = None;
                    dropped("the session that made the call has gone", trace)?;
                }
                Some(current) if current == turn => {}
                Some(_) => {
                    look_wanted = None;
                    dropped("another turn began first", trace)?;
                }
                None if clock.saturating_sub(asked_us)
                    > crate::sight::LOOK_BEGINS_WITHIN.as_micros() as u64 =>
                {
                    look_wanted = None;
                    dropped("it could not begin in time", trace)?;
                }
                // An unclear turn is asked again first: "Sorry?" goes, not
                // a still for an answer nobody heard.
                None if asked_again => {
                    look_wanted = None;
                    dropped("its turn is asked again", trace)?;
                }
                None => {
                    if look.due(waited)
                        && provider_queue.is_empty()
                        && !admission.owns_stream()
                        && !submission_draining
                        && snapshot.listening_ready(now())
                        && let Some(provider_session) = unasked_session(&cloud, paused)
                    {
                        look_wanted = None;
                        let (note, with_picture) = look.turn();
                        // A search's walk names the place it looked, with
                        // the skill's note for the picture; with no
                        // picture, the eye-shut note alone.
                        let note = match look_note.walk_line(turn) {
                            Some(line) if with_picture => format!("{note}{line}"),
                            _ => note.to_owned(),
                        };
                        if look.full() && !with_picture {
                            eprintln!(
                                "lamp-voice: a full look went without its still: {}",
                                look.missing(std::time::SystemTime::now())
                            );
                        }
                        if look.full() {
                            record(
                                trace,
                                json!({"kind":"look_full_still","turn":turn,"at_us":clock,
                                    "after_asked_ms":waited.as_millis() as u64,
                                    "still":with_picture}),
                            )?;
                        }
                        snapshot = commit_written_turn(
                            owner,
                            reply,
                            &mut provider_queue,
                            &note,
                            with_picture,
                            "lamp_looks",
                            provider_session,
                            rig.playout_start_now(),
                            trace,
                            |state| rig.publish(state),
                        )?;
                        last_publish = monotonic_us();
                        // A search's look's turn goes on with it: the
                        // skill's next look may be taken there.
                        if by_tool.is_some()
                            && let Some(current) = reply.as_ref()
                            && look_note.looked(turn, current.owner.turn())
                        {
                            look_note::mark_search(&mut provider_queue, current.owner.turn());
                        }
                    }
                }
            }
        }
        // Asked again, as soon as the unclear turn is over. Someone speaking
        // first takes the turn and it is dropped; so is one not begun within
        // five seconds: a "Sorry?" later than that is about nothing.
        if let Some(wanted) = asking.wanted() {
            match ask_again_now(
                wanted,
                reply.as_ref().map(|current| current.owner.turn()),
                clock,
            ) {
                AskAgain::Wait
                    if reply.as_ref().is_some_and(|current| {
                        current.owner.turn() == wanted.0 && current.local_input_ended
                    }) =>
                {
                    // The service goes on generating into a turn whose sound
                    // is dropped (lamp-4ace, 08:00: provider_idle false for
                    // 10 s): once the person has finished, the turn is cut
                    // there and then, as a stop is, and the asking follows.
                    cancel_reply(owner, reply, trace, "unclear")?;
                    // Its unsent input goes with it, as an answer with
                    // nothing said does: an End still queued after a full
                    // socket would otherwise reach the flush with no owner.
                    speaker_pending = None;
                    provider_queue.clear();
                    snapshot = owner.snapshot(now())?;
                    rig.publish(snapshot)?;
                    last_publish = monotonic_us();
                }
                AskAgain::Wait => {
                    // Its answer dropped, the unclear turn is still current a
                    // second on while the person goes on: what keeps it,
                    // said once (#926, lamp-4ace).
                    if !asking.open_said
                        && clock.saturating_sub(wanted.1) > 1_000_000
                        && let Some(current) = reply.as_ref()
                    {
                        asking.open_said = true;
                        record(
                            trace,
                            json!({"kind":"ask_again_turn_open","turn":wanted.0,"at_us":clock,
                                "provider_idle":current.provider_idle,"provider_outcome":current.cloud.outcome,
                                "local_input_ended":current.local_input_ended,"queued_frames":current.pcm.len(),
                                "speaker_pending":speaker_pending.is_some(),"playing":current.playback.is_some(),
                                "notice_settled":current.notice.settled(),
                                "admission_owns_stream":admission.owns_stream()}),
                        )?;
                    }
                }
                AskAgain::Drop => {
                    let why = match reply.as_ref().map(|current| current.owner.turn()) {
                        Some(current) if current == wanted.0 => {
                            "the unclear turn stayed open too long"
                        }
                        Some(_) => "another turn began",
                        None => "not begun within 5 s",
                    };
                    record(
                        trace,
                        json!({"kind":"ask_again_dropped","turn":wanted.0,"at_us":clock,
                            "current_turn":reply.as_ref().map(|current| current.owner.turn()),
                            "why":why}),
                    )?;
                    // A misheard cut-in whose turn someone took first: no
                    // "hm?" is owed, and the journal says so (the reviewer
                    // on #1659).
                    if asking.hum {
                        eprintln!(
                            "lamp-voice: request {}: a misheard cut-in: no \"hm?\" ({why})",
                            wanted.0
                        );
                    }
                    asking.done();
                }
                // A misheard cut-in, with `LAMP_MISHEARD_HUM`: no model turn,
                // whose answer would go back to the story still in its
                // context, but the lamp's "hm?" once it is quiet (#828).
                AskAgain::Due if asking.hum => {
                    asking.done();
                    unclear_hum.owe_misheard(wanted.0, wanted.1, clock);
                    record(
                        trace,
                        json!({"kind":"unclear_hum_owed","turn":wanted.0,"at_us":clock,"after":"misheard_cut_in"}),
                    )?;
                }
                AskAgain::Due => {
                    // What holds a due asking back, said once each time it
                    // changes (#926: on lamp-4ace one was never asked).
                    let held = [
                        (!provider_queue.is_empty(), "provider_queue"),
                        (admission.owns_stream(), "admission_owns_stream"),
                        (submission_draining, "submission_draining"),
                        (!snapshot.listening_ready(now()), "not_listening_ready"),
                        (
                            unasked_session(&cloud, paused).is_none(),
                            "no_ready_session_or_paused",
                        ),
                    ]
                    .into_iter()
                    .filter(|(holds, _)| *holds)
                    .map(|(_, what)| what)
                    .collect::<Vec<_>>();
                    if !held.is_empty() && asking.held.as_ref() != Some(&held) {
                        record(
                            trace,
                            json!({"kind":"ask_again_waiting","turn":wanted.0,"at_us":clock,"held_by":held}),
                        )?;
                        asking.held = Some(held);
                    }
                    if provider_queue.is_empty()
                        && !admission.owns_stream()
                        && !submission_draining
                        && snapshot.listening_ready(now())
                        && let Some(provider_session) = unasked_session(&cloud, paused)
                    {
                        asking.done();
                        snapshot = commit_written_turn(
                            owner,
                            reply,
                            &mut provider_queue,
                            crate::speak_up::ASK_AGAIN_NOTE,
                            false,
                            "asked_again",
                            provider_session,
                            rig.playout_start_now(),
                            trace,
                            |state| rig.publish(state),
                        )?;
                        last_publish = monotonic_us();
                        if unclear_hum_on && let Some(current) = reply.as_ref() {
                            unclear_hum.asked(current.owner.turn());
                        }
                    }
                }
            }
        }
        // A stop word while the reply sounds: the next block begins a
        // yield, which the duplex rules then commit or resume (#828).
        if let Some(stops) = stop_in.as_mut()
            && let Some((began, score)) = stops.newest_scored(clock)
        {
            let sounding = reply_sounding(&playback_mask, reply, clock);
            let yields = sounding && playback_mask.force_yield();
            record(
                trace,
                json!({"kind":"stop_heard","at_us":clock,"began_us":began,
                    "score":score,"sounding":sounding,"yields":yields}),
            )?;
            stop_began_us = stop_began_us.max(Some(began));
        }
        if let Some(names) = name_in.as_mut()
            && let Some((began, score)) = names.newest_scored(clock)
        {
            second_look.name_heard(began, score);
            name_began_us = name_began_us.max(Some(began));
        }
        if let (Some(rule), Some(names)) = (asker_first.as_mut(), asker_named.as_mut())
            && let Some((began, _)) = names.newest_scored(clock)
        {
            rule.named(began);
        }
        if let Some(rising) = rising_in.as_mut()
            && let Some(began) = hear_the_call(
                rising,
                clock,
                &mut detector,
                reply.as_ref().map(|current| current.owner.turn()),
                trace,
            )?
        {
            name_began_us = name_began_us.max(Some(began));
        }
        if fixture_due_us.is_some() {
            let at_rest = reply.is_none()
                && provider_queue.is_empty()
                && !admission.owns_stream()
                && !submission_draining
                && snapshot.listening_ready(now());
            if let Some(started) = start_timed_fixture(
                &mut fixture_due_us,
                clock,
                at_rest.then(|| unasked_session(&cloud, paused)).flatten(),
                owner,
                reply,
                &mut provider_queue,
                rig.playout_start_now(),
                trace,
                |state| rig.publish(state),
            )? {
                snapshot = started;
                last_publish = monotonic_us();
            }
        }
        // A holding phrase is not the promised answer. Ask once on the
        // same connection, after playback and one second of fresh quiet.
        // The same ownership, capture and speaker gates as a reminder
        // apply; a new turn or a lost session drops the opportunity.
        let holding_now_us = monotonic_us();
        if let Some(started) = holding::start(
            &mut holding_answer,
            holding::Gates {
                at_us: holding_now_us,
                cycle_started_us: reminder_cycle_started_us,
                last_speech_us,
                last_capture_us: last_capture,
                session: unasked_session(&cloud, paused),
                worker: rig.provider.boot,
                snapshot: &snapshot,
                admission: &admission,
                playback: &playback_mask,
                speaker_pending: speaker_pending.is_some(),
                submission_draining,
            },
            owner,
            reply,
            &mut provider_queue,
            rig.playout_start_now(),
            trace,
            |state| rig.publish(state),
        )? {
            snapshot = started;
            last_publish = monotonic_us();
        }
        // A skill's search left after a look is noted once to go on, under
        // the same gates; the person speaking or a new turn drops it.
        // After the note, the walk on to the next place, under the same
        // gates: its look is taken as a skill's look.
        let look_note_now_us = monotonic_us();
        match look_note::start(
            &mut look_note,
            holding::Gates {
                at_us: look_note_now_us,
                cycle_started_us: reminder_cycle_started_us,
                last_speech_us,
                last_capture_us: last_capture,
                session: unasked_session(&cloud, paused),
                worker: rig.provider.boot,
                snapshot: &snapshot,
                admission: &admission,
                playback: &playback_mask,
                speaker_pending: speaker_pending.is_some(),
                submission_draining,
            },
            &walk_skill,
            owner,
            reply,
            &mut provider_queue,
            rig.playout_start,
            trace,
            |state| rig.publish(state),
        )? {
            look_note::Started::Nothing => {}
            look_note::Started::Turn(started) => {
                snapshot = started;
                last_publish = monotonic_us();
            }
            // The skill looked toward the next place: its still is taken as
            // for a skill's look, from now (nothing is playing), and goes
            // as the search's next look turn.
            look_note::Started::Look { turn, session } => {
                if look.full() && look_wanted.is_none() {
                    look_wanted = Some((turn, look_note_now_us));
                    let by_second = rig
                        .second
                        .as_ref()
                        .is_some_and(second::Route::second_active);
                    look_held = Some((turn, session, by_second));
                    look_note.asked(turn, false, None);
                    record(
                        trace,
                        json!({"kind":"look_asked","turn":turn,"by":"walk","at_us":look_note_now_us}),
                    )?;
                } else {
                    look_note.clear();
                    record(
                        trace,
                        json!({"kind":"look_walk_dropped","turn":turn,"why":"its look could not be taken","at_us":look_note_now_us}),
                    )?;
                }
            }
        }
        // A second look at something said to the lamp by name that got
        // nothing, as soon as the lamp is at rest. Someone who has spoken
        // since is being listened to, and it was dropped for them.
        if let Some(turn) = second_look.due(clock)
            && reply.is_none()
            && provider_queue.is_empty()
            && !admission.owns_stream()
            && !submission_draining
            && snapshot.listening_ready(now())
            && let Some(provider_session) = unasked_session(&cloud, paused)
        {
            let heard_it = second_look.is_for(provider_session);
            second_look.taken();
            if heard_it {
                eprintln!(
                    "lamp-voice: request {turn} was said to the lamp by name and got nothing: the model is told its name was heard"
                );
                snapshot = commit_written_turn(
                    owner,
                    reply,
                    &mut provider_queue,
                    crate::second_look::NOTE,
                    false,
                    "lamp_looks_again",
                    provider_session,
                    rig.playout_start_now(),
                    trace,
                    |state| rig.publish(state),
                )?;
                last_publish = monotonic_us();
            } else {
                // The note speaks of what was just said, and this session
                // was not there for it.
                eprintln!(
                    "lamp-voice: request {turn} was said to the lamp by name and got nothing, and the session that heard it has gone: nothing is asked"
                );
                record(
                    trace,
                    json!({"kind":"second_look_dropped","turn":turn,"at_us":monotonic_us(),"why":"session_gone"}),
                )?;
            }
        }
        // "I'm back online." (#830): once the link is back after an outage in
        // which the lamp said it could not reach the internet, when nothing
        // holds the lamp and the room is quiet; within a minute, or not at all.
        if let Some(since) = back_owed_since {
            if clock.saturating_sub(since) >= notices::BACK_ONLINE_OWED_US {
                back_owed_since = None;
            } else if reply.is_none()
                && provider_queue.is_empty()
                && speaker_pending.is_none()
                && !paused
                && playback_mask.available(clock)
                && reminder_quiet(
                    clock,
                    reminder_cycle_started_us,
                    last_speech_us,
                    last_capture,
                )
                && owner.snapshot(now())?.microphone_permission() == Permission::Allowed
            {
                snapshot =
                    commit_back_online(owner, reply, rig.playout_start_now(), trace, |state| {
                        rig.publish(state)
                    })?;
                last_publish = monotonic_us();
                back_owed_since = None;
            }
        }
        // Finish the camera/name follow-ups owed above before a reminder can
        // take the next turn. A reminder is not someone speaking first.
        let reminder_now_us = if reminders.is_some() {
            monotonic_us()
        } else {
            clock
        };
        // A reminder due an hour ago or more, or by an unknown amount,
        // waits for someone at the lamp and is never told to an empty room.
        // While it waits it holds no skill's ask (the reviewer on #1103).
        let someone_here = crate::reminders::someone_here(
            presence
                .as_ref()
                .is_some_and(|presence| presence.present(clock)),
            said_at_us,
            clock,
        );
        let overdue_may_begin = reminders
            .as_ref()
            .and_then(|reminders| reminders.offer(reminder_now_us))
            .is_none_or(|offer| overdue_wait.may_begin(&offer, someone_here));
        if !overdue_may_begin {
            reminder_held = None;
        }
        if let Some(reminders) = reminders.as_mut()
            && reply.is_none()
            && provider_queue.is_empty()
            && speaker_pending.is_none()
            && admission.idle_for_reminder()
            && !submission_draining
            && snapshot.listening_ready(MonoTime::from_micros(reminder_now_us))
            && reminder_quiet(
                reminder_now_us,
                reminder_cycle_started_us,
                last_speech_us,
                last_capture,
            )
            && playback_mask.available(reminder_now_us)
            && let Some(provider_session) = unasked_session(&cloud, paused)
            && let Some(offer) = idle_reminder_offer(
                reminders,
                reply,
                look_wanted,
                &mut second_look,
                reminder_now_us,
            )
            && overdue_may_begin
            && let Some(ticket) = for_skill(
                says.as_mut(),
                crate::say::Source::Schedule,
                reminders.by_skill(),
                &reminder_key(&offer),
                None,
                &mut reminder_held,
                reminder_now_us,
            )
        {
            snapshot = commit_reminder_turn(
                owner,
                reply,
                &mut provider_queue,
                offer,
                reminders,
                provider_session,
                rig.playout_start_now(),
                trace,
                |state| rig.publish(state),
            )?;
            last_reminder_turn = reply.as_ref().map(|current| current.owner.turn());
            last_publish = monotonic_us();
            begun_for(ticket, reply);
        }
        // A reminder held for its skill, and still offered, is said before
        // anything else the lamp would say unasked: the say service and the
        // news wait for it (Fourth on #1038).
        let reminder_waiting = reminders.as_ref().is_some_and(|reminders| {
            reminder_waits(&reminder_held, reminders.offer(monotonic_us()).as_ref())
        });
        // What another part asked the voice to say (#939): after a due
        // reminder, before the news, through a reminder's
        // gates and a quiet since the last turn that its priority sets.
        let say_now_us = if says.is_some() {
            monotonic_us()
        } else {
            clock
        };
        if let Some(says) = says.as_mut() {
            says.poll(say_now_us);
        }
        // The shutdown's last words (#3697) wait for none of it: the lamp's
        // own speech is cut for them. A muted lamp stays silent.
        let last_words_now = says
            .as_ref()
            .is_some_and(crate::say::Says::last_words_waiting)
            && snapshot.microphone_permission() == Permission::Allowed;
        if let Some(says) = says.as_mut()
            && (last_words_now
                || (!reminder_waiting
                    && says
                        .quiet_needed_us()
                        .is_some_and(|quiet| clock.saturating_sub(last_turn_us) >= quiet)
                    && reply.is_none()
                    && provider_queue.is_empty()
                    && speaker_pending.is_none()
                    && admission.idle_for_reminder()
                    && !submission_draining
                    && snapshot.listening_ready(MonoTime::from_micros(say_now_us))
                    && reminder_quiet(
                        say_now_us,
                        reminder_cycle_started_us,
                        last_speech_us,
                        last_capture,
                    )
                    && playback_mask.available(say_now_us)))
            && let Some(provider_session) = unasked_session(&cloud, paused)
            && let Some((turn, priority, ticket, quoted)) = says.take(say_now_us)
        {
            if ticket.last_words() {
                submission_draining |= cut_for_last_words(
                    owner,
                    reply,
                    &mut provider_queue,
                    &mut speaker_pending,
                    &mut admission,
                    &mut detector,
                    trace,
                )?;
            }
            // An add-on's quoted line is the second route's alone: there its
            // turn is asked with no functions (#2530 part 2). The Live
            // session's functions are declared once, so it never takes one.
            if let Some(why) = quoted_refused(quoted, &rig.provider.role) {
                eprintln!(
                    "lamp-voice: the add-on {}'s say is refused: {why}",
                    ticket.skill()
                );
                ticket.answer(crate::say::Outcome::Refused(why.into()));
            } else {
                eprintln!(
                    "lamp-voice: saying what the skill {} asked (priority {priority})",
                    ticket.skill()
                );
                // A note whose skill asked (`who`, Welcome back alone) is
                // told who is at the lamp: a known face by name, someone who
                // comes by often one beat warmer (#2825). No other's is.
                let here = presence
                    .as_ref()
                    .filter(|presence| presence.present(clock))
                    .map(|presence| {
                        (
                            crate::known_faces::known_from_environment(),
                            presence.familiar(),
                        )
                    });
                let (told_who, familiar) =
                    crate::person::told_record(ticket.told_who(), here.as_ref());
                record(
                    trace,
                    json!({"kind":"skill_says","at_us":say_now_us,"skill":ticket.skill(),"priority":priority,"provider_session":provider_session,"told_who":told_who,"familiar":familiar}),
                )?;
                let turn = crate::person::told_who(turn, ticket.told_who(), here);
                snapshot = commit_written_turn(
                    owner,
                    reply,
                    &mut provider_queue,
                    &turn,
                    false,
                    "skill_says",
                    provider_session,
                    rig.playout_start_now(),
                    trace,
                    |state| rig.publish(state),
                )?;
                last_publish = monotonic_us();
                // Marked on its turn: the second route asks it with no
                // functions and keeps it out of memory.
                if quoted
                    && let Some(ProviderInput::Written {
                        quoted: written_quoted,
                        ..
                    }) = provider_queue.back_mut()
                {
                    *written_quoted = true;
                }
                // Begun: the asker may move the body and the light with it.
                ticket.begun(monotonic_us());
                if let Some(current) = reply.as_mut() {
                    current.say = Some(ticket);
                }
            }
        }
        // A skill's own sound (#938): played now, as a turn of the lamp's
        // own like the head tap's "hm?", only at rest as a say needs it;
        // otherwise refused now with why. It never waits, so it never comes
        // late. Its file was read on the asker's thread.
        if let Some(says) = says.as_mut() {
            let at_us = monotonic_us();
            let now_is = skill_sound_now(
                SoundGates {
                    at_us,
                    paused,
                    snapshot: &snapshot,
                    reply,
                    provider_queue: &provider_queue,
                    speaker_pending: &speaker_pending,
                    playback_mask: &playback_mask,
                    admission: &admission,
                    submission_draining,
                    heard_quiet: reminder_quiet(
                        at_us,
                        reminder_cycle_started_us,
                        last_speech_us,
                        last_capture,
                    ),
                    reply_owed: look_wanted.is_some(),
                    reminder_waiting,
                    since_turn_us: clock.saturating_sub(last_turn_us),
                },
                says,
            );
            if let Some(now_state) = play_skill_sound(
                says,
                &now_is,
                at_us,
                owner,
                reply,
                &mut provider_queue,
                &mut speaker_pending,
                &mut admission,
                &mut detector,
                &mut submission_draining,
                rig.playout_start_now(),
                trace,
                |state| rig.publish(state),
            )? {
                snapshot = now_state;
                last_publish = monotonic_us();
            }
        }
        // The self-test's speaker check, answered from the last speaking turn.
        if let Some(check) = speaker_echo.as_mut() {
            check.answer_requests(monotonic_us());
        }
        if let Some(walk) = onboarding.as_mut() {
            #[cfg(test)]
            if let Some(polls) = trace.onboarding_test_polls.as_ref() {
                polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(current) = reply.as_ref()
                && onboarding_invitation_turn == Some((current.owner.boot(), current.owner.turn()))
            {
                onboarding_invitation_audio |= current.reached_speaker();
            }
            if let Some((boot, turn)) = walk.invitation_binding()
                && reply
                    .as_ref()
                    .map(|current| (current.owner.boot(), current.owner.turn()))
                    != Some((boot, turn))
            {
                // Gone unsettled (the route failed, the service stopped,
                // a privacy gap): with none of it played, it was never said.
                let delivery = crate::onboarding::Delivery::unsettled(onboarding_invitation_audio);
                walk.invitation_finished(boot, turn, delivery, monotonic_us());
                onboarding_invitation_turn = None;
            }
            let can_speak = reply.is_none()
                && speaker_pending.is_none()
                && provider_queue.is_empty()
                && !admission.owns_stream()
                && admission.idle_for_reminder()
                && playback_mask.available(monotonic_us())
                && !submission_draining
                && snapshot.listening_ready(now())
                && unasked_session(&cloud, paused).is_some();
            let face = walk.face_now(clock);
            let still = walk.still_now(clock);
            let note = walk.tick(
                clock,
                clock.saturating_sub(last_turn_us),
                can_speak,
                paused,
                face,
                still,
            );
            let pending = walk.pending();
            if let Some(bus) = bus_out.as_mut() {
                bus.onboarding_pending(&pending, monotonic_us());
            }
            if can_speak
                && let Some(offer) = pending.invitations.first().filter(|offer| offer.ready)
                && let Some(says) = says.as_mut()
                && let Some(ticket) = says.take_onboarding(offer.invitation, monotonic_us())
            {
                if let (Some(words), Some(provider_session)) = (
                    walk.take_invitation(offer.invitation, monotonic_us()),
                    unasked_session(&cloud, paused),
                ) {
                    snapshot = commit_written_turn(
                        owner,
                        reply,
                        &mut provider_queue,
                        &words,
                        false,
                        match offer.step {
                            lamp_msgs::OnboardingStep::Privacy => "onboarding_privacy_invitation",
                            lamp_msgs::OnboardingStep::HowYouSound => "onboarding_near_call_prompt",
                            lamp_msgs::OnboardingStep::GivingWay => {
                                "onboarding_giving_way_invitation"
                            }
                            // Only read, never offered by this voice.
                            lamp_msgs::OnboardingStep::Unknown => "onboarding_invitation",
                        },
                        provider_session,
                        rig.playout_start,
                        trace,
                        |state| rig.publish(state),
                    )?;
                    if let Some(current) = reply.as_ref() {
                        let bound = (current.owner.boot(), current.owner.turn());
                        if walk.bind_invitation(offer.invitation, bound.0, bound.1) {
                            onboarding_invitation_turn = Some(bound);
                            onboarding_invitation_audio = false;
                        }
                    }
                    begun_for(Some(ticket), reply);
                    last_publish = monotonic_us();
                } else {
                    ticket.answer(crate::say::Outcome::Refused(
                        "onboarding invitation is no longer eligible".into(),
                    ));
                    walk.invitation_refused(offer.invitation, monotonic_us());
                }
            }
            for line in walk.take_records() {
                record(trace, line)?;
            }
            // A step done: one move asked of the body, by the path the
            // answers' nods take. None without a body or while paused.
            if let Some(gesture) = walk.take_gesture()
                && !paused
                && let Some(body) = body_link.as_mut()
            {
                body.gesture(gesture);
                // The face's accent with the move (#940).
                if let Some(choreography) = choreography.as_mut() {
                    choreography.set_accent(crate::ring_face::accent_code(gesture), monotonic_us());
                }
                record(
                    trace,
                    json!({"kind":"body_gesture_asked","why":"onboarding","gesture":gesture,"at_us":clock}),
                )?;
            }
            if let Some(note) = note
                && let Some(provider_session) = unasked_session(&cloud, paused)
            {
                snapshot = commit_written_turn(
                    owner,
                    reply,
                    &mut provider_queue,
                    &note,
                    false,
                    "lamp_leads_setup",
                    provider_session,
                    rig.playout_start_now(),
                    trace,
                    |state| rig.publish(state),
                )?;
                last_publish = monotonic_us();
                lamp_began = reply.as_ref().map(|current| current.owner.turn());
                // "Where you sit"'s intro is reported as an invitation is,
                // so it is kept as said only when it was heard.
                if let Some(current) = reply.as_ref() {
                    let bound = (current.owner.boot(), current.owner.turn());
                    if walk.bind_opener(bound.0, bound.1) {
                        onboarding_invitation_turn = Some(bound);
                        onboarding_invitation_audio = false;
                    }
                }
            }
        }
        // A tap on the head of a quiet lamp (#711): a soft "hm?" of its own,
        // as its own turn, rarely; the tap is a call either way.
        if let (Some(sound), Some((acquired_at_us, began_us))) = (head_cue.as_ref(), head_tap) {
            let at_us = monotonic_us();
            let state = crate::head_cue::Now {
                at_us,
                began_us,
                allowed: snapshot.microphone_permission() == Permission::Allowed,
                quiet: reply.is_none()
                    && provider_queue.is_empty()
                    && speaker_pending.is_none()
                    && !admission.owns_stream()
                    && admission.idle_for_reminder()
                    && !submission_draining
                    && !paused
                    && playback_mask.available(at_us),
                listening_ready: snapshot.listening_ready(MonoTime::from_micros(at_us)),
                reply_owed: look_wanted.is_some(),
                last_said_us: last_spoke_us,
                last_hum_us,
            };
            let decided = crate::head_cue::may_hum(&state);
            record(
                trace,
                json!({"kind":"head_tap_cue","acquired_at_us":acquired_at_us,"began_us":began_us,
                    "at_us":at_us,"plays":decided.is_ok(),"why":decided.err(),
                    "boundary":"Controls' tap acquisition to this decision; the first sample is head_tap_cue_first_write"}),
            )?;
            if decided.is_ok() {
                // The listening face from the pass that admits it: never a
                // frame of thinking first.
                if let Some(choreography) = choreography.as_mut() {
                    choreography.set_unshown(false);
                    choreography.set_listening(true);
                }
                let (now_state, _) = speaker_tone::commit_sound_turn(
                    owner,
                    reply,
                    NoticeDelivery::sound(SpeechOrigin::HeadCue, sound.clone()),
                    "head_tap_cue",
                    "a tap on the head: nothing heard, nothing sent to the provider",
                    0,
                    rig.playout_start_now(),
                    trace,
                    |state| rig.publish(state),
                )?;
                snapshot = now_state;
                last_hum_us = Some(at_us);
                last_publish = monotonic_us();
            }
        }
        // A hand kept on the head into the last second of the three that put
        // the lamp to sleep (#1362): the falling breath, once a hold, as it
        // starts to settle. The sleep is Controls' own, at three seconds:
        // the mute above ends this conversation on the pass that sees it,
        // the breath cut there if it still plays, so the microphones close
        // not one sample later for it (the first builder on #1362, 10:35).
        if let (Some(sound), Some((contact, held_us))) = (sleep_breath.as_ref(), head_held)
            && crate::head_cue::sighs_at(held_us)
            && sighed_for != Some(contact)
        {
            sighed_for = Some(contact);
            let at_us = monotonic_us();
            // No sigh while the walk is under way: the hold puts nothing to
            // sleep then (#3747).
            let decided = if walk_under_way {
                Err("onboarding_under_way")
            } else {
                crate::head_cue::may_sigh(&crate::head_cue::Now {
                    at_us,
                    began_us: at_us.saturating_sub(held_us),
                    allowed: snapshot.microphone_permission() == Permission::Allowed,
                    quiet: reply.is_none()
                        && provider_queue.is_empty()
                        && speaker_pending.is_none()
                        && !admission.owns_stream()
                        && admission.idle_for_reminder()
                        && !submission_draining
                        && !paused
                        && playback_mask.available(at_us),
                    listening_ready: snapshot.listening_ready(MonoTime::from_micros(at_us)),
                    reply_owed: look_wanted.is_some(),
                    last_said_us: last_spoke_us,
                    last_hum_us,
                })
            };
            record(
                trace,
                json!({"kind":"head_sleep_breath","held_us":held_us,"at_us":at_us,
                    "plays":decided.is_ok(),"why":decided.err(),
                    "boundary":"this decision, at Controls' hand time on the head; the first sample is head_sleep_breath_first_write"}),
            )?;
            if decided.is_ok() {
                let (now_state, _) = speaker_tone::commit_sound_turn(
                    owner,
                    reply,
                    NoticeDelivery::sound(SpeechOrigin::SleepBreath, sound.clone()),
                    "head_sleep_breath",
                    "a hand kept on the head: the lamp settles to sleep, nothing heard, nothing sent to the provider",
                    0,
                    rig.playout_start_now(),
                    trace,
                    |state| rig.publish(state),
                )?;
                snapshot = now_state;
                last_publish = monotonic_us();
            }
        }
        // Woken by the person from a hand's sleep (#1362): its rising "hm?",
        // the first thing this conversation says, once it is quiet and
        // listening is ready, within a second and a half of its start or not
        // at all. Not held to the tap's 30 s: the person asked for it.
        if let (Some(sound), Some((woken_us, began_us))) = (head_cue.as_ref(), *waking) {
            let at_us = monotonic_us();
            let decided = crate::head_cue::may_wake_hum(
                &crate::head_cue::Now {
                    at_us,
                    began_us: woken_us,
                    allowed: snapshot.microphone_permission() == Permission::Allowed,
                    quiet: reply.is_none()
                        && provider_queue.is_empty()
                        && speaker_pending.is_none()
                        && !admission.owns_stream()
                        && admission.idle_for_reminder()
                        && !submission_draining
                        && !paused
                        && playback_mask.available(at_us),
                    listening_ready: snapshot.listening_ready(MonoTime::from_micros(at_us)),
                    reply_owed: look_wanted.is_some(),
                    last_said_us: last_spoke_us,
                    last_hum_us: None,
                },
                began_us,
            );
            // Startup privacy stabilization is temporary. Keep this one wake
            // pending without extending its original deadline or playing while
            // the microphones are denied; may_wake_hum checks expiry first
            // and an expired cue is always terminal.
            let waits = matches!(
                decided,
                Err("microphone_not_allowed" | "not_quiet" | "listening_not_ready")
            );
            if !waits {
                *waking = None;
                record(
                    trace,
                    json!({"kind":"head_wake_cue","acquired_at_us":woken_us,"at_us":at_us,
                        "plays":decided.is_ok(),"why":decided.err(),
                        "boundary":"the waking input's acquisition to this decision; the first sample is head_wake_cue_first_write"}),
                )?;
            }
            if decided.is_ok() {
                if let Some(choreography) = choreography.as_mut() {
                    choreography.set_unshown(false);
                    choreography.set_listening(true);
                }
                let (now_state, _) = speaker_tone::commit_sound_turn(
                    owner,
                    reply,
                    NoticeDelivery::sound(SpeechOrigin::WakeHum, sound.clone()),
                    "head_wake_cue",
                    "the person woke the lamp: nothing heard, nothing sent to the provider",
                    0,
                    rig.playout_start_now(),
                    trace,
                    |state| rig.publish(state),
                )?;
                snapshot = now_state;
                last_hum_us = Some(at_us);
                last_publish = monotonic_us();
            }
        }
        // Onboarding persistence runs only after any immediate tap cue has
        // been dispatched and its reply has drained. It never precedes revoke.
        if let Some(walk) = onboarding.as_mut() {
            let ready = !trace.onboarding_privacy_disabled
                && !paused
                && reply.is_none()
                && speaker_pending.is_none()
                && provider_queue.is_empty()
                && !admission.owns_stream()
                && !submission_draining
                && snapshot.listening_ready(now())
                && playback_mask.available(monotonic_us());
            if let Err(error) = walk.maintain_privacy(monotonic_us(), ready) {
                record(
                    trace,
                    json!({"kind":"onboarding_privacy_incomplete",
                    "at_us":monotonic_us(),"reason":error.to_string()}),
                )?;
            }
        }
        // The lamp's own "Sorry?" came back with nothing said (#1417): its
        // "hm?" instead, as a creature that didn't catch you goes "hm?" and
        // waits, once the lamp is quiet; never over someone, never late.
        if let (Some(sound), Some(owed_us)) = (head_cue.as_ref(), unclear_hum.owed()) {
            let at_us = monotonic_us();
            let decided = match unclear_hum.due(reply.as_ref().map(|c| c.owner.turn()), at_us) {
                HumDue::Drop(why) => Err(why),
                HumDue::Now => crate::head_cue::may_hum(&crate::head_cue::Now {
                    at_us,
                    began_us: owed_us,
                    allowed: snapshot.microphone_permission() == Permission::Allowed,
                    quiet: reply.is_none()
                        && provider_queue.is_empty()
                        && speaker_pending.is_none()
                        && !admission.owns_stream()
                        && admission.idle_for_reminder()
                        && !submission_draining
                        && !paused
                        && playback_mask.available(at_us),
                    listening_ready: snapshot.listening_ready(MonoTime::from_micros(at_us)),
                    reply_owed: look_wanted.is_some(),
                    last_said_us: last_spoke_us,
                    // The tap's 30 s is for a sound nobody asked for. This
                    // answers the person's own words each time they aren't
                    // made out, and each needs words of theirs first (the
                    // Seventh on #1421: a second empty "Sorry?" must not go
                    // blank).
                    last_hum_us: None,
                }),
            };
            match decided {
                // Not yet quiet: tried again next pass, within the window,
                // and kept to say why if the window runs out.
                Err(why @ ("not_quiet" | "listening_not_ready")) => unclear_hum.hold(why),
                Err(why) => {
                    record(
                        trace,
                        json!({"kind":"unclear_hum","owed_at_us":owed_us,"at_us":at_us,"plays":false,
                            "why":why,"held_by":unclear_hum.held(),"misheard_turn":unclear_hum.misheard().map(|(turn, _)| turn)}),
                    )?;
                    if let Some((turn, _)) = unclear_hum.misheard() {
                        eprintln!(
                            "lamp-voice: request {turn}: a misheard cut-in: its \"hm?\" was not played ({why})"
                        );
                    }
                    unclear_hum.done();
                }
                Ok(()) => {
                    let misheard = unclear_hum.misheard();
                    record(
                        trace,
                        json!({"kind":"unclear_hum","owed_at_us":owed_us,"at_us":at_us,"plays":true,
                            "misheard_turn":misheard.map(|(turn, _)| turn)}),
                    )?;
                    if let Some((turn, held_us)) = misheard {
                        // The time from the hold to the sound, for rule 7.
                        eprintln!(
                            "lamp-voice: request {turn}: a misheard cut-in: the lamp's \"hm?\" {} ms after it was held, and no model turn",
                            at_us.saturating_sub(held_us) / 1_000
                        );
                    }
                    unclear_hum.done();
                    if let Some(choreography) = choreography.as_mut() {
                        choreography.set_unshown(false);
                        choreography.set_listening(true);
                    }
                    let (now_state, _) = speaker_tone::commit_sound_turn(
                        owner,
                        reply,
                        NoticeDelivery::sound(SpeechOrigin::UnclearHum, sound.clone()),
                        "unclear_hum",
                        if misheard.is_some() {
                            "a cut-in was misheard: the lamp's \"hm?\", nothing sent to the provider"
                        } else {
                            "the lamp's own \"Sorry?\" said nothing: nothing heard, nothing sent to the provider"
                        },
                        0,
                        rig.playout_start_now(),
                        trace,
                        |state| rig.publish(state),
                    )?;
                    snapshot = now_state;
                    last_hum_us = Some(at_us);
                    last_publish = monotonic_us();
                }
            }
        }
        if let Some(presence) = presence.as_mut() {
            presence.watch(clock);
        }
        // Memory has forgotten someone, asked by the app or by voice: their
        // tone baseline goes too (`memory/forgot`).
        if let Some(forgot) = forgot_in.as_mut() {
            for person in forgot.forgotten(clock) {
                tone_forgotten(&mut tone_baselines, person.as_deref(), trace, clock)?;
                // Kept: a line that goes on puts back no baseline from
                // before it.
                if let Some(held) = tone_held.reading.as_mut() {
                    held.before = None;
                }
            }
        }
        // A reading with its words is told once no line can still go on
        // from it ([`TonePending::due`]).
        if tone_held
            .reading
            .as_ref()
            .is_some_and(|held| held.due(clock, tone_meter.as_ref()))
            && let Some(held) = tone_held.reading.take()
        {
            held.tell(trace, tone_out.as_mut())?;
        }
        // The person's line is over once the lamp's line it was heard in is
        // no longer the lamp's, or its pieces have stopped after that line's
        // End ([`ToneLine::over`]).
        if tone_line.over(
            clock,
            reply.as_ref().map(|current| current.owner.turn()),
            reply
                .as_ref()
                .is_none_or(|current| current.local_input_ended),
        ) {
            let (words, heard_in) = tone_line.take();
            tone_line_over(
                &words,
                heard_in,
                &mut tone_held,
                &mut tone_baselines,
                tone_person.as_deref(),
                trace,
            )?;
        }
        // A face added from a photo at the lamp, unconfirmed: asked once,
        // gently, as the lamp's own written turn, while it is quiet (#1968).
        // The person's spoken yes or no is theirs to give; no answer in the
        // meeting is the eyes' to let go.
        if let Some(watch) = photo_faces.as_ref()
            && reply.is_none()
            && provider_queue.is_empty()
            && !admission.owns_stream()
            && !submission_draining
            && snapshot.listening_ready(now())
            && clock.saturating_sub(last_turn_us) >= 3_000_000
            && let Some(provider_session) = unasked_session(&cloud, paused)
        {
            // The eyes took it as asked: say it now. Else ask them to.
            if let Some(name) = watch.asked() {
                eprintln!("lamp-voice: a face added from a photo is at the lamp; asking them once");
                snapshot = commit_written_turn(
                    owner,
                    reply,
                    &mut provider_queue,
                    &crate::photo_faces::note(&name),
                    false,
                    "photo_face_ask",
                    provider_session,
                    rig.playout_start,
                    trace,
                    |state| rig.publish(state),
                )?;
                last_publish = monotonic_us();
            } else if let Some(name) = watch.to_ask() {
                watch.ask(name);
            }
        }
        // News from an agent, told the way a chief of staff would: when
        // someone is at the lamp (its eyes say so, or a person said
        // something a moment ago; a bump is not a person, and a restart is
        // not someone speaking: on lamp-4ace on 2026-10-15 the first build
        // took the start of the service for it and told the empty office at
        // 00:49:48), nothing has been said for a little while (or, with
        // `LAMP_TASK_NEWS_AT_PAUSE=on`, at the first pause of a second, as
        // a due reminder waits, #560), nothing is owed, and the room is not
        // busy with other talk.
        // A sign-in's news, put first, is told at the next pause whatever
        // the switch says (#2602, finding 4): the person just asked for it,
        // and its question has a deadline. Put first before the switch is
        // looked at, so it goes ahead with the switch on too (the
        // reviewer-lead's P2 on #2642).
        if let Some(news) = task_news.as_mut()
            && !reminder_waiting
            && news.waiting().is_some()
            && crate::task_news::quiet_for_news(
                news.sign_in_first() || news_at_pause,
                clock,
                last_turn_us,
                last_spoke_us,
                last_speech_us,
                reminder_quiet(
                    clock,
                    reminder_cycle_started_us,
                    last_speech_us,
                    last_capture,
                ),
            )
            && (presence
                .as_ref()
                .is_some_and(|presence| presence.present(clock))
                || said_at_us
                    .is_some_and(|at| clock.saturating_sub(at) < crate::task_news::HEARD_WITHIN_US))
            && !room.busy()
            && reply.is_none()
            && provider_queue.is_empty()
            && !admission.owns_stream()
            && !submission_draining
            && snapshot.listening_ready(now())
            && let Some(provider_session) = unasked_session(&cloud, paused)
            && let Some(ticket) = news_for_skill(news, says.as_mut(), news_by_skill, &mut news_held)
            // With the switch, tasks that finished together are told in
            // one turn (the reviewer on #1161). Not while news waits for a
            // skill: the skill tells each task in its own run (#1042), and a
            // later run's ask would find its news told already. Nor when this
            // is told with a skill's ticket, or while a skill asks for news:
            // each run takes its own task's news (the reviewer on #1042 and
            // #1223).
            && let Some(told) = if news_at_pause
                && !news_by_skill
                && ticket.is_none()
                && !says.as_ref().is_some_and(|says| {
                    says.asks_for(crate::say::Source::Tasks, monotonic_us())
                }) {
                news.tell_together()
            } else {
                news.tell()
            }
        {
            eprintln!(
                "lamp-voice: telling news from the work handed over ({}{})",
                told.key,
                if told.also.is_empty() {
                    String::new()
                } else {
                    format!(", with {}", told.also.join(", "))
                }
            );
            record(
                trace,
                json!({"kind":"task_news","at_us":clock,"key":told.key,"also":told.also,"provider_session":provider_session,
                    "at_pause":news_at_pause || crate::sign_in::is_news(&told.key),"since_turn_ms":clock.saturating_sub(last_turn_us) / 1_000}),
            )?;
            snapshot = commit_written_turn(
                owner,
                reply,
                &mut provider_queue,
                &told.note,
                false,
                "lamp_tells_news",
                provider_session,
                rig.playout_start_now(),
                trace,
                |state| rig.publish(state),
            )?;
            last_publish = monotonic_us();
            begun_for(ticket, reply);
            news_on(told, reply);
        }
        service_optional_ring(rig, &mut choreography, owner, trace, &mut ring_trace)?;
        if clock.saturating_sub(last_health) >= 50_000 {
            rig.running()?;
            last_health = clock;
        }
        if clock.saturating_sub(last_hand_health) >= 1_000_000
            && let Some(controls) = controls.as_ref()
        {
            let health = controls.health();
            record(
                trace,
                json!({"kind":"hand_controls_health","at_us":monotonic_us(),
                "received":health.received,"rejected":health.rejected,"lost":health.lost,
                "reconnects":health.reconnects,"discontinuities":health.discontinuities,
                "pump_running":health.pump_running,"persistence_failed":health.persistence_failed}),
            )?;
            last_hand_health = clock;
        }
        flush_cues(trace, cues);
        #[cfg(test)]
        if crate::scenario_clock::is_active() {
            continue;
        }
        stall_pass.done();
        // Nothing going on: no reply, nothing for the provider, no admitted
        // speech, nothing draining, the room not busy, no speech lately.
        let quiet = crate::quiet_pace::Going {
            reply: reply.is_some(),
            provider_queue: !provider_queue.is_empty(),
            stream: admission.owns_stream(),
            draining: submission_draining,
            room_busy: room.busy(),
            since_speech_us: monotonic_us().saturating_sub(last_speech_us),
        }
        .quiet();
        if let Some(wait) = pace.pass(quiet).checked_sub(loop_start.elapsed()) {
            std::thread::sleep(wait);
        }
    }
}

/// Close the one coordinator withholding interval. This never changes input,
/// output ownership, raw accepted-zero receipts or their starvation meaning.
fn finish_duplex_yield(
    trace: &mut SessionState,
    at_us: u64,
    outcome: &str,
    frame_sequence: Option<u64>,
    read_completed_at_us: Option<u64>,
) -> Result<()> {
    let Some(interval) = trace.yield_interval.take() else {
        return Ok(());
    };
    record(
        trace,
        json!({"kind":"duplex_yield_end", "owner":interval.owner,
        "started_at_us":interval.started_at_us, "at_us":at_us,
        "first_frame_sequence":interval.first_frame_sequence,
        "first_read_completed_at_us":interval.first_read_completed_at_us,
        "last_frame_sequence":interval.last_frame_sequence,
        "ending_frame_sequence":frame_sequence,
        "ending_read_completed_at_us":read_completed_at_us,
        "missing_evidence_since_sequence":interval.missing_evidence_since,
        "outcome":outcome,
        "clock_regressed":at_us < interval.started_at_us,
        "boundary":"coordinator withholding interval; not an acoustic gap or input admission",
        "starvation_excluded":false}),
    )
}

/// Run immediately before the normal speaker dispatch. No stale hearing state
/// can leave an open audit interval after the mask ceases to withhold output.
fn settle_duplex_yield_before_dispatch(
    mask: &PlaybackMask,
    reply: &Option<Reply>,
    at_us: u64,
    trace: &mut SessionState,
) -> Result<()> {
    if let Some(interval) = trace.yield_interval {
        let current_owner = reply.as_ref().map(|current| current.owner);
        if interval.owner != current_owner {
            finish_duplex_yield(trace, at_us, "owner_changed", None, None)?;
        } else if !mask.yielded(at_us) {
            finish_duplex_yield(trace, at_us, "hearing_expired", None, None)?;
        }
    }
    Ok(())
}

fn clear_capture_lineage(
    detector: &mut TurnDetector,
    mask: &mut PlaybackMask,
    trace: &mut SessionState,
    at_us: u64,
    sequence: u64,
    read_completed_at_us: u64,
) -> Result<()> {
    finish_duplex_yield(
        trace,
        at_us,
        "capture_lineage_changed",
        Some(sequence),
        Some(read_completed_at_us),
    )?;
    detector.discard_inactive_prefix();
    mask.clear_read_lineage();
    Ok(())
}

/// Whether a start heard now comes as the lamp's sound ends, so that with
/// `LAMP_REPLY_END_REACH=on` it may begin at the onset that sound hid
/// ([`crate::activity::TurnDetector::reach_back_to_onset`], #1722): the
/// lamp's sound quiet under [`lamp_audio::evidence::QUIET_BLOCKS_CEILING`]
/// blocks, 640 ms, past the 300 ms the duplex rule waits before it lets a
/// person start.
fn reaches_past_reply(on: bool, echo: Option<&lamp_audio::evidence::EchoEvidence>) -> bool {
    on && echo
        .is_some_and(|echo| echo.render_quiet_blocks < lamp_audio::evidence::QUIET_BLOCKS_CEILING)
}

/// Duplex hearing for one capture block, before admission sees it. Returns
/// the score the turn detector is to be given: the block's own, except that
/// the lamp's own voice counts as silence and a person over it as speech.
/// Every other policy gets the block's own score and no trace.
#[allow(clippy::too_many_arguments)]
fn hear_capture_block(
    mask: &mut PlaybackMask,
    reply: &Option<Reply>,
    heard_reply: &mut Option<TurnOwner>,
    sequence: u64,
    captured_at_us: u64,
    probability: f32,
    echo: Option<&lamp_audio::evidence::EchoEvidence>,
    at_us: u64,
    trace: &mut SessionState,
) -> Result<(f32, bool)> {
    if !mask.is_duplex() {
        return Ok((probability, false));
    }
    let owner = reply.as_ref().map(|current| current.owner);
    if owner != *heard_reply {
        finish_duplex_yield(
            trace,
            at_us,
            "owner_changed",
            Some(sequence),
            Some(captured_at_us),
        )?;
        mask.reply_closed();
        *heard_reply = owner;
    }
    let speaking = reply_sounding(mask, reply, at_us);
    let listened = mask.listen(sequence, probability, echo, speaking, at_us)?;
    trace.heard(
        sequence,
        speaking,
        listened.heard,
        probability,
        echo,
        mask.hearing_levels_db(),
        at_us,
    )?;
    if listened.forgiven {
        record(
            trace,
            json!({"kind":"duplex_fruitless_forgiven","at_us":at_us,"frame_sequence":sequence,
            "forgiven_so_far":mask.fruitless_forgiven(),"forgiven_after_ms":crate::turn_taking::FORGIVE_FRUITLESS_US / 1_000}),
        )?;
    }
    if let Some(interval) = trace.yield_interval.as_mut() {
        interval.last_frame_sequence = sequence;
        if echo.is_none() && interval.missing_evidence_since.is_none() {
            interval.missing_evidence_since = Some(sequence);
        }
    }
    if listened.step == DuplexStep::Yield {
        finish_duplex_yield(
            trace,
            at_us,
            "superseded_decision",
            Some(sequence),
            Some(captured_at_us),
        )?;
        trace.yield_interval = Some(YieldInterval {
            owner,
            started_at_us: at_us,
            first_frame_sequence: sequence,
            first_read_completed_at_us: captured_at_us,
            last_frame_sequence: sequence,
            missing_evidence_since: None,
        });
    }
    let kind = match listened.step {
        DuplexStep::Carry => {
            if trace.yield_interval.is_some() && !mask.yielded(at_us) {
                finish_duplex_yield(
                    trace,
                    at_us,
                    "evidence_discontinuity",
                    Some(sequence),
                    Some(captured_at_us),
                )?;
            }
            return Ok((listened.score, false));
        }
        DuplexStep::Yield => "duplex_yield",
        DuplexStep::Commit => "duplex_commit",
        DuplexStep::Resume => "duplex_resume",
    };
    let mut event = json!({"kind":kind,"at_us":at_us,"frame_sequence":sequence,
    "last_block_host_read_us":captured_at_us,"owner":owner,
    "reply_queued_frames":reply.as_ref().map(|current| current.pcm.len()),
    "hearing_margin_db":mask.hearing_margin_db(),
    "boundary":"coordinator hearing decision; not acoustic onset or stopped output"});
    if listened.step != DuplexStep::Yield {
        // #2939: how long the commit was held for the yield's words, and
        // whether they were a backchannel (null: none came).
        let (held_ms, backchannel) = mask.yield_words();
        if held_ms > 0 && backchannel.is_none() {
            // Words that come after this are dropped with the yield: say
            // that the hold ran out before them. With no words the answer
            // goes on; only a sentence commits without them.
            eprintln!(
                "lamp-voice: a yield's words had not come after its commit was held {held_ms} ms: {}",
                if listened.step == DuplexStep::Commit {
                    "committed"
                } else {
                    "the answer goes on"
                }
            );
        }
        event["held_ms"] = json!(held_ms);
        event["backchannel"] = json!(backchannel);
    }
    record(trace, event)?;
    match listened.step {
        DuplexStep::Commit => finish_duplex_yield(
            trace,
            at_us,
            "commit_decision",
            Some(sequence),
            Some(captured_at_us),
        )?,
        DuplexStep::Resume => finish_duplex_yield(
            trace,
            at_us,
            "resumed",
            Some(sequence),
            Some(captured_at_us),
        )?,
        _ => {}
    }
    Ok((listened.score, listened.step == DuplexStep::Yield))
}

/// The asker-first rule's record, once for `turn` (`crate::asker_first`):
/// speech it kept, or would have kept (`acted` false: recorded only, or the
/// answer began first), with both levels and when it was judged.
#[allow(clippy::too_many_arguments)]
fn record_asker_first(
    rule: &mut crate::asker_first::AskerFirst,
    trace: &mut SessionState,
    turn: u64,
    at_us: u64,
    kept: crate::asker_first::Kept,
    acted: bool,
    judged: &str,
) -> Result<()> {
    if !rule.first_for(turn) {
        return Ok(());
    }
    let under_db = f32::from(kept.asker - kept.heard) / 7.5;
    eprintln!(
        "lamp-voice: asker first: speech {under_db:.1} dB under the asker's {} request {turn}'s answer",
        if acted {
            "took no turn from"
        } else {
            "would have taken no turn from"
        }
    );
    record(
        trace,
        json!({"kind":"asker_first","turn":turn,"at_us":at_us,
            "asker_level":kept.asker,"heard_level":kept.heard,"heard_upper_tenth":kept.upper,"blocks":kept.blocks,
            "margin":crate::asker_first::MARGIN,"acted":acted,"judged":judged,
            "boundary":"speech well under the asker's, judged on its own blocks (its upper tenth under the asker's middle less the margin), while its answer was awaited; acted: it took no turn, else recorded only or the answer began first; levels on the face's scale, 7.5 to a decibel"}),
    )
}

/// What the stop words hear of a frame, taken out of it: the block before
/// the canceller (`raw`), else the canceller's linear output (`linear`), as
/// the capture worker was asked (`LAMP_STOP_WORDS_SOURCE`). `None`, and they
/// hear the frame's `samples`: with `processed`, and with `linear` on a block
/// the canceller did not run for.
fn stop_words_block(frame: &mut MicrophoneFrame) -> Option<Vec<i16>> {
    frame.raw.take().or_else(|| frame.linear.take())
}

/// One block for a yield's words (#828, `LAMP_STOP_WORDS=heard`): its held
/// audio when it begins, each block while it is in force, and the duplex
/// rule told when they come. Nothing but what is said in a yield is heard.
#[allow(clippy::too_many_arguments)]
fn hear_stop_words(
    words: &mut crate::stop_words::StopWords,
    mask: &mut PlaybackMask,
    admission: &InputAdmission,
    step: &AdmissionStep,
    replying: bool,
    sounding: bool,
    captured_at_us: u64,
    samples: &[i16],
    trace: &mut SessionState,
) -> Result<()> {
    let held = match step {
        AdmissionStep::Candidate(candidate) if candidate.displaced_owner.is_some() => {
            admission.pending_audio(candidate.id)
        }
        _ => None,
    };
    let tell = words.block(
        replying,
        sounding,
        mask.yield_in_force(),
        mask.still_blocks(),
        held,
        captured_at_us,
        samples,
    );
    if words.take_asked() {
        mask.words_asked();
    }
    match tell {
        crate::stop_words::Tell::Nothing => {}
        crate::stop_words::Tell::Await => mask.await_words(),
        crate::stop_words::Tell::Words {
            stop,
            count,
            backchannel,
        } => {
            // With backchannel holds, words that came empty are none: the
            // yield is held as a wordless one and resumes at the bound
            // (#2939, lamp-52e6 on 23 Oct: "0 words" committed a turn).
            let wordless = count == 0 && mask.holds_backchannels();
            if !wordless {
                mask.words(stop, backchannel);
            }
            eprintln!(
                "lamp-voice: a yield's words came: {count} {}, {}",
                if count == 1 { "word" } else { "words" },
                if wordless {
                    "held as a yield with no words"
                } else if stop {
                    "a stop phrase"
                } else if backchannel && mask.holds_backchannels() {
                    "a backchannel: the answer goes on"
                } else {
                    "not a stop phrase"
                }
            );
            record(
                trace,
                json!({"kind":"stop_words","stop":stop,"words":count,"backchannel":backchannel,"at_us":monotonic_us()}),
            )?;
        }
        crate::stop_words::Tell::Unheard => {
            // With backchannel holds such a yield resumed the answer,
            // unless it reached a sentence.
            eprintln!("lamp-voice: a yield ended before its words came");
            record(
                trace,
                json!({"kind":"stop_words","stop":false,"words":null,"at_us":monotonic_us()}),
            )?;
        }
    }
    Ok(())
}

/// [`observe_prospective_heard`] for a block heard as its own score.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn observe_prospective_input(
    mask: &mut PlaybackMask,
    detector: &mut TurnDetector,
    admission: &mut InputAdmission,
    context: AdmissionContext,
    observation: ObservedAudio,
    first_read_us: u64,
    probability: f32,
    at_us: u64,
) -> Result<(AdmissionUpdate, bool)> {
    observe_prospective_heard(
        mask,
        detector,
        admission,
        context,
        observation,
        first_read_us,
        probability,
        Some(probability),
        at_us,
    )
}

#[allow(clippy::too_many_arguments)]
fn observe_prospective_heard(
    mask: &mut PlaybackMask,
    detector: &mut TurnDetector,
    admission: &mut InputAdmission,
    context: AdmissionContext,
    observation: ObservedAudio,
    first_read_us: u64,
    probability: f32,
    own: Option<f32>,
    at_us: u64,
) -> Result<(AdmissionUpdate, bool)> {
    mask.remember_read(
        observation.sequence,
        first_read_us,
        observation.captured_at_us,
        at_us,
    )?;
    if !admission.owns_stream() && !admission.draining() && mask.frame() == Disposition::Suppress {
        let rejection = admission.suppress_prospective(context, at_us)?;
        detector
            .suppress(&observation, probability)
            .map_err(io::Error::other)?;
        mask.suppressed()?;
        return Ok((
            AdmissionUpdate {
                rejection,
                step: AdmissionStep::None,
            },
            false,
        ));
    }
    let activity = detector.push_heard(observation, probability, own);
    let ended = matches!(activity, Activity::End(_));
    Ok((admission.observe(activity, context, at_us)?, ended))
}

fn decide_prospective_input(
    mask: &PlaybackMask,
    detector: &mut TurnDetector,
    admission: &mut InputAdmission,
    candidate: crate::admission::Candidate,
    context: AdmissionContext,
    at_us: u64,
    trace: &mut SessionState,
) -> Result<Option<Accepted>> {
    if let Some(rejection) = admission.maintain(context, at_us)? {
        record_input_rejection(trace, rejection)?;
        return Ok(None);
    }
    match mask.candidate(candidate)? {
        Disposition::AwaitWriteEvidence => return Ok(None),
        Disposition::Suppress => {
            if let Some(rejection) = admission.suppress_prospective(context, at_us)? {
                record_input_rejection(trace, rejection)?;
            }
            detector.clear_prospective();
            return Ok(None);
        }
        Disposition::Pass => {}
    }
    if mask.enabled() {
        let Some(audio) = admission.pending_audio(candidate.id) else {
            return Ok(None);
        };
        let mut last_masked = None;
        let mut awaiting_write = false;
        for frame in audio {
            let disposition = match mask.retained_frame(frame) {
                Ok(disposition) => disposition,
                Err(error)
                    if error
                        .get_ref()
                        .is_some_and(|cause| cause.is::<RetainedHistoryLoss>()) =>
                {
                    if let Some(rejection) =
                        admission.reject_retained_history(candidate.id, context, at_us)?
                    {
                        let loss = error
                            .get_ref()
                            .and_then(|cause| cause.downcast_ref::<RetainedHistoryLoss>())
                            .expect("matched history loss");
                        record(
                            trace,
                            json!({"kind":"input_candidate_rejected", "candidate":rejection.candidate,
                            "reason":rejection.reason, "history_loss":loss, "at_us":at_us}),
                        )?;
                    }
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            };
            match disposition {
                Disposition::Suppress => last_masked = Some(frame.sequence),
                Disposition::AwaitWriteEvidence => awaiting_write = true,
                Disposition::Pass => {}
            }
        }
        // A delayed receipt may cover pre-roll retained before the six fresh
        // onset blocks. Never splice disjoint blocks or erase an accepted input.
        if let Some(through_sequence) = last_masked {
            let first_onset = candidate
                .trigger_sequence
                .checked_sub(u64::from(crate::activity::START_BLOCKS) - 1)
                .ok_or_else(|| io::Error::other("candidate has no complete onset"))?;
            if through_sequence >= first_onset {
                if let Some(rejection) = admission.suppress_prospective(context, at_us)? {
                    record_input_rejection(trace, rejection)?;
                }
                detector.clear_prospective();
                return Ok(None);
            }
            let removed = admission.discard_pending_prefix(candidate.id, through_sequence)?;
            record(
                trace,
                json!({"kind":"input_prefix_sanitized","candidate":candidate.id,
                    "through_sequence":through_sequence,"removed_blocks":removed,
                    "reason":"own_playback_prefix","at_us":at_us}),
            )?;
        }
        if awaiting_write {
            return Ok(None);
        }
    }
    // A yield's first words go only as far back as the provider takes
    // audio: those older than `FIRST_WORDS_AGE_US` are dropped from the
    // front before the commit, never the usual pre-roll or the onset, and
    // what is sent keeps its own capture times (#828, the reviewer on #936).
    if let Some(through) = admission.lead_older_than(
        candidate.id,
        at_us.saturating_sub(crate::admission::FIRST_WORDS_AGE_US),
    ) {
        let removed = admission.discard_pending_prefix(candidate.id, through)?;
        record(
            trace,
            json!({"kind":"input_prefix_sanitized","candidate":candidate.id,
                "through_sequence":through,"removed_blocks":removed,
                "reason":"first_words_too_old","at_us":at_us}),
        )?;
    }
    let basis = if mask.is_duplex() {
        AcceptanceBasis::DirectedSessionDuplex
    } else if mask.enabled() {
        AcceptanceBasis::DirectedSessionV1Baseline
    } else {
        AcceptanceBasis::DirectedSessionVadOnly
    };
    let Some((_, evidence)) = admission.pending_evidence(at_us, basis) else {
        return Ok(None);
    };
    accepted_decision(admission.decide(evidence, context, at_us)?, trace)
}

/// What the say service's gates read for a skill's sound (#938).
struct SoundGates<'a> {
    at_us: u64,
    paused: bool,
    snapshot: &'a Snapshot,
    reply: &'a Option<Reply>,
    provider_queue: &'a VecDeque<ProviderInput>,
    speaker_pending: &'a Option<PendingSpeaker>,
    playback_mask: &'a PlaybackMask,
    admission: &'a InputAdmission,
    submission_draining: bool,
    /// [`reminder_quiet`]: a fresh capture, and nothing heard for 1 s.
    heard_quiet: bool,
    /// The head cue's: the lamp said it would look, and owes the answer.
    reply_owed: bool,
    reminder_waiting: bool,
    since_turn_us: u64,
}

/// The say service's gates, as a say has them, for a skill's sound: a
/// reply or a candidate open, or anything still to play, is not rest.
fn skill_sound_now(gates: SoundGates<'_>, says: &crate::say::Says) -> crate::say::SoundNow {
    let at = MonoTime::from_micros(gates.at_us);
    crate::say::SoundNow {
        paused: gates.paused,
        muted: gates.snapshot.microphone_permission() != Permission::Allowed,
        speaking: gates.reply.is_some()
            || !gates.provider_queue.is_empty()
            || gates.speaker_pending.is_some()
            || !gates.playback_mask.available(gates.at_us),
        listening: gates.admission.owns_stream()
            || !gates.admission.idle_for_reminder()
            || gates.submission_draining
            || !gates.snapshot.listening_ready(at)
            || !gates.heard_quiet,
        reply_owed: gates.reply_owed,
        reminder_waiting: gates.reminder_waiting,
        since_turn_us: gates.since_turn_us,
        since_sound_us: says
            .last_sound_us()
            .map(|played| gates.at_us.saturating_sub(played)),
    }
}

/// What the ring is told of the turn in hand, from what plays in it. The
/// head tap's "hm?" (#711), the same hum after an empty "Sorry?" (#1417)
/// and as the person wakes a hand's sleep (#1362), show the listening face,
/// with the hum, which the ring keeps at rest for the tap's window; a
/// skill's sound (#1144) and a hand's sleep's breath (#1362, under the
/// hold's own dimming) show nothing, so no frame is asked for while they
/// play.
fn tell_ring_of_turn(choreography: &mut RingChoreographer, origin: Option<SpeechOrigin>) {
    let cue = matches!(
        origin,
        Some(SpeechOrigin::HeadCue | SpeechOrigin::UnclearHum | SpeechOrigin::WakeHum)
    );
    choreography.set_listening(cue || origin == Some(SpeechOrigin::RequestedAudio));
    choreography.set_unshown(origin.is_some_and(SpeechOrigin::unshown) && !cue);
}

/// The room sees the owned turn, except for the lamp's short unshown sounds.
fn tell_voice_of_turn(
    bus: &mut crate::bus_out::BusOut,
    snapshot: Snapshot,
    origin: Option<SpeechOrigin>,
    at_us: u64,
) {
    bus.state(
        crate::ring_wire::RingPhase::from_snapshot(snapshot)
            .filter(|_| !origin.is_some_and(SpeechOrigin::unshown)),
        at_us,
    );
}

/// A skill's sound asked for, if one waits: played now as a turn of the
/// lamp's own, on the head cue's speaker path, or refused now with why.
/// The shutdown's last words (#3697) cut the lamp's own speech for it.
/// The authority's new snapshot when it plays.
#[allow(clippy::too_many_arguments)]
fn play_skill_sound(
    says: &mut crate::say::Says,
    now_is: &crate::say::SoundNow,
    at_us: u64,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    provider_queue: &mut VecDeque<ProviderInput>,
    speaker_pending: &mut Option<PendingSpeaker>,
    admission: &mut InputAdmission,
    detector: &mut TurnDetector,
    submission_draining: &mut bool,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Option<Snapshot>> {
    let Some((samples, ticket)) = says.take_sound(at_us) else {
        return Ok(None);
    };
    let decided = if ticket.last_words() {
        crate::say::may_play_last_words(now_is)
    } else {
        crate::say::may_play_sound(now_is)
    };
    record(
        trace,
        json!({"kind":"skill_sound","at_us":at_us,"skill":ticket.skill(),"plays":decided.is_ok(),"why":decided.err(),"last_words":ticket.last_words()}),
    )?;
    if let Err(why) = decided {
        ticket.answer(crate::say::Outcome::Refused(why.into()));
        return Ok(None);
    }
    if ticket.last_words() {
        *submission_draining |= cut_for_last_words(
            owner,
            reply,
            provider_queue,
            speaker_pending,
            admission,
            detector,
            trace,
        )?;
    }
    let (snapshot, _) = speaker_tone::commit_sound_turn(
        owner,
        reply,
        NoticeDelivery::sound(SpeechOrigin::SkillSound, samples),
        "skill_sound",
        "a skill's own sound: nothing heard, nothing sent to the provider",
        0,
        playout_start,
        trace,
        publish,
    )?;
    says.sound_played(at_us);
    ticket.begun(monotonic_us());
    if let Some(current) = reply.as_mut() {
        current.say = Some(ticket);
    }
    Ok(Some(snapshot))
}

/// The shutdown's last words (#3697) cut what the lamp was doing: its reply,
/// unsent input and unwritten speech, and, as a Stop does, the rest of a line
/// still being said, drained until its end. Without the drain that line's
/// microphone frames went on into the last words' turn, which sends nothing,
/// and filled the provider queue past its bound (lamp-52e6, 27 Oct: the voice
/// ended). Whether input is now drained.
#[allow(clippy::too_many_arguments)]
fn cut_for_last_words(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    provider_queue: &mut VecDeque<ProviderInput>,
    speaker_pending: &mut Option<PendingSpeaker>,
    admission: &mut InputAdmission,
    detector: &mut TurnDetector,
    trace: &mut SessionState,
) -> Result<bool> {
    cancel_reply(owner, reply, trace, "last_words")?;
    provider_queue.clear();
    *speaker_pending = None;
    let at_us = monotonic_us();
    admission.abandon_until_end(at_us)?;
    detector.abandon_input(at_us);
    Ok(admission.draining())
}

/// When a turn's answer begins to be heard, for the session's records
/// (`answer_first_audio`), so an A/B times the answer and never a cue (the
/// first builder, 19 Oct: on the second route the "first sound" had been
/// its wait cue, 1.5 s, and the answer came 4.0 to 4.5 s after the words).
/// On the Live route the first audio of a turn is its answer. On the second
/// route a wait cue, the down line and the announcement come with no words,
/// and each sentence of the answer comes with its words before its audio:
/// the answer's audio is the first after the turn's first words.
#[derive(Default)]
struct AnswerAudio {
    worded: Option<u64>,
    recorded: Option<u64>,
    /// The turn whose end of speech was decided (`local_endpoint`), and when.
    decided: Option<(u64, u64)>,
}

impl AnswerAudio {
    /// The end of speech of `turn` was decided at `at_us`.
    fn decided(&mut self, turn: u64, at_us: u64) {
        self.decided = Some((turn, at_us));
    }

    /// The lamp's words for `turn` came.
    fn words(&mut self, turn: u64) {
        self.worded = Some(turn);
    }

    /// Audio for `turn` came at `now_us`, from the second route or not: when
    /// it is the first of its answer, how long after the end of speech was
    /// decided (`None` for a turn with no decision: the lamp's own).
    fn audio(&mut self, turn: u64, second: bool, now_us: u64) -> Option<Option<u64>> {
        if self.recorded == Some(turn) || (second && self.worded != Some(turn)) {
            return None;
        }
        self.recorded = Some(turn);
        Some(
            self.decided
                .filter(|(of, _)| *of == turn)
                .map(|(_, at)| now_us.saturating_sub(at) / 1_000),
        )
    }

    /// The decision for `turn`, when there was one.
    fn decided_at(&self, turn: u64) -> Option<u64> {
        self.decided.filter(|(of, _)| *of == turn).map(|(_, at)| at)
    }
}

/// Whether `output` carries the person's words: a transcript of what was
/// heard (no request) with at least one word in it: talk, which ends the
/// quiet the lamp's own unasked turns wait for (`last_turn_us`), as the
/// detector's own speech does.
fn words_heard(output: &ProviderOutput) -> bool {
    matches!(output, ProviderOutput::Transcript { request: None, text, .. }
        if !crate::not_speech::plain_words(text).is_empty())
}

fn reminder_quiet(
    now_us: u64,
    cycle_started_us: u64,
    last_speech_us: u64,
    last_capture_us: Option<u64>,
) -> bool {
    last_capture_us.is_some_and(|captured| {
        now_us
            .checked_sub(captured)
            .is_some_and(|age| age <= INPUT_LEASE_US)
    }) && now_us
        .checked_sub(last_speech_us.max(cycle_started_us))
        .is_some_and(|age| age >= 1_000_000)
}

/// What a due reminder or news waits for (#939, (c)). A skill's `from` ask
/// for what `source` keeps, if one waits, is taken: `Some(Some(ticket))`,
/// to say it with that ask's ticket, with the switch or without it. With
/// `by_skill` and no ask yet: `None` to wait, and `Some(None)` once it has
/// waited [`crate::say::SKILL_WAITS_US`] since `key` was first held, to say
/// it as the voice always did. Without `by_skill`, or with no say service
/// (nothing can ask), `Some(None)` at once. Called last before the turn is
/// committed: a ticket taken is always said.
fn for_skill(
    says: Option<&mut crate::say::Says>,
    source: crate::say::Source,
    by_skill: bool,
    key: &str,
    task: Option<u64>,
    held: &mut Option<(String, u64)>,
    now_us: u64,
) -> Option<Option<crate::say::Ticket>> {
    // Onboarding has an exact invitation ticket and no autonomous fallback.
    if source == crate::say::Source::Onboarding {
        return None;
    }
    // No say service: no skill can ask (Fourth on #1038).
    let Some(says) = says else {
        *held = None;
        return Some(None);
    };
    // An ask that waits is taken, switch or not (Fourth on #1038): the
    // skill moves with the words.
    if let Some(ticket) = says.take_from(source, task, now_us) {
        *held = None;
        return Some(Some(ticket));
    }
    if !by_skill {
        *held = None;
        return Some(None);
    }
    let since = match held {
        Some((held_key, since)) if held_key == key => *since,
        _ => {
            *held = Some((key.to_owned(), now_us));
            now_us
        }
    };
    if now_us.saturating_sub(since) >= crate::say::SKILL_WAITS_US {
        eprintln!(
            "lamp-voice: no skill asked for the {} due within {} s: said by the voice",
            match source {
                crate::say::Source::Schedule => "reminder",
                crate::say::Source::Tasks => "news",
                crate::say::Source::Onboarding => "onboarding invitation",
            },
            crate::say::SKILL_WAITS_US / 1_000_000
        );
        *held = None;
        Some(None)
    } else {
        None
    }
}

/// The key a reminder offer is held by for its skill: its store, item and
/// delivery.
fn reminder_key(offer: &crate::reminders::Offer) -> String {
    format!(
        "{}/{}/{}",
        offer.token.key.store_id, offer.token.key.id, offer.token.key.delivery
    )
}

/// Whether the reminder held for its skill (`held`, by [`for_skill`]) is the
/// one still offered: while it is, nothing else unasked is said first. One
/// no longer offered (said, or let go) holds nothing.
fn reminder_waits(held: &Option<(String, u64)>, offered: Option<&crate::reminders::Offer>) -> bool {
    held.as_ref()
        .zip(offered)
        .is_some_and(|((key, _), offer)| *key == reminder_key(offer))
}

/// [`for_skill`] for the news first in line. Progress is the Policy's to
/// time already, and is told as it was. News waits for its skill from when
/// the voice had it ([`hold_from`]).
fn news_for_skill(
    news: &mut crate::task_news::TaskNews,
    says: Option<&mut crate::say::Says>,
    by_skill: bool,
    held: &mut Option<(String, u64)>,
) -> Option<Option<crate::say::Ticket>> {
    let (key, progress, task) = news
        .waiting()
        .map(|front| (front.key.clone(), front.progress.is_some(), front.task()))?;
    let now_us = monotonic_us();
    let found_us = news.found_at(&key).map(|found| {
        now_us.saturating_sub(u64::try_from(found.elapsed().as_micros()).unwrap_or(u64::MAX))
    });
    hold_from(held, &key, found_us);
    for_skill(
        says,
        crate::say::Source::Tasks,
        by_skill && !progress,
        &key,
        task,
        held,
        now_us,
    )
}

/// `key`'s wait for its skill's ask counted from `found_us`, when the voice
/// had it, and not from when it could first be said, unless it is held
/// already (the reviewer's P2 on #1667). The Task news skill asks the
/// moment work settles, and an ask lives `say::FROM_LIVES_US`: news held
/// back past that by nobody there or by a talk would otherwise wait 10 s
/// more, in silence, for an ask that never comes again. So it is told as
/// soon as it can be, by the voice; within those 10 s, with the skill's
/// ask, as before. A reminder is held from when it can be said, since
/// Reminders asks only then (`skills/reminders`, `voice/state` resting).
fn hold_from(held: &mut Option<(String, u64)>, key: &str, found_us: Option<u64>) {
    if let Some(found_us) = found_us
        && held.as_ref().is_none_or(|(held_key, _)| held_key != key)
    {
        *held = Some((key.to_owned(), found_us));
    }
}

/// The news this turn tells, its own key and those told with it, put on the
/// turn: handed to the tasks part only once it is heard whole (#1390).
fn news_on(told: crate::task_news::News, reply: &mut Option<Reply>) {
    if let Some(current) = reply.as_mut() {
        current.news = std::iter::once(told.key).chain(told.also).collect();
    }
}

/// A skill asked for what this turn says: it is begun, and is answered as
/// the turn ends, on the proof a reminder's acknowledgement takes.
fn begun_for(ticket: Option<crate::say::Ticket>, reply: &mut Option<Reply>) {
    if let Some(ticket) = ticket {
        eprintln!(
            "lamp-voice: saying what the skill {} asked for",
            ticket.skill()
        );
        ticket.begun(monotonic_us());
        if let Some(current) = reply.as_mut() {
            current.say = Some(ticket);
        }
    }
}

/// A prepared reminder waits behind an existing turn's promised follow-up,
/// including the gap after its final PCM retires and before the next owner.
fn idle_reminder_offer(
    reminders: &crate::reminders::Reminders,
    reply: &Option<Reply>,
    look_wanted: Option<(u64, u64)>,
    second_look: &mut crate::second_look::SecondLook,
    now_us: u64,
) -> Option<crate::reminders::Offer> {
    if reply.is_some() || look_wanted.is_some() || second_look.due(now_us).is_some() {
        return None;
    }
    reminders.offer(now_us)
}

fn record_look_intent(
    enabled: bool,
    request: Option<u64>,
    text: &str,
    last_reminder_turn: Option<u64>,
    answer_opening: &mut Option<(u64, String)>,
    look_wanted: &mut Option<(u64, u64)>,
    trace: &mut SessionState,
) -> Result<()> {
    // A turn the lamp wrote itself (its own look, a hello, a "Sorry?", a
    // reminder) takes no still from its words: nobody spoke, so pictures
    // cannot chain with nobody speaking. The worker's `heard` rule.
    let Some(turn) = request.filter(|turn| {
        enabled
            && last_reminder_turn.is_none_or(|reminder| *turn > reminder)
            && own_turn().load(std::sync::atomic::Ordering::Acquire) != *turn
    }) else {
        return Ok(());
    };
    // Ordinary answer words retain the existing camera-request policy. Stored
    // reminder text, including a delayed transcript, is never that authority.
    if answer_opening.as_ref().map(|(of, _)| *of) != Some(turn) {
        *answer_opening = Some((turn, String::new()));
    }
    if let Some((_, opening)) = answer_opening.as_mut()
        && opening.len() < 64
    {
        opening.push_str(text);
        if look_wanted.map(|(of, _)| of) != Some(turn) && crate::speak_up::asks_to_look(opening) {
            *look_wanted = Some((turn, monotonic_us()));
            record(
                trace,
                json!({"kind":"look_asked","turn":turn,"at_us":monotonic_us()}),
            )?;
        }
    }
    Ok(())
}

/// `LAMP_CAMERA_FOLLOWS_EYES`: exactly `on`; off unless set (the first
/// builder on #2233). On, a voice whose camera is the eyes' says so in its
/// authority, which starts per-person memory and #764's per-person Live
/// sessions on a product lamp.
pub const CAMERA_FOLLOWS_EYES: &str = "LAMP_CAMERA_FOLLOWS_EYES";

fn camera_follows_eyes() -> bool {
    std::env::var(CAMERA_FOLLOWS_EYES).ok().as_deref() == Some("on")
}

/// With the microphone just permitted: the eyes (`lamp-eyes`, a process of
/// their own, `eyes`, with `LAMP_CAMERA_FOLLOWS_EYES=on`) look exactly while
/// it is (`EyesLink`), so with no camera of the voice's own (`own_camera`)
/// the camera is permitted too, and what is read from them counts (#1895).
/// A mute denies both (`hand.rs`).
fn eyes_look_with_the_microphone(
    owner: &mut Controller,
    eyes: bool,
    own_camera: bool,
) -> std::result::Result<(), lamp_turn_owner::Error> {
    if eyes && !own_camera {
        owner.set_camera_permission(now(), Permission::Allowed)?;
    }
    Ok(())
}

/// A mute ends the cycle: the eyes are told no at once rather than left to
/// close on their 1.5 s without a yes (#1802), and the journal marks it.
/// One datagram, never waited on; no eyes is nothing to tell.
/// A privacy gap ridden ends with the conversation (#2602): its trace mark
/// and one journal line, on every exit while one is ridden.
fn gap_ended(
    gap_ride: &crate::privacy_gap::GapRide,
    trace: &mut SessionState,
    why: &str,
) -> Result<()> {
    let at_us = monotonic_us();
    if let Some(lasted_us) = gap_ride.lasted(at_us) {
        eprintln!(
            "lamp-voice: the privacy gap ends the conversation after {} ms: {why}",
            lasted_us / 1_000
        );
        record(
            trace,
            json!({"kind":"privacy_gap","state":"ended","at_us":at_us,"lasted_us":lasted_us,"why":why}),
        )?;
    }
    Ok(())
}

/// Whether the microphone's retention heartbeat has run out at `now_us`:
/// 1.5 s with no frame retained since `last_capture`, counted from a ridden
/// privacy gap's reopen (`from_us`) when that is later (#2602), so capture
/// has its whole bound to come back after a reopen near the gap's second.
fn retention_expired(last_capture: Option<u64>, from_us: u64, now_us: u64) -> bool {
    last_capture.is_some_and(|at| now_us.saturating_sub(at.max(from_us)) >= CAPTURE_GAP_US)
}

fn tell_eyes_no(eyes: Option<&mut crate::body_link::EyesLink>, exit: &str) {
    if eyes.is_some_and(|eyes| eyes.close()) {
        eprintln!("lamp-voice: the eyes are told no at once: {exit}");
    }
}

/// The eyes asked for a full look's still, once a turn (#836): whether
/// they were asked now.
fn ask_for_still(
    look: &mut crate::sight::Look,
    eyes: Option<&mut crate::body_link::EyesLink>,
    turn: u64,
    clock: u64,
    trace: &mut SessionState,
) -> Result<bool> {
    let Some((id, side)) = look.ask(turn) else {
        return Ok(false);
    };
    if let Some(eyes) = eyes {
        eyes.look_full(id, side);
    }
    #[cfg(test)]
    if TEST_EYES_ANSWER.get()
        && let Some(path) = TEST_FULL_LOOK.with(|still| still.borrow().clone())
    {
        std::fs::write(path, crate::sight::still_of(id))?;
    }
    record(
        trace,
        json!({"kind":"look_full_asked","turn":turn,"side":side,"at_us":clock}),
    )?;
    Ok(true)
}

/// Whether a tool's look for `turn` may ask the eyes now (#941): once the
/// turn's first audio plays or its reply is over, whichever is first. Not
/// at the answer: lamp-skills has only just sent the head down. Another
/// turn's reply drops the look, and asks nothing.
fn tool_look_due(reply: Option<&Reply>, turn: u64) -> bool {
    reply.is_none_or(|current| current.owner.turn() == turn && current.playback.is_some())
}

/// A look owed to `turn`, whose answer was cut off unsaid, is dropped and
/// says so: no still goes for an answer nobody heard.
fn drop_unsaid_look(
    look_wanted: &mut Option<(u64, u64)>,
    look_held: Option<(u64, u64, bool)>,
    turn: u64,
    why: &str,
    trace: &mut SessionState,
) -> Result<()> {
    if look_wanted.take_if(|(of, _)| *of == turn).is_some() {
        let by = if look_held.is_some_and(|(of, ..)| of == turn) {
            "tool"
        } else {
            "words"
        };
        record(
            trace,
            json!({"kind":"look_dropped","turn":turn,"by":by,"why":why,"at_us":monotonic_us()}),
        )?;
    }
    Ok(())
}

/// A skill's tool asked for a look in its answer (#941): the full look is
/// taken for that turn as if the lamp had said it would look, whatever the
/// model says. Only with a full look on (`LAMP_LOOK_FULL`), only for the
/// turn in hand, never for a reminder's, and once a turn: the model's own
/// "Let me take a look." after it asks nothing more.
fn record_tool_look(
    full: bool,
    request: u64,
    current: Option<u64>,
    last_reminder_turn: Option<u64>,
    look_wanted: &mut Option<(u64, u64)>,
    trace: &mut SessionState,
) -> Result<bool> {
    if !full || look_wanted.is_some_and(|(of, _)| of == request) {
        return Ok(false);
    }
    let why = if current != Some(request) {
        Some("its turn is not the one in hand")
    } else if last_reminder_turn.is_some_and(|reminder| request <= reminder) {
        Some("a reminder's turn")
    } else {
        None
    };
    if let Some(why) = why {
        record(
            trace,
            json!({"kind":"look_dropped","turn":request,"by":"tool","why":why,"at_us":monotonic_us()}),
        )?;
        return Ok(false);
    }
    *look_wanted = Some((request, monotonic_us()));
    record(
        trace,
        json!({"kind":"look_asked","turn":request,"by":"tool","at_us":monotonic_us()}),
    )?;
    Ok(true)
}

/// A service cycle's turn owner. Its turns number from 1 again (a mute, a
/// head sleep, a privacy switch), so the last cycle's own turn goes with it:
/// a spoken turn with the same number is not the lamp's.
fn cycle_controller(boot: BootId) -> Controller {
    own_turn().store(0, std::sync::atomic::Ordering::Release);
    Controller::new(boot, now())
}

worker_global! {
    /// The newest turn the lamp wrote itself (`commit_written_turn`), as the
    /// provider worker's `attention::own_request` is for its requests.
    fn own_turn() -> std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// A turn the lamp begins itself. Nothing was heard: the request is a
/// written note, started and ended at once, and what comes back is spoken
/// like any answer. The caller has checked that nothing else owns the lamp.
#[allow(clippy::too_many_arguments)]
fn commit_reminder_turn(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    provider_queue: &mut VecDeque<ProviderInput>,
    offer: crate::reminders::Offer,
    reminders: &mut crate::reminders::Reminders,
    provider_session: u64,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    // Reuse the existing local owner transition. This call only queues input;
    // replace that unsent item with the reminder-only provider purpose before
    // any flush can run. Ordinary Written bytes and behavior are unchanged.
    let snapshot = commit_written_turn(
        owner,
        reply,
        provider_queue,
        &offer.text,
        false,
        "schedule_reminder",
        provider_session,
        playout_start,
        trace,
        publish,
    )?;
    let current = reply
        .as_mut()
        .ok_or_else(|| io::Error::other("reminder owner missing"))?;
    let command = provider_queue
        .back_mut()
        .ok_or_else(|| io::Error::other("reminder input missing"))?;
    *command = ProviderInput::Reminder {
        request: current.owner.turn(),
        session: provider_session,
        privacy_generation: snapshot.microphone_generation(),
        text: offer.text.clone(),
        late: offer.late,
        late_by_ms: offer.late_by_ms,
    };
    current.reminder = Some(reminders.admitted(
        offer,
        current.owner,
        snapshot.microphone_generation(),
        provider_session,
    ));
    Ok(snapshot)
}

/// A turn held as unclear, to be asked again once it is over (#926).
#[derive(Debug, Default)]
struct AskAgainState {
    /// The turn and when it was judged unclear.
    wanted: Option<(u64, u64)>,
    /// What last held a due asking back, as recorded.
    held: Option<Vec<&'static str>>,
    /// Whether what keeps the unclear turn open was said.
    open_said: bool,
    /// Whether the lamp hums its "hm?" for it rather than ask the model: a
    /// misheard cut-in, with `LAMP_MISHEARD_HUM` ([`crate::misheard_hum`]).
    hum: bool,
}

impl AskAgainState {
    /// `request` was judged unclear. Taken only if it is the turn in hand
    /// (`current`): one cancelled first is never asked about.
    fn unclear(&mut self, request: u64, current: Option<u64>, at_us: u64, hum: bool) -> bool {
        if current != Some(request) {
            return false;
        }
        *self = Self {
            wanted: Some((request, at_us)),
            hum,
            ..Self::default()
        };
        true
    }

    /// Its turn was cancelled (a hand's Stop, the mute, a capture gap):
    /// nothing is asked. The turn it was for, if any.
    fn cancelled(&mut self) -> Option<u64> {
        let turn = self.wanted.map(|(turn, _)| turn);
        *self = Self::default();
        turn
    }

    fn wanted(&self) -> Option<(u64, u64)> {
        self.wanted
    }

    /// Asked, or dropped.
    fn done(&mut self) {
        *self = Self::default();
    }
}

/// The lamp's own "Sorry?" turn (#926), watched to its end (#1417): one that
/// came back with nothing said owes the person the lamp's "hm?", since
/// nothing else would follow it. Only that turn: one that was the room's
/// talk stays silent, and one the model answered owes nothing.
#[derive(Debug, Default)]
struct UnclearHum {
    /// The "Sorry?" turn in hand.
    asked: Option<u64>,
    /// When its empty end owed a hum.
    owed: Option<u64>,
    /// What last held the owed hum back (`may_hum`'s word).
    held: Option<&'static str>,
    /// The misheard cut-in it is owed for, and when that was held, if it
    /// is owed for one (`LAMP_MISHEARD_HUM`, #828).
    misheard: Option<(u64, u64)>,
}

/// What becomes of an owed hum, as the loop sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HumDue {
    /// In time and nobody has spoken: it may play, if the lamp is quiet.
    Now,
    /// Too late, or someone spoke first.
    Drop(&'static str),
}

/// A "hm?" later than this after the empty "Sorry?" is about nothing.
const UNCLEAR_HUM_WITHIN_US: u64 = 2_000_000;

impl UnclearHum {
    /// The lamp asked "Sorry?" in `turn`.
    fn asked(&mut self, turn: u64) {
        *self = Self {
            asked: Some(turn),
            ..Self::default()
        };
    }

    /// `turn` finished, with nothing said if `said_nothing`. Whether a hum is
    /// owed now: only for the "Sorry?" turn, and only when it said nothing.
    /// Any finish ends the watch: one turn is in hand at a time.
    fn finished(&mut self, turn: u64, said_nothing: bool, at_us: u64) -> bool {
        let owes = self.asked.take() == Some(turn) && said_nothing;
        if owes {
            self.owed = Some(at_us);
            self.held = None;
        }
        owes
    }

    fn owed(&self) -> Option<u64> {
        self.owed
    }

    /// A misheard cut-in, `turn`, held at `held_us`: the lamp owes its "hm?"
    /// from `at_us`, in place of a model turn (#828).
    fn owe_misheard(&mut self, turn: u64, held_us: u64, at_us: u64) {
        *self = Self {
            owed: Some(at_us),
            misheard: Some((turn, held_us)),
            ..Self::default()
        };
    }

    fn misheard(&self) -> Option<(u64, u64)> {
        self.misheard
    }

    /// The owed hum at `at_us`, with `current` the turn in hand: any turn
    /// begun since is someone speaking first.
    fn due(&self, current: Option<u64>, at_us: u64) -> HumDue {
        match self.owed {
            _ if current.is_some() => HumDue::Drop("another_turn_began"),
            Some(owed) if at_us.saturating_sub(owed) > UNCLEAR_HUM_WITHIN_US => {
                HumDue::Drop("late")
            }
            _ => HumDue::Now,
        }
    }

    /// Held back this pass by `why`, tried again next.
    fn hold(&mut self, why: &'static str) {
        self.held = Some(why);
    }

    fn held(&self) -> Option<&'static str> {
        self.held
    }

    /// The person's Stop (or the mute, a gap): an owed hum is dropped, and
    /// so is the watch. When the dropped one had been owed, if one was.
    fn cancelled(&mut self) -> Option<u64> {
        let owed = self.owed;
        *self = Self::default();
        owed
    }

    /// Played, or dropped.
    fn done(&mut self) {
        self.owed = None;
        self.held = None;
        self.misheard = None;
    }
}

/// What becomes of a turn to be asked again (#926), as the loop sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AskAgain {
    /// Its own turn is not over yet.
    Wait,
    /// Someone spoke first, or it is too late to ask about it.
    Drop,
    /// Ask now, if the lamp is at rest.
    Due,
}

/// A "Sorry?" more than this after the unclear turn is about nothing.
const ASK_AGAIN_WITHIN_US: u64 = 5_000_000;

/// The unclear turn may stay current after its answer was dropped (on
/// lamp-4ace a cut-in's turn stayed 15 s); past this it is let go unasked.
const ASK_AGAIN_TURN_LONGEST_US: u64 = 10_000_000;

fn ask_again_now((turn, asked_us): (u64, u64), current: Option<u64>, clock: u64) -> AskAgain {
    match current {
        Some(current)
            if current == turn && clock.saturating_sub(asked_us) > ASK_AGAIN_TURN_LONGEST_US =>
        {
            AskAgain::Drop
        }
        Some(current) if current == turn => AskAgain::Wait,
        Some(_) => AskAgain::Drop,
        None if clock.saturating_sub(asked_us) > ASK_AGAIN_WITHIN_US => AskAgain::Drop,
        None => AskAgain::Due,
    }
}

/// The written note of the fixture's timed turn; the fixture reads no text.
const FIXTURE_AT_NOTE: &str = "(diagnostic: the fixture's timed start)";

/// `directed-fixture --fixture-at-ms` (diagnostic only): once `due_us` has
/// come and the lamp is at rest (`session`, the provider session to ask),
/// the fixture's own written turn is admitted, with no heard turn needed.
/// A turn admitted before then is handled as today, with no fixture audio;
/// one still open at the due time holds the start until it is over. Once.
#[allow(clippy::too_many_arguments)]
fn start_timed_fixture(
    due_us: &mut Option<u64>,
    clock: u64,
    session: Option<u64>,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    provider_queue: &mut VecDeque<ProviderInput>,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Option<Snapshot>> {
    let Some(due) = *due_us else {
        return Ok(None);
    };
    let Some(session) = session.filter(|_| clock >= due) else {
        return Ok(None);
    };
    *due_us = None;
    record(
        trace,
        json!({"kind":"fixture_timed_start","at_us":clock,"due_us":due,"provider_session":session}),
    )?;
    commit_written_turn(
        owner,
        reply,
        provider_queue,
        FIXTURE_AT_NOTE,
        false,
        "fixture_at_ms",
        session,
        playout_start,
        trace,
        publish,
    )
    .map(Some)
}

/// Why an add-on's quoted say may not be taken by the provider in `role`,
/// if so (#2530 part 2): only the second route asks a turn with no
/// functions declared, so only it takes one. Any other say is taken.
fn quoted_refused(quoted: bool, role: &str) -> Option<&'static str> {
    (quoted && role != "second").then_some("add-on says need the second route")
}

#[allow(clippy::too_many_arguments)]
fn commit_written_turn(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    provider_queue: &mut VecDeque<ProviderInput>,
    note: &str,
    look: bool,
    basis: &'static str,
    provider_session: u64,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    let turn = owner.admit(now(), AdmittedInput::NewTurn)?;
    own_turn().store(turn.turn(), std::sync::atomic::Ordering::Release);
    let plan = owner.plan_output(now(), turn, OutputKind::Speech, None)?;
    let mut snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    let admitted_at_us = monotonic_us();
    trace
        .receipts
        .admitted(turn, snapshot.issued_at().as_micros())?;
    record(
        trace,
        json!({"kind":"input_admitted","owner":turn,"turn":turn.turn(),"at_us":admitted_at_us,"authority_issued_at_us":snapshot.issued_at().as_micros(),
            "admission_basis":basis,"provider_session":provider_session,
            "boundary":"no sound was heard: a written note stands in for speech"}),
    )?;
    provider_queue.push_back(ProviderInput::Written {
        request: turn.turn(),
        session: provider_session,
        privacy_generation: snapshot.microphone_generation(),
        text: note.to_owned(),
        look,
        search: false,
        quoted: false,
    });
    let current = Reply {
        owner: turn,
        cloud: CloudRequest::new(provider_session)?,
        local_input_ended: true,
        plan,
        playback: None,
        pcm: VecDeque::new(),
        generation_done: false,
        provider_idle: false,
        input_sequence: 0,
        had_audio: false,
        voice_level: 0,
        told_stop: None,
        final_chunk: None,
        playback_gaps: 0,
        notice: NoticeDelivery::default(),
        media: Default::default(),
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
        handed: 0,
        sounded: false,
    };
    owner.input_ended(now(), current.owner)?;
    snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    *reply = Some(current);
    Ok(snapshot)
}

/// The first sign of the lamp's name as it is said (`name/rising`), for a
/// call's wait (#782): an utterance that is only the call so far waits for
/// the rest, and the record says so. When the name began, if it was heard.
fn hear_the_call(
    rising: &mut crate::bus_out::NameIn,
    clock: u64,
    detector: &mut TurnDetector,
    turn: Option<u64>,
    trace: &mut SessionState,
) -> Result<Option<u64>> {
    let Some((began, score)) = rising.newest_scored(clock) else {
        return Ok(None);
    };
    if detector.name_heard(began) {
        record(
            trace,
            json!({"kind":"call_wait","at_us":clock,"name_began_us":began,"score":score,"turn":turn}),
        )?;
    }
    Ok(Some(began))
}

/// How much of `current` was played: its request and the samples handed to
/// the speaker (`LAMP_SAID_AS_HEARD`).
fn heard_of(current: &Reply) -> crate::said_as_heard::HeardOfCut {
    crate::said_as_heard::HeardOfCut {
        request: current.owner.turn(),
        samples: current.handed,
    }
}

/// With `on` (`LAMP_SAID_AS_HEARD`), what a Stop that cut `cancelled` keeps
/// for the next line on its session: how much of it was played, and that
/// session.
fn stop_cut_of(
    on: bool,
    cancelled: Option<&Reply>,
) -> Option<(crate::said_as_heard::HeardOfCut, u64)> {
    cancelled
        .filter(|_| on)
        .map(|current| (heard_of(current), current.cloud.session))
}

/// With `on` (`LAMP_SAID_AS_HEARD`), how much was played of the reply a line
/// admitted on `session` cuts, or else of the one a Stop cut on that session
/// (`stop_cut`). The Stop's is taken either way: it tells only the next line.
fn heard_of_cut(
    on: bool,
    reply: Option<&Reply>,
    stop_cut: &mut Option<(crate::said_as_heard::HeardOfCut, u64)>,
    session: u64,
) -> Option<crate::said_as_heard::HeardOfCut> {
    let stopped = stop_cut
        .take()
        .filter(|(_, of)| *of == session)
        .map(|(cut, _)| cut);
    on.then(|| reply.map(heard_of).or(stopped)).flatten()
}

/// Whether the lamp's own answer could be heard: the reply in hand had sound
/// or is playing, not only in hand and silent while it waits for an answer
/// (#828, the reviewer on #1291).
fn lamps_voice_heard(reply: Option<&Reply>) -> bool {
    reply.is_some_and(|current| current.had_audio || current.playback.is_some())
}

#[allow(clippy::too_many_arguments)]
fn commit_admitted_input(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    speaker_pending: &mut Option<PendingSpeaker>,
    provider_queue: &mut VecDeque<ProviderInput>,
    accepted: Accepted,
    provider_session: u64,
    continues: bool,
    // It goes on from a line that ended before its name, whose words the
    // second route asks with its own (`LAMP_SECOND_OPEN_NAME`).
    goes_on_from_name: bool,
    // It takes over from the lamp's own answer: its End, if queued here, says so (#926).
    cut_in: bool,
    // The Stop this request tells of, when it is the first after one
    // (#829): when it came, and the session it cut.
    after_stop: Option<(u64, u64)>,
    // How much was played of the reply it, or the Stop before it, cut
    // (`LAMP_SAID_AS_HEARD`).
    heard_of_cut: Option<crate::said_as_heard::HeardOfCut>,
    // Begun soon after the lamp's words finished (`crate::follows_on`).
    follows_on: bool,
    // Begun in onboarding's sentence window (#1803): its Start is followed
    // at once by `CountOnly`, before any of its sound.
    count_only: bool,
    // Begun while the walk is under way ([`sleep_not_now`]): its Start is
    // followed at once by `Onboarding`, before any of its sound.
    not_now: bool,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    let prefix = accepted.audio;
    let replacing = reply.is_some();
    let over_the_lamps_voice = lamps_voice_heard(reply.as_ref());
    let mut interrupted = take_interrupted_reminder(reply);
    cancel_reply(owner, reply, trace, "user_interrupted")?;
    let input = if replacing {
        AdmittedInput::Interruption
    } else {
        AdmittedInput::NewTurn
    };
    *speaker_pending = None;
    provider_queue.clear();
    let turn = owner.admit(now(), input)?;
    let plan = owner.plan_output(now(), turn, OutputKind::Speech, None)?;
    let mut snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    let admitted_at_us = monotonic_us();
    trace
        .receipts
        .admitted(turn, snapshot.issued_at().as_micros())?;
    let mut admitted = json!({"kind":"input_admitted","owner":turn,"turn":turn.turn(),"at_us":admitted_at_us,"authority_issued_at_us":snapshot.issued_at().as_micros(),"prefix_first_host_read_us":prefix.first().map(|p|p.captured_at_us),"candidate":accepted.candidate.id,"admission_basis":accepted.basis,"provider_session":provider_session});
    // Said only when it is so, which only `LAMP_CUT_KEEPS_NAME=on` allows.
    if continues {
        admitted["continues"] = json!(true);
    }
    if after_stop.is_some() {
        admitted["after_stop"] = json!(true);
    }
    if follows_on && after_stop.is_none() {
        admitted["follows_on"] = json!(true);
    }
    if let Some(cut) = heard_of_cut {
        admitted["heard_of_cut"] = json!({"request":cut.request,"samples":cut.samples});
    }
    if goes_on_from_name {
        admitted["goes_on_from_its_name"] = json!(true);
    }
    record(trace, admitted)?;
    provider_queue.push_back(ProviderInput::Start {
        session: provider_session,
        request: turn.turn(),
        privacy_generation: snapshot.microphone_generation(),
        // The same utterance going on after the lamp ended it (#782). The
        // model is told of a cut only after an end sent early, or here when
        // it goes on from a line that ended before its name.
        continues,
        continues_cut_off: goes_on_from_name,
        // Said with a reply in hand: no staff's note. Its voice is judged
        // unless the lamp's could be heard (`over_the_lamps_voice`, #829).
        over_the_lamp: replacing,
        // The first request after a hand or the head stopped the lamp: the
        // model is told so (#829, 16:33).
        after_stop: after_stop.is_some(),
        // Begun soon after the lamp's words finished: the model is told it
        // may follow on (`LAMP_FOLLOWS_ON=on`, #829, 09:01). Not the first
        // line after a Stop, which is told the lamp's words are over (the
        // reviewer on #1278).
        follows_on: follows_on && after_stop.is_none(),
        // Begun while the lamp's answer could be heard (#828).
        over_the_lamps_voice,
        heard_of_cut,
    });
    let marks = after_start(
        turn.turn(),
        snapshot.microphone_generation(),
        count_only,
        not_now,
    );
    let marked = marks.len();
    provider_queue.extend(marks);
    let mut current = Reply {
        owner: turn,
        cloud: CloudRequest::new(provider_session)?,
        local_input_ended: false,
        plan,
        playback: None,
        pcm: VecDeque::new(),
        generation_done: false,
        provider_idle: false,
        input_sequence: 0,
        had_audio: false,
        voice_level: 0,
        told_stop: after_stop,
        final_chunk: None,
        playback_gaps: 0,
        notice: NoticeDelivery::default(),
        media: RequestedMedia::spoken(),
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
        handed: 0,
        sounded: false,
    };
    // One candidate's audio, already bounded by the admission's hold. The
    // standard hold's prefix fits the standing bound as it always has, and
    // the marks after the Start ([`after_start`]) have slots of their own.
    let bound = PROVIDER_QUEUE_BOUND.max(prefix.len() + 2) + marked;
    for audio in prefix {
        queue_input(
            provider_queue,
            &mut current,
            snapshot.microphone_generation(),
            audio,
            bound,
        )?;
    }
    if accepted.ended {
        current.local_input_ended = true;
        owner.input_ended(now(), current.owner)?;
        queue_input_end(
            provider_queue,
            &current,
            snapshot.microphone_generation(),
            bound,
            false,
            cut_in,
        )?;
        snapshot = owner.snapshot(now())?;
        publish(snapshot)?;
    }
    *reply = Some(current);
    // The old owner is revoked, its queues cleared, and the replacement
    // authority successfully published. Only this known interruption retries.
    if let Some(receipt) = &mut interrupted {
        receipt.interrupted_after_owner_fenced();
    }
    Ok(snapshot)
}

/// Someone spoke while the internet could not be reached (#830,
/// `LAMP_OFFLINE_VOICE=on`): the utterance is admitted as a turn of its own,
/// listened to and ended as any other, so the ring can show the person was
/// heard; but its request is failed locally from the start and nothing of it
/// goes to the provider (`queue_input` and `queue_input_end` send nothing for
/// it). What it may say is the offline line, chosen when it has settled
/// (`select_failure_notice`), through the same speaker path and permits.
#[allow(clippy::too_many_arguments)]
fn commit_offline_input(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    speaker_pending: &mut Option<PendingSpeaker>,
    provider_queue: &mut VecDeque<ProviderInput>,
    accepted: Accepted,
    playout_start: PlayoutStart,
    service: bool,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    let replacing = reply.is_some();
    let mut interrupted = take_interrupted_reminder(reply);
    cancel_reply(owner, reply, trace, "user_interrupted")?;
    let input = if replacing {
        AdmittedInput::Interruption
    } else {
        AdmittedInput::NewTurn
    };
    *speaker_pending = None;
    provider_queue.clear();
    let turn = owner.admit(now(), input)?;
    let plan = owner.plan_output(now(), turn, OutputKind::Speech, None)?;
    let mut snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    trace
        .receipts
        .admitted(turn, snapshot.issued_at().as_micros())?;
    record(
        trace,
        json!({"kind":"input_admitted","owner":turn,"turn":turn.turn(),"at_us":monotonic_us(),
            "authority_issued_at_us":snapshot.issued_at().as_micros(),
            "prefix_first_host_read_us":accepted.audio.first().map(|p|p.captured_at_us),
            "candidate":accepted.candidate.id,"admission_basis":accepted.basis,
            "provider_session":null,"local_only":true,
            "boundary":if service {"the voice service cannot be reached: listened to, never sent"}
                else {"the internet cannot be reached: listened to, never sent"}}),
    )?;
    let mut current = Reply {
        owner: turn,
        cloud: CloudRequest::local_only(),
        local_input_ended: false,
        plan,
        playback: None,
        pcm: VecDeque::new(),
        generation_done: true,
        provider_idle: true,
        input_sequence: 0,
        had_audio: false,
        voice_level: 0,
        told_stop: None,
        final_chunk: None,
        playback_gaps: 0,
        notice: NoticeDelivery::default(),
        media: Default::default(),
        local_only: Some(notices::LocalOnly {
            speech: false,
            back: false,
            service,
        }),
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
        handed: 0,
        sounded: false,
    };
    current.input_sequence = accepted.audio.len() as u64;
    if accepted.ended {
        current.local_input_ended = true;
        owner.input_ended(now(), current.owner)?;
        snapshot = owner.snapshot(now())?;
        publish(snapshot)?;
    }
    *reply = Some(current);
    // The old owner is revoked, its queues cleared, and the replacement
    // authority successfully published. Only this known interruption retries.
    if let Some(receipt) = &mut interrupted {
        receipt.interrupted_after_owner_fenced();
    }
    Ok(snapshot)
}

/// Whether the lamp speaks and shows itself as offline now (#830): only with
/// `LAMP_OFFLINE_VOICE=on`, and only once the link has been down for the
/// grace the bus gives it (`lamp/online`), so a session renewing itself is
/// not an outage. Without the switch, never: every turn is as before.
/// With `LAMP_REACH_PROBE=on` and no verdict from the probe, how long the
/// link is down before the voice takes it for no internet all the same: the
/// link's grace, the probe's bound and two of health's report periods, so a
/// probe that answers always comes first (El on #1247), and a lamp whose
/// `lamp-health` does not run is still said offline.
pub const REACH_WAIT_US: u64 = crate::bus_out::LINK_GRACE_US + 5_000_000;

/// Whether the lamp is offline for its words and its light (#830): with the
/// switch, the link down past its grace. With the probe on (`reach` is
/// `Some`), only on its verdict: the internet answering is never offline,
/// the internet not answering is, and with no verdict yet only after
/// [`REACH_WAIT_US`].
fn offline_now(
    switch: bool,
    link_down_since: Option<u64>,
    clock: u64,
    reach: Option<Option<bool>>,
) -> bool {
    let Some(down_for) = link_down_since.map(|since| clock.saturating_sub(since)) else {
        return false;
    };
    switch
        && down_for >= crate::bus_out::LINK_GRACE_US
        && match reach {
            None | Some(Some(false)) => true,
            Some(Some(true)) => false,
            Some(None) => down_for >= REACH_WAIT_US,
        }
}

/// Whether the voice service alone is down for the lamp's words and its
/// light (#830, `LAMP_SERVICE_DOWN_LINE`): with the switch, the link down
/// past its grace while the probe finds the internet answering. No verdict,
/// or the internet not answering, is not this ([`offline_now`] has those).
fn service_down_now(
    switch: bool,
    link_down_since: Option<u64>,
    clock: u64,
    reach: Option<Option<bool>>,
) -> bool {
    switch
        && link_down_since
            .is_some_and(|since| clock.saturating_sub(since) >= crate::bus_out::LINK_GRACE_US)
        && reach == Some(Some(true))
}

/// `on` for the voice to take its link down for no internet only while
/// `lamp/online` does not say the internet answers (`LAMP_REACH_PROBE`,
/// #830). With it, a link down because the relay is, while
/// `lamp-health --reach-probe on` finds the internet answering, gives no
/// offline line and no offline breath: it is the service's trouble, not
/// the home's internet. Only the second route answers then
/// (`LAMP_VOICE_SECOND_CONFIG`, its down line with `LAMP_SECOND_DOWN_LINE`);
/// without it, speech is dropped without a word (`accepted_without_cloud`),
/// so set this where the second route is. Off unless set.
pub const REACH_PROBE_ENVIRONMENT: &str = "LAMP_REACH_PROBE";

/// The probe's verdict in the kept `lamp/online`: fresh, well formed and
/// from `reach` (`lamp-health`'s probe), whether the internet answers. A
/// stale value, one from the link, or none, is no verdict.
fn reach_verdict_in(message: Option<lamp_bus::Message>) -> Option<bool> {
    message
        .filter(|message| !message.expired)
        .and_then(|message| message.parse::<lamp_msgs::Online>().ok())
        .filter(|online| online.check().is_ok() && online.from == "reach")
        .and_then(|online| online.online)
}

/// The probe's verdict, asked while the link is down, at most once a
/// second, never waiting longer than a file read. `None` without the switch
/// (the link alone decides, as before); `Some(None)` with it and no verdict.
type ReachVerdict = Box<dyn FnMut(u64) -> Option<Option<bool>>>;

fn reach_verdict(on: bool, bus: io::Result<lamp_bus::Bus>) -> ReachVerdict {
    if !on {
        return Box::new(|_| None);
    }
    let bus = bus.ok();
    let mut last: Option<(u64, Option<bool>)> = None;
    Box::new(move |clock| {
        let Some(bus) = bus.as_ref() else {
            return Some(None);
        };
        if let Some((at, verdict)) = last
            && clock.saturating_sub(at) < 1_000_000
        {
            return Some(verdict);
        }
        let verdict = reach_verdict_in(bus.latest(lamp_msgs::Online::TOPIC).ok().flatten());
        last = Some((clock, verdict));
        Some(verdict)
    })
}

/// The link is back after an outage in which the lamp said it could not
/// reach the internet (#830): a turn the lamp takes for itself, with no
/// input, to say "I'm back online." through the same speaker path and
/// permits. Its request, as the offline turn's, is never sent.
fn commit_back_online(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    let turn = owner.admit(now(), AdmittedInput::NewTurn)?;
    let plan = owner.plan_output(now(), turn, OutputKind::Speech, None)?;
    let mut snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    trace
        .receipts
        .admitted(turn, snapshot.issued_at().as_micros())?;
    record(
        trace,
        json!({"kind":"input_admitted","owner":turn,"turn":turn.turn(),"at_us":monotonic_us(),
            "authority_issued_at_us":snapshot.issued_at().as_micros(),
            "admission_basis":"back_online","provider_session":null,"local_only":true,
            "boundary":"no sound was heard: the lamp says the internet is back"}),
    )?;
    let current = Reply {
        owner: turn,
        cloud: CloudRequest::local_only(),
        local_input_ended: true,
        plan,
        playback: None,
        pcm: VecDeque::new(),
        generation_done: true,
        provider_idle: true,
        input_sequence: 0,
        had_audio: false,
        voice_level: 0,
        told_stop: None,
        final_chunk: None,
        playback_gaps: 0,
        notice: NoticeDelivery::default(),
        media: Default::default(),
        local_only: Some(notices::LocalOnly {
            speech: true,
            back: true,
            service: false,
        }),
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
        handed: 0,
        sounded: false,
    };
    owner.input_ended(now(), current.owner)?;
    snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    *reply = Some(current);
    Ok(snapshot)
}

/// Someone spoke again after the end was sent early and before the lamp was
/// sure they had finished. The request is given up exactly as a continued
/// utterance is in `commit_admitted_input`: cancelled as `user_interrupted`
/// and its successor admitted at once, with no authority published between
/// the two, which the provider worker reads as a continuation (with
/// `--provider-continuation kept-start` its start then waits for the given-up
/// request's answer to begin, and stops it). Nothing of that answer was
/// played: the lamp's authority never stopped listening. The blocks heard
/// since the end are the successor's opening, and the utterance goes on in
/// the detector, so its further blocks come as the admitted stream's.
#[allow(clippy::too_many_arguments)]
fn continue_after_early_end(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    speaker_pending: &mut Option<PendingSpeaker>,
    provider_queue: &mut VecDeque<ProviderInput>,
    opening: Vec<ObservedAudio>,
    provider_session: u64,
    cut_keeps_name: bool,
    // The sentence going on (#1803): count-only, as its first part was.
    count_only: bool,
    // Going on while the walk is under way ([`sleep_not_now`]).
    not_now: bool,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    let mut interrupted = take_interrupted_reminder(reply);
    cancel_reply(owner, reply, trace, "user_interrupted")?;
    *speaker_pending = None;
    provider_queue.clear();
    let turn = owner.admit(now(), AdmittedInput::Interruption)?;
    let plan = owner.plan_output(now(), turn, OutputKind::Speech, None)?;
    let snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    let admitted_at_us = monotonic_us();
    trace
        .receipts
        .admitted(turn, snapshot.issued_at().as_micros())?;
    record(
        trace,
        json!({"kind":"input_admitted","owner":turn,"turn":turn.turn(),"at_us":admitted_at_us,"authority_issued_at_us":snapshot.issued_at().as_micros(),"prefix_first_host_read_us":opening.first().map(|p|p.captured_at_us),"candidate":null,"admission_basis":"continued_after_early_end","provider_session":provider_session}),
    )?;
    provider_queue.push_back(ProviderInput::Start {
        session: provider_session,
        request: turn.turn(),
        privacy_generation: snapshot.microphone_generation(),
        // The same utterance going on: with the switch, the busy-room rule
        // keeps what it knew of the first part (#782).
        continues: cut_keeps_name,
        // The model is told this goes on from the request given up.
        continues_cut_off: true,
        // Nothing of the answer given up was played.
        over_the_lamp: false,
        after_stop: false,
        // A first part that followed on was told so beside its own sound,
        // in this same session (a continuation never crosses one), so the
        // model has it; marked again, it would be told twice (El on #1278).
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    });
    let marks = after_start(
        turn.turn(),
        snapshot.microphone_generation(),
        count_only,
        not_now,
    );
    let marked = marks.len();
    provider_queue.extend(marks);
    let mut current = Reply {
        owner: turn,
        cloud: CloudRequest::new(provider_session)?,
        local_input_ended: false,
        plan,
        playback: None,
        pcm: VecDeque::new(),
        generation_done: false,
        provider_idle: false,
        input_sequence: 0,
        had_audio: false,
        voice_level: 0,
        told_stop: None,
        final_chunk: None,
        playback_gaps: 0,
        notice: NoticeDelivery::default(),
        media: RequestedMedia::spoken(),
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
        handed: 0,
        sounded: false,
    };
    // At most a pre-roll, as a new candidate's opening is.
    let bound = PROVIDER_QUEUE_BOUND.max(opening.len() + 2) + marked;
    for audio in opening {
        queue_input(
            provider_queue,
            &mut current,
            snapshot.microphone_generation(),
            audio,
            bound,
        )?;
    }
    *reply = Some(current);
    // The old owner is revoked, its queues cleared, and the replacement
    // authority successfully published. Only this known interruption retries.
    if let Some(receipt) = &mut interrupted {
        receipt.interrupted_after_owner_fenced();
    }
    Ok(snapshot)
}

/// What a record says of an end sent early, by the time `at_us`.
fn early_end_json(sent: &early_end::Sent, at_us: u64) -> Value {
    json!({"at_us":sent.at_us,"last_block_host_read_us":sent.last_block_host_read_us,
        "before_this_ms":sent.ago_us(at_us) / 1_000})
}

/// Presentation failure disables only the optional ring. Cleanup is observed
/// in bounded slices; no wait or hardware write runs on this path, and a
/// restart after a fault runs on a thread of its own (#1620).
fn service_optional_ring(
    rig: &mut Rig,
    choreography: &mut Option<RingChoreographer>,
    owner: &mut Controller,
    trace: &mut SessionState,
    ring_trace: &mut RingTrace,
) -> Result<()> {
    if rig.ring.is_active()
        && let Err(error) = service_ring(
            rig.ring.active_mut(),
            choreography.as_mut(),
            owner,
            trace,
            ring_trace,
        )
        && !resynced_after(choreography, ring_trace, &*error, trace)?
    {
        rig.ring.disable(error);
    }
    if !rig.ring.is_active() {
        *choreography = None;
    }
    rig.ring.poll_cleanup(trace)?;
    // A ring started again after a fault (#1620) begins afresh: a new
    // choreography, and no request of the old one outstanding.
    if rig.ring.restart_tick(trace)? {
        *choreography = rig
            .ring_channel_ceiling
            .map(RingChoreographer::new)
            .transpose()?;
        *ring_trace = RingTrace::default();
    }
    Ok(())
}

/// After a ring fault: whether the choreography started afresh. A late write
/// or receipt, a `TimedOut` fault, comes of a loop or ring held up a moment
/// (#1178, the lamp's midnight log jobs), and the lamp would otherwise listen
/// with no light for the rest of the serve cycle; a few such faults start it
/// afresh ([`RingChoreographer::resync`]). Any other fault, or one past the
/// budget, disables the ring as before.
fn resynced_after(
    choreography: &mut Option<RingChoreographer>,
    ring_trace: &mut RingTrace,
    error: &(dyn std::error::Error + Send + Sync + 'static),
    trace: &mut SessionState,
) -> Result<bool> {
    let late = error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::TimedOut);
    let at = now();
    if !late
        || !choreography
            .as_mut()
            .is_some_and(|policy| policy.resync(at))
    {
        return Ok(false);
    }
    ring_trace.resync(
        choreography
            .as_ref()
            .is_some_and(RingChoreographer::holding),
    );
    eprintln!(
        "lamp-voice: the ring's feedback came late ({error}); its choreography starts afresh"
    );
    record(
        trace,
        json!({"kind":"ring_resynced","at_us":monotonic_us(),"error":error.to_string()}),
    )?;
    Ok(true)
}

/// All hardware I/O remains in the child. This path has one outstanding frame,
/// bounded nonblocking socket work, and no retry wait that can block capture.
/// Worker reports are evidence only; they never change conversational ownership.
fn service_ring(
    worker: Option<&mut Worker>,
    choreography: Option<&mut RingChoreographer>,
    owner: &mut Controller,
    trace: &mut SessionState,
    ring_trace: &mut RingTrace,
) -> Result<()> {
    service_ring_with_clock(worker, choreography, owner, trace, ring_trace, now)
}

fn service_ring_with_clock(
    worker: Option<&mut Worker>,
    choreography: Option<&mut RingChoreographer>,
    owner: &mut Controller,
    trace: &mut SessionState,
    ring_trace: &mut RingTrace,
    mut clock: impl FnMut() -> MonoTime,
) -> Result<()> {
    let (worker, choreography) = match (worker, choreography) {
        (None, None) => return Ok(()),
        (Some(worker), Some(choreography)) => (worker, choreography),
        _ => return Err(io::Error::other("ring worker and policy configuration disagree").into()),
    };
    let result: Result<()> = (|| {
        for _ in 0..16 {
            match worker.channels.control.receive::<WorkerEvent>()? {
                Some(WorkerEvent::Ring { report }) => {
                    let validation = owner
                        .snapshot(clock())
                        .map_err(io::Error::other)
                        .and_then(|current| choreography.feedback(&report, current, clock()));
                    let error = validation.as_ref().err().map(ToString::to_string);
                    ring_trace.feedback(report, clock().as_micros(), error.as_deref(), trace)?;
                    validation?;
                }
                Some(WorkerEvent::Fault { code }) => return Err(io::Error::other(code).into()),
                Some(_) => return Err(io::Error::other("unexpected ring worker event").into()),
                None => break,
            }
        }
        if let Some(frame) = choreography.prepare(owner, clock())? {
            // The data frame can never install authority or extend its own lease.
            // The worker rereads this priority path after receiving the data frame.
            worker.channels.control.send(Control::Authority {
                snapshot: frame.snapshot,
            })?;
            worker.channels.data.send(frame)?;
            ring_trace.requested(frame, trace)?;
        }
        Ok(())
    })();
    if let Err(error) = &result {
        // Include a submitted renewal's original identity even when the next
        // receipt/transport faults. Trace exhaustion still retains run_end.
        let _ = record(
            trace,
            json!({"kind":"ring_service_fault",
            "at_us":clock().as_micros(), "error":error.to_string(),
            "pending_frame":choreography.pending()}),
        );
    }
    result
}

/// Called only after this tick's retained input and cancellation work. Keeping
/// one outstanding chunk bounds handoff. The optional reply-start gate makes
/// one bounded queue decision and never waits inside this loop.
#[allow(clippy::too_many_arguments)]
fn dispatch_speaker_chunk(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    channel: &mut Channel,
    pending: &mut Option<PendingSpeaker>,
    sequence: &mut u64,
    snapshot: &mut Snapshot,
    playback_mask: &mut PlaybackMask,
) -> Result<Option<(TurnOwner, Value)>> {
    if pending.is_some() {
        return Ok(None);
    }
    // Duplex: someone is being heard over the lamp. Keep the reply and hand
    // nothing more to the speaker until that is settled; it resumes or is
    // replaced through the ordinary cancellation path.
    if playback_mask.yielded(monotonic_us()) {
        return Ok(None);
    }
    let Some(current) = reply.as_mut() else {
        return Ok(None);
    };
    // Recheck at the final handoff as well as the cancellation pass. A guard
    // revoked between them cannot enqueue a first or subsequent audio chunk.
    if current
        .say
        .as_ref()
        .is_some_and(|ticket| !ticket.authority_valid(monotonic_us()))
    {
        return Ok(None);
    }
    let origin = current.notice.origin();
    // Local notices never forge a provider Started receipt. Provider PCM keeps
    // its exact existing proof; both sources require the real local endpoint.
    if !current.local_input_ended || (origin == SpeechOrigin::Provider && !current.cloud.started) {
        return Ok(None);
    }
    if origin == SpeechOrigin::Provider && !current.may_start(monotonic_us()) {
        return Ok(None);
    }
    if origin.local() && (!current.notice.has_chunk() || current.final_chunk.is_some()) {
        return Ok(None);
    }
    // Listening gaps (#828): at a sentence end of a long reply the next
    // words wait until the lamp has been quiet a while, the speaker writing
    // zeros meanwhile as for a provider pause. Off unless set.
    if origin == SpeechOrigin::Provider
        && current.has_speaker_chunk()
        && let Some(gaps) = playback_mask.listen_gaps()
    {
        let next: Vec<i16> = current.pcm.iter().take(240).copied().collect();
        if gaps.holds(current.owner, &next, monotonic_us()) {
            return Ok(None);
        }
    }
    let permit_at = now();
    let lease = if origin == SpeechOrigin::RequestedAudio {
        let Some(lease) = current.requested_audio_permit_us(permit_at.as_micros()) else {
            return Ok(None);
        };
        lease
    } else {
        SPEAKER_PERMIT_LEASE_US
    };
    let permit = owner.issue_output(permit_at, current.plan, lease)?;
    let (samples, final_chunk, first_notice) = if origin.local() {
        current.notice.reserve_chunk(monotonic_us())?
    } else {
        let Some((samples, last)) = current.next_speaker_chunk() else {
            return Ok(None);
        };
        (samples, last, false)
    };
    *snapshot = owner.snapshot(now())?;
    *sequence = sequence
        .checked_add(1)
        .ok_or_else(|| io::Error::other("speaker counter exhausted"))?;
    let dispatch_at = monotonic_us();
    if origin == SpeechOrigin::Provider
        && let Some(gaps) = playback_mask.listen_gaps()
    {
        gaps.sent(permit.owner(), &samples, dispatch_at);
    }
    channel.send(SpeakerChunk {
        snapshot: *snapshot,
        permit,
        chunk_sequence: *sequence,
        end_of_speech: final_chunk,
        samples,
    })?;
    playback_mask.dispatched(permit.owner(), *sequence, dispatch_at, 240)?;
    *pending = Some(PendingSpeaker {
        origin,
        sequence: *sequence,
        permit,
        accepted: 0,
        final_chunk,
        report_due_us: dispatch_at + SPEAKER_PERMIT_LEASE_US + SPEAKER_REPORT_AGE_US,
    });
    Ok(first_notice.then(|| (current.owner, current.notice.evidence())))
}

/// What follows a request's Start at once, before any of its sound: the
/// sentence's `CountOnly` (#1803), then `Onboarding` while the walk is
/// under way, so a `go_to_sleep` the request calls is answered not now
/// however soon the model calls it, never the line, then a sleep refused
/// (Sixth's P3 on #3835).
fn after_start(
    request: u64,
    privacy_generation: u64,
    count_only: bool,
    not_now: bool,
) -> Vec<ProviderInput> {
    let mut marks = Vec::new();
    if count_only {
        marks.push(ProviderInput::CountOnly {
            request,
            privacy_generation,
        });
    }
    if not_now {
        marks.push(ProviderInput::Onboarding {
            request,
            privacy_generation,
        });
    }
    marks
}

/// Whether a request begun now is in the walk for `go_to_sleep`
/// (`LAMP_SLEEP_ASKED=on`, #3748): the hand controls' own onboarding bit,
/// or the walk under way.
fn sleep_not_now(
    switched_on: bool,
    controls: Option<&HandControls>,
    onboarding: Option<&crate::onboarding::Onboarding>,
) -> bool {
    switched_on
        && (controls.is_some_and(HandControls::onboarding)
            || onboarding
                .and_then(crate::onboarding::Onboarding::under_way)
                .unwrap_or(false))
}

/// Queue threshold before appending PCM: a start, the largest accepted prefix
/// and what arrives while it drains. One terminal End may follow at this bound;
/// total command capacity is 65 in standard mode or 115 in duplex mode.
const PROVIDER_QUEUE_BOUND: usize = 64;

/// A longer hold accepts a longer prefix; the queue keeps the same spare
/// beyond it. The standard hold gives [`PROVIDER_QUEUE_BOUND`] itself.
fn provider_queue_bound(hold: Hold) -> usize {
    PROVIDER_QUEUE_BOUND
        + hold
            .decision_blocks
            .saturating_sub(Hold::STANDARD.decision_blocks)
        + hold.lead_blocks
}

fn queue_input(
    queue: &mut VecDeque<ProviderInput>,
    reply: &mut Reply,
    privacy_generation: u64,
    audio: ObservedAudio,
    bound: usize,
) -> Result<()> {
    // A local-only turn (#830) sends nothing: its request never went out.
    if reply.local_only.is_some() {
        reply.input_sequence += 1;
        return Ok(());
    }
    if queue.len() >= bound {
        return Err(io::Error::other("microphone to provider queue exceeded bound").into());
    }
    reply.input_sequence += 1;
    queue.push_back(ProviderInput::Audio {
        request: reply.owner.turn(),
        privacy_generation,
        sequence: reply.input_sequence,
        read_completed_at_us: audio.captured_at_us,
        samples: audio.samples.to_vec(),
    });
    Ok(())
}

/// The request's line has gone quiet for a moment
/// (`LAMP_SECOND_EARLY_FINALIZE`): nothing for a local-only turn (#830),
/// whose request never went out, nor with the queue at its bound, since it
/// only saves time.
fn queue_input_paused(
    queue: &mut VecDeque<ProviderInput>,
    reply: &Reply,
    privacy_generation: u64,
    bound: usize,
) {
    if reply.local_only.is_some() || queue.len() >= bound {
        return;
    }
    queue.push_back(ProviderInput::Paused {
        request: reply.owner.turn(),
        privacy_generation,
    });
}

/// What tells the model that the request ending now was only the lamp's
/// name, to go just before its end: when its end is a call's wait that ran
/// out (`--call-wait-ms`, #782). Nothing for any other end, nor for a
/// local-only turn (#830), whose request never went out. Nor when `queued`
/// is already at `pcm_bound`: the end keeps the one slot past the bound it
/// has always had, and the note is left out.
fn call_note(
    decision: Option<&crate::activity::EndpointDecision>,
    request: u64,
    local_only: bool,
    privacy_generation: u64,
    queued: usize,
    pcm_bound: usize,
) -> Option<ProviderInput> {
    decision
        .and_then(|decision| decision.call_wait_ms)
        .filter(|_| !local_only && queued < pcm_bound)
        .map(|_| ProviderInput::Called {
            request,
            privacy_generation,
        })
}

/// The endpoint has one explicit slot after the PCM threshold. It cannot
/// grow an already-full terminal queue, and it does not enlarge PCM admission.
fn queue_input_end(
    queue: &mut VecDeque<ProviderInput>,
    reply: &Reply,
    privacy_generation: u64,
    pcm_bound: usize,
    sound: bool,
    cut_in: bool,
) -> Result<()> {
    if reply.local_only.is_some() {
        return Ok(());
    }
    if queue.len() > pcm_bound {
        return Err(io::Error::other("provider terminal queue exceeded bound").into());
    }
    queue.push_back(ProviderInput::End {
        request: reply.owner.turn(),
        privacy_generation,
        sound,
        cut_in,
    });
    Ok(())
}

/// Returns the epoch a stall closed, if the worker said so: its frames still
/// in flight are dropped, and the next start opens a new epoch (#784).
fn drain_capture_control(
    channel: &mut Channel,
    epoch: &mut Option<u64>,
    expected_noise_suppression: NoiseSuppression,
    trace: &mut SessionState,
) -> Result<Option<u64>> {
    let mut closed = None;
    for _ in 0..16 {
        let Some(event) = channel.receive::<WorkerEvent>()? else {
            break;
        };
        match event {
            WorkerEvent::CapturePrepared { waited, .. } => {
                // One prepare a capture worker: its start is the one said,
                // and a later start that opened at once clears it.
                trace.mic_at_start = waited;
                if let Some(waited) = waited {
                    record(
                        trace,
                        json!({"kind":"mic_waited_at_start","at_us":monotonic_us(),
                            "said":waited.plain(),"details":waited}),
                    )?;
                }
                record(
                    trace,
                    json!({"kind":"capture_prepared","at_us":monotonic_us(),"details":event}),
                )?
            }
            WorkerEvent::CaptureStarted {
                epoch: started,
                noise_suppression,
                ..
            } if epoch.is_none() => {
                if noise_suppression != expected_noise_suppression {
                    record(
                        trace,
                        json!({"kind":"capture_processing_mismatch", "at_us":monotonic_us(),
                        "expected_noise_suppression":expected_noise_suppression, "details":event}),
                    )?;
                    return Err(io::Error::other(
                        "capture processing mode differs from requested mode",
                    )
                    .into());
                }
                *epoch = Some(started);
                record(
                    trace,
                    json!({"kind":"capture_started","at_us":monotonic_us(),"details":event}),
                )?;
            }
            WorkerEvent::CaptureDspReset { .. } => record(
                trace,
                json!({"kind":"echo_reference_reset","at_us":monotonic_us(),"details":event}),
            )?,
            WorkerEvent::CaptureGap { epoch: gone, .. } if *epoch == Some(gone) => {
                record(
                    trace,
                    json!({"kind":"capture_gap_closed","at_us":monotonic_us(),"details":event}),
                )?;
                *epoch = None;
                closed = Some(gone);
            }
            WorkerEvent::ReferenceStatus { status } => {
                record(trace, reference_status_trace(&status))?
            }
            WorkerEvent::ReferenceClock { timing } => record(
                trace,
                json!({"kind":"reference_clock","at_us":monotonic_us(),"details":timing}),
            )?,
            WorkerEvent::ReferenceFault { fault } => {
                record(
                    trace,
                    json!({"kind":"reference_fault","worker":"capture","details":fault}),
                )?;
                return Err(fault.into());
            }
            WorkerEvent::Fault { code } => return Err(io::Error::other(code).into()),
            _ => return Err(io::Error::other("unexpected capture lifecycle event").into()),
        }
    }
    Ok(closed)
}

/// Frames dropped since the capture last delivered one on time.
#[derive(Default)]
struct CaptureGap {
    dropped: u64,
    /// The worker closed the microphone: the next frame on time, of a new
    /// epoch that counts from 1 again, ends a gap however short it was.
    closed: bool,
}

/// A capture gap that has just ended, for one journal line.
#[derive(Debug, PartialEq)]
struct EndedGap {
    stalled_us: u64,
    dropped: u64,
}

impl std::fmt::Display for EndedGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the microphone stalled {} ms; {} late frames dropped, listening again",
            self.stalled_us / 1_000,
            self.dropped
        )
    }
}

impl CaptureGap {
    /// A frame on time arrived, read at `read_at`. If the one before it was
    /// read a lease or more earlier, or late frames were dropped since, a
    /// gap has just ended.
    fn ended(&mut self, previous: Option<u64>, read_at: u64) -> Option<EndedGap> {
        let stalled_us = previous.map_or(0, |at| read_at.saturating_sub(at));
        if stalled_us < INPUT_LEASE_US && self.dropped == 0 && !self.closed {
            return None;
        }
        self.closed = false;
        let dropped = std::mem::take(&mut self.dropped);
        Some(EndedGap {
            stalled_us,
            dropped,
        })
    }
}

fn validate_capture_frame(
    channel: &mut Channel,
    epoch: &mut Option<u64>,
    privacy_generation: u64,
    frame: &MicrophoneFrame,
    expected_noise_suppression: NoiseSuppression,
    trace: &mut SessionState,
) -> Result<Option<u64>> {
    // The worker sends its start report before its first frame, but the two
    // sockets may become readable between our control drain and data receive.
    // Re-read control after receiving the frame instead of discarding its words.
    // The same holds after a stall: Gap(n), Started(n+1) and the first frame
    // of n+1 can all arrive between two drains (#784).
    let mut closed = None;
    if epoch.is_none() || Some(frame.epoch) != *epoch {
        closed = drain_capture_control(channel, epoch, expected_noise_suppression, trace)?;
        if closed == Some(frame.epoch) {
            return Ok(closed);
        }
    }
    if Some(frame.epoch) != *epoch || frame.privacy_generation != privacy_generation {
        return Err(io::Error::other("microphone lineage changed during test").into());
    }
    if frame.timing.reference.playback_epoch == 0
        || (frame.frame_sequence == 1 && frame.timing.reference.clock_started_at_us.is_none())
    {
        return Err(
            io::Error::other("capture frame preceded certified running reference clock").into(),
        );
    }
    Ok(closed)
}

/// A change of trust in the capture worker's echo reference. It is a status,
/// not a fault: the session goes on. While the reference is untrusted the
/// frames carry no echo evidence and did not pass the canceller, so a policy
/// that masks own playback is what keeps the lamp's voice from being a turn.
fn reference_status_trace(status: &crate::wire::ReferenceStatus) -> Value {
    json!({"kind":"reference_status","at_us":monotonic_us(),"details":status,
        "boundary":"capture worker's pairing of reference and capture by count; not an acoustic measurement"})
}

fn record_input_rejection(trace: &mut SessionState, rejection: Rejection) -> Result<()> {
    record(
        trace,
        json!({"kind":"input_candidate_rejected", "candidate":rejection.candidate,
        "reason":rejection.reason, "at_us":monotonic_us()}),
    )
}

/// Retain only an otherwise live reminder across a positively admitted
/// interruption. A known provider failure can still have unretired output;
/// its normal Reply drop must keep that uncertain attempt held. Taking a
/// receipt proves no fence: the caller settles it only after cancellation,
/// queue clearing and successful authority publication, or drops it on error.
fn take_interrupted_reminder(reply: &mut Option<Reply>) -> Option<crate::reminders::Receipt> {
    let current = reply.as_mut()?;
    if matches!(current.cloud.outcome, Some(RequestOutcome::Failed { .. })) {
        return None;
    }
    current.reminder.take()
}

fn cancel_reply(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    trace: &mut SessionState,
    reason: &str,
) -> Result<()> {
    if let Some(current) = reply.take() {
        let cancel = owner.cancel_turn(now(), current.owner);
        record_cancelled_reply(current, trace, reason, monotonic_us())?;
        cancel?;
    }
    Ok(())
}

fn record_cancelled_reply(
    current: Reply,
    trace: &mut SessionState,
    reason: &str,
    cancelled_at_us: u64,
) -> Result<()> {
    let mut current = current;
    if let Some(say) = current.say.take() {
        say.answer(crate::say::Outcome::Dropped(reason.to_owned()));
    }
    if !current.news.is_empty() {
        eprintln!(
            "lamp-voice: the news ({}) was cut short ({reason}): its harness stays open",
            current.news.join(", ")
        );
    }
    finish_duplex_yield(trace, cancelled_at_us, reason, None, None)?;
    trace
        .receipts
        .cancelled(current.owner, cancelled_at_us, reason)?;
    record(
        trace,
        json!({"kind":"turn_finished","owner":current.owner,"turn":current.owner.turn(),"at_us":cancelled_at_us,"outcome":reason,"provider_audio_seen":current.had_audio,"playback_gaps":current.playback_gaps,
                "provider_session":current.cloud.session,"provider_outcome":current.cloud.outcome,"failure_notice":current.notice.evidence(),
                "start_gate":current.start_trace(),"start_deficit_us":current.need.deficit_us(),
                "start_horizon_needed_us":current.need.horizon_us(),
                "start_need_scope":"all retained reply arrivals including later generations; excludes local handoff",
                "start_need_censored":true}),
    )?;
    Ok(())
}

/// An old accepted tail can retire after the successor has started or even
/// finished. This receipt is evidence only: it cannot complete either answer or
/// borrow the newer turn's playback occurrence.
fn handle_cancelled_tail_retirement(
    reply: &Option<Reply>,
    trace: &mut SessionState,
    event: WorkerEvent,
    received_at_us: u64,
) -> Result<()> {
    let WorkerEvent::CancelledTailRetired {
        owner: affected,
        superseded_by,
        first_sample,
        end_sample,
        queued_frames,
        speech_frames,
        started_at_us,
        observed_at_us,
    } = event
    else {
        return Err(io::Error::other("expected cancelled tail retirement event").into());
    };
    let elapsed = observed_at_us.checked_sub(started_at_us);
    if affected.boot() != superseded_by.boot()
        || affected.turn() >= superseded_by.turn()
        || affected.generation() >= superseded_by.generation()
        || first_sample.epoch == 0
        || first_sample.epoch != end_sample.epoch
        || end_sample.frame.checked_sub(first_sample.frame) != Some(queued_frames as u64)
        || queued_frames == 0
        || queued_frames > lamp_audio::ownership::MAX_QUEUED_FRAMES
        || speech_frames == 0
        || speech_frames > queued_frames
        || started_at_us == 0
        || elapsed.is_none_or(|elapsed| elapsed >= lamp_audio::ownership::MAX_CANCELLED_TAIL_US)
        || observed_at_us > received_at_us
        || reply
            .as_ref()
            .is_some_and(|current| current.owner == affected)
    {
        return Err(io::Error::other("invalid cancelled tail retirement receipt").into());
    }
    trace
        .receipts
        .tail(affected, superseded_by, started_at_us)?;
    record(
        trace,
        json!({"kind":"cancelled_tail_retired","owner":affected,"turn":affected.turn(),
        "superseded_by":superseded_by,"first_sample":first_sample,"end_sample":end_sample,
        "queued_frames":queued_frames,"speech_frames":speech_frames,
        "started_at_us":started_at_us,"at_us":observed_at_us,
        "coordinator_observed_at_us":received_at_us,
        "outcome":"cancelled","boundary":"ALSA retirement of previously accepted cancelled PCM; acoustic silence unmeasured"}),
    )?;
    Ok(())
}

fn handle_playback_discard(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    trace: &mut SessionState,
    event: WorkerEvent,
) -> Result<()> {
    let WorkerEvent::PlaybackDiscarded {
        owner: affected,
        reason,
        queued_frames,
        other_owners,
        observed_at_us,
    } = event
    else {
        return Err(io::Error::other("expected playback discard event").into());
    };
    if queued_frames == 0 || queued_frames > lamp_audio::ownership::MAX_QUEUED_FRAMES {
        return Err(io::Error::other("invalid playback discard frame count").into());
    }
    // #1178: the speaker's authority lapsed while it was held up, and what
    // it had queued is gone. That costs the answer (cut by the caller if it
    // is the current one), never the voice (the reviewer on #1214).
    let lapsed = reason == PlaybackDiscardReason::ExpiredState;
    let expected = (reason == PlaybackDiscardReason::StaleOwner || lapsed)
        && !other_owners
        && reply
            .as_ref()
            .is_none_or(|current| current.owner != affected)
        && trace.receipts.discard(affected);
    record(
        trace,
        json!({"kind":"playback_discarded","owner":affected,"turn":affected.turn(),
            "reason":reason,"queued_frames":queued_frames,"other_owners":other_owners,
            "observed_at_us":observed_at_us,"at_us":monotonic_us(),
            "expected_cancellation":expected,"lapsed":lapsed}),
    )?;
    if expected || lapsed {
        return Ok(());
    }
    // The physical writer already stopped. Revoke any remaining output and end
    // this trial explicitly; late queue-empty callbacks cannot turn it into success.
    cancel_reply(owner, reply, trace, "playback_discarded")?;
    Err(io::Error::other(format!(
        "unexpected playback discard: turn={} reason={reason:?} frames={queued_frames} mixed={other_owners}",
        affected.turn()
    ))
    .into())
}

/// Why a reply is cut when the speaker's authority lapsed (#1178).
const SPEAKER_LAPSED: &str = "speaker_lapsed";

/// #1178: the speaker's authority lapsed while it was held up, and what
/// `lapsed`'s answer had queued or waiting is gone. If it is still the
/// current answer it is cut, as a reminder's lapsed lease cuts one: what is
/// queued from the provider for it is dropped, and the voice goes on (the
/// reviewer on #1214). Whether it was cut; the caller publishes.
fn cut_after_speaker_lapse<T>(
    lapsed: TurnOwner,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    provider_queue: &mut VecDeque<T>,
    speaker_pending: &mut Option<PendingSpeaker>,
    trace: &mut SessionState,
) -> Result<bool> {
    if reply.as_ref().is_none_or(|current| current.owner != lapsed) {
        return Ok(false);
    }
    eprintln!(
        "lamp-voice: the speaker's authority lapsed while it was held up; its answer is cut, the voice goes on"
    );
    cancel_reply(owner, reply, trace, SPEAKER_LAPSED)?;
    provider_queue.clear();
    *speaker_pending = None;
    Ok(true)
}

/// The speaker's accepted zeros while a reply was unfinished: a delivery
/// gap, unless they are a listening gap's (`listening`, #828). One whose
/// zeros ran on past its hold (`overran_us`) is counted once it resumes,
/// for its words came late.
fn handle_playback_gap(
    reply: &mut Option<Reply>,
    pending: Option<&PendingSpeaker>,
    trace: &mut SessionState,
    event: WorkerEvent,
    listening: bool,
    overran_us: Option<u64>,
) -> Result<()> {
    let WorkerEvent::PlaybackGap {
        phase,
        gap,
        observed_at_us,
    } = event
    else {
        return Err(io::Error::other("expected playback gap event").into());
    };
    if !gap.is_valid_receipt(phase, observed_at_us) {
        return Err(io::Error::other("invalid accepted playback gap receipt").into());
    }
    let current = reply.as_mut().filter(|reply| reply.owner == gap.owner);
    let origin = current.as_ref().map(|r| r.notice.origin());
    let (queued, held, last_provider_audio) = if let Some(current) = current {
        if phase == PlaybackGapPhase::Started && origin.is_some_and(SpeechOrigin::local) {
            current.notice.gap()?;
        } else if match phase {
            PlaybackGapPhase::Started => !listening,
            PlaybackGapPhase::Resumed => listening && overran_us.is_some(),
            PlaybackGapPhase::Cancelled => false,
        } && !origin.is_some_and(SpeechOrigin::local)
        {
            current.playback_gaps = current
                .playback_gaps
                .checked_add(1)
                .ok_or_else(|| io::Error::other("reply playback gap counter exhausted"))?;
        }
        (
            Some(current.pcm.len()),
            Some(current.held_final_samples()),
            current.last_provider_audio_at_us,
        )
    } else {
        (None, None, None)
    };
    record(
        trace,
        json!({"kind":"playback_gap","origin":origin,"phase":phase,"gap":gap,
        "listen_gap":listening,"listen_gap_overran_us":overran_us,
        "at_us":observed_at_us,"coordinator_observed_at_us":monotonic_us(),
        "reply_queued_frames_at_observation":queued,"held_final_frames_at_observation":held,
        "speaker_pending_sequence_at_observation":pending.map(|pending| pending.sequence),
        "last_provider_audio_enqueued_at_us":last_provider_audio,
        "boundary":"actual zero PCM accepted while reply unfinished; acoustic gap unmeasured"}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// #2231 (the reviewer-lead on #2459): a refused key waits only the
    /// second time in a row; a quota or a rate limit at once; any other end
    /// clears the stamp.
    #[test]
    fn a_refused_key_waits_only_the_second_time_in_a_row() {
        use crate::provider_worker::Fault;
        let folder = std::env::temp_dir().join(format!("lamp-voice-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let stamp = folder.join(KEY_REFUSED_STAMP);
        let _ = std::fs::remove_file(&stamp);
        assert!(!refusal_waits(Some(Fault::Authentication), Some(&stamp)));
        assert!(stamp.exists(), "the first refusal is stamped");
        assert!(refusal_waits(Some(Fault::Authentication), Some(&stamp)));
        assert!(!refusal_waits(None, Some(&stamp)));
        assert!(!stamp.exists(), "another end clears it");
        assert!(!refusal_waits(Some(Fault::Authentication), Some(&stamp)));
        assert!(refusal_waits(Some(Fault::QuotaExceeded), Some(&stamp)));
        assert!(refusal_waits(Some(Fault::RateLimited), None));
        assert!(!refusal_waits(Some(Fault::Authentication), None));
        let _ = std::fs::remove_dir_all(&folder);
    }

    mod call_wait;
    mod duplex;
    mod early_end;
    mod endpoint_session;
    mod failure_notice;
    mod fixture_at;
    mod hand_controls;
    mod holding_answer;
    mod pilot_faults;
    mod playout_continuity;
    mod playout_start;
    mod provider_pair;
    mod provider_terminal;
    mod reminder_roundtrip;
    mod reminders;
    mod requested_audio;
    mod say;
    mod second_route;
    mod service;
    mod service_mute;
    mod speaker_lapse;
    mod speaker_tone;
    mod startup_cues;
    mod termination;
    mod tool_look;
    mod turn_taking;

    use super::*;

    /// #1895 on lamp-52e6: with the eyes their own process and no camera of
    /// the voice's own, the camera is permitted with the microphone, so the
    /// second route's turns can be the person's and not always the room's.
    /// A voice with its own camera keeps deciding it there; no eyes, none.
    #[test]
    fn the_eyes_camera_is_permitted_with_the_microphone_when_the_voice_has_none() {
        for (eyes, own_camera, allowed) in [
            (true, false, true),
            (true, true, false),
            (false, false, false),
        ] {
            let mut owner = Controller::new(new_boot().unwrap(), now());
            owner
                .set_microphone_permission(now(), Permission::Allowed)
                .unwrap();
            eyes_look_with_the_microphone(&mut owner, eyes, own_camera).unwrap();
            assert_eq!(
                owner.snapshot(now()).unwrap().camera_allowed(now()),
                allowed,
                "eyes {eyes}, own camera {own_camera}"
            );
            // A mute denies both, as `hand.rs` does: the eyes' camera with it.
            owner
                .set_microphone_permission(now(), Permission::Denied)
                .unwrap();
            owner
                .set_camera_permission(now(), Permission::Denied)
                .unwrap();
            assert!(!owner.snapshot(now()).unwrap().camera_allowed(now()));
        }
    }
    use crate::{
        admission::{Evidence, Verdict},
        wire::CaptureTiming,
    };
    use lamp_turn_owner::BoundaryGuard;

    #[test]
    fn ring_lifetime_includes_startup_and_shutdown_around_maximum_active_session() {
        let required = Duration::from_secs(MAX_DIRECTED_SECONDS)
            + crate::process::WORKER_STARTUP_TIMEOUT
            + INPUT_READINESS_TIMEOUT
            + DIAGNOSTIC_SHUTDOWN_GRACE;
        assert!(required < Duration::from_micros(crate::ring_worker::MAX_RUNTIME_US));
    }

    /// #1182's review (#1178): a ring held 150 ms in a turn makes its receipt
    /// late. The choreography starts afresh instead of the ring being
    /// disabled for the rest of the serve cycle: it holds the frame, asks no
    /// other until that frame's late receipt comes back, then a new frame
    /// goes out, so the light comes back, and its receipt is taken.
    #[test]
    fn a_ring_held_up_mid_turn_starts_afresh_and_its_light_comes_back() {
        use crate::ring_wire::RingFrame;

        let directory = SessionDirectory::create().unwrap();
        let (mut worker, mut peer) = Worker::sleeping_fixture(&directory.path, "ring");
        let mut owner = Controller::new(new_boot().unwrap(), now());
        let renew = |owner: &mut Controller| {
            let at = now();
            let until = at.checked_add(INPUT_LEASE_US).unwrap();
            owner
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            owner
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
        };
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        renew(&mut owner);
        owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        let mut policy = Some(RingChoreographer::new(24).unwrap());
        let mut trace = SessionState::default();
        let mut ring_trace = RingTrace::default();

        service_ring(
            Some(&mut worker),
            policy.as_mut(),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        let held = peer.data.receive::<RingFrame>().unwrap().unwrap();
        // The ring is held 150 ms: no receipt comes. The coordinator itself
        // goes on renewing the turn's input meanwhile.
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(50));
            renew(&mut owner);
        }
        let late = service_ring(
            Some(&mut worker),
            policy.as_mut(),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap_err();
        assert!(
            late.downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::TimedOut),
            "{late}"
        );
        assert!(resynced_after(&mut policy, &mut ring_trace, &*late, &mut trace).unwrap());
        assert_eq!(
            trace
                .iter()
                .filter(|event| event["kind"] == "ring_resynced")
                .count(),
            1
        );
        // While the frame is held, no other is asked for.
        service_ring(
            Some(&mut worker),
            policy.as_mut(),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        assert!(peer.data.receive::<RingFrame>().unwrap().is_none());
        // The held frame's receipt comes late; then the light comes back.
        let report = |frame: RingFrame, written: u64| WorkerEvent::Ring {
            report: RingFeedback::Presented {
                owner: frame.permit.owner(),
                phase: frame.phase,
                requested_at_us: frame.requested_at_us,
                write_started_at_us: written,
                write_finished_at_us: written,
            },
        };
        peer.control
            .send(report(held, held.requested_at_us + 1))
            .unwrap();
        renew(&mut owner);
        service_ring(
            Some(&mut worker),
            policy.as_mut(),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        let next = peer.data.receive::<RingFrame>().unwrap().unwrap();
        assert!(next.requested_at_us > held.requested_at_us);
        // The new frame's receipt is taken.
        peer.control.send(report(next, monotonic_us())).unwrap();
        renew(&mut owner);
        service_ring(
            Some(&mut worker),
            policy.as_mut(),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        assert!(!policy.as_ref().unwrap().is_faulted());
    }

    #[test]
    fn ring_service_orders_authority_and_preserves_successor_on_late_feedback() {
        use crate::ring_wire::{RingFrame, RingPhase};

        let directory = SessionDirectory::create().unwrap();
        let (mut worker, mut peer) = Worker::sleeping_fixture(&directory.path, "ring");
        let mut owner = Controller::new(new_boot().unwrap(), now());
        let at = now();
        let until = at.checked_add(INPUT_LEASE_US).unwrap();
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let previous = owner.admit(at, AdmittedInput::NewTurn).unwrap();
        let mut policy = RingChoreographer::new(24).unwrap();
        let mut trace = SessionState::default();
        let mut ring_trace = RingTrace::default();

        service_ring(
            Some(&mut worker),
            Some(&mut policy),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        let frame = peer.data.receive::<RingFrame>().unwrap().unwrap();
        assert_eq!(frame.permit.owner(), previous);
        assert!(matches!(peer.control.receive::<Control>().unwrap(),
            Some(Control::Authority { snapshot }) if snapshot == frame.snapshot));
        let written_at_us = monotonic_us();
        let successor = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        peer.control
            .send(WorkerEvent::Ring {
                report: RingFeedback::Presented {
                    owner: previous,
                    phase: frame.phase,
                    requested_at_us: frame.requested_at_us,
                    write_started_at_us: written_at_us,
                    write_finished_at_us: written_at_us,
                },
            })
            .unwrap();

        service_ring(
            Some(&mut worker),
            Some(&mut policy),
            &mut owner,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(successor));
        let next = peer.data.receive::<RingFrame>().unwrap().unwrap();
        assert_eq!(next.permit.owner(), successor);
        assert_eq!(next.phase, RingPhase::Listening);
        assert!(matches!(peer.control.receive::<Control>().unwrap(),
            Some(Control::Authority { snapshot }) if snapshot == next.snapshot));
        assert_eq!(
            trace
                .iter()
                .filter(|event| event["kind"] == "ring_requested")
                .count(),
            2
        );
        assert_eq!(
            trace
                .iter()
                .filter(|event| event["kind"] == "ring_feedback")
                .count(),
            1
        );
    }

    #[test]
    fn diagnostic_shutdown_uses_one_deadline_and_reaps_every_hung_worker() {
        let directory = SessionDirectory::create().unwrap();
        let (speaker, _speaker_peer) = Worker::sleeping_fixture(&directory.path, "speaker");
        let (capture, _capture_peer) = Worker::sleeping_fixture(&directory.path, "capture");
        let (provider, _provider_peer) = Worker::sleeping_fixture(&directory.path, "provider");
        let (privacy, _privacy_peer) = Worker::sleeping_fixture(&directory.path, "privacy");
        let (ring, _ring_peer) = Worker::sleeping_fixture(&directory.path, "ring");
        let mut rig = Rig {
            expression_out: None,
            second: None,
            end_silence_blocks: None,
            early_end_blocks: None,
            early_finalize: false,
            open_name: false,
            call_takes_next: false,
            unfinished_wait: false,
            faint_level: None,
            microphone: crate::options::Microphone::Mic2,
            speaker,
            capture,
            provider,
            privacy,
            privacy_controls: false,
            capture_held: false,
            privacy_receipts: false,
            ring: Some(ring).into(),
            camera: None,
            motor: None,
            ring_channel_ceiling: Some(24),
            diagnostic_directory: Some(directory.path.clone()),
            noise_suppression: NoiseSuppression::On,
            turn_taking: TurnTaking::VadOnly,
            playout_start: PlayoutStart::default(),
            fixture_at_ms: None,
            endpoint: None,
            call_wait_blocks: None,
            cut_keeps_name: false,
            asker_first: crate::asker_first::Mode::Off,
            follows_on: false,
            paused: Box::new(|| false),
        };
        let started = Instant::now();
        let mut trace = SessionState::default();
        assert!(rig.shutdown(&mut trace).is_err());
        // A five-times-750 ms serial grace would fail this generous regression
        // bound. It is a test observation, not a kernel kill/wait deadline.
        assert!(started.elapsed() < Duration::from_millis(2500));
        for worker in [
            &mut rig.speaker,
            &mut rig.capture,
            &mut rig.provider,
            &mut rig.privacy,
        ]
        .into_iter()
        .chain(rig.ring.iter_mut())
        {
            assert!(worker.try_exit().unwrap().is_some());
        }
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry["kind"] == "audio_diagnostics_certified"
                    && entry["valid"] == false)
                .count(),
            2
        );
    }

    fn pair(dir: &SessionDirectory, role: &str) -> (WorkerChannels, WorkerChannels) {
        let parent = new_boot().unwrap();
        let worker = new_boot().unwrap();
        let mut a = WorkerChannels::bind(&dir.path, role, parent, worker, false).unwrap();
        let mut b = WorkerChannels::bind(&dir.path, role, parent, worker, true).unwrap();
        a.connect(&dir.path, role, false).unwrap();
        b.connect(&dir.path, role, true).unwrap();
        (a, b)
    }

    fn capture_started(mode: NoiseSuppression) -> WorkerEvent {
        let at = monotonic_us();
        WorkerEvent::CaptureStarted {
            epoch: 1,
            dsp_epoch: 1,
            noise_suppression: mode,
            privacy_generation: 2,
            rate: 16_000,
            channels: 1,
            period_frames: 160,
            buffer_frames: 640,
            monotonic_status_timestamps: false,
            open_started_at_us: at,
            prepared_at_us: at,
            capture_started_at_us: at,
            start_completed_at_us: at,
        }
    }

    #[test]
    fn actual_capture_mode_must_match_requested_mode_before_epoch_acceptance() {
        for requested in [NoiseSuppression::On, NoiseSuppression::Off] {
            for observed in [NoiseSuppression::On, NoiseSuppression::Off] {
                let dir = SessionDirectory::create().unwrap();
                let (mut parent, mut child) = pair(&dir, "capture");
                let mut epoch = None;
                let mut trace = SessionState::default();
                child.control.send(capture_started(observed)).unwrap();
                let result =
                    drain_capture_control(&mut parent.control, &mut epoch, requested, &mut trace);
                assert_eq!(result.is_ok(), requested == observed);
                assert_eq!(epoch, (requested == observed).then_some(1));
                assert_eq!(trace.len(), 1);
                assert_eq!(trace[0]["details"]["noise_suppression"], observed.as_str());
                assert_eq!(
                    trace[0]["kind"],
                    if requested == observed {
                        "capture_started"
                    } else {
                        "capture_processing_mismatch"
                    }
                );
            }
        }
    }

    /// A voice mic waited for at start (#1178) is one plain line in the
    /// session's events, and is kept for the voice's health until the next
    /// capture start; a prepare with no wait says nothing more than before.
    #[test]
    fn a_mic_waited_for_at_start_is_said_plainly_and_kept_for_the_health() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut child) = pair(&dir, "capture");
        let mut epoch = None;
        let mut trace = SessionState::default();
        let waited = crate::wire::MicWaited {
            gone: true,
            waited_ms: 4_300,
            opened_on_try: 18,
        };
        for waited in [None, Some(waited)] {
            child
                .control
                .send(WorkerEvent::CapturePrepared {
                    epoch: 1,
                    privacy_generation: 0,
                    prepared_at_us: 1,
                    waited,
                })
                .unwrap();
        }
        drain_capture_control(
            &mut parent.control,
            &mut epoch,
            NoiseSuppression::Off,
            &mut trace,
        )
        .unwrap();
        let kinds: Vec<_> = trace.iter().map(|event| event["kind"].clone()).collect();
        assert_eq!(
            kinds,
            [
                json!("capture_prepared"),
                json!("mic_waited_at_start"),
                json!("capture_prepared")
            ]
        );
        assert_eq!(
            trace[1]["said"],
            "the voice mic was off USB for 4.3 s at start; opened on try 18"
        );
        assert_eq!(trace.mic_at_start, Some(waited));
        // A later capture start (after a mute or a sleep) that opened at once
        // clears it: the note is the latest start's.
        child
            .control
            .send(WorkerEvent::CapturePrepared {
                epoch: 2,
                privacy_generation: 1,
                prepared_at_us: 2,
                waited: None,
            })
            .unwrap();
        drain_capture_control(
            &mut parent.control,
            &mut epoch,
            NoiseSuppression::Off,
            &mut trace,
        )
        .unwrap();
        assert_eq!(trace.mic_at_start, None);
    }

    #[test]
    fn an_old_or_malformed_start_report_cannot_silently_infer_a_processing_mode() {
        let mut report = serde_json::to_value(capture_started(NoiseSuppression::Off)).unwrap();
        report.as_object_mut().unwrap().remove("noise_suppression");
        assert!(serde_json::from_value::<WorkerEvent>(report.clone()).is_err());
        report["noise_suppression"] = json!(false);
        assert!(serde_json::from_value::<WorkerEvent>(report).is_err());
    }

    #[test]
    fn the_stop_words_hear_the_raw_block_else_the_linear_output_else_the_samples() {
        let frame = |raw: Option<Vec<i16>>, linear: Option<Vec<i16>>| MicrophoneFrame {
            raw,
            linear,
            epoch: 1,
            dsp_epoch: 1,
            privacy_generation: 2,
            frame_sequence: 1,
            read_completed_at_us: 1,
            probability: 0.0,
            echo: None,
            reference: Default::default(),
            samples: vec![3; 160],
            timing: CaptureTiming {
                first_read_started_at_us: 1,
                last_read_started_at_us: 1,
                successful_reads: 1,
                alsa_status_monotonic_us: None,
                status_observed_at_us: 1,
                available_frames: 0,
                delayed_frames: 0,
                processing_completed_at_us: 1,
                aec_queue_delay_ms: 0,
                reference: crate::wire::ReferenceTiming::default(),
            },
        };
        // #1635's P3 (the reviewer): with the linear output left out of the
        // choice, `linear` heard `processed` while each reply's counts said
        // `linear`.
        for (raw, linear, heard) in [
            (Some(vec![1; 160]), None, Some(vec![1; 160])),
            (None, Some(vec![2; 160]), Some(vec![2; 160])),
            (None, None, None),
        ] {
            let mut frame = frame(raw, linear);
            assert_eq!(stop_words_block(&mut frame), heard);
            assert!(frame.raw.is_none() && frame.linear.is_none());
            assert_eq!(frame.samples, vec![3; 160]);
        }
    }

    #[test]
    fn first_microphone_frame_survives_start_report_between_channel_drains() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut capture) = pair(&dir, "capture");
        let mut epoch = None;
        let mut trace = SessionState::default();
        drain_capture_control(
            &mut parent.control,
            &mut epoch,
            NoiseSuppression::On,
            &mut trace,
        )
        .unwrap();
        assert!(epoch.is_none());
        let at = monotonic_us();
        capture
            .control
            .send(WorkerEvent::CaptureStarted {
                epoch: 1,
                dsp_epoch: 1,
                noise_suppression: NoiseSuppression::On,
                privacy_generation: 2,
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
        capture
            .data
            .send(MicrophoneFrame {
                raw: None,
                linear: None,
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: 2,
                frame_sequence: 1,
                read_completed_at_us: at,
                probability: 0.95,
                echo: None,
                reference: Default::default(),
                samples: vec![123; 160],
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
                    reference: crate::wire::ReferenceTiming {
                        playback_epoch: 1,
                        clock_started_at_us: Some(at),
                        ..crate::wire::ReferenceTiming::default()
                    },
                },
            })
            .unwrap();
        let mut frame = parent.data.receive::<MicrophoneFrame>().unwrap().unwrap();
        validate_capture_frame(
            &mut parent.control,
            &mut epoch,
            2,
            &frame,
            NoiseSuppression::On,
            &mut trace,
        )
        .unwrap();
        assert_eq!(epoch, Some(1));
        assert_eq!(frame.samples, vec![123; 160]);
        assert_eq!(trace.len(), 1);
        frame.epoch = 2;
        assert!(
            validate_capture_frame(
                &mut parent.control,
                &mut epoch,
                2,
                &frame,
                NoiseSuppression::On,
                &mut trace
            )
            .is_err()
        );
        // A stall closed epoch 1 and the worker reopened it: Gap(1),
        // Started(2) and the first frame of 2 all queued between drains.
        capture
            .control
            .send(WorkerEvent::CaptureGap {
                epoch: 1,
                reason: "AuthorityExpired".to_owned(),
                closed_at_us: monotonic_us(),
            })
            .unwrap();
        let mut started = capture_started(NoiseSuppression::On);
        if let WorkerEvent::CaptureStarted {
            epoch: ref mut reopened,
            ref mut privacy_generation,
            ..
        } = started
        {
            *reopened = 2;
            *privacy_generation = 2;
        }
        capture.control.send(started).unwrap();
        assert_eq!(
            validate_capture_frame(
                &mut parent.control,
                &mut epoch,
                2,
                &frame,
                NoiseSuppression::On,
                &mut trace
            )
            .unwrap(),
            Some(1)
        );
        assert_eq!(epoch, Some(2));
    }

    fn ready_link(revision: u64, session: u64) -> ProviderLink {
        let at = monotonic_us();
        ProviderLink {
            revision,
            session,
            observed_at_us: at,
            source_clock: None,
            state: ProviderLinkState::Ready {
                setup_us: 1234,
                attempts: 1,
                context: if session == 1 {
                    crate::provider_link::LinkContext::Initial
                } else {
                    crate::provider_link::LinkContext::Resumed
                },
                outage_started_at_us: (session > 1).then_some(at - 10),
                after: (session > 1).then_some(crate::provider_worker::Fault::Transport),
            },
        }
    }

    #[test]
    fn startup_consumes_provider_ready_before_later_worker_exceeds_packet_age() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut provider) = pair(&dir, "provider");
        provider
            .control
            .send(WorkerEvent::ProviderLink {
                link: ready_link(1, 1),
            })
            .unwrap();
        let mut startup = StartupEvents::default();
        startup.poll(&mut parent, "provider").unwrap();
        // Readiness is retained state, not an unread packet whose 100 ms
        // transport lease could expire during a later child's startup.
        std::thread::sleep(Duration::from_millis(120));
        startup.poll(&mut parent, "provider").unwrap();
        assert_eq!(startup.cloud.ready_session(), Some(1));
        assert_eq!(startup.events[0]["setup_us"], 1234);
        provider
            .control
            .send(WorkerEvent::ProviderLink {
                link: ready_link(3, 2),
            })
            .unwrap();
        startup.poll(&mut parent, "provider").unwrap();
        assert_eq!(startup.cloud.ready_session(), Some(2));
        provider
            .data
            .send(ProviderOutput::TurnComplete {
                request: 1,
                idle: true,
            })
            .unwrap();
        assert!(startup.poll(&mut parent, "provider").is_err());
    }

    /// The reviewer on #1686: a wrong `LAMP_VOICE_ROUTE`, `second` without
    /// its configuration, or a wrong `LAMP_SECOND_*` switch is refused by
    /// `run_serve` before anything starts: no session folder is made (and so
    /// no hand controls restored, no `run_start`, no worker), and the voice
    /// never runs on Gemini in its place.
    #[test]
    fn run_serve_refuses_a_wrong_route_before_anything_starts() {
        let dir = SessionDirectory::create().unwrap();
        for (set, why) in [
            (
                vec![("LAMP_VOICE_ROUTE", "live")],
                "LAMP_VOICE_ROUTE is gemini or second",
            ),
            (
                vec![("LAMP_VOICE_ROUTE", "second")],
                "LAMP_VOICE_ROUTE=second needs LAMP_VOICE_SECOND_CONFIG",
            ),
            (
                vec![("LAMP_SECOND_CHAT", "on")],
                "LAMP_SECOND_CHAT is gemini, openrouter, deepseek or nothing",
            ),
            (
                vec![("LAMP_SECOND_SEARCH", "openrouter")],
                "needs LAMP_SECOND_CHAT=openrouter",
            ),
        ] {
            let output = dir.path.join("session");
            second::TEST_ENVIRONMENT.with(|environment| {
                *environment.borrow_mut() = Some(
                    set.iter()
                        .map(|(name, value)| (*name, std::ffi::OsString::from(*value)))
                        .collect(),
                )
            });
            let result = run_serve(
                Path::new("/nonexistent/provider.json"),
                &output,
                SessionOptions::parse(&[]).unwrap(),
            );
            second::TEST_ENVIRONMENT.with(|environment| *environment.borrow_mut() = None);
            let error = result.unwrap_err().to_string();
            assert!(error.contains(why), "{error}");
            assert!(!output.exists(), "{why}: a session folder was made");
        }
    }

    /// With `LAMP_VOICE_ROUTE=second` the second route's worker starts in
    /// `rig.provider`: its readiness is taken during startup as Gemini's is,
    /// and output before input still refused.
    #[test]
    fn startup_takes_the_second_route_alone_as_the_provider() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut second) = pair(&dir, "second");
        second
            .control
            .send(WorkerEvent::ProviderLink {
                link: ready_link(1, 1),
            })
            .unwrap();
        let mut startup = StartupEvents::default();
        startup.poll(&mut parent, "second").unwrap();
        assert_eq!(startup.cloud.ready_session(), Some(1));
        assert_eq!(startup.events[0]["kind"], "provider_ready");
        // Its output is read as Gemini's is: a Ready counts against its credit.
        second
            .data
            .send(ProviderOutput::Ready { setup_us: 7 })
            .unwrap();
        startup.poll(&mut parent, "second").unwrap();
        assert_eq!(startup.events[1]["kind"], "provider_ready_data_ignored");
        second
            .data
            .send(ProviderOutput::TurnComplete {
                request: 1,
                idle: true,
            })
            .unwrap();
        assert!(startup.poll(&mut parent, "second").is_err());
    }

    /// #1567 (the reviewer on #1592): a frame the capture sends before its
    /// activation is a microphone open and giving audio: startup refuses it,
    /// and says the run heard, so its fault keeps the restart guard.
    #[test]
    fn a_microphone_that_speaks_before_activation_counts_as_heard() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut capture) = pair(&dir, "capture");
        let mut startup = StartupEvents::default();
        startup.poll(&mut parent, "capture").unwrap();
        assert!(!startup.microphone_spoke, "nothing sent: nothing heard");
        let at = monotonic_us();
        capture
            .data
            .send(MicrophoneFrame {
                raw: None,
                linear: None,
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: 2,
                frame_sequence: 1,
                read_completed_at_us: at,
                probability: 0.95,
                echo: None,
                reference: Default::default(),
                samples: vec![123; 160],
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
                    reference: crate::wire::ReferenceTiming::default(),
                },
            })
            .unwrap();
        let refused = startup.poll(&mut parent, "capture").unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("microphone opened before activation")
        );
        assert!(startup.microphone_spoke);
    }

    #[test]
    fn the_keeper_pause_is_read_as_the_body_reads_it() {
        use lamp_msgs::Pause;
        // A bus it cannot reach is not paused.
        assert!(!keeper_pause(lamp_bus::Bus::new("not/absolute"))());
        let root = std::env::temp_dir().join(format!("lamp-voice-pause-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        // Nothing kept is not paused.
        assert!(!keeper_pause(Ok(bus.clone()))());
        let mut keeper = bus
            .publisher_declared(Pause::TOPIC, &Pause::declaration(), "keeper")
            .unwrap();
        keeper.publish_json(1, &Pause { paused: false }).unwrap();
        assert!(!keeper_pause(Ok(bus.clone()))());
        keeper.publish_json(2, &Pause { paused: true }).unwrap();
        assert!(keeper_pause(Ok(bus.clone()))());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_probe_verdict_is_read_from_lamp_online_only_when_it_is_from_reach() {
        use lamp_msgs::Online;
        // Off: no verdict asked at all, the link alone decides.
        assert_eq!(
            reach_verdict(false, lamp_bus::Bus::new("not/absolute"))(0),
            None
        );
        // On, with a bus it cannot reach: no verdict.
        assert_eq!(
            reach_verdict(true, lamp_bus::Bus::new("not/absolute"))(0),
            Some(None)
        );
        let root = std::env::temp_dir().join(format!("lamp-voice-reach-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        // Nothing kept: no verdict.
        assert_eq!(reach_verdict(true, Ok(bus.clone()))(0), Some(None));
        let mut health = bus
            .publisher_declared(Online::TOPIC, &Online::declaration(), "health")
            .unwrap();
        let said = |online: Option<bool>, from: &str, down_for_s: Option<u64>| Online {
            online,
            from: from.into(),
            down_for_s,
        };
        // From the voice's link, either way: no verdict.
        health
            .publish_json(1, &said(Some(false), "voice", Some(30)))
            .unwrap();
        assert_eq!(reach_verdict(true, Ok(bus.clone()))(0), Some(None));
        health
            .publish_json(2, &said(Some(true), "voice", None))
            .unwrap();
        assert_eq!(reach_verdict(true, Ok(bus.clone()))(0), Some(None));
        // From the probe: its verdict.
        health
            .publish_json(3, &said(Some(false), "reach", Some(9)))
            .unwrap();
        assert_eq!(reach_verdict(true, Ok(bus.clone()))(0), Some(Some(false)));
        health
            .publish_json(4, &said(Some(true), "reach", None))
            .unwrap();
        let mut reader = reach_verdict(true, Ok(bus.clone()));
        assert_eq!(reader(0), Some(Some(true)));
        // Read at most once a second: a change shows only after it.
        health
            .publish_json(5, &said(Some(false), "reach", Some(10)))
            .unwrap();
        assert_eq!(reader(999_999), Some(Some(true)));
        assert_eq!(reader(1_000_000), Some(Some(false)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unasked_turns_wait_for_the_pause_and_are_otherwise_unchanged() {
        let mut startup = StartupEvents::default();
        assert_eq!(unasked_session(&startup.cloud, false), None);
        assert_eq!(unasked_session(&startup.cloud, true), None);
        startup
            .cloud
            .observe(ready_link(1, 1), monotonic_us())
            .unwrap();
        // Not paused, exactly the ready session, as before the pause.
        assert_eq!(
            unasked_session(&startup.cloud, false),
            startup.cloud.ready_session()
        );
        assert_eq!(unasked_session(&startup.cloud, false), Some(1));
        assert_eq!(unasked_session(&startup.cloud, true), None);
    }

    fn submitted_request() -> CloudRequest {
        let mut request = CloudRequest::new(1).unwrap();
        request
            .submitted(&ProviderInput::Start {
                session: 1,
                request: 1,
                privacy_generation: 1,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
            })
            .unwrap();
        request.confirm_start(1).unwrap();
        request
            .submitted(&ProviderInput::End {
                request: 1,
                privacy_generation: 1,
                sound: false,
                cut_in: false,
            })
            .unwrap();
        request
    }

    pub(super) fn active_reply() -> (Controller, Reply, OutputPermit) {
        let mut owner = Controller::new(new_boot().unwrap(), now());
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(monotonic_us() + 500_000);
        owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let turn = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        let plan = owner
            .plan_output(now(), turn, OutputKind::Speech, None)
            .unwrap();
        owner.input_ended(now(), turn).unwrap();
        let permit = owner.issue_output(now(), plan, 100_000).unwrap();
        let playback = owner.playback_started(now(), permit).unwrap();
        (
            owner,
            Reply {
                owner: turn,
                cloud: submitted_request(),
                local_input_ended: true,
                plan,
                playback: Some(playback),
                pcm: vec![123; 480].into(),
                generation_done: false,
                provider_idle: false,
                input_sequence: 1,
                had_audio: true,
                voice_level: 0,
                told_stop: None,
                final_chunk: None,
                playback_gaps: 0,
                notice: NoticeDelivery::default(),
                media: Default::default(),
                local_only: None,
                reminder: None,
                say: None,
                news: Vec::new(),
                last_provider_audio_at_us: None,
                start: StartGate::new(0),
                need: StartNeed::default(),
                handed: 0,
                sounded: false,
            },
            permit,
        )
    }

    fn provider_audio(sequence: u64, samples: Vec<i16>) -> ProviderOutput {
        ProviderOutput::Audio {
            request: 1,
            sequence,
            provider_event_at_us: monotonic_us(),
            source_frames: samples.len(),
            source_offset: 0,
            samples,
        }
    }

    #[test]
    fn continued_generation_preserves_the_accepted_final_chunk_retirement() {
        let (mut owner, mut current, _) = active_reply();
        let original_playback = current.playback.unwrap();
        current.pcm.clear();
        current.generation_done = true;
        current.final_chunk = Some(41);
        current.append_provider_audio(vec![22; 480], monotonic_us());
        let token = current.retire_playback(41, 720).unwrap();
        assert_eq!(token, original_playback);
        owner.playback_ended(now(), token).unwrap();
        assert_eq!(current.final_chunk, None);
        assert_eq!(current.pcm, vec![22; 480]);
        assert!(!current.generation_done);
        assert!(current.playback.is_none());
        assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(current.owner));
    }

    #[test]
    fn continued_generation_waits_for_the_previous_playback_occurrence_to_retire() {
        let (_, mut current, _) = active_reply();
        current.pcm.clear();
        current.generation_done = true;
        current.final_chunk = Some(41);
        current.append_provider_audio(vec![22; 480], monotonic_us());
        assert!(
            current.next_speaker_chunk().is_none(),
            "a later generation must not overwrite the speaker's unretired final cursor"
        );
        assert_eq!(current.pcm, vec![22; 480]);
    }

    #[test]
    fn continued_generation_interleavings_preserve_pcm_and_distinct_playback_tokens() {
        use crate::playback::SpeechPlayback;
        use lamp_audio::ownership::{PlaybackCursor, QueuedRange};

        // Audio may continue before final acceptance, before final retirement,
        // or after retirement. All are legal within one Gemini request.
        for arrival in 0..3 {
            let dir = SessionDirectory::create().unwrap();
            let (mut parent, mut speaker) = pair(&dir, "speaker");
            let (mut owner, mut current, _) = active_reply();
            let turn = current.owner;
            let first_token = current.playback.unwrap();
            current.pcm = vec![11; 240].into();
            current.generation_done = true;
            let mut reply = Some(current);
            let mut pending = None;
            let mut sequence = 0;
            let mut snapshot = owner.snapshot(now()).unwrap();
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::VadOnly),
            )
            .unwrap();
            let first = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
            assert!(first.end_of_speech);
            let mut played = first.samples.clone();
            if arrival == 0 {
                reply
                    .as_mut()
                    .unwrap()
                    .append_provider_audio(vec![22; 480], monotonic_us());
            }
            // No second chunk can bypass the outstanding final acceptance.
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::VadOnly),
            )
            .unwrap();
            assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
            assert_eq!(pending.as_ref().unwrap().sequence, first.chunk_sequence);
            pending = None; // Simulated full speaker acceptance of this chunk.
            reply.as_mut().unwrap().final_chunk = Some(first.chunk_sequence);
            if arrival == 1 {
                reply
                    .as_mut()
                    .unwrap()
                    .append_provider_audio(vec![22; 480], monotonic_us());
            }
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::VadOnly),
            )
            .unwrap();
            assert!(pending.is_none());
            assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());

            let mut playback = SpeechPlayback::default();
            let first_cursor = PlaybackCursor { epoch: 1, frame: 0 };
            let end_cursor = PlaybackCursor {
                epoch: 1,
                frame: 240,
            };
            playback
                .speech_accepted(
                    turn,
                    first.chunk_sequence,
                    first_cursor,
                    end_cursor,
                    1,
                    true,
                )
                .unwrap();
            let retired = playback
                .take_retired(QueuedRange {
                    epoch: 1,
                    retired_through: 240,
                    accepted_through: 240,
                })
                .unwrap();
            let token = reply
                .as_mut()
                .unwrap()
                .retire_playback(retired.sequence, retired.cursor.frame)
                .unwrap();
            assert_eq!(token, first_token);
            owner.playback_ended(now(), token).unwrap();
            assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(turn));
            if arrival == 2 {
                reply
                    .as_mut()
                    .unwrap()
                    .append_provider_audio(vec![22; 480], monotonic_us());
            }
            reply.as_mut().unwrap().generation_done = true;
            let mut frame = 240;
            let mut second_token = None;
            for _ in 0..2 {
                dispatch_speaker_chunk(
                    &mut owner,
                    &mut reply,
                    &mut parent.data,
                    &mut pending,
                    &mut sequence,
                    &mut snapshot,
                    &mut PlaybackMask::new(TurnTaking::VadOnly),
                )
                .unwrap();
                let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
                let accepted = pending.take().unwrap();
                assert_eq!(accepted.sequence, chunk.chunk_sequence);
                let current = reply.as_mut().unwrap();
                if current.playback.is_none() {
                    let token = owner.playback_started(now(), accepted.permit).unwrap();
                    assert_eq!(token.owner(), turn);
                    assert!(token.sequence() > first_token.sequence());
                    current.playback = Some(token);
                    second_token = Some(token);
                }
                if accepted.final_chunk {
                    current.final_chunk = Some(accepted.sequence);
                }
                // A duplicated receipt from occurrence one cannot retire two.
                assert!(current.retire_playback(first.chunk_sequence, 240).is_err());
                assert_eq!(current.playback, second_token);
                playback
                    .speech_accepted(
                        turn,
                        chunk.chunk_sequence,
                        PlaybackCursor { epoch: 1, frame },
                        PlaybackCursor {
                            epoch: 1,
                            frame: frame + 240,
                        },
                        frame,
                        chunk.end_of_speech,
                    )
                    .unwrap();
                frame += 240;
                played.extend(chunk.samples);
            }
            let retired = playback
                .take_retired(QueuedRange {
                    epoch: 1,
                    retired_through: frame,
                    accepted_through: frame,
                })
                .unwrap();
            let current = reply.as_mut().unwrap();
            assert_eq!(retired.cursor.epoch, 1);
            let token = current
                .retire_playback(retired.sequence, retired.cursor.frame)
                .unwrap();
            assert_eq!(Some(token), second_token);
            owner.playback_ended(now(), token).unwrap();
            assert!(current.pcm.is_empty());
            assert!(current.playback.is_none());
            assert!(current.final_chunk.is_none());
            assert!(
                !current.provider_idle,
                "retirement is not provider completion"
            );
            assert_eq!(played, [vec![11; 240], vec![22; 480]].concat());
            current.provider_idle = true;
            owner.complete_turn(now(), current.owner).unwrap();
            assert!(owner.snapshot(now()).unwrap().owner().is_none());
        }
    }

    #[test]
    fn cancellation_between_or_during_later_generations_revokes_the_same_turn() {
        for second_started in [false, true] {
            let (mut owner, mut current, first_permit) = active_reply();
            current.pcm.clear();
            current.final_chunk = Some(1);
            let token = current.retire_playback(1, 240).unwrap();
            owner.playback_ended(now(), token).unwrap();
            current.append_provider_audio(vec![22; 480], monotonic_us());
            let permit = owner.issue_output(now(), current.plan, 100_000).unwrap();
            if second_started {
                current.playback = Some(owner.playback_started(now(), permit).unwrap());
                current.final_chunk = Some(3);
            }
            let turn = current.owner;
            let mut reply = Some(current);
            let mut trace = SessionState::default();
            cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
            assert!(reply.is_none());
            let state = owner.snapshot(now()).unwrap();
            assert!(state.owner().is_none());
            let mut guard = BoundaryGuard::new(state.boot(), now());
            guard.install(now(), state).unwrap();
            for permit in [first_permit, permit] {
                assert!(guard.check(now(), permit, OutputKind::Speech).is_err());
            }
            assert_eq!(trace[0]["outcome"], "user_interrupted");
            assert_eq!(trace[0]["owner"], serde_json::to_value(turn).unwrap());
        }
    }

    #[test]
    fn ninety_second_reply_uses_bounded_credit_and_preserves_every_sample() {
        use crate::provider_flow::OutputSender;
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut provider) = pair(&dir, "provider");
        let mut send_flow = OutputSender::default();
        let mut receive_flow = OutputReceiver::default();
        let (_, mut reply, _) = active_reply();
        reply.pcm.clear();
        let original: Vec<i16> = (0..24_000 * 90 + 73)
            .map(|index| (index % 32_749) as i16 - 16_000)
            .collect();
        let mut source = VecDeque::from([
            ProviderOutput::Ready { setup_us: 1 },
            ProviderOutput::Started {
                request: 1,
                session: 1,
                waiting_for_barrier: false,
            },
        ]);
        source.extend(
            original
                .chunks(PACKET_SAMPLES)
                .enumerate()
                .map(|(index, samples)| provider_audio(index as u64 + 1, samples.to_vec())),
        );
        source.extend([
            ProviderOutput::Transcript {
                request: Some(1),
                text: "done".into(),
                finished: true,
            },
            ProviderOutput::GenerationComplete { request: 1 },
            ProviderOutput::TurnComplete {
                request: 1,
                idle: true,
            },
        ]);
        let packets = source.len();
        let mut received = 0;
        let mut played = Vec::new();
        let mut high_water = 0;
        let mut credit_waits = 0;
        let mut final_chunks = 0;
        for tick in 0..50_000 {
            while let Some(Control::ProviderOutputCapacity { through }) =
                provider.control.receive().unwrap()
            {
                send_flow.grant(through).unwrap();
            }
            for _ in 0..8 {
                let Some(event) = source.front() else {
                    break;
                };
                if !send_flow.try_send(|| provider.data.send(event)).unwrap() {
                    credit_waits += 1;
                    break;
                }
                source.pop_front();
            }
            let mut slice = ProviderSlice::new(monotonic_us());
            while let Some(event) = slice.receive(&mut parent.data, monotonic_us()).unwrap() {
                receive_flow.received().unwrap();
                received += 1;
                match event {
                    ProviderOutput::Audio { samples, .. } => reply.pcm.extend(samples),
                    ProviderOutput::GenerationComplete { .. } => reply.generation_done = true,
                    ProviderOutput::TurnComplete { idle, .. } => reply.provider_idle = idle,
                    _ => {}
                }
                high_water = high_water.max(reply.pcm.len());
                assert!(reply.pcm.len() <= MAX_REPLY_SAMPLES);
            }
            // The first second deliberately withholds playback, then the
            // production 240-frame chunk drains once per virtual 10 ms. This
            // exercises playback-rate flow control without claiming wall-clock
            // or acoustic timing from a unit test.
            if tick >= 500
                && tick % 5 == 0
                && let Some((samples, final_chunk)) = reply.next_speaker_chunk()
            {
                played.extend(samples);
                final_chunks += usize::from(final_chunk);
            }
            receive_flow
                .replenish(reply.pcm.len(), &mut parent.control)
                .unwrap();
            if reply.provider_idle && reply.pcm.is_empty() {
                break;
            }
        }
        assert_eq!(received, packets);
        assert!(source.is_empty());
        assert!(high_water >= MAX_REPLY_SAMPLES - PACKET_SAMPLES);
        assert!(credit_waits > 1_000);
        assert_eq!(final_chunks, 1);
        assert_eq!(&played[..original.len()], original);
        assert!(played[original.len()..].iter().all(|sample| *sample == 0));
        assert_eq!(played.len(), original.len().div_ceil(240) * 240);
    }

    #[test]
    fn provider_bursts_yield_to_real_speaker_channel_without_losing_or_reordering_pcm() {
        // These are the two observed burst sizes and the PCM already waiting
        // before each burst, not claimed hardware timing in this host test.
        for (burst_packets, existing_frames) in [(13, 3_840), (18, 43_920)] {
            let dir = SessionDirectory::create().unwrap();
            let (mut provider_parent, mut provider_child) = pair(&dir, "provider");
            let (mut speaker_parent, mut speaker_child) = pair(&dir, "speaker");
            let expected: Vec<i16> = (0..existing_frames + burst_packets * 960)
                .map(|index| (index % 32_767) as i16)
                .collect();
            let mut packets: VecDeque<_> = (0..burst_packets)
                .map(|index| {
                    provider_audio(
                        index as u64 + 1,
                        expected
                            [existing_frames + index * 960..existing_frames + (index + 1) * 960]
                            .to_vec(),
                    )
                })
                .collect();
            let (mut owner, mut current, _) = active_reply();
            current.pcm = expected[..existing_frames].to_vec().into();
            let mut reply = Some(current);
            let mut snapshot = owner.snapshot(now()).unwrap();
            let mut pending = None;
            let mut speaker_sequence = 0;
            let mut received = 0;
            let mut handed_off = Vec::new();
            // Kernel queue capacity differs by OS. Backpressure leaves each
            // unsent packet at the source instead of enlarging a socket/queue.
            for slice_index in 0..burst_packets {
                for _ in 0..burst_packets {
                    let Some(packet) = packets.front() else {
                        break;
                    };
                    match provider_child.data.send(packet) {
                        Ok(()) => {
                            packets.pop_front();
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) => panic!("provider fixture send: {error}"),
                    }
                }
                let mut slice = ProviderSlice::new(0);
                let before = received;
                // 750 us per handled packet models the observed processing
                // cost deterministically; no sleep or CI latency assertion.
                while let Some(event) = slice
                    .receive(&mut provider_parent.data, (received - before) * 750)
                    .unwrap()
                {
                    let ProviderOutput::Audio {
                        sequence, samples, ..
                    } = event
                    else {
                        panic!("unexpected provider fixture event");
                    };
                    received += 1;
                    assert_eq!(sequence, received);
                    reply.as_mut().unwrap().pcm.extend(samples);
                }
                assert!(received - before <= 2);
                if slice_index == 0 {
                    assert_eq!(received, 2);
                    assert!(received < burst_packets as u64);
                }
                dispatch_speaker_chunk(
                    &mut owner,
                    &mut reply,
                    &mut speaker_parent.data,
                    &mut pending,
                    &mut speaker_sequence,
                    &mut snapshot,
                    &mut PlaybackMask::new(TurnTaking::VadOnly),
                )
                .unwrap();
                let chunk = speaker_child
                    .data
                    .receive::<SpeakerChunk>()
                    .unwrap()
                    .unwrap();
                assert_eq!(chunk.chunk_sequence, speaker_sequence);
                assert!(!chunk.end_of_speech);
                handed_off.extend(chunk.samples);
                let retained = reply.as_ref().unwrap().pcm.len();
                dispatch_speaker_chunk(
                    &mut owner,
                    &mut reply,
                    &mut speaker_parent.data,
                    &mut pending,
                    &mut speaker_sequence,
                    &mut snapshot,
                    &mut PlaybackMask::new(TurnTaking::VadOnly),
                )
                .unwrap();
                assert_eq!(reply.as_ref().unwrap().pcm.len(), retained);
                assert!(
                    speaker_child
                        .data
                        .receive::<SpeakerChunk>()
                        .unwrap()
                        .is_none()
                );
                // Model successful acceptance of this one outstanding chunk;
                // the next slice is permitted to dispatch its successor.
                pending = None;
                if received == burst_packets as u64 {
                    break;
                }
            }
            assert!(packets.is_empty());
            assert_eq!(received, burst_packets as u64);
            handed_off.extend(reply.unwrap().pcm);
            assert_eq!(handed_off, expected);
            assert!(
                provider_parent
                    .data
                    .receive::<ProviderOutput>()
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn exhausted_provider_time_slice_leaves_next_real_packet_for_the_next_tick() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut child) = pair(&dir, "provider");
        child.data.send(provider_audio(1, vec![11; 960])).unwrap();
        child.data.send(provider_audio(2, vec![22; 960])).unwrap();
        let mut slice = ProviderSlice::new(10_000);
        assert!(slice.receive(&mut parent.data, 10_000).unwrap().is_some());
        assert!(slice.receive(&mut parent.data, 12_000).unwrap().is_none());
        let mut next_tick = ProviderSlice::new(12_000);
        let Some(ProviderOutput::Audio {
            sequence, samples, ..
        }) = next_tick.receive(&mut parent.data, 12_000).unwrap()
        else {
            panic!("budget consumed the next packet instead of yielding");
        };
        assert_eq!(sequence, 2);
        assert_eq!(samples, vec![22; 960]);
        assert!(next_tick.receive(&mut parent.data, 11_999).is_err());
    }

    #[test]
    fn unplug_proof_loss_blocks_delayed_audio_and_cancels_existing_authority() {
        for before_first_audio in [true, false] {
            let dir = SessionDirectory::create().unwrap();
            let (mut speaker_parent, mut speaker_child) = pair(&dir, "speaker");
            let (mut owner, mut current, old_permit) = active_reply();
            let (ticket, answered, deadline) = crate::say::guarded_ticket_for_test();
            assert!(ticket.authority_valid(monotonic_us()));
            current.say = Some(ticket);
            if before_first_audio {
                current.playback = None;
            }
            // Model audio arrives after the ticket was accepted; proof is lost
            // before final handoff, including the first-audio case.
            current.pcm.extend(vec![42; 960]);
            deadline.store(0, std::sync::atomic::Ordering::Release);
            let mut reply = Some(current);
            let mut pending = None;
            let mut sequence = 0;
            let mut snapshot = owner.snapshot(now()).unwrap();
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut speaker_parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::VadOnly),
            )
            .unwrap();
            assert!(
                speaker_child
                    .data
                    .receive::<SpeakerChunk>()
                    .unwrap()
                    .is_none()
            );
            assert!(pending.is_none());
            assert_eq!(sequence, 0);
            cancel_reply(
                &mut owner,
                &mut reply,
                &mut SessionState::default(),
                "unplug_authority_lost",
            )
            .unwrap();
            assert_eq!(
                answered.try_recv().unwrap(),
                crate::say::Answer::Done(crate::say::Outcome::Dropped(
                    "unplug_authority_lost".into()
                ))
            );
            let snapshot = owner.snapshot(now()).unwrap();
            let mut boundary = BoundaryGuard::new(snapshot.boot(), now());
            boundary.install(now(), snapshot).unwrap();
            assert!(
                boundary
                    .check(now(), old_permit, OutputKind::Speech)
                    .is_err()
            );
        }
        // Ordinary say tickets retain the existing dispatch behavior.
        let dir = SessionDirectory::create().unwrap();
        let (mut speaker_parent, mut speaker_child) = pair(&dir, "speaker");
        let (mut owner, mut current, _) = active_reply();
        current.say = Some(crate::say::ticket_for_test("reminders").0);
        let mut reply = Some(current);
        let mut snapshot = owner.snapshot(now()).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut speaker_parent.data,
            &mut None,
            &mut 0,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
        )
        .unwrap();
        assert_eq!(
            speaker_child
                .data
                .receive::<SpeakerChunk>()
                .unwrap()
                .unwrap()
                .samples,
            vec![123; 240]
        );
    }

    #[test]
    fn interruption_after_provider_slice_revokes_old_pcm_before_speaker_dispatch() {
        let dir = SessionDirectory::create().unwrap();
        let (mut provider_parent, mut provider_child) = pair(&dir, "provider");
        let (mut speaker_parent, mut speaker_child) = pair(&dir, "speaker");
        for sequence in 1..=3 {
            provider_child
                .data
                .send(provider_audio(sequence, vec![sequence as i16; 960]))
                .unwrap();
        }
        let (mut owner, current, old_permit) = active_reply();
        let mut reply = Some(current);
        let mut slice = ProviderSlice::new(0);
        while let Some(event) = slice.receive(&mut provider_parent.data, 0).unwrap() {
            let ProviderOutput::Audio { samples, .. } = event else {
                panic!("unexpected fixture event");
            };
            reply.as_mut().unwrap().pcm.extend(samples);
        }
        // The production loop handles retained input after this slice and before
        // dispatch. Cancelling here must not dispatch even the preexisting PCM.
        cancel_reply(
            &mut owner,
            &mut reply,
            &mut SessionState::default(),
            "user_interrupted",
        )
        .unwrap();
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut speaker_parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
        )
        .unwrap();
        assert!(
            speaker_child
                .data
                .receive::<SpeakerChunk>()
                .unwrap()
                .is_none()
        );
        assert!(pending.is_none());
        assert_eq!(sequence, 0);
        let mut guard = BoundaryGuard::new(snapshot.boot(), now());
        guard.install(now(), snapshot).unwrap();
        assert!(guard.check(now(), old_permit, OutputKind::Speech).is_err());
        assert!(matches!(
            provider_parent.data.receive::<ProviderOutput>().unwrap(),
            Some(ProviderOutput::Audio { sequence: 3, .. })
        ));
    }

    #[test]
    fn provider_interruption_revokes_delivered_permit_and_discards_queued_speech() {
        let (mut owner, current, permit) = active_reply();
        let before = owner.snapshot(now()).unwrap();
        let mut guard = BoundaryGuard::new(before.boot(), now());
        guard.install(now(), before).unwrap();
        assert!(guard.check(now(), permit, OutputKind::Speech).is_ok());
        let mut reply = Some(current);
        let mut trace = SessionState::default();
        cancel_reply(&mut owner, &mut reply, &mut trace, "provider_interrupted").unwrap();
        let after = owner.snapshot(now()).unwrap();
        guard.install(now(), after).unwrap();
        assert!(guard.check(now(), permit, OutputKind::Speech).is_err());
        assert!(reply.is_none());
        assert_eq!(trace[0]["outcome"], "provider_interrupted");
        assert!(after.owner().is_none());
    }

    #[test]
    fn a_100_ms_provider_burst_exposes_its_final_holdback_without_losing_pcm() {
        let (_, mut current, _) = active_reply();
        let original: Vec<i16> = (0..2400).map(|i| i as i16).collect();
        current.pcm = original.clone().into();
        let mut delivered = Vec::new();
        for _ in 0..9 {
            let (samples, final_chunk) = current.next_speaker_chunk().unwrap();
            assert!(!final_chunk);
            delivered.extend(samples);
        }
        assert!(current.next_speaker_chunk().is_none());
        assert_eq!(current.pcm.len(), 240);
        assert_eq!(current.held_final_samples(), 240);
        assert_eq!(&delivered, &original[..2160]);
        current.generation_done = true;
        assert_eq!(current.held_final_samples(), 0);
        let (samples, final_chunk) = current.next_speaker_chunk().unwrap();
        assert!(final_chunk);
        delivered.extend(samples);
        assert_eq!(delivered, original);
        assert!(current.next_speaker_chunk().is_none());
    }

    /// #3754 (the 20:05 HOLD on #3831): provider PCM that arrived but never
    /// reached the speaker is not "said": an invitation interrupted before
    /// the speaker accepted any of it stays `Unsaid` and gives the try back.
    #[test]
    fn invitation_pcm_queued_then_interrupted_before_acceptance_stays_unsaid() {
        let (_, mut current, _) = active_reply();
        let bound = Some((current.owner.boot(), current.owner.turn()));
        // Queued from the provider; the speaker accepted none of it.
        current.playback = None;
        current.sounded = false;
        current.had_audio = true;
        current.pcm = vec![7; 480].into();
        assert!(!current.reached_speaker());
        let mut audio = false;
        latch_invitation_sounded(&Some(current), bound, &mut audio);
        assert!(!audio, "provider PCM alone is not said");
        assert!(matches!(
            crate::onboarding::Delivery::unsettled(audio),
            crate::onboarding::Delivery::Unsaid
        ));
    }

    /// Audio the speaker accepted, then cut before the walk's next poll,
    /// was heard: the latch keeps it past the reply, so it is `Cut`.
    #[test]
    fn invitation_audio_accepted_then_cut_is_cut() {
        let (_, mut current, _) = active_reply();
        let bound = Some((current.owner.boot(), current.owner.turn()));
        // Accepted once, then paused: `sounded` alone carries it.
        current.playback = None;
        current.had_audio = true;
        current.sounded = true;
        let mut audio = false;
        latch_invitation_sounded(&Some(current), bound, &mut audio);
        assert!(audio);
        // The reply is then dropped by the cut; the latch outlives it.
        latch_invitation_sounded(&None, bound, &mut audio);
        assert!(matches!(
            crate::onboarding::Delivery::unsettled(audio),
            crate::onboarding::Delivery::Cut
        ));
    }

    /// Another turn's audio can't latch the bound invitation.
    #[test]
    fn another_owners_audio_does_not_latch_the_invitation() {
        let (_, mut current, _) = active_reply();
        current.had_audio = true;
        current.sounded = true;
        let other = Some((current.owner.boot(), current.owner.turn() + 1));
        let mut audio = false;
        latch_invitation_sounded(&Some(current), other, &mut audio);
        assert!(!audio);
        latch_invitation_sounded(&None, None, &mut audio);
        assert!(!audio);
    }

    #[test]
    fn empty_completion_is_not_fabricated_and_all_zero_provider_pcm_is_preserved() {
        let (_, mut current, _) = active_reply();
        current.pcm.clear();
        current.had_audio = false;
        current.generation_done = true;
        assert!(current.next_speaker_chunk().is_none());
        assert_eq!(current.delivery_outcome(), "no_audio_answer");
        current.pcm = vec![0; 240].into();
        current.had_audio = true;
        let (samples, final_chunk) = current.next_speaker_chunk().unwrap();
        assert_eq!(samples, vec![0; 240]);
        assert!(
            final_chunk,
            "zero-valued provider speech still has a final owned cursor"
        );
        assert!(current.next_speaker_chunk().is_none());
        // This internal outcome explicitly remains unscored; zeros do not prove
        // a substantive audible answer. The room recording decides that metric.
        assert_eq!(current.delivery_outcome(), "audio_written_unscored");
        current.pcm = vec![9; 73].into();
        let (samples, final_chunk) = current.next_speaker_chunk().unwrap();
        assert!(final_chunk);
        assert_eq!(&samples[..73], &[9; 73]);
        assert!(samples[73..].iter().all(|sample| *sample == 0));
    }

    #[test]
    fn invalid_playback_gap_receipts_leave_the_reply_and_trace_unchanged() {
        let (_, mut current, _) = active_reply();
        current.pcm = vec![7; 240].into();
        let gap = crate::wire::PlaybackGap {
            owner: current.owner,
            first_sample: lamp_audio::ownership::PlaybackCursor { epoch: 1, frame: 0 },
            end_sample: lamp_audio::ownership::PlaybackCursor {
                epoch: 1,
                frame: 240,
            },
            started_at_us: 100,
            last_zero_accepted_at_us: 100,
            zero_frames: 240,
        };
        let mut reply = Some(current);
        let mut trace = SessionState::default();
        for phase in [
            PlaybackGapPhase::Started,
            PlaybackGapPhase::Resumed,
            PlaybackGapPhase::Cancelled,
        ] {
            let mut invalid = gap;
            match phase {
                PlaybackGapPhase::Started => {
                    invalid.end_sample.frame = 241;
                    invalid.zero_frames = 241;
                }
                PlaybackGapPhase::Resumed => invalid.end_sample.epoch = 2,
                PlaybackGapPhase::Cancelled => invalid.last_zero_accepted_at_us = 101,
            }
            assert!(
                handle_playback_gap(
                    &mut reply,
                    None,
                    &mut trace,
                    WorkerEvent::PlaybackGap {
                        phase,
                        gap: invalid,
                        observed_at_us: 100
                    },
                    false,
                    None,
                )
                .is_err()
            );
            assert!(trace.is_empty());
            assert_eq!(reply.as_ref().unwrap().playback_gaps, 0);
            assert_eq!(reply.as_ref().unwrap().pcm, vec![7; 240]);
        }
    }

    #[test]
    fn accepted_gap_latches_discontinuous_delivery_and_old_gap_cannot_taint_new_reply() {
        let (_, mut current, _) = active_reply();
        current.pcm = vec![7; 240].into();
        let affected = current.owner;
        let mut reply = Some(current);
        let mut trace = SessionState::default();
        let gap = crate::wire::PlaybackGap {
            owner: affected,
            first_sample: lamp_audio::ownership::PlaybackCursor {
                epoch: 1,
                frame: 2640,
            },
            end_sample: lamp_audio::ownership::PlaybackCursor {
                epoch: 1,
                frame: 2880,
            },
            started_at_us: 11,
            last_zero_accepted_at_us: 11,
            zero_frames: 240,
        };
        let event = WorkerEvent::PlaybackGap {
            phase: PlaybackGapPhase::Started,
            gap,
            observed_at_us: 11,
        };
        let bytes = crate::wire::encode(&event).unwrap();
        assert!(bytes.len() < 1024);
        handle_playback_gap(
            &mut reply,
            None,
            &mut trace,
            crate::wire::decode(&bytes).unwrap(),
            false,
            None,
        )
        .unwrap();
        let current = reply.as_ref().unwrap();
        assert_eq!(
            current.pcm,
            vec![7; 240],
            "gap accounting cannot consume owed speech"
        );
        assert_eq!(current.playback_gaps, 1);
        assert_eq!(
            current.delivery_outcome(),
            "audio_written_with_playback_gaps_unscored"
        );
        assert_eq!(trace[0]["held_final_frames_at_observation"], 240);
        let (_, next, _) = active_reply();
        assert_ne!(next.owner, affected);
        reply = Some(next);
        handle_playback_gap(
            &mut reply,
            None,
            &mut trace,
            WorkerEvent::PlaybackGap {
                phase: PlaybackGapPhase::Cancelled,
                gap,
                observed_at_us: 12,
            },
            false,
            None,
        )
        .unwrap();
        assert_eq!(reply.as_ref().unwrap().playback_gaps, 0);
        assert_eq!(
            reply.as_ref().unwrap().delivery_outcome(),
            "audio_written_unscored"
        );
        assert!(trace[1]["reply_queued_frames_at_observation"].is_null());
    }

    /// Nine's P3s on #1264: a listening gap's zeros (#828) are no delivery
    /// gap. Started and resumed inside its hold, the reply keeps no playback
    /// gap and its outcome stays clean, and the record says `listen_gap`.
    /// One whose zeros ran on past the hold is counted once it resumes, and
    /// the record says by how much.
    #[test]
    fn a_listening_gaps_zeros_are_no_delivery_gap_unless_they_ran_past_its_hold() {
        for overran_us in [None, Some(300_000)] {
            let (_, mut current, _) = active_reply();
            current.pcm = vec![7; 240].into();
            let gap = crate::wire::PlaybackGap {
                owner: current.owner,
                first_sample: lamp_audio::ownership::PlaybackCursor {
                    epoch: 1,
                    frame: 2640,
                },
                end_sample: lamp_audio::ownership::PlaybackCursor {
                    epoch: 1,
                    frame: 2880,
                },
                started_at_us: 11,
                last_zero_accepted_at_us: 11,
                zero_frames: 240,
            };
            let mut reply = Some(current);
            let mut trace = SessionState::default();
            handle_playback_gap(
                &mut reply,
                None,
                &mut trace,
                WorkerEvent::PlaybackGap {
                    phase: PlaybackGapPhase::Started,
                    gap,
                    observed_at_us: 11,
                },
                true,
                None,
            )
            .unwrap();
            assert_eq!(reply.as_ref().unwrap().playback_gaps, 0);
            assert_eq!(trace[0]["listen_gap"], true);
            let resumed = crate::wire::PlaybackGap {
                end_sample: lamp_audio::ownership::PlaybackCursor {
                    epoch: 1,
                    frame: 2640 + 6_000,
                },
                last_zero_accepted_at_us: 250_011,
                zero_frames: 6_000,
                ..gap
            };
            handle_playback_gap(
                &mut reply,
                None,
                &mut trace,
                WorkerEvent::PlaybackGap {
                    phase: PlaybackGapPhase::Resumed,
                    gap: resumed,
                    observed_at_us: 250_011,
                },
                true,
                overran_us,
            )
            .unwrap();
            let current = reply.as_ref().unwrap();
            assert_eq!(current.playback_gaps, u64::from(overran_us.is_some()));
            assert_eq!(
                current.delivery_outcome(),
                if overran_us.is_some() {
                    "audio_written_with_playback_gaps_unscored"
                } else {
                    "audio_written_unscored"
                }
            );
            assert_eq!(trace[1]["listen_gap_overran_us"], json!(overran_us));
        }
    }

    #[test]
    fn session_deadline_records_unfinished_reply_once() {
        let (mut owner, current, _) = active_reply();
        let mut reply = Some(current);
        let mut trace = SessionState::default();
        cancel_reply(&mut owner, &mut reply, &mut trace, "deadline_cancelled").unwrap();
        cancel_reply(&mut owner, &mut reply, &mut trace, "runtime_failed").unwrap();
        assert!(reply.is_none());
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0]["kind"], "turn_finished");
        assert_eq!(trace[0]["outcome"], "deadline_cancelled");
    }

    fn discarded(
        affected: TurnOwner,
        reason: PlaybackDiscardReason,
        other_owners: bool,
    ) -> WorkerEvent {
        WorkerEvent::PlaybackDiscarded {
            owner: affected,
            reason,
            queued_frames: 240,
            other_owners,
            observed_at_us: monotonic_us(),
        }
    }

    fn cancelled_tail_fixture() -> (Controller, Option<Reply>, SessionState, WorkerEvent, u64) {
        let (mut controller, old, _) = active_reply();
        let affected = old.owner;
        let mut reply = Some(old);
        let mut trace = SessionState::default();
        cancel_reply(&mut controller, &mut reply, &mut trace, "user_interrupted").unwrap();
        let successor = controller
            .admit(now(), AdmittedInput::Interruption)
            .unwrap();
        trace
            .receipts
            .admitted(
                successor,
                controller.snapshot(now()).unwrap().issued_at().as_micros(),
            )
            .unwrap();
        trace.push(
            json!({"kind":"input_admitted","owner":successor,"at_us":monotonic_us(),
            "authority_issued_at_us":controller.snapshot(now()).unwrap().issued_at().as_micros()}),
        );
        controller.input_ended(now(), successor).unwrap();
        let plan = controller
            .plan_output(now(), successor, OutputKind::Speech, None)
            .unwrap();
        let permit = controller.issue_output(now(), plan, 100_000).unwrap();
        let playback = controller.playback_started(now(), permit).unwrap();
        reply = Some(Reply {
            owner: successor,
            cloud: submitted_request(),
            local_input_ended: true,
            plan,
            playback: Some(playback),
            pcm: vec![456; 240].into(),
            generation_done: false,
            provider_idle: false,
            input_sequence: 1,
            had_audio: true,
            voice_level: 0,
            told_stop: None,
            final_chunk: None,
            playback_gaps: 0,
            notice: NoticeDelivery::default(),
            media: Default::default(),
            local_only: None,
            reminder: None,
            say: None,
            news: Vec::new(),
            last_provider_audio_at_us: None,
            start: StartGate::new(0),
            need: StartNeed::default(),
            handed: 0,
            sounded: false,
        });
        let started = monotonic_us();
        let observed = started + 20_000;
        let event = WorkerEvent::CancelledTailRetired {
            owner: affected,
            superseded_by: successor,
            first_sample: lamp_audio::ownership::PlaybackCursor {
                epoch: 1,
                frame: 156_024,
            },
            end_sample: lamp_audio::ownership::PlaybackCursor {
                epoch: 1,
                frame: 156_480,
            },
            queued_frames: 456,
            speech_frames: 288,
            started_at_us: started,
            observed_at_us: observed,
        };
        (controller, reply, trace, event, observed)
    }

    #[test]
    fn cancelled_tail_receipt_preserves_successor_and_original_cancelled_outcome() {
        let (mut controller, reply, mut trace, event, observed) = cancelled_tail_fixture();
        let previous_owner = controller.snapshot(now()).unwrap().owner();
        let previous_playback = reply.as_ref().unwrap().playback;
        let bytes = crate::wire::encode(&event).unwrap();
        assert!(bytes.len() < 1024);
        handle_cancelled_tail_retirement(
            &reply,
            &mut trace,
            crate::wire::decode(&bytes).unwrap(),
            observed,
        )
        .unwrap();
        assert_eq!(controller.snapshot(now()).unwrap().owner(), previous_owner);
        let current = reply.as_ref().unwrap();
        assert_eq!(current.playback, previous_playback);
        assert_eq!(current.pcm, vec![456; 240]);
        assert!(!current.generation_done);
        assert_eq!(trace[0]["outcome"], "user_interrupted");
        assert_eq!(trace[2]["kind"], "cancelled_tail_retired");
        assert_eq!(trace[2]["outcome"], "cancelled");
        // A duplicate cannot become a second delivery receipt.
        assert!(handle_cancelled_tail_retirement(&reply, &mut trace, event, observed).is_err());
        assert_eq!(trace.len(), 3);
    }

    #[test]
    fn late_cancelled_tail_does_not_require_the_successor_to_still_be_current() {
        let (mut controller, mut reply, mut trace, event, observed) = cancelled_tail_fixture();
        cancel_reply(&mut controller, &mut reply, &mut trace, "user_interrupted").unwrap();
        let third = controller
            .admit(now(), AdmittedInput::Interruption)
            .unwrap();
        handle_cancelled_tail_retirement(&reply, &mut trace, event, observed).unwrap();
        assert_eq!(controller.snapshot(now()).unwrap().owner(), Some(third));
        assert!(reply.is_none());
        assert_eq!(
            trace
                .iter()
                .filter(|entry| entry["kind"] == "turn_finished")
                .count(),
            2
        );
        assert_eq!(trace.last().unwrap()["outcome"], "cancelled");
    }

    #[test]
    fn cancelled_tail_rejects_invalid_ranges_deadlines_and_owner_identity() {
        let (_, reply, trace, event, observed) = cancelled_tail_fixture();
        let original = serde_json::to_value(event).unwrap();
        let started = original["started_at_us"].as_u64().unwrap();
        let mut invalid = Vec::new();
        for (field, value) in [
            ("queued_frames", json!(961)),
            ("speech_frames", json!(0)),
            ("speech_frames", json!(457)),
            ("started_at_us", json!(0)),
            ("started_at_us", json!(observed + 1)),
            (
                "observed_at_us",
                json!(started + lamp_audio::ownership::MAX_CANCELLED_TAIL_US),
            ),
            ("observed_at_us", json!(observed + 1)),
            ("superseded_by", original["owner"].clone()),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            invalid.push(changed);
        }
        let mut epoch = original.clone();
        epoch["end_sample"]["epoch"] = json!(2);
        invalid.push(epoch);
        let mut other_boot = original.clone();
        let (_, unrelated, _) = active_reply();
        other_boot["superseded_by"] = json!(unrelated.owner);
        invalid.push(other_boot);
        for value in invalid {
            let mut trace_copy = trace.clone();
            let event = serde_json::from_value(value).unwrap();
            assert!(
                handle_cancelled_tail_retirement(&reply, &mut trace_copy, event, observed).is_err()
            );
            assert_eq!(trace_copy, trace);
            assert_eq!(reply.as_ref().unwrap().pcm, vec![456; 240]);
        }
    }

    #[test]
    fn cancelled_tail_requires_actual_cancellation_and_following_admission() {
        let (_, reply, original, event, observed) = cancelled_tail_fixture();
        let WorkerEvent::CancelledTailRetired {
            owner,
            superseded_by,
            started_at_us,
            ..
        } = event
        else {
            unreachable!()
        };
        let cancelled = original[0]["at_us"].as_u64().unwrap();
        for case in 0..7 {
            let mut trace = SessionState::default();
            // Plausible JSON cannot mint live ownership/cancellation lineage.
            trace.extend(original.iter().cloned());
            if case != 0 {
                trace
                    .receipts
                    .cancelled(
                        owner,
                        cancelled,
                        if case == 1 {
                            "provider_interrupted"
                        } else {
                            "user_interrupted"
                        },
                    )
                    .unwrap();
            }
            if case != 2 {
                let successor = if case == 3 {
                    active_reply().1.owner
                } else {
                    superseded_by
                };
                let issued = match case {
                    4 => started_at_us + 1,
                    5 => cancelled - 1,
                    _ => started_at_us,
                };
                if case != 6 {
                    trace.receipts.admitted(successor, issued).unwrap();
                }
            }
            assert!(
                handle_cancelled_tail_retirement(
                    &reply,
                    &mut trace,
                    crate::wire::decode(&crate::wire::encode(&event).unwrap()).unwrap(),
                    observed
                )
                .is_err()
            );
        }
        let mut no_metadata = original.clone();
        no_metadata.clear();
        handle_cancelled_tail_retirement(&reply, &mut no_metadata, event, observed).unwrap();
        assert_eq!(no_metadata.len(), 1);
    }

    #[test]
    fn expired_current_playback_is_failed_delivery_not_a_completed_answer() {
        let (mut owner, current, _) = active_reply();
        let affected = current.owner;
        let mut reply = Some(current);
        let mut trace = SessionState::default();
        let event = discarded(affected, PlaybackDiscardReason::ExpiredPermit, false);
        // Exercise the actual typed process envelope; no display-string parsing.
        let bytes = crate::wire::encode(&event).unwrap();
        assert!(bytes.len() < 1024);
        let event = crate::wire::decode(&bytes).unwrap();
        assert!(handle_playback_discard(&mut owner, &mut reply, &mut trace, event).is_err());
        assert!(reply.is_none());
        assert!(owner.snapshot(now()).unwrap().owner().is_none());
        assert_eq!(trace[0]["reason"], "expired_permit");
        assert_eq!(trace[0]["expected_cancellation"], false);
        assert_eq!(trace[1]["outcome"], "playback_discarded");
        // The enclosing failure path cannot append a second or successful result.
        cancel_reply(&mut owner, &mut reply, &mut trace, "runtime_failed").unwrap();
        assert_eq!(trace.len(), 2);
    }

    #[test]
    fn a_head_or_button_stop_during_speech_drops_its_audio_without_ending_the_voice() {
        // lamp-4ace, 16 Oct 12:20: a tap on the head during a story, then the
        // speaker's stale-owner discard of the rest, ended the voice.
        for reason in ["hand_stop", "head_stop", "capture_gap"] {
            let (mut owner, current, _) = active_reply();
            let affected = current.owner;
            let mut reply = Some(current);
            let mut trace = SessionState::default();
            cancel_reply(&mut owner, &mut reply, &mut trace, reason).unwrap();
            handle_playback_discard(
                &mut owner,
                &mut reply,
                &mut trace,
                discarded(affected, PlaybackDiscardReason::StaleOwner, false),
            )
            .unwrap();
            let discard = (0..trace.len())
                .map(|row| &trace[row])
                .find(|row| row["kind"] == "playback_discarded")
                .unwrap();
            assert_eq!(discard["expected_cancellation"], true, "{reason}");
        }
    }

    /// The first builder's P1 on #1047: a stop said over the lamp's voice
    /// cancels as the hand's does, so the speaker's discard of the rest of
    /// its audio is expected and ends nothing, as for a head tap (#1024).
    #[test]
    fn a_spoken_stop_during_speech_drops_its_audio_without_ending_the_voice() {
        let (mut owner, current, _) = active_reply();
        let affected = current.owner;
        let mut reply = Some(current);
        let mut trace = SessionState::default();
        cancel_reply(
            &mut owner,
            &mut reply,
            &mut trace,
            crate::receipt_history::SPOKEN_STOP,
        )
        .unwrap();
        handle_playback_discard(
            &mut owner,
            &mut reply,
            &mut trace,
            discarded(affected, PlaybackDiscardReason::StaleOwner, false),
        )
        .unwrap();
        let discard = (0..trace.len())
            .map(|row| &trace[row])
            .find(|row| row["kind"] == "playback_discarded")
            .unwrap();
        assert_eq!(discard["expected_cancellation"], true);
    }

    #[test]
    fn cancelled_old_owner_discard_preserves_the_new_answer_and_permission() {
        let (mut owner, old, _) = active_reply();
        let affected = old.owner;
        let mut reply = Some(old);
        let mut trace = SessionState::default();
        cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
        let turn = owner.admit(now(), AdmittedInput::Interruption).unwrap();
        owner.input_ended(now(), turn).unwrap();
        let plan = owner
            .plan_output(now(), turn, OutputKind::Speech, None)
            .unwrap();
        let permit = owner.issue_output(now(), plan, 100_000).unwrap();
        let playback = owner.playback_started(now(), permit).unwrap();
        reply = Some(Reply {
            owner: turn,
            cloud: submitted_request(),
            local_input_ended: true,
            plan,
            playback: Some(playback),
            pcm: vec![456; 240].into(),
            generation_done: false,
            provider_idle: false,
            input_sequence: 1,
            had_audio: true,
            voice_level: 0,
            told_stop: None,
            final_chunk: None,
            playback_gaps: 0,
            notice: NoticeDelivery::default(),
            media: Default::default(),
            local_only: None,
            reminder: None,
            say: None,
            news: Vec::new(),
            last_provider_audio_at_us: None,
            start: StartGate::new(0),
            need: StartNeed::default(),
            handed: 0,
            sounded: false,
        });
        handle_playback_discard(
            &mut owner,
            &mut reply,
            &mut trace,
            discarded(affected, PlaybackDiscardReason::StaleOwner, false),
        )
        .unwrap();
        let current = reply.as_ref().unwrap();
        assert_eq!(current.owner, turn);
        assert_eq!(current.pcm, vec![456; 240]);
        assert_eq!(current.playback, Some(playback));
        assert_eq!(trace[1]["expected_cancellation"], true);
        let state = owner.snapshot(now()).unwrap();
        let mut guard = BoundaryGuard::new(state.boot(), now());
        guard.install(now(), state).unwrap();
        guard.check(now(), permit, OutputKind::Speech).unwrap();

        // An expiry cannot borrow the earlier cancellation classification, even
        // when the affected owner is older than the current answer.
        assert!(
            handle_playback_discard(
                &mut owner,
                &mut reply,
                &mut trace,
                discarded(affected, PlaybackDiscardReason::ExpiredPermit, false)
            )
            .is_err()
        );
        assert!(reply.is_none());
        assert_eq!(trace.last().unwrap()["outcome"], "playback_discarded");
    }

    #[test]
    fn mixed_or_unrecorded_owner_loss_never_counts_as_expected_cancellation() {
        for mixed in [false, true] {
            let (mut owner, current, _) = active_reply();
            let affected = current.owner;
            let mut reply = Some(current);
            let mut trace = SessionState::default();
            if mixed {
                cancel_reply(&mut owner, &mut reply, &mut trace, "provider_interrupted").unwrap();
            }
            assert!(
                handle_playback_discard(
                    &mut owner,
                    &mut reply,
                    &mut trace,
                    discarded(affected, PlaybackDiscardReason::StaleOwner, mixed)
                )
                .is_err()
            );
            assert!(
                trace
                    .iter()
                    .any(|event| event["kind"] == "playback_discarded"
                        && event["expected_cancellation"] == false)
            );
        }
    }

    fn unavailable_link(revision: u64, session: u64) -> ProviderLink {
        ProviderLink {
            revision,
            session,
            observed_at_us: monotonic_us(),
            source_clock: None,
            state: ProviderLinkState::Unavailable {
                reason: crate::provider_worker::Fault::Transport,
            },
        }
    }

    fn current_input() -> (Controller, Option<Reply>) {
        let (mut owner, mut reply, _) = active_reply();
        owner.cancel_turn(now(), reply.owner).unwrap();
        reply.owner = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        reply.plan = owner
            .plan_output(now(), reply.owner, OutputKind::Speech, None)
            .unwrap();
        reply.cloud = CloudRequest::new(1).unwrap();
        reply.local_input_ended = false;
        reply.playback = None;
        reply.pcm.clear();
        reply.had_audio = false;
        (owner, Some(reply))
    }

    fn start_command(reply: &Option<Reply>) -> ProviderInput {
        ProviderInput::Start {
            session: reply.as_ref().unwrap().cloud.session,
            request: reply.as_ref().unwrap().owner.turn(),
            privacy_generation: 1,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
        }
    }

    fn end_command(reply: &Option<Reply>) -> ProviderInput {
        ProviderInput::End {
            request: reply.as_ref().unwrap().owner.turn(),
            privacy_generation: 1,
            sound: false,
            cut_in: false,
        }
    }

    fn ended_output() -> ProviderOutput {
        ProviderOutput::Ended {
            reason: crate::provider_link::LinkFailure::NoResumableContext,
            fault: crate::provider_worker::Fault::Transport,
            attempts: 1,
        }
    }

    #[test]
    fn priority_outage_wins_over_queued_data_ready_without_revoking_owned_playback() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut provider) = pair(&dir, "provider");
        let (mut speaker_parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, current, _) = active_reply();
        let original_owner = current.owner;
        let mut reply = Some(current);
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(1, 1), monotonic_us()).unwrap();
        provider
            .control
            .send(WorkerEvent::ProviderLink {
                link: unavailable_link(2, 1),
            })
            .unwrap();
        provider
            .data
            .send(ProviderOutput::Ready { setup_us: 999 })
            .unwrap();
        let mut trace = SessionState::default();
        assert!(
            service_provider_control(&mut parent.control, &mut cloud, &mut 16, &mut trace).unwrap()
        );
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &parent.data.receive().unwrap().unwrap(),
        )
        .unwrap();
        assert!(!cloud.available());
        let mut admission = InputAdmission::default();
        let mut draining = false;
        let mut queue = VecDeque::new();
        apply_submission_gate(
            &cloud,
            &mut reply,
            &mut queue,
            &mut admission,
            &mut draining,
            &mut trace,
        )
        .unwrap();
        assert!(!draining, "submitted End needs no utterance drain");
        let mut state = owner.snapshot(now()).unwrap();
        assert!(state.listening_ready(now()));
        let mut pending = None;
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut speaker_parent.data,
            &mut pending,
            &mut 0,
            &mut state,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
        )
        .unwrap();
        let packet = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
        assert_eq!(packet.samples, vec![123; 240]);
        assert_eq!(state.owner(), Some(original_owner));
        assert_eq!(reply.as_ref().unwrap().pcm.len(), 240);
        assert_eq!(trace[0]["kind"], "provider_unavailable");
    }

    #[test]
    fn crossed_session_start_cannot_authorize_current_audio_or_a_speaker_write() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, mut reply) = current_input();
        let command = start_command(&reply);
        reply.as_mut().unwrap().cloud.submitted(&command).unwrap();
        let request = reply.as_ref().unwrap().owner.turn();
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(3, 2), monotonic_us()).unwrap();
        assert!(
            observe_provider_output(
                &mut cloud,
                &mut reply,
                &ProviderOutput::Started {
                    request,
                    session: 2,
                    waiting_for_barrier: false
                }
            )
            .is_err()
        );
        let mut event = provider_audio(1, vec![8; 480]);
        if let ProviderOutput::Audio { request: id, .. } = &mut event {
            *id = request;
        }
        assert!(observe_provider_output(&mut cloud, &mut reply, &event).is_err());
        // Even an accidentally populated PCM queue cannot bypass source proof.
        reply.as_mut().unwrap().pcm = vec![8; 480].into();
        reply.as_mut().unwrap().local_input_ended = true;
        owner
            .input_ended(now(), reply.as_ref().unwrap().owner)
            .unwrap();
        let mut state = owner.snapshot(now()).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut None,
            &mut 0,
            &mut state,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
        )
        .unwrap();
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        assert_eq!(reply.as_ref().unwrap().pcm.len(), 480);
        assert!(!reply.as_ref().unwrap().cloud.started);
    }

    #[test]
    fn would_block_end_is_not_submitted_and_recovery_never_replays_queued_tail() {
        let (_owner, mut reply) = current_input();
        let request = reply.as_ref().unwrap().owner.turn();
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(1, 1), monotonic_us()).unwrap();
        let mut queue = VecDeque::from([start_command(&reply), end_command(&reply)]);
        let mut sends = 0;
        flush_provider_input_with(&cloud, &mut reply, &mut queue, |_| {
            sends += 1;
            if sends == 2 {
                Err(io::ErrorKind::WouldBlock.into())
            } else {
                Ok(())
            }
        })
        .unwrap();
        let current = reply.as_ref().unwrap();
        assert!(current.cloud.start_submitted);
        assert!(!current.cloud.input_end_submitted);
        assert_eq!(queue.len(), 1);
        // Skip directly to Ready B: its coalesced outage is sufficient.
        cloud.observe(ready_link(3, 2), monotonic_us()).unwrap();
        let mut admission = InputAdmission::default();
        let mut draining = false;
        apply_submission_gate(
            &cloud,
            &mut reply,
            &mut queue,
            &mut admission,
            &mut draining,
            &mut SessionState::default(),
        )
        .unwrap();
        assert!(draining);
        assert!(queue.is_empty());
        assert!(
            reply.as_ref().unwrap().cloud.outcome.is_none(),
            "submitted Start still needs its exact outcome"
        );
        flush_provider_input_with(&cloud, &mut reply, &mut queue, |_| panic!("tail replayed"))
            .unwrap();
        // A's delayed source-stamped Started is legitimate even though B is now
        // available; it only confirms A, and cannot reopen input to B.
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::Started {
                request,
                session: 1,
                waiting_for_barrier: false,
            },
        )
        .unwrap();
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::TurnFailed {
                request,
                stage: crate::provider_worker::FailedStage::InputOpen,
                reason: crate::provider_worker::Fault::Transport,
            },
        )
        .unwrap();
        assert_eq!(
            reply.as_ref().unwrap().delivery_outcome(),
            "provider_failed"
        );
        assert!(!reply.as_ref().unwrap().cloud.can_submit(&cloud));
    }

    #[test]
    fn local_never_submitted_request_fails_without_fabricating_a_provider_receipt() {
        let (_owner, mut reply) = current_input();
        let mut queue = VecDeque::from([start_command(&reply)]);
        let mut draining = false;
        apply_submission_gate(
            &CloudAvailability::default(),
            &mut reply,
            &mut queue,
            &mut InputAdmission::default(),
            &mut draining,
            &mut SessionState::default(),
        )
        .unwrap();
        let current = reply.as_ref().unwrap();
        assert!(matches!(
            current.cloud.outcome,
            Some(RequestOutcome::Failed {
                stage: crate::provider_worker::FailedStage::NotDelivered,
                local_not_submitted: true,
                ..
            })
        ));
        assert!(queue.is_empty());
        assert!(!current.cloud.start_submitted);
        assert!(
            !current.local_input_ended,
            "cloud loss is not an acoustic endpoint"
        );
        assert!(draining);
    }

    #[test]
    fn ordered_failure_preserves_received_pcm_at_zero_new_credit_and_never_succeeds() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, mut current, _) = active_reply();
        current.pcm = vec![44; 240].into();
        let request = current.owner.turn();
        let mut reply = Some(current);
        let mut cloud = CloudAvailability::default();
        cloud
            .observe(unavailable_link(2, 1), monotonic_us())
            .unwrap();
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::TurnFailed {
                request,
                stage: crate::provider_worker::FailedStage::Responding,
                reason: crate::provider_worker::Fault::Transport,
            },
        )
        .unwrap();
        observe_provider_output(&mut cloud, &mut reply, &ended_output()).unwrap();
        assert!(cloud.output_ended());
        assert_eq!(reply.as_ref().unwrap().pcm.len(), 240);
        let mut state = owner.snapshot(now()).unwrap();
        let mut pending = None;
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut 0,
            &mut state,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
        )
        .unwrap();
        let sent = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
        assert!(sent.end_of_speech);
        assert_eq!(sent.samples, vec![44; 240]);
        assert_eq!(
            reply.as_ref().unwrap().delivery_outcome(),
            "provider_failed"
        );
        assert!(
            reply.as_ref().unwrap().playback.is_some(),
            "Ended is not playback retirement"
        );
        assert!(
            observe_provider_output(&mut cloud, &mut reply, &provider_audio(2, vec![1; 240]))
                .is_err()
        );
    }

    #[test]
    fn ended_waits_for_in_flight_start_refusal_and_does_not_mark_an_open_input_complete() {
        let (_owner, mut reply) = current_input();
        let request = reply.as_ref().unwrap().owner.turn();
        let start = start_command(&reply);
        reply.as_mut().unwrap().cloud.submitted(&start).unwrap();
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(1, 1), monotonic_us()).unwrap();
        observe_provider_output(&mut cloud, &mut reply, &ended_output()).unwrap();
        let mut queue = VecDeque::from([end_command(&reply)]);
        let mut draining = false;
        apply_submission_gate(
            &cloud,
            &mut reply,
            &mut queue,
            &mut InputAdmission::default(),
            &mut draining,
            &mut SessionState::default(),
        )
        .unwrap();
        assert!(reply.as_ref().unwrap().cloud.outcome.is_none());
        let refused = ProviderOutput::TurnFailed {
            request,
            stage: crate::provider_worker::FailedStage::NotDelivered,
            reason: crate::provider_worker::Fault::Transport,
        };
        observe_provider_output(&mut cloud, &mut reply, &refused).unwrap();
        assert_eq!(
            reply.as_ref().unwrap().delivery_outcome(),
            "provider_failed"
        );
        assert!(!reply.as_ref().unwrap().local_input_ended);
        assert!(
            observe_provider_output(&mut cloud, &mut reply, &refused).is_err(),
            "duplicate outcome"
        );
        assert!(observe_provider_output(&mut cloud, &mut reply, &ended_output()).is_err());
    }

    #[test]
    fn abandoned_stream_recovery_keeps_original_endpoint_and_only_accepts_fresh_speech() {
        let (mut owner, mut reply) = current_input();
        let request = reply.as_ref().unwrap().owner.turn();
        let start = start_command(&reply);
        reply.as_mut().unwrap().cloud.submitted(&start).unwrap();
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(1, 1), monotonic_us()).unwrap();
        let mut detector = TurnDetector::default();
        let mut admission = InputAdmission::default();
        let capture = CaptureLineage {
            worker: new_boot().unwrap(),
            epoch: 1,
            dsp_epoch: 1,
            privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
        };
        let base = monotonic_us() - 1_000;
        let mut first = None;
        let mut draining = false;
        let mut queue = VecDeque::new();
        let mut trace = SessionState::default();
        for sequence in 1..=90 {
            let context = AdmissionContext {
                capture,
                authority: owner.snapshot(now()).unwrap(),
            };
            let probability = if sequence <= 20 || sequence > 80 {
                0.99
            } else {
                0.0
            };
            let activity = detector.push(
                ObservedAudio {
                    sequence,
                    captured_at_us: base + sequence,
                    samples: [sequence as i16; 160],
                },
                probability,
            );
            let is_endpoint = matches!(activity, Activity::End(_));
            let update = admission
                .observe(activity, context, monotonic_us())
                .unwrap();
            if sequence == 6 {
                let AdmissionStep::Candidate(candidate) = update.step else {
                    panic!("no opening prefix")
                };
                first = Some(candidate.id);
                assert!(matches!(
                    admission
                        .decide(
                            Evidence {
                                candidate: candidate.id,
                                through_sequence: candidate.trigger_sequence,
                                produced_at_us: monotonic_us(),
                                verdict: Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly)
                            },
                            context,
                            monotonic_us()
                        )
                        .unwrap(),
                    Decision::Accepted(_)
                ));
                cloud
                    .observe(unavailable_link(2, 1), monotonic_us())
                    .unwrap();
                apply_submission_gate(
                    &cloud,
                    &mut reply,
                    &mut queue,
                    &mut admission,
                    &mut draining,
                    &mut trace,
                )
                .unwrap();
                assert!(draining);
            } else if sequence < 80 {
                assert!(matches!(update.step, AdmissionStep::None));
                if sequence == 10 {
                    cloud.observe(ready_link(3, 2), monotonic_us()).unwrap();
                }
            } else if sequence == 80 {
                assert!(is_endpoint);
                assert!(matches!(update.step, AdmissionStep::None));
                draining = false;
                let current = reply.as_mut().unwrap();
                current.local_input_ended = true;
                owner.input_ended(now(), current.owner).unwrap();
            } else if sequence == 86 {
                let AdmissionStep::Candidate(candidate) = update.step else {
                    panic!("fresh input lost")
                };
                assert_ne!(Some(candidate.id), first);
                let accepted = admission
                    .decide(
                        Evidence {
                            candidate: candidate.id,
                            through_sequence: candidate.trigger_sequence,
                            produced_at_us: monotonic_us(),
                            verdict: Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
                        },
                        context,
                        monotonic_us(),
                    )
                    .unwrap();
                let Decision::Accepted(accepted) = accepted else {
                    panic!("fresh input not accepted")
                };
                assert_eq!(accepted.audio.first().unwrap().sequence, 81);
                break;
            }
        }
        assert!(!draining);
        assert_eq!(cloud.ready_session(), Some(2));
        assert_eq!(reply.as_ref().unwrap().owner.turn(), request);
        assert!(reply.as_ref().unwrap().cloud.input_abandoned);
        assert!(queue.is_empty());
        assert_eq!(trace.len(), 1);
    }

    #[test]
    fn assumed_ownership_grants_nothing_and_completion_without_submitted_end_fails_closed() {
        let (_owner, mut reply) = current_input();
        let start = start_command(&reply);
        reply.as_mut().unwrap().cloud.submitted(&start).unwrap();
        let request = reply.as_ref().unwrap().owner.turn();
        let mut cloud = CloudAvailability::default();
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::Started {
                request,
                session: 1,
                waiting_for_barrier: false,
            },
        )
        .unwrap();
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::OwnershipAssumed {
                request: Some(request),
                superseded: None,
            },
        )
        .unwrap();
        assert!(reply.as_ref().unwrap().cloud.outcome.is_none());
        assert!(
            observe_provider_output(
                &mut cloud,
                &mut reply,
                &ProviderOutput::TurnComplete {
                    request,
                    idle: true
                }
            )
            .is_err()
        );
        assert!(reply.as_ref().unwrap().cloud.outcome.is_none());
    }

    #[test]
    fn actual_fixture_worker_enters_the_same_priority_readiness_gate() {
        use sha2::{Digest, Sha256};
        let dir = SessionDirectory::create().unwrap();
        let wav = dir.path.join("reply.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 24_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..240 {
            writer.write_sample(100_i16).unwrap();
        }
        writer.finalize().unwrap();
        let hash = format!("{:x}", Sha256::digest(std::fs::read(&wav).unwrap()));
        let (mut parent, worker) = pair(&dir, "provider");
        let mut startup = StartupEvents::default();
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                crate::fixture_provider::run(worker, new_boot().unwrap(), &wav, &hash, false)
            });
            let until = Instant::now() + Duration::from_secs(1);
            loop {
                if let Some(event) = parent.control.receive::<WorkerEvent>().unwrap() {
                    assert!(matches!(event, WorkerEvent::Ready));
                    break;
                }
                assert!(Instant::now() < until, "fixture process readiness deadline");
                std::thread::yield_now();
            }
            while !startup.cloud.available() {
                startup.poll(&mut parent, "provider").unwrap();
                assert!(
                    Instant::now() < until,
                    "fixture priority availability deadline"
                );
                std::thread::yield_now();
            }
            assert_eq!(startup.cloud.ready_session(), Some(1));
            assert_eq!(startup.events.len(), 1);
            assert_eq!(startup.events[0]["kind"], "provider_ready");
            assert!(
                parent.data.receive::<ProviderOutput>().unwrap().is_none(),
                "no credited Ready may bypass the gate"
            );
            parent.control.send(Control::Stop).unwrap();
            handle.join().unwrap().unwrap();
        });
    }

    fn local_policy_decision(
        owner: &mut Controller,
        admission: &mut InputAdmission,
        verdict: Verdict,
    ) -> Decision {
        let context = AdmissionContext {
            capture: CaptureLineage {
                worker: new_boot().unwrap(),
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
            },
            authority: owner.snapshot(now()).unwrap(),
        };
        let at = monotonic_us();
        let update = admission
            .observe(
                Activity::Start(
                    (1..=6)
                        .map(|sequence| ObservedAudio {
                            sequence,
                            captured_at_us: at - 7 + sequence,
                            samples: [123; 160],
                        })
                        .collect(),
                ),
                context,
                at,
            )
            .unwrap();
        let AdmissionStep::Candidate(candidate) = update.step else {
            panic!("no candidate")
        };
        admission
            .decide(
                Evidence {
                    candidate: candidate.id,
                    through_sequence: candidate.trigger_sequence,
                    produced_at_us: monotonic_us(),
                    verdict,
                },
                context,
                monotonic_us(),
            )
            .unwrap()
    }

    #[test]
    fn accepted_local_interruption_during_outage_revokes_old_playback_without_cloud_submission() {
        let (mut owner, current, permit) = active_reply();
        let old = current.owner;
        let mut reply = Some(current);
        let mut admission = InputAdmission::default();
        let decision = local_policy_decision(
            &mut owner,
            &mut admission,
            Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
        );
        let mut trace = SessionState::default();
        let accepted = accepted_decision(decision, &mut trace).unwrap().unwrap();
        let (state, draining) =
            accepted_without_cloud(accepted, &mut owner, &mut reply, &mut admission, &mut trace)
                .unwrap();
        assert!(reply.is_none());
        assert!(draining);
        assert!(state.owner().is_none());
        assert!(state.listening_ready(now()));
        let mut guard = BoundaryGuard::new(state.boot(), now());
        guard.install(now(), state).unwrap();
        assert!(guard.check(now(), permit, OutputKind::Speech).is_err());
        assert_eq!(trace[0]["outcome"], "user_interrupted");
        assert_eq!(trace[0]["turn"], old.turn());
        assert_eq!(trace[1]["outcome"]["stage"], "not_delivered");
        assert_eq!(trace[1]["outcome"]["local_not_submitted"], true);
        assert!(trace[1]["provider_request"].is_null());
        assert!(trace[1]["provider_session"].is_null());
        assert!(
            !trace
                .iter()
                .any(|event| event["kind"] == "provider_input_started")
        );
    }

    #[test]
    fn rejected_background_or_ignored_decision_during_outage_preserves_playback() {
        let (mut owner, current, permit) = active_reply();
        let old = current.owner;
        let reply = Some(current);
        let mut admission = InputAdmission::default();
        let decision = local_policy_decision(&mut owner, &mut admission, Verdict::Reject);
        let mut trace = SessionState::default();
        assert!(accepted_decision(decision, &mut trace).unwrap().is_none());
        assert!(accepted_decision(Decision::Ignored, &mut trace).is_err());
        let state = owner.snapshot(now()).unwrap();
        assert_eq!(state.owner(), Some(old));
        let mut guard = BoundaryGuard::new(state.boot(), now());
        guard.install(now(), state).unwrap();
        guard.check(now(), permit, OutputKind::Speech).unwrap();
        assert_eq!(reply.as_ref().unwrap().pcm.len(), 480);
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0]["kind"], "input_candidate_rejected");
    }
}

#[cfg(test)]
#[path = "coordinator/ring_trace_tests.rs"]
mod ring_trace_tests;

#[cfg(test)]
#[path = "coordinator/optional_ring_tests.rs"]
mod optional_ring_tests;
