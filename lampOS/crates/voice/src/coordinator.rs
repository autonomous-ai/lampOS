//! Finite, explicitly directed voice qualification on real hardware.
//!
//! This integration has optional guarded ring cues, but no addressee inference or automatic motor gestures.
//! Refuse to present its directed-session results as general desk-conversation
//! qualification. Audio-critical failures end the run; optional ring failure disables
//! presentation while retaining the child and truthful cleanup evidence.
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use crate::{
    activity::{Activity, ObservedAudio, TurnDetector},
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
mod cloud;
mod early_end;
mod endpoint;
mod hand;
mod notices;
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
/// How long after its trigger a new sound waits, with the switch, before it
/// may take the turn; and how long before its own decision deadline it is
/// decided at the latest, so that a real follow-up is never let go for
/// lateness (the duplex hold gives 1.2 s; the standard one 200 ms, which
/// leaves nearly nothing to wait).
const WAIT_KEEPS_FOR_US: u64 = 1_000_000;
const WAIT_DECIDES_BEFORE_US: u64 = 100_000;
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
    // Only an answer to someone heard: not the lamp's own hello, look,
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

struct Reply {
    owner: TurnOwner,
    cloud: CloudRequest,
    local_input_ended: bool,
    plan: OutputOwner,
    playback: Option<PlaybackToken>,
    pcm: VecDeque<i16>,
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
        self.provider_idle
            && self.cloud.outcome.is_some()
            && self.local_input_ended
            && self.pcm.is_empty()
            && speaker_pending.is_none()
            && self.playback.is_none()
            && self.notice.settled()
    }

    /// Whether this turn, as it ends, was the model's and heard whole:
    /// settled and complete, its audio played with no gap, nobody else
    /// owning the turn, and the mic allowed.
    fn heard_whole(&self, speaker_pending: &Option<PendingSpeaker>, snapshot: Snapshot) -> bool {
        self.settled(speaker_pending)
            && snapshot.owner().is_none()
            && snapshot.microphone_permission() == Permission::Allowed
            && matches!(self.cloud.outcome, Some(RequestOutcome::Complete))
            && self.had_audio
            && self.playback_gaps == 0
            && self.notice.origin() == SpeechOrigin::Provider
    }

    fn complete_reminder(&mut self, speaker_pending: &Option<PendingSpeaker>, snapshot: Snapshot) {
        if self.heard_whole(speaker_pending, snapshot)
            && let Some(reminder) = &mut self.reminder
        {
            reminder.completed();
        }
    }

    /// The news this turn told, as it ends: all of it when the turn was
    /// heard whole ([`Self::heard_whole`], the proof a say's `spoken`
    /// takes), and none otherwise, so its harness stays open. A Stop, a
    /// reply cut off or with no audio, or a muted mic is no news heard (the
    /// reviewer on #1425).
    fn heard_news(
        &mut self,
        speaker_pending: &Option<PendingSpeaker>,
        snapshot: Snapshot,
    ) -> Vec<String> {
        let news = std::mem::take(&mut self.news);
        if news.is_empty() || self.heard_whole(speaker_pending, snapshot) {
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
        let spoken = sound || self.heard_whole(speaker_pending, snapshot);
        say.answer(if spoken {
            crate::say::Outcome::Spoken
        } else {
            crate::say::Outcome::Dropped(self.delivery_outcome().to_owned())
        });
    }

    fn append_provider_audio(&mut self, samples: Vec<i16>, received_at_us: u64) {
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
    /// The privacy worker reads Controls' switch (`LAMP_PRIVACY_CONTROLS=on`):
    /// a switch it cannot read closes the cycle as a mute does, for the
    /// service to wait and read again, never ending it (#795).
    privacy_controls: bool,
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
    faint_level: Option<u8>,
    playout_start: PlayoutStart,
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

/// The provider session a turn nobody asked for may begin on: the hello,
/// the news, a reminder, the look the lamp said it would take, and the
/// second look. None while the keeper has paused the lamp; otherwise
/// exactly the ready session.
fn unasked_session(cloud: &CloudAvailability, paused: bool) -> Option<u64> {
    cloud.ready_session().filter(|_| !paused)
}

/// Whether the asleep face (#712) may show while the service waits: with its
/// switch on, and never with the Controls privacy route, whose wait paints
/// the physical-mute indication; the ring has one writer (#795).
fn asleep_face_allowed(switch_on: bool, privacy_controls: bool) -> bool {
    switch_on && !privacy_controls
}

/// Whether a tap on the head while the lamp waits out a mute shows the
/// asleep face for about a second (#711, First on #711, 17 Oct 10:18):
/// with head touch, where the asleep face is the muted look (not the
/// Controls privacy route, whose ring is the switch's own indication and
/// has one writer, #755), and not already shown all the time by
/// `LAMP_RING_ASLEEP=on`.
fn tap_face_allowed(head_touch: bool, asleep_on: bool, privacy_controls: bool) -> bool {
    head_touch && !asleep_on && !privacy_controls
}

/// The privacy wait (`wait`) of one muted cycle, with what the service does
/// meanwhile: each state into the trace and the slide's last state kept;
/// at each look, the reminders, the controls and a tap's face (#711). The
/// face is stopped, black and closed, before this returns. The wait is a
/// parameter so a test can run these very closures without a worker (the
/// reviewer on #1288).
fn muted_wait<F: FnMut() -> Option<crate::ring_asleep::Asleep>>(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    controls: &RefCell<Option<HandControls>>,
    reminders: &RefCell<Option<crate::reminders::Reminders>>,
    tap_face: Option<crate::ring_asleep::TapFace<F>>,
    mut health: Option<crate::bus_out::WaitHealth>,
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
    let outcome = wait(
        &mut |state, boot, at_us| {
            slide.set(Some(state));
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
            let (allowed, asleep, restored) = match &mut *controls.borrow_mut() {
                None => (true, false, false),
                Some(controls) => {
                    let (allowed, observation) = controls.quiescent_observation()?;
                    record_hand_pats(&mut trace.borrow_mut(), observation.pets)?;
                    answer_muted_tap(
                        tap_face.borrow_mut().as_mut(),
                        &observation,
                        may_hear(allowed, slide.get()),
                        &mut trace.borrow_mut(),
                    )?;
                    (allowed, observation.asleep, controls.restored_after_fault())
                }
            };
            // No session runs while it waits, so the wait says how the
            // voice is: its microphones off, and why (#1371).
            if let Some(health) = &mut health {
                let now = monotonic_us();
                let unreadable = unread_since.get().is_some_and(|since| {
                    now.saturating_sub(since) >= crate::bus_out::WAIT_UNREADABLE_AFTER_US
                });
                health.report(
                    now,
                    crate::bus_out::Waiting::of(slide.get(), unreadable, allowed, asleep, restored),
                );
            }
            Ok(allowed)
        },
    );
    if let Some(face) = tap_face.into_inner() {
        face.stop();
    }
    // The wait ends on the look that read the switch open, with no look
    // after it: say the voice is opening, not still off, until the
    // session's own report (#1376).
    if let (Ok(crate::service_privacy::WaitOutcome::Allowed), Some(health)) =
        (&outcome, &mut health)
    {
        health.report(monotonic_us(), crate::bus_out::Waiting::Opening);
    }
    outcome
}

/// One look of the muted wait at the head (#711): a tap taken while the
/// lamp still may not hear gets the asleep face for about a second, said in
/// the trace as `head_tap_muted_face`.
fn answer_muted_tap<F: FnMut() -> Option<crate::ring_asleep::Asleep>>(
    face: Option<&mut crate::ring_asleep::TapFace<F>>,
    observation: &crate::hand_controls::Observation,
    allowed: bool,
    trace: &mut SessionState,
) -> Result<()> {
    if let Some(face) = face
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
/// optional, in whichever slot it is (`rig.provider` while it is the active
/// route): one that exited is dropped by `second::dropped_if_gone` (#960).
/// Gemini's worker stays mandatory when it waits in `Route::worker`.
fn mandatory(worker: &Worker) -> bool {
    worker.role != "second"
}

impl Rig {
    /// A hand cancellation has already reached the speaker. Deliver revocation
    /// to every remaining child even if one local channel has gone away.
    fn publish_hand_revocation(&mut self, snapshot: Snapshot) -> io::Result<()> {
        let mut failure = None;
        for worker in [&mut self.capture, &mut self.privacy, &mut self.provider] {
            if let Err(error) = worker
                .channels
                .control
                .send(Control::Authority { snapshot })
                && mandatory(worker)
            {
                failure.get_or_insert(error);
            }
        }
        if let Some(route) = &mut self.second
            && let Err(error) = route.publish(snapshot)
            && mandatory(route.worker_mut())
        {
            failure.get_or_insert(error);
        }
        if let Some(motor) = &mut self.motor {
            motor.publish(snapshot);
        }
        self.ring.publish(snapshot);
        if let Some(camera) = &mut self.camera {
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
        for worker in [
            &mut self.speaker,
            &mut self.capture,
            &mut self.privacy,
            &mut self.provider,
        ] {
            if let Err(error) = worker
                .channels
                .control
                .send(Control::Authority { snapshot })
                && mandatory(worker)
            {
                return Err(error);
            }
        }
        if let Some(route) = &mut self.second
            && let Err(error) = route.publish(snapshot)
            && mandatory(route.worker_mut())
        {
            return Err(error);
        }
        self.ring.publish(snapshot);
        if let Some(camera) = self.camera.as_mut()
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
        for worker in [
            &mut self.provider,
            &mut self.capture,
            &mut self.speaker,
            &mut self.privacy,
        ]
        .into_iter()
        .chain(self.second.as_mut().map(second::Route::worker_mut))
        {
            if mandatory(worker) {
                worker.check_running()?;
            }
        }
        self.ring.check_running();
        Ok(())
    }
    fn shutdown(&mut self, trace: &mut SessionState) -> io::Result<()> {
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
        let mut failure = self.ring.cleanup_error();
        let ring_stop_requested_at_us = self.ring.stop_requested_at(stop_requested_at_us);
        let mut exited = vec![
            false;
            4 + usize::from(self.ring.is_some())
                + usize::from(self.camera.is_some())
                + usize::from(self.second.is_some())
        ];
        let mut receipts: [Option<DiagnosticFinished>; 2] = [None, None];
        let mut ring_blank_confirmed = self.ring.blank_confirmed();
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
                failure = Some(error);
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
                                failure =
                                    Some(io::Error::other("duplicate diagnostic finish receipt"));
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
                                failure =
                                    Some(io::Error::other("invalid ring shutdown write receipt"));
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
                                failure = Some(io::Error::other(format!(
                                    "{} failed during shutdown: {event:?}",
                                    worker.role
                                )));
                            }
                            let _ = record(
                                trace,
                                json!({"kind":"worker_shutdown_event", "worker":worker.role, "at_us":monotonic_us(), "details":event}),
                            );
                        }
                        Ok(None) => break,
                        Err(error) => {
                            // What it queued before ending cleanly may have
                            // aged out on the way; its exit is the check.
                            let ended_cleanly = privacy_controls
                                && worker.role == "privacy"
                                && matches!(worker.try_exit(), Ok(Some(status)) if status.success());
                            if !ended_cleanly {
                                failure = Some(error);
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
                                    failure = Some(error);
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
                                failure = Some(error);
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
                                    "at_us":monotonic_us(), "error":error.to_string()}),
                                );
                                failure = Some(error);
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
                            failure = Some(worker.terminate_and_reap(
                                io::ErrorKind::TimedOut,
                                "camera local-revoke/driver-teardown deadline expired".into(),
                            ));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            exited[index] = true;
                            failure = Some(worker.terminate_and_reap(
                                io::ErrorKind::Other,
                                format!("shutdown status failed: {error}"),
                            ));
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
                        let error = worker.terminate_and_reap(io::ErrorKind::TimedOut,
                            format!("did not stop within shared {} ms deadline; forced termination required", grace.as_millis()));
                        let _ = record(
                            trace,
                            json!({"kind":"worker_forced_shutdown", "worker":worker.role,
                            "at_us":monotonic_us(), "error":error.to_string()}),
                        );
                        failure = Some(error);
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
                failure = Some(error);
            }
        }
        // Mandatory hardware cleanup preceded bounded metadata reads. Native kill/wait
        // has no userspace hard deadline; its result is never hidden as success.
        if !ring_blank_confirmed {
            failure = Some(io::Error::other(
                "ring shutdown has no verified black-write receipt",
            ));
        }
        if let Some(watch) = camera_watch
            && let Err(error) = watch.finish()
        {
            failure = Some(error);
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
                failure = Some(error);
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
                    failure = Some(error);
                }
            }
        } else if receipts.iter().any(Option::is_some) {
            failure = Some(io::Error::other("unsolicited audio diagnostics receipt"));
        }
        failure.map_or(Ok(()), Err)
    }
}

#[derive(Clone, Copy)]
enum ProviderSource<'a> {
    Gemini(&'a Path),
    Fixture { wav: &'a Path, sha256: &'a str },
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
            Self::Fixture { wav, sha256 } => {
                Ok(vec!["--fixture".into(), text(wav)?, sha256.into()])
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
        ProviderSource::Fixture { wav, sha256 },
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
/// The provider refuses for now: its quota or its rate limit. The run ended
/// in order like any other terminal end.
#[derive(Debug)]
pub struct ProviderRefusesForNow;
impl std::fmt::Display for ProviderRefusesForNow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "terminal provider output settled; the provider refuses for now (quota or rate limit); service stopped without retry",
        )
    }
}
impl std::error::Error for ProviderRefusesForNow {}

/// How long the service stays before it leaves after such a refusal. Whoever
/// restarts it would ask the provider again at once: on lamp-4ace on
/// 2026-10-13 systemd did, every five seconds, about 380 times in half an
/// hour of a spent quota.
const REFUSAL_WAIT: Duration = Duration::from_secs(120);

pub fn run_serve(config: &Path, output: &Path, options: SessionOptions) -> Result<()> {
    options.validate_serve()?;
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
    if result
        .as_ref()
        .is_err_and(|error| error.is::<ProviderRefusesForNow>())
    {
        eprintln!(
            "lamp-voice: the provider refuses for now (quota or rate limit); staying {} s before leaving",
            REFUSAL_WAIT.as_secs()
        );
        let until = Instant::now() + REFUSAL_WAIT;
        while Instant::now() < until && termination.signal_name().is_none() {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    result
}

/// The first record of a run: what it was asked to do.
fn run_start_record(
    options: &DirectedOptions,
    lifetime: Lifetime,
    camera_requested: bool,
    motor_requested: bool,
    fixture: Option<&FixtureInfo>,
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
    lifetime.validate()?;
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
        SessionState::service(output)
    } else {
        SessionState::finite()
    };
    record(
        &mut trace,
        run_start_record(
            &options,
            lifetime,
            camera_config.is_some(),
            motor_options.is_some(),
            fixture.as_ref(),
            cues.is_some(),
        ),
    )?;
    // Bounded local reads happen once before any capture/speaker worker.
    let notices = FailureNotices::load(notice_options);
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
            // Not with the Controls privacy route: its wait paints the
            // physical-mute indication, and the ring has one writer (#795).
            let privacy_controls =
                matches!(crate::privacy_bus::root_from_environment(), Ok(Some(_)));
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
                    let outcome = muted_wait(
                        trace,
                        cues,
                        &controls,
                        &reminders,
                        tap_face,
                        crate::bus_out::WaitHealth::from_environment(),
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

fn record(trace: &mut SessionState, event: Value) -> Result<()> {
    trace.record(event).map_err(Into::into)
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
                        *snapshot = owner.snapshot(now())?;
                        io.publish_speaker_snapshot(*snapshot)?;
                        *last_publish = monotonic_us();
                        record(
                            trace,
                            json!({"kind":format!("{}_first_write",pending.origin.trace_name()),"origin":pending.origin,"notice":current.notice.evidence(),"turn":turn,"owner":current.owner,"playback_sequence":token.sequence(),"at_us":observed_at_us,"boundary":"ALSA accepted; acoustic onset unmeasured",
                                "start_gate":current.start_trace()}),
                        )?;
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
        | ProviderOutput::ReminderDeferred { .. }
        | ProviderOutput::InputDiscarded { .. } => {}
        // Only a provider configured to take a silent earlier response as
        // over reports this. It grants nothing here: the request still needs
        // its own Started before any of its output is usable.
        ProviderOutput::OwnershipAssumed { .. } => {}
        // Asked again by the loop once the turn is over (#926); a tool's
        // look is taken by the loop too (#941).
        ProviderOutput::Unclear { .. } | ProviderOutput::LookAsked { .. } => {}
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
}
impl StartupEvents {
    fn push(&mut self, event: Value) -> io::Result<()> {
        if self.events.len() >= 16 {
            return Err(io::Error::other("startup event capacity exceeded"));
        }
        self.events.push(event);
        Ok(())
    }
    fn poll(&mut self, channels: &mut WorkerChannels, role: &str) -> io::Result<()> {
        for _ in 0..16 {
            let Some(event) = channels.control.receive::<WorkerEvent>()? else {
                break;
            };
            match (&event, role) {
                (WorkerEvent::ProviderLink { link }, "provider") => {
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
                "provider" => match channels.data.receive::<ProviderOutput>()? {
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
                        return Err(io::Error::other("microphone opened before activation"));
                    }
                    break;
                }
                _ => return Err(io::Error::other("unexpected startup worker role")),
            }
        }
        if role == "provider" {
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
    arguments.extend(crate::not_speech::arguments_from_environment());
    arguments.extend(crate::not_speech::unclear_arguments_from_environment());
    arguments.extend(crate::not_speech::transcription_arguments_from_environment());
    arguments.extend(crate::policy_holds::arguments_from_environment());
    arguments.extend(crate::staff_note::arguments_from_environment());
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
    // A service that runs all day may keep a second voice route standing by.
    let second_config = second::configured(
        std::env::var_os(crate::second_config::SECOND_CONFIG_ENV)
            .filter(|_| lifetime.continuous() && matches!(source, ProviderSource::Gemini(_))),
    );
    let arguments = provider_arguments(
        source,
        &options,
        lifetime,
        camera.as_ref(),
        second_config.is_some(),
    )?;
    let second_arguments = second_config
        .map(|config| {
            second::arguments(
                &arguments,
                &config,
                crate::second_provider::worker::wait_cue_from_environment(),
                crate::second_provider::worker::down_line_from_environment(),
            )
        })
        .transpose()?;
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let mut provider = Worker::spawn_with_boot_and_tick(
        &executable,
        directory,
        "provider",
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
        crate::stop_words::listen() != crate::stop_words::Listen::Off
            && crate::stop_words::raw_source(),
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
    if lifetime.continuous() {
        ring_arguments.extend(ring_bus.iter().map(String::as_str));
        ring_arguments.extend(ring_eye.iter().map(String::as_str));
        ring_arguments.extend(ring_head.iter().map(String::as_str));
        ring_arguments.extend(ring_requests.iter().map(String::as_str));
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
    let privacy_root = crate::privacy_bus::root_from_environment()?;
    let privacy_arguments = crate::privacy_bus::worker_arguments(privacy_root.as_deref())?;
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
    let privacy = Worker::spawn_with_tick(
        &executable,
        directory,
        "privacy",
        boot,
        &privacy_arguments,
        || later_tick(second.as_mut()),
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
    Ok(Rig {
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: privacy_root.is_some(),
        ring,
        camera,
        motor: None,
        ring_channel_ceiling: options.ring_channel_ceiling,
        diagnostic_directory: diagnostic_directory.map(Path::to_path_buf),
        noise_suppression: options.noise_suppression,
        turn_taking: options.turn_taking,
        end_silence_blocks: options.end_silence_blocks,
        early_end_blocks: options.early_end_blocks,
        faint_level: options.faint_level,
        playout_start: options.playout_start,
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
    let mut rig = spawn_rig(
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
    )?;
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
    let requested_stop =
        guard_released_on_stop(result.is_ok() || interrupted, termination.signal_name());
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
    {
        if result.is_err() {
            eprintln!(
                "lamp-voice: a requested stop's cleanup reported a fault; the mute is kept as it was, not restored muted"
            );
        }
        controls.disarm_after_cleanup()?;
    }
    result
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
    mut gate: PrivacyGate,
    #[cfg(test)] host_deadline: Option<Instant>,
) -> Result<()> {
    let mut cloud = startup.cloud;
    let mut output_flow = startup.output_flow;
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
    let mut permitted = false;
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
    let mut answer_kept_for: Option<crate::admission::CandidateId> = None;
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
    // The turn that was asked while the room was busy, and when the lamp
    // began to be heard answering it.
    let mut asked_in_busy_room: Option<u64> = None;
    let mut busy_answer_since: Option<(u64, u64)> = None;
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
    let mut spoke_in_turn: Option<u64> = None;
    // The head tap's "hm?" (#711): its sound, made once for the cycle when
    // the head is read, and when it last played, so it plays at most once
    // in 30 s.
    let head_cue = if controls.as_ref().is_some_and(HandControls::head_touch) {
        let level = crate::head_cue::level_from_environment().map_err(io::Error::other)?;
        Some(crate::head_cue::hum(level))
    } else {
        None
    };
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
    // The answer whose words are arriving, and whether any of them has been
    // a word yet.
    let mut answer_words: Option<(u64, bool)> = None;
    // Saying hello first to someone who comes and sits down, when the eyes
    // keep a presence file for it; and when anything was last said to the
    // lamp or by it.
    let mut arrival = if lifetime.continuous() {
        crate::speak_up::Arrival::from_environment(monotonic_us())
    } else {
        None
    };
    let mut last_turn_us = monotonic_us();
    // When the lamp's own words last finished playing: only a reply that
    // played to its end, never one cut off (`LAMP_FOLLOWS_ON=on`, #829, the
    // first builder at 09:01 on 17 October).
    let mut lamp_words_ended_us: Option<u64> = None;
    // The first five minutes with a new person, led by the lamp
    // (`docs/onboarding.md`). Off unless `LAMP_ONBOARDING=on`.
    let mut onboarding = if lifetime.continuous() {
        crate::onboarding::Onboarding::from_environment()
    } else {
        None
    };
    // News from the work handed to agents on other computers, told
    // unasked at a good moment (`docs/tasks.md`).
    let mut task_news = if lifetime.continuous() {
        crate::task_news::TaskNews::from_environment()
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
    // What other parts ask the voice to say, skills first (#939,
    // `LAMP_SAY=on`).
    let mut says = if lifetime.continuous() {
        crate::say::Says::from_environment()
    } else {
        None
    };
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
    // With the session that made the call: the look's turn goes to it, or
    // the look is dropped.
    let mut look_held: Option<(u64, u64)> = None;
    // A turn held as unclear in a conversation, and when (#926): asked again
    // with a written turn once it is over.
    let mut asking = AskAgainState::default();
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
    // The voice's own timing, for the part that plans what the lamp shows
    // (docs/expression.md). Only if that is switched on.
    let mut expression_out = crate::expression_out::ExpressionOut::from_environment();
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
    // Listening gaps (#828, `LAMP_LISTEN_GAPS=on`): a long reply's
    // sentence ends held quiet, and a person heard in the lamp's own
    // silence. Off unless set; only with ears.
    let listen_gaps = duplex && crate::listen_gaps::on_from_environment();
    detector.keep_first_words(first_words);
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
            Ok(Some(config)) => crate::stop_words::RelayEars::start(
                {
                    eprintln!(
                        "lamp-voice: stop words: {}, through the second voice's speech-to-text at {}",
                        if always { "heard-always, all the lamp's speech" } else { "heard, in a yield" },
                        crate::stop_words::where_heard(&config)
                    );
                    config
                },
                always,
            )
                .map(|ears| {
                    if always {
                        crate::stop_words::StopWords::always(Box::new(ears))
                            .with_cadence(crate::stop_words::raw_source())
                            .with_hold(crate::stop_words::holds_for_own_words())
                    } else {
                        crate::stop_words::StopWords::new(Box::new(ears))
                    }
                })
                .map_err(|error| eprintln!("lamp-voice: stop words are not heard: {error}"))
                .ok(),
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
            "decision_budget_us":hold.decision_budget_us,
            "first_words_kept_blocks":first_words.then_some(crate::activity::REACH_BACK_BLOCKS),
            "listen_gaps":listen_gaps.then_some(json!({"after_ms":crate::listen_gaps::AFTER_FRAMES * 1_000 / u64::from(lamp_audio::RENDER_RATE),"quiet_run_ms":crate::listen_gaps::LOW_RUN_CHUNKS * 10,"gap_ms":crate::listen_gaps::GAP_US / 1_000,"silence_heard":true})),
            "stop_words_after_still_blocks":(stop_words.is_some() && !always).then_some(crate::stop_words::FINALIZE_AFTER_STILL_BLOCKS),
            "stop_words_always":(stop_words.is_some() && always).then_some(true),
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
        if lifetime.continuous()
            && privacy_sequence != 0
            && gate.state(now()) == PrivacyState::Unavailable
        {
            return Err(lost(
                "service privacy freshness lost before next observation".into(),
            ));
        }
        for _ in 0..16 {
            let received = rig.privacy.channels.control.receive::<WorkerEvent>();
            let received = match received {
                Err(error) if privacy_controls => return Err(service_privacy::unavailable(error)),
                received => received?,
            };
            let Some(event) = received else {
                break;
            };
            match event {
                WorkerEvent::Privacy {
                    acquired_at_us,
                    muted,
                } => {
                    privacy_sequence = privacy_sequence
                        .checked_add(1u64)
                        .ok_or_else(|| io::Error::other("privacy sequence overflow"))?;
                    gate.observe(
                        privacy_sequence,
                        MonoTime::from_micros(acquired_at_us),
                        now(),
                        muted,
                    );
                    if lifetime.continuous() {
                        match gate.state(now()) {
                            PrivacyState::Unavailable => {
                                return Err(lost(
                                    "invalid or stale service privacy observation".into(),
                                ));
                            }
                            PrivacyState::Muted => {
                                let observed_at_us = monotonic_us();
                                record(
                                    trace,
                                    json!({"kind":"physical_mute", "at_us":observed_at_us,"acquired_at_us":acquired_at_us,"fresh_observation":true}),
                                )?;
                                return Err(Box::new(PhysicalMute { observed_at_us }));
                            }
                            _ => {}
                        }
                    }
                }
                WorkerEvent::Fault { code } => return Err(lost(code)),
                _ => return Err(io::Error::other("unexpected privacy event").into()),
            }
        }
        // Provider loss closes cloud submissions, never the microphone authority
        // needed to retain interruptions or finish already-owned playback.
        let privacy_state = gate.state(now());
        if lifetime.continuous() {
            match privacy_state {
                PrivacyState::Muted => {
                    let at_us = monotonic_us();
                    record(
                        trace,
                        json!({"kind":"physical_mute", "at_us":at_us,"fresh_observation":true}),
                    )?;
                    return Err(Box::new(PhysicalMute {
                        observed_at_us: at_us,
                    }));
                }
                PrivacyState::Unavailable
                    if privacy_sequence != 0
                        || session_instant().saturating_duration_since(privacy_started)
                            >= Duration::from_micros(gate.freshness_us()) =>
                {
                    return Err(lost("physical privacy unavailable; not a user mute".into()));
                }
                _ => {}
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
        let spoken_stop = waited || heard_over;
        {
            if stop || muted || gap_cut || head_stop || spoken_stop {
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
                // A Stop, a head touch, the mute or a gap ends the unclear
                // turn's asking too: nothing is asked again after it (#926).
                if let Some(turn) = asking.cancelled() {
                    record(
                        trace,
                        json!({"kind":"ask_again_dropped","turn":turn,"at_us":monotonic_us(),"why":"cancelled with its turn"}),
                    )?;
                }
                // What a yield heard is dropped with the reply (#828).
                if let Some(words) = stop_words.as_mut() {
                    words.close();
                }
                // So is what a waited sound heard (#1123).
                if let Some(words) = wait_words.as_mut() {
                    words.close();
                }
                if gap_cut && !stop && !head_stop && !spoken_stop && !muted {
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
                submission_draining = admission.draining();
                look_wanted = None;
                answer_opening = None;
                second_look =
                    crate::second_look::SecondLook::default().with_the_note(crate::called::on());
                last_turn_us = monotonic_us();
                speech_ended = None;
                ended_turn = None;
                floor_held_for = None;
                asked_in_busy_room = None;
                busy_answer_since = None;
                use crate::receipt_history::{
                    CAPTURE_GAP, HAND_STOP, HEAD_SLEEP, HEAD_STOP, SOFTWARE_MUTED, SPOKEN_STOP,
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
                if spoken_stop && !muted {
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
        let can_open = privacy_state == PrivacyState::Allowed;
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
            return Err(lost("physical privacy closed or lost freshness".into()));
        }
        if can_open && !permitted {
            owner.set_microphone_permission(now(), Permission::Allowed)?;
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
                        // real owner has been revoked and published. No PCM
                        // ever received is the only automatic retry case.
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
                ProviderOutput::Unclear { request, why } => {
                    // Only for the turn in hand: one cancelled first (a hand's
                    // Stop, a capture gap) is never asked about.
                    let current = asking.unclear(
                        request,
                        reply.as_ref().map(|current| current.owner.turn()),
                        monotonic_us(),
                    );
                    record(
                        trace,
                        json!({"kind":"provider_turn_unclear","turn":request,"why":why,"at_us":monotonic_us(),"asked_again":current}),
                    )?;
                }
                ProviderOutput::LookAsked {
                    request,
                    session,
                    dropped,
                } => match dropped {
                    Some(why) if look.full() => record(
                        trace,
                        json!({"kind":"look_dropped","turn":request,"by":"tool","why":why,"at_us":monotonic_us()}),
                    )?,
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
                            look_held = Some((request, session));
                        }
                    }
                },
                ProviderOutput::TurnComplete { request, idle } => {
                    record(
                        trace,
                        json!({"kind":"provider_turn_complete","turn":request,"idle":idle,"at_us":monotonic_us()}),
                    )?;
                }
                ProviderOutput::Transcript {
                    request,
                    text,
                    finished,
                } => {
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
                    "fallen_back_at_end":fallen_back,"hearing_margin_db":playback_mask.hearing_margin_db(),
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
            // How loud what the detector calls speech has been lately, for
            // the record of each utterance: its last eight seconds.
            if probability.is_finite() && probability >= 0.5 {
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
            // Before the canceller, for the stop words, when they asked.
            let raw = frame.raw.take();
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
            }
            let samples = observation.samples;
            let captured_at_us = observation.captured_at_us;
            // The lamp's sound still reaches the microphone (#828,
            // `heard-always`).
            let sounding = frame.echo.as_ref().is_some_and(|echo| {
                echo.render_quiet_blocks < Hearing::EVALUATED.clear_after_blocks
            });
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
                    raw.as_deref().unwrap_or(&samples),
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
                    && words_awaited(reply.as_ref(), asked_until_us, monotonic_us());
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
                        continue;
                    }
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
                    // In a room busy with other people's talk, what the
                    // lamp has just been asked is not dropped for the next
                    // voice: until its answer begins, or 1.8 s have gone
                    // without one, a new sound does not take the turn.
                    let at_us = monotonic_us();
                    let holding = room.busy()
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
                    let mut waits = while_answer_waits(
                        wait,
                        reply.as_ref(),
                        &candidate,
                        admission.pending_ended(),
                        detector.pause_blocks(),
                        at_us,
                    );
                    // With `words`, its words too (step 2).
                    if let Some(words) = wait_words.as_mut() {
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
                    let Some(provider_session) = cloud
                        .ready_session()
                        .filter(|_| provider_control_empty && !submission_draining)
                    else {
                        if (offline || service_down) && !cloud.available() {
                            // Heard while offline, or while only the voice
                            // service is down: a turn of its own, never sent.
                            snapshot = commit_offline_input(
                                owner,
                                reply,
                                &mut speaker_pending,
                                &mut provider_queue,
                                accepted,
                                rig.playout_start,
                                service_down && !offline,
                                trace,
                                |state| rig.publish(state),
                            )?;
                            last_publish = monotonic_us();
                            turn_blocks.extend(loose_blocks.drain(..).map(|(_, level)| level));
                            continue;
                        }
                        let (state, draining) =
                            accepted_without_cloud(accepted, owner, reply, &mut admission, trace)?;
                        provider_queue.clear();
                        speaker_pending = None;
                        submission_draining = draining;
                        snapshot = state;
                        rig.publish(snapshot)?;
                        last_publish = monotonic_us();
                        continue;
                    };
                    // Someone is answered: the lamp has shown it is back.
                    back_owed_since = None;
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
                    snapshot = commit_admitted_input(
                        owner,
                        reply,
                        &mut speaker_pending,
                        &mut provider_queue,
                        accepted,
                        provider_session,
                        continues,
                        took_over,
                        stop_untold.take_for(provider_session, monotonic_us()),
                        follows_on,
                        rig.playout_start,
                        trace,
                        |state| rig.publish(state),
                    )?;
                    last_publish = monotonic_us();
                    if took_over {
                        took_over_turn = reply.as_ref().map(|current| current.owner.turn());
                    }
                    if let Some(current) = reply.as_ref() {
                        second_look.request_began(current.owner.turn(), monotonic_us());
                    }
                    // The sound just before a turn began is the turn's.
                    turn_blocks.extend(loose_blocks.drain(..).map(|(_, level)| level));
                }
                AdmissionStep::Audio(audio) => {
                    let Some(current) = reply.as_mut() else {
                        return Err(io::Error::other("speech continuation lost its owner").into());
                    };
                    let turn = current.owner.turn();
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
                                snapshot = continue_after_early_end(
                                    owner,
                                    reply,
                                    &mut speaker_pending,
                                    &mut provider_queue,
                                    opening,
                                    provider_session,
                                    rig.cut_keeps_name,
                                    rig.playout_start,
                                    trace,
                                    |state| rig.publish(state),
                                )?;
                                if took_over {
                                    took_over_turn =
                                        reply.as_ref().map(|current| current.owner.turn());
                                }
                                if let Some(current) = reply.as_ref() {
                                    second_look.request_began(current.owner.turn(), monotonic_us());
                                }
                            }
                            None => {
                                // No provider to take it: given up all the
                                // same, and the rest of the utterance is
                                // drained, as for any input it cannot take.
                                let displaced = reply.as_ref().map(|current| current.owner);
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
                        crate::not_speech::dropped(
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
                    // A sound, not something said. It is not sent on as a
                    // finished question: the turn ends here, unanswered.
                    if let Some(current) = reply.as_ref() {
                        let mut endpoint_record = json!({"kind":"local_endpoint","owner":current.owner,"turn":current.owner.turn(),"at_us":at,
                                "last_block_host_read_us":audio.captured_at_us,
                                "includes_silence_wait_ms":detector.last_endpoint().map_or(detector.endpoint_silence_ms(), |decision| decision.silence_ms),
                                "acoustic_speech_end":null,
                                "not_speech":{"speech_blocks":speech_levels.len(),"fewer_than":crate::not_speech::SCRAP_BLOCKS,
                                    "said_close":said_close(&speech_levels, &loud_blocks, &rise),
                                    "last_blocks":last_blocks(&judged_blocks)}});
                        if crate::not_speech::just_stopped(
                            stopped_at_us,
                            last_spoke_us,
                            monotonic_us(),
                        ) {
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
                    // Its End never went: the Stop it was to tell of goes to
                    // the next request (the first builder on #1148).
                    if let Some(stop) = reply.as_ref().and_then(|current| current.told_stop) {
                        stop_untold.given_back(stop);
                    }
                    cancel_reply(owner, reply, trace, "not_speech")?;
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
                    let mut endpoint_record = json!({"kind":"local_endpoint","owner":current.owner,"turn":current.owner.turn(),"at_us":at,"last_block_host_read_us":input_end_us,
                        "includes_silence_wait_ms":detector.last_endpoint().map_or(detector.endpoint_silence_ms(), |decision| decision.silence_ms),
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
        if let Some(at) = last_capture
            && monotonic_us().saturating_sub(at) >= CAPTURE_GAP_US
        {
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
            trace.receipts.completed(current.owner)?;
            snapshot = owner.snapshot(now())?;
            admission.owner_completed(current.owner, snapshot, monotonic_us())?;
            rig.publish(snapshot)?;
            last_publish = monotonic_us();
            current.complete_reminder(&speaker_pending, snapshot);
            current.answer_say(&speaker_pending, snapshot);
            let heard = current.heard_news(&speaker_pending, snapshot);
            if !heard.is_empty()
                && let Some(news) = task_news.as_ref()
            {
                news.heard(heard);
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
            let sound_finished = current.notice.origin().unshown();
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
            if cloud.refused_for_now() {
                return Err(ProviderRefusesForNow.into());
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
            choreography.set_offline(offline || service_down);
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
            bus.state(
                crate::ring_wire::RingPhase::from_snapshot(snapshot).filter(|_| !sound_now),
                clock,
            );
            bus.report(cloud.available(), clock, trace.mic_at_start);
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
        if let Some(eyes) = eyes_link.as_mut() {
            eyes.permit(
                snapshot.microphone_permission() == Permission::Allowed,
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
        // being heard, nothing owed, a provider ready to take the turn.
        if reply.is_some() || admission.owns_stream() {
            last_turn_us = clock;
        }
        // The look the lamp said it would take, as soon as it has finished
        // saying so. Someone speaking first takes the turn and the look is
        // dropped; so is one that could not begin within fifteen seconds of being asked for.
        if let Some((turn, mut asked_us)) = look_wanted {
            // A tool's look waits for its turn's first audio or its end; the
            // still's wait counts from the eyes' ask.
            let by_tool = look_held
                .filter(|(of, _)| *of == turn)
                .map(|(_, session)| session);
            let asked_again = asking.wanted().is_some_and(|(of, _)| of == turn);
            // A tool's look goes only to the session that made the call: one
            // renewed meanwhile never saw it, nor the second route, whose
            // sessions are numbered apart (only Gemini's worker asks a
            // tool's look). Worked out before the ask, so none is made.
            let gone = by_tool.is_some_and(|session| {
                rig.second
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
                            note,
                            with_picture,
                            "lamp_looks",
                            provider_session,
                            rig.playout_start,
                            trace,
                            |state| rig.publish(state),
                        )?;
                        last_publish = monotonic_us();
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
                    asking.done();
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
                            rig.playout_start,
                            trace,
                            |state| rig.publish(state),
                        )?;
                        last_publish = monotonic_us();
                    }
                }
            }
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
                    rig.playout_start,
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
                snapshot = commit_back_online(owner, reply, rig.playout_start, trace, |state| {
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
            arrival
                .as_ref()
                .is_some_and(|arrival| arrival.present(clock)),
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
                rig.playout_start,
                trace,
                |state| rig.publish(state),
            )?;
            last_reminder_turn = reply.as_ref().map(|current| current.owner.turn());
            last_publish = monotonic_us();
            begun_for(ticket, reply);
        }
        // A reminder held for its skill, and still offered, is said before
        // anything else the lamp would say unasked: the say service, the
        // hello and the news wait for it (Fourth on #1038).
        let reminder_waiting = reminders.as_ref().is_some_and(|reminders| {
            reminder_waits(&reminder_held, reminders.offer(monotonic_us()).as_ref())
        });
        // What another part asked the voice to say (#939): after a due
        // reminder, before the hello and the news, through a reminder's
        // gates and a quiet since the last turn that its priority sets.
        let say_now_us = if says.is_some() {
            monotonic_us()
        } else {
            clock
        };
        if let Some(says) = says.as_mut() {
            says.poll(say_now_us);
        }
        if let Some(says) = says.as_mut()
            && !reminder_waiting
            && let Some(quiet) = says.quiet_needed_us()
            && clock.saturating_sub(last_turn_us) >= quiet
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
            && playback_mask.available(say_now_us)
            && let Some(provider_session) = unasked_session(&cloud, paused)
            && let Some((turn, priority, ticket)) = says.take(say_now_us)
        {
            eprintln!(
                "lamp-voice: saying what the skill {} asked (priority {priority})",
                ticket.skill()
            );
            record(
                trace,
                json!({"kind":"skill_says","at_us":say_now_us,"skill":ticket.skill(),"priority":priority,"provider_session":provider_session}),
            )?;
            snapshot = commit_written_turn(
                owner,
                reply,
                &mut provider_queue,
                &turn,
                false,
                "skill_says",
                provider_session,
                rig.playout_start,
                trace,
                |state| rig.publish(state),
            )?;
            last_publish = monotonic_us();
            // Begun: the asker may move the body and the light with it.
            ticket.begun(monotonic_us());
            if let Some(current) = reply.as_mut() {
                current.say = Some(ticket);
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
                rig.playout_start,
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
            let can_speak = reply.is_none()
                && provider_queue.is_empty()
                && !admission.owns_stream()
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
                    note,
                    false,
                    "lamp_leads_setup",
                    provider_session,
                    rig.playout_start,
                    trace,
                    |state| rig.publish(state),
                )?;
                last_publish = monotonic_us();
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
                    rig.playout_start,
                    trace,
                    |state| rig.publish(state),
                )?;
                snapshot = now_state;
                last_hum_us = Some(at_us);
                last_publish = monotonic_us();
            }
        }
        if let Some(arrival) = arrival.as_mut() {
            arrival.watch(clock);
        }
        // A hello that is due and could be begun; with
        // `LAMP_HELLO_ASKS_POLICY=on` the policy is asked first and it
        // waits up to 2 s for the answer (`speak_up::Hello`).
        let hello_session = arrival
            .as_ref()
            .is_some_and(|arrival| {
                onboarding.as_ref().is_none_or(|walk| !walk.leading())
                    && !reminder_waiting
                    && arrival.due(clock.saturating_sub(last_turn_us), clock)
                    && reply.is_none()
                    && provider_queue.is_empty()
                    && !admission.owns_stream()
                    && !submission_draining
                    && snapshot.listening_ready(now())
            })
            .then(|| unasked_session(&cloud, paused))
            .flatten();
        let hello = arrival.as_mut().map(|arrival| {
            arrival.hello(
                hello_session.is_some(),
                clock.saturating_sub(last_turn_us),
                clock,
            )
        });
        if let Some(told) = arrival.as_mut().and_then(|arrival| arrival.take_told()) {
            record(trace, told)?;
        }
        if let Some(arrival) = arrival.as_mut()
            && let Some(provider_session) = hello_session
            && let Some(hello) = hello.filter(|hello| *hello != crate::speak_up::Hello::NotYet)
        {
            arrival.said(clock);
            if hello == crate::speak_up::Hello::Wait {
                eprintln!("lamp-voice: someone has sat down; the policy said not now");
                record(
                    trace,
                    json!({"kind":"hello_due","at_us":clock,"said":false,"policy":"wait","provider_session":provider_session}),
                )?;
            } else if arrival.speaks() {
                // By name: someone who asked the lamp to know their face,
                // else the name typed at setup (`LAMP_PERSON_NAME`, #806).
                let by_name = crate::person::hello_for(
                    crate::known_faces::known_from_environment().as_deref(),
                    crate::person::from_environment().as_deref(),
                );
                snapshot = commit_written_turn(
                    owner,
                    reply,
                    &mut provider_queue,
                    &by_name,
                    false,
                    "lamp_speaks_first",
                    provider_session,
                    rig.playout_start,
                    trace,
                    |state| rig.publish(state),
                )?;
                last_publish = monotonic_us();
            } else {
                eprintln!(
                    "lamp-voice: someone has sat down; a hello was due and is not switched on"
                );
                record(
                    trace,
                    json!({"kind":"hello_due","at_us":clock,"said":false,"provider_session":provider_session}),
                )?;
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
        if let Some(news) = task_news.as_mut()
            && !reminder_waiting
            && news.waiting().is_some()
            && crate::task_news::quiet_for_news(
                news_at_pause,
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
            && (arrival
                .as_ref()
                .is_some_and(|arrival| arrival.present(clock))
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
                    "at_pause":news_at_pause,"since_turn_ms":clock.saturating_sub(last_turn_us) / 1_000}),
            )?;
            snapshot = commit_written_turn(
                owner,
                reply,
                &mut provider_queue,
                &told.note,
                false,
                "lamp_tells_news",
                provider_session,
                rig.playout_start,
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
    record(
        trace,
        json!({"kind":kind,"at_us":at_us,"frame_sequence":sequence,
        "last_block_host_read_us":captured_at_us,"owner":owner,
        "reply_queued_frames":reply.as_ref().map(|current| current.pcm.len()),
        "hearing_margin_db":mask.hearing_margin_db(),
        "boundary":"coordinator hearing decision; not acoustic onset or stopped output"}),
    )?;
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
    match words.block(
        replying,
        sounding,
        mask.yield_in_force(),
        mask.still_blocks(),
        held,
        captured_at_us,
        samples,
    ) {
        crate::stop_words::Tell::Nothing => {}
        crate::stop_words::Tell::Await => mask.await_words(),
        crate::stop_words::Tell::Words { stop, count } => {
            mask.words(stop);
            eprintln!(
                "lamp-voice: a yield's words came: {count} {}, {}",
                if count == 1 { "word" } else { "words" },
                if stop {
                    "a stop phrase"
                } else {
                    "not a stop phrase"
                }
            );
            record(
                trace,
                json!({"kind":"stop_words","stop":stop,"words":count,"at_us":monotonic_us()}),
            )?;
        }
        crate::stop_words::Tell::Unheard => {
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
/// head tap's "hm?" (#711) shows the listening face, with the hum, which
/// the ring keeps at rest for the tap's window; a skill's sound (#1144)
/// shows nothing, so no frame is asked for while it plays.
fn tell_ring_of_turn(choreography: &mut RingChoreographer, origin: Option<SpeechOrigin>) {
    let cue = origin == Some(SpeechOrigin::HeadCue);
    choreography.set_listening(cue);
    choreography.set_unshown(origin.is_some_and(SpeechOrigin::unshown) && !cue);
}

/// A skill's sound asked for, if one waits: played now as a turn of the
/// lamp's own, on the head cue's speaker path, or refused now with why.
/// The authority's new snapshot when it plays.
#[allow(clippy::too_many_arguments)]
fn play_skill_sound(
    says: &mut crate::say::Says,
    now_is: &crate::say::SoundNow,
    at_us: u64,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Option<Snapshot>> {
    let Some((samples, ticket)) = says.take_sound(at_us) else {
        return Ok(None);
    };
    let decided = crate::say::may_play_sound(now_is);
    record(
        trace,
        json!({"kind":"skill_sound","at_us":at_us,"skill":ticket.skill(),"plays":decided.is_ok(),"why":decided.err()}),
    )?;
    if let Err(why) = decided {
        ticket.answer(crate::say::Outcome::Refused(why.into()));
        return Ok(None);
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
/// time already, and is told as it was.
fn news_for_skill(
    news: &mut crate::task_news::TaskNews,
    says: Option<&mut crate::say::Says>,
    by_skill: bool,
    held: &mut Option<(String, u64)>,
) -> Option<Option<crate::say::Ticket>> {
    let (key, progress, task) = news
        .waiting()
        .map(|front| (front.key.clone(), front.progress.is_some(), front.task()))?;
    for_skill(
        says,
        crate::say::Source::Tasks,
        by_skill && !progress,
        &key,
        task,
        held,
        monotonic_us(),
    )
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
    look_held: Option<(u64, u64)>,
    turn: u64,
    why: &str,
    trace: &mut SessionState,
) -> Result<()> {
    if look_wanted.take_if(|(of, _)| *of == turn).is_some() {
        let by = if look_held.is_some_and(|(of, _)| of == turn) {
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
}

impl AskAgainState {
    /// `request` was judged unclear. Taken only if it is the turn in hand
    /// (`current`): one cancelled first is never asked about.
    fn unclear(&mut self, request: u64, current: Option<u64>, at_us: u64) -> bool {
        if current != Some(request) {
            return false;
        }
        *self = Self {
            wanted: Some((request, at_us)),
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
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
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
    // It takes over from the lamp's own answer: its End, if queued here, says so (#926).
    cut_in: bool,
    // The Stop this request tells of, when it is the first after one
    // (#829): when it came, and the session it cut.
    after_stop: Option<(u64, u64)>,
    // Begun soon after the lamp's words finished (`crate::follows_on`).
    follows_on: bool,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
    let prefix = accepted.audio;
    let replacing = reply.is_some();
    let over_the_lamps_voice = lamps_voice_heard(reply.as_ref());
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
    record(trace, admitted)?;
    provider_queue.push_back(ProviderInput::Start {
        session: provider_session,
        request: turn.turn(),
        privacy_generation: snapshot.microphone_generation(),
        // The same utterance going on after the lamp ended it (#782). The
        // model is told of a cut only after an end sent early.
        continues,
        continues_cut_off: false,
        // Said over the lamp's reply: never judged for its voice (#829).
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
    });
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
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
    };
    // One candidate's audio, already bounded by the admission's hold. The
    // standard hold's prefix fits the standing bound as it always has.
    let bound = PROVIDER_QUEUE_BOUND.max(prefix.len() + 2);
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
    };
    current.input_sequence = accepted.audio.len() as u64;
    if accepted.ended {
        current.local_input_ended = true;
        owner.input_ended(now(), current.owner)?;
        snapshot = owner.snapshot(now())?;
        publish(snapshot)?;
    }
    *reply = Some(current);
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
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<Snapshot> {
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
    });
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
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
    };
    // At most a pre-roll, as a new candidate's opening is.
    let bound = PROVIDER_QUEUE_BOUND.max(opening.len() + 2);
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
    Ok(snapshot)
}

/// What a record says of an end sent early, by the time `at_us`.
fn early_end_json(sent: &early_end::Sent, at_us: u64) -> Value {
    json!({"at_us":sent.at_us,"last_block_host_read_us":sent.last_block_host_read_us,
        "before_this_ms":sent.ago_us(at_us) / 1_000})
}

/// Presentation failure disables only the optional ring. Cleanup is observed
/// in bounded slices; no wait, restart or hardware write runs on this path.
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
    rig.ring.poll_cleanup(trace)
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
    let permit = owner.issue_output(now(), current.plan, SPEAKER_PERMIT_LEASE_US)?;
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
    if gap.first_sample.epoch == 0
        || gap.first_sample.epoch != gap.end_sample.epoch
        || gap.zero_frames == 0
        || gap.end_sample.frame.checked_sub(gap.first_sample.frame) != Some(gap.zero_frames)
        || gap.started_at_us == 0
        || gap.last_zero_accepted_at_us < gap.started_at_us
        || observed_at_us < gap.last_zero_accepted_at_us
        || (phase == PlaybackGapPhase::Started && gap.zero_frames > 240)
    {
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
    mod call_wait;
    mod duplex;
    mod early_end;
    mod endpoint_session;
    mod failure_notice;
    mod hand_controls;
    mod pilot_faults;
    mod playout_continuity;
    mod playout_start;
    mod provider_pair;
    mod provider_terminal;
    mod reminder_roundtrip;
    mod reminders;
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
            second: None,
            end_silence_blocks: None,
            early_end_blocks: None,
            faint_level: None,
            microphone: crate::options::Microphone::Mic2,
            speaker,
            capture,
            provider,
            privacy,
            privacy_controls: false,
            ring: Some(ring).into(),
            camera: None,
            motor: None,
            ring_channel_ceiling: Some(24),
            diagnostic_directory: Some(directory.path.clone()),
            noise_suppression: NoiseSuppression::On,
            turn_taking: TurnTaking::VadOnly,
            playout_start: PlayoutStart::default(),
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
                local_only: None,
                reminder: None,
                say: None,
                news: Vec::new(),
                last_provider_audio_at_us: None,
                start: StartGate::new(0),
                need: StartNeed::default(),
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
            local_only: None,
            reminder: None,
            say: None,
            news: Vec::new(),
            last_provider_audio_at_us: None,
            start: StartGate::new(0),
            need: StartNeed::default(),
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
            local_only: None,
            reminder: None,
            say: None,
            news: Vec::new(),
            last_provider_audio_at_us: None,
            start: StartGate::new(0),
            need: StartNeed::default(),
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
            let handle = scope
                .spawn(|| crate::fixture_provider::run(worker, new_boot().unwrap(), &wav, &hash));
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
