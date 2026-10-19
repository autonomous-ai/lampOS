//! The second voice route: speech-to-text, an OpenAI-compatible chat model
//! and text-to-speech in place of one Live session, configured by
//! [`crate::second_config`].
//!
//! [`Engine`] speaks the provider protocol ([`ProviderInput`] in,
//! [`ProviderOutput`] out) over three services behind [`Services`]; the
//! worker loop in [`worker::run`] carries it over the ordinary provider IPC,
//! as [`crate::fixture_provider`] does; [`worker::serve`] is the process
//! `lamp-voice worker second` (role `second`).
//!
//! A spoken turn: `Started` at once and speech-to-text opened while the
//! person is still talking; at the end, their words (`Transcript` with no
//! request), then the chat answer cut into sentences, each said as a
//! `Transcript` for the request and its audio. Speech of a sentence starts
//! while the chat is still writing the next one; audio keeps sentence order.
//! A written turn is the same from the chat on.
//!
//! With `--sounds-checked` (`LAMP_SHORT_SOUNDS=quiet-room-checked`, #800),
//! a spoken turn whose end came marked as a short sound let through is
//! answered only when what was heard is, whole, a short word said to the
//! lamp ([`crate::not_speech::short_word`]), or it has a word in it and
//! the lamp's own last line was a question in a conversation going on
//! ([`Asked`], 17 October); otherwise it ends quietly after its words, as
//! the Live route holds it. This route has no ears and no eyes, so the
//! heard words and the thread are all that can show it was said to the
//! lamp: a sound said close or facing the lamp but not heard as a short
//! word is held here where the Live route would answer it.
//!
//! With a [`Heed`] (`--attention`, `worker::Heeding`), a spoken turn's words
//! are judged by the Live route's rule for whom it answers before the chat
//! is asked; words it holds end the turn unanswered (#1014).
//!
//! Failures are `TurnFailed` with the stage the coordinator accepts
//! (`coordinator/cloud.rs`): `InputOpen` while the request's input is still
//! coming (speech-to-text could not be opened or fed); `AwaitingResponse`
//! after its end and before any answer audio (final transcript, chat or
//! speech failed); `Responding` once some answer audio was sent.
//! `NotDelivered` is never used: `Started` always comes first. `Interrupted`
//! and `Ended` are never sent.
use crate::provider_flow::PACKET_SAMPLES;
use crate::provider_worker::{FailedStage, Fault, ProviderInput, ProviderOutput, ReminderDeferral};
use futures_util::{StreamExt, future::BoxFuture};
use lamp_ipc::monotonic_us;
use net::{ChatEvent, Message, Sentences};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

pub mod net;
pub mod probe;
mod services;
pub mod worker;

pub use services::{CHAT_WARM_ENVIRONMENT, Listener, Net, Services};

#[cfg(test)]
mod tests;

/// Said once before the first answer after a quiet spell, so the person
/// knows the different voice is the lamp's. Never said when this route is
/// the lamp's only voice ([`Engine::announcing`]).
pub const ANNOUNCEMENT: &str = "My voice sounds a little different just now.";
/// How long without an answer before the announcement is said again.
pub const ANNOUNCE_AFTER: Duration = Duration::from_secs(120);
/// Chat requests in one turn, tool rounds included.
pub const MAX_ROUNDS: usize = 4;
/// A turn's first chat round that has not begun (no word, no call) this
/// long after it was asked, which is as soon as speech to text gave the
/// words, is given up and the turn ends unanswered: an answer said 12 s
/// after the question is worse than none (lamp-4ace on 2026-10-16: first
/// events 1.6 to 20.5 s). A round after a tool ran is not bounded: what was
/// done has to be said, or the person asks again and it is done twice
/// (Second on #1049).
pub const BEGIN_WITHIN: Duration = Duration::from_secs(3);
/// With `LAMP_SECOND_WAIT_CUE=on` (sixth-P3, #754): a spoken turn whose
/// first round has not begun this long after the words makes one soft
/// sound, and then waits until [`BEGIN_WITHIN_AFTER_CUE`]: a "hmm" followed
/// by silence is the worst of both (the first builder, 15:43). On lamp-4ace
/// on 2026-10-16, 9 of 10 backup turns waited past 1.5 s.
pub const CUE_AFTER: Duration = Duration::from_millis(1_500);
pub const BEGIN_WITHIN_AFTER_CUE: Duration = Duration::from_millis(4_500);
/// A look's first round may take this long: a picture is slower to read
/// than words (lamp-52e6 on 2026-10-16, one of two real looks past 3 s; the
/// first builder, 21:25). Past it, after "Let me look.", the lamp says
/// [`COULD_NOT_LOOK`] rather than nothing.
pub const LOOK_BEGIN_WITHIN: Duration = Duration::from_secs(5);
/// What the lamp says when a look's first round has not begun within
/// [`LOOK_BEGIN_WITHIN`]: after "Let me look.", silence reads as broken.
pub const COULD_NOT_LOOK: &str = "I couldn't get a good look.";
/// A spoken request's words cut short before any of its answer was heard
/// are answered in the next line's place when that line is held, within
/// this ([`Shared::cut`], #1712).
pub const CARRY_WITHIN: Duration = Duration::from_secs(30);
/// The most of the last line's sound kept for the next: the audio queue's
/// six seconds, at 16 kHz.
pub const KEPT_SAMPLES: usize = 6 * 16_000;
/// A line beginning this soon after the last one ended, that one not
/// answered, goes on from it: its sound goes first ([`KeptSound`]).
pub const CARRY_AFTER_END: Duration = Duration::from_millis(1_500);
/// The last line's sound is let go this long after it ended, whatever comes.
pub const KEEP_AFTER_END: Duration = Duration::from_secs(2);
/// The function the model looks with on this route ([`Engine::looking_by_call`]).
pub const LOOK_CALL: &str = "take_a_look";
/// What the lamp says as it looks by a call the model made with no words.
pub const LOOKING: &str = "Let me look.";
/// With Gemini's speech, the answer's first piece goes once this many words
/// have come, if no sentence or clause ended sooner
/// ([`Engine::cutting_first_words`]; the first builder on #1712: "cut at the
/// first clause, a comma or about 6 words").
pub const FIRST_WORDS: usize = 6;
/// With the configured voice (ElevenLabs on the lamps), the first piece
/// goes once this many words have come (#1712, the first builder at 18:26:
/// a 9-word first clause waited 2.8 s before it went to the voice).
pub const CONFIGURED_FIRST_WORDS: usize = 4;
/// Where the worker reads how many words the first piece waits for: a
/// number from 0 (whole clauses and sentences only) to 20, for an A/B;
/// unset, [`FIRST_WORDS`] for Gemini's voice and [`CONFIGURED_FIRST_WORDS`]
/// for the configured one.
pub const FIRST_WORDS_ENVIRONMENT: &str = "LAMP_SECOND_FIRST_WORDS";

/// How many words the answer's first piece waits for, as `value` says
/// ([`FIRST_WORDS_ENVIRONMENT`]); `None`: whole clauses and sentences only.
pub fn first_words_for(value: Option<&str>, gemini_speech: bool) -> Option<usize> {
    match value.map(str::parse::<usize>) {
        Some(Ok(0)) => None,
        Some(Ok(words)) if words <= 20 => Some(words),
        _ if gemini_speech => Some(FIRST_WORDS),
        _ => Some(CONFIGURED_FIRST_WORDS),
    }
}
/// With `LAMP_SECOND_DOWN_LINE=on`: what the lamp says when a turn's first
/// round has not begun within its bound, rather than nothing, at most once
/// in [`DOWN_LINE_EVERY`] (the first builder, 2026-10-17 02:48: the relay's
/// chat stopped answering at 02:29 and the backup voice went silent).
pub const DOWN_LINE: &str = "I can't think right now; give me a minute.";
pub const DOWN_LINE_EVERY: Duration = Duration::from_secs(60);
/// The kept line is at most 4 s of speech.
const DOWN_LINE_MOST_SAMPLES: usize = 4 * 24_000;
/// The sounds, said by this route's own voice, so at the reply's level.
pub const CUE_WORDS: [&str; 3] = ["Hmm.", "Mm.", "Hm."];
/// A sound is at most 0.6 s, its last 30 ms faded out.
const CUE_MOST_SAMPLES: usize = net::TTS_SAMPLE_RATE as usize * 6 / 10;
const CUE_FADE_SAMPLES: usize = net::TTS_SAMPLE_RATE as usize * 3 / 100;
/// The tool whose call means the words were not for the lamp.
const NOT_FOR_ME: &str = "not_for_me";
/// Microphone packets (10 ms each) held while speech-to-text connects:
/// 30 s, more than its 10 s connect bound and as much as the coordinator
/// replays at once when a line moves to this route (`coordinator/second.rs`).
const AUDIO_QUEUE: usize = 3_000;
/// Output events waiting for the worker loop; the rest wait in their task.
const OUTPUT_QUEUE: usize = 4;
/// Sentences queued ahead of the one being said.
const SENTENCES_AHEAD: usize = 2;
/// Packets of one sentence held while an earlier one is said: a whole
/// reply's worth, so a waiting download is never left unread for long.
const SENTENCE_PACKETS: usize = crate::provider_flow::MAX_REPLY_SAMPLES / PACKET_SAMPLES;

/// Runs one tool call: `(request, heard, name, arguments)` to the result the
/// model reads. `heard` is false for a request the lamp wrote itself (a
/// `Written` turn): nothing anyone said can confirm work in it, as on the
/// Live route (`tools::run_call`).
pub type ToolRunner =
    Arc<dyn Fn(u64, bool, String, Value) -> BoxFuture<'static, Value> + Send + Sync>;
/// A tool runner carrying the admitted request's immutable memory authority.
pub type ScopedToolRunner = Arc<
    dyn Fn(u64, bool, String, Value, crate::memory_scope::ScopeLease) -> BoxFuture<'static, Value>
        + Send
        + Sync,
>;

/// The system message for a turn, built when the turn's chat begins.
pub type System = Arc<dyn Fn() -> Told + Send + Sync>;

/// What a turn's chat is told: the system message, and the exchanges just
/// before it as messages (user, then assistant), oldest first. Both come
/// from the thread, so they are the person's at the lamp only and nothing
/// that was forgotten (`crate::thread::Thread::forget`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Told {
    pub text: String,
    pub history: Vec<Message>,
}
impl From<String> for Told {
    /// With no exchanges before it.
    fn from(text: String) -> Self {
        Self {
            text,
            history: Vec::new(),
        }
    }
}
/// Whether a spoken turn's words were said to the lamp, judged before they
/// are answered: `(request, (began_us, ended_us), heard)` to why it is not
/// answered, or nothing. The window is the microphone's read time of the
/// request's first and last packets, so a line replayed to this route keeps
/// the time it was said. The worker's is the Live route's own rule
/// (`worker::Heeding`, `crate::attention`).
/// The rule's verdict on a spoken turn: its request, when its line was said,
/// what was heard, and the request it goes on from (`continues`): the one
/// begun just before it, in the same session.
pub type Heed =
    Arc<dyn Fn(u64, (u64, u64), &str, Option<u64>) -> Option<&'static str> + Send + Sync>;
/// Whether the lamp's own last line before `request` was a question, in a
/// conversation going on (`worker::lamp_just_asked`): then a checked sound
/// with a word in it is the person's answer, as on the Live route.
pub type Asked = Arc<dyn Fn(u64) -> bool + Send + Sync>;
/// The lamp's eye for a look: the newest picture, as a JPEG, or why there
/// is none (`worker::serve` reads the eyes' look file with the Live route's
/// bounds).
pub type Look = Arc<dyn Fn() -> Result<Vec<u8>, &'static str> + Send + Sync>;
/// The engine's clock, for the announcement's spell.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;
/// The note a spoken request's words earn when they name one of the staff
/// (`worker::staff_noting`, `crate::staff_note`): `(request, heard)` to the
/// note, or nothing.
pub type StaffNote = Arc<dyn Fn(u64, &str) -> Option<String> + Send + Sync>;
/// The note a spoken request earns when the lamp's name was heard as it
/// began (`worker::Heeding::telling_the_name`, `crate::called`): asked
/// after the request is judged, with its request.
pub type NameNote = Arc<dyn Fn(u64) -> Option<&'static str> + Send + Sync>;
/// The note a spoken request gets while a task this voice prepared waits
/// for the person's yes (`crate::agent_tasks::pending_note`): with its
/// request, to the note or nothing.
pub type TaskNote = Arc<dyn Fn(u64) -> Option<String> + Send + Sync>;
/// What the lamp did, for the rule for whom it answers
/// (`worker::Heeding::lamp_did`), as the Live worker tells its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Did {
    /// A task, a timer or a reminder asked of its parts, at this time: a
    /// question about its work later is for it (`attention::worked`).
    Worked(u64),
    /// Its own turn (news, a reminder) began to be heard at this time: a
    /// reply to it is for it (`attention::news_told`).
    Spoke(u64),
}
/// Where the engine says what the lamp did.
pub type LampDid = Arc<dyn Fn(Did) + Send + Sync>;
/// The note a spoken request gets on whether it called the lamp by name
/// ([`Engine::noting_whether_called`]): with its request and words.
pub type CallNote = Arc<dyn Fn(u64, &str) -> Option<&'static str> + Send + Sync>;
/// Told a spoken request's words once they are heard and it is to be
/// answered, before its chat begins ([`Engine::hearing_words`]).
pub type WordsHeard = Arc<dyn Fn(u64, &str) + Send + Sync>;
/// Whether a call, by its request and name, is held as not said to the
/// lamp ([`Engine::holding_calls`]).
pub type CallHold = Arc<dyn Fn(u64, &str) -> bool + Send + Sync>;

/// What every turn task shares.
struct Shared {
    services: Arc<dyn Services>,
    tools: ToolRunner,
    scoped_tools: Option<ScopedToolRunner>,
    system: System,
    clock: Clock,
    /// When answer audio was last sent.
    answered_at: Mutex<Option<Instant>>,
    /// Audio packets sent by this process.
    sequence: AtomicU64,
    out: mpsc::Sender<ProviderOutput>,
    heed: Option<Heed>,
    asked: Option<Asked>,
    /// The sounds made while a first round is awaited, when switched on.
    cues: Option<Arc<Cues>>,
    /// [`DOWN_LINE`], when switched on.
    down: Option<Arc<DownLine>>,
    /// A turn after a quiet spell opens with [`ANNOUNCEMENT`]: not when
    /// this route is the lamp's only voice (`LAMP_VOICE_ROUTE=second`).
    announce: bool,
    /// The words last heard, with their request: a request that goes on
    /// from one given up after an early end begins with them.
    heard_last: Mutex<Option<(u64, String)>>,
    /// The stop to be told (`after_stop`), with the request it is for.
    stop_untold: Mutex<Option<StopToTell>>,
    /// With `LAMP_STAFF_NOTE=on`: the note for words that name the staff.
    staff: Option<StaffNote>,
    /// With `LAMP_NAME_NOTE=on`: the note for a line the lamp's name began.
    name_note: Option<NameNote>,
    /// The note for a line let out only as the conversation going on.
    conversation_alone: Option<NameNote>,
    /// The note on whether a line called the lamp by name.
    call_note: Option<CallNote>,
    /// With the tasks part: the note that a prepared task waits for a yes.
    task_waiting: Option<TaskNote>,
    /// What the lamp did, told to the rule ([`Engine::telling_what_it_did`]).
    did: Option<LampDid>,
    /// Calls held as not said to the lamp ([`Engine::holding_calls`]).
    hold: Option<CallHold>,
    /// Told a spoken request's words before its chat ([`Engine::hearing_words`]).
    words_heard: Option<WordsHeard>,
    /// The model looks by calling [`LOOK_CALL`] ([`Engine::looking_by_call`]).
    look_by_call: bool,
    /// A spoken turn's chat begins on the line's early words
    /// ([`Engine::preparing`]).
    prepare: bool,
    /// The request begun last, with its session, for a look asked by a call.
    begun: Mutex<Option<(u64, u64)>>,
    /// The spoken request being answered and its words, until any of its
    /// answer is heard or its turn ends.
    in_flight: Mutex<Option<Carried>>,
    /// The last request whose answer was heard.
    answered_request: Mutex<Option<u64>>,
    /// A spoken request's words whose turn ended before any of its answer
    /// was heard (a newer line began): answered in the next line's place
    /// if that line is held ([`CARRY_WITHIN`], #1712).
    cut: Mutex<Option<Carried>>,
    /// The answer's first piece goes once this many words have come
    /// ([`Engine::cutting_first_words`]).
    first_words: Option<usize>,
}

/// The last spoken line's sound, at most [`KEPT_SAMPLES`], for a line that
/// goes on from it in the same session: one the coordinator says continues
/// it, or one begun within [`CARRY_AFTER_END`] of its end when it was not
/// answered. Let go [`KEEP_AFTER_END`] after its end, at the next start.
struct KeptSound {
    request: u64,
    session: u64,
    samples: VecDeque<i16>,
    /// When the line's first sound was read, by this process's clock.
    said_from: u64,
    /// When it ended, by the engine's clock; `None` while it goes on.
    ended: Option<Instant>,
}

/// A spoken request's words, kept while it is answered ([`Shared::in_flight`],
/// [`Shared::cut`]).
#[derive(Clone)]
struct Carried {
    request: u64,
    text: String,
    at: Instant,
}

/// A stop to be told beside a request's chat ([`crate::provider_worker::STOPPED_NOTE`]).
#[derive(Clone, Copy)]
struct StopToTell {
    request: u64,
    /// When the request that carried `after_stop` began: told only within
    /// [`crate::not_speech::TELL_STOP_WITHIN_US`] of it, as the coordinator
    /// tells it.
    since: tokio::time::Instant,
    /// Its request's chat was asked with it.
    asked: bool,
}

/// The wait's sounds, rendered once by [`Services::speak`] and kept in
/// memory; none is written anywhere.
#[derive(Default)]
struct Cues {
    clips: Mutex<Vec<Vec<i16>>>,
    rendering: AtomicBool,
    /// The clip played last, so the next is another.
    last: AtomicU64,
}

impl Cues {
    /// Render the sounds the first time a turn may need them.
    fn render(self: &Arc<Self>, services: &Arc<dyn Services>) {
        if self.rendering.swap(true, Ordering::AcqRel) {
            return;
        }
        let cues = self.clone();
        let services = services.clone();
        tokio::spawn(async move {
            for words in CUE_WORDS {
                let mut clip = Vec::new();
                let mut speech = services.speak(words.to_owned());
                while let Some(Ok(packet)) = speech.next().await {
                    clip.extend(packet);
                    if clip.len() >= CUE_MOST_SAMPLES {
                        break;
                    }
                }
                if let Ok(mut clips) = cues.clips.lock()
                    && !clip.is_empty()
                {
                    clips.push(soft(clip));
                }
            }
            // None came (speech failed): tried again at the next spoken turn.
            if cues.clips.lock().is_ok_and(|clips| clips.is_empty()) {
                eprintln!(
                    "lamp-voice second: the wait's sounds could not be said; trying again at the next turn"
                );
                cues.rendering.store(false, Ordering::Release);
            }
        });
    }

    /// The next sound, never the one played last; none until rendered.
    fn next(&self) -> Option<Vec<i16>> {
        let clips = self.clips.lock().ok()?;
        if clips.is_empty() {
            return None;
        }
        let index = (self.last.fetch_add(1, Ordering::Relaxed) as usize + 1) % clips.len();
        clips.get(index).cloned()
    }
}

/// [`DOWN_LINE`], rendered by [`Services::speak`] after a turn, while the
/// relay answers, and kept in memory, so it can be said when the chat no
/// longer does.
#[derive(Default)]
struct DownLine {
    clip: Mutex<Option<Vec<i16>>>,
    rendering: AtomicBool,
    said_at: Mutex<Option<tokio::time::Instant>>,
}

impl DownLine {
    fn render(self: &Arc<Self>, services: &Arc<dyn Services>) {
        if self.rendering.swap(true, Ordering::AcqRel) {
            return;
        }
        let line = self.clone();
        let services = services.clone();
        tokio::spawn(async move {
            let mut clip = Vec::new();
            let mut speech = services.speak(DOWN_LINE.to_owned());
            while let Some(packet) = speech.next().await {
                match packet {
                    Ok(packet) => clip.extend(packet),
                    Err(_) => {
                        clip.clear();
                        break;
                    }
                }
                if clip.len() >= DOWN_LINE_MOST_SAMPLES {
                    clip.truncate(DOWN_LINE_MOST_SAMPLES);
                    break;
                }
            }
            match line.clip.lock() {
                Ok(mut kept) if !clip.is_empty() => {
                    // The real length, for whether the 4 s cap ever cuts it
                    // (the reviewer on #1195).
                    eprintln!(
                        "lamp-voice second: the down line is {} ms",
                        clip.len() as u64 * 1000 / 24_000
                    );
                    *kept = Some(clip);
                }
                _ => {
                    eprintln!(
                        "lamp-voice second: the down line could not be said; trying again after the next turn"
                    );
                    line.rendering.store(false, Ordering::Release);
                }
            }
        });
    }

    /// The line, if rendered and not said in the last [`DOWN_LINE_EVERY`].
    fn due(&self) -> Option<Vec<i16>> {
        let said_at = *self.said_at.lock().ok()?;
        let now = tokio::time::Instant::now();
        if said_at.is_some_and(|at| now.duration_since(at) < DOWN_LINE_EVERY) {
            return None;
        }
        self.clip.lock().ok()?.clone()
    }

    /// The line was played (Nine on #1195: a cue that failed keeps it due).
    fn said(&self) {
        if let Ok(mut said_at) = self.said_at.lock() {
            *said_at = Some(tokio::time::Instant::now());
        }
    }
}

/// At most [`CUE_MOST_SAMPLES`], at half the reply's level (about -6 dB),
/// its end faded out.
fn soft(mut clip: Vec<i16>) -> Vec<i16> {
    clip.truncate(CUE_MOST_SAMPLES);
    for sample in &mut clip {
        *sample /= 2;
    }
    let len = clip.len();
    let fade = CUE_FADE_SAMPLES.min(len);
    for (index, sample) in clip[len - fade..].iter_mut().enumerate() {
        *sample = (i32::from(*sample) * (fade - index) as i32 / fade as i32) as i16;
    }
    clip
}

struct Turn {
    request: u64,
    /// Open until the request's End; `None` after it.
    audio: Option<mpsc::Sender<Vec<i16>>>,
    /// The End came marked as a short sound let through; set before the
    /// audio queue closes, read by the turn's task after.
    sound: Arc<AtomicBool>,
    /// When its first and last packets were read from the microphone.
    said: Arc<Mutex<(u64, u64)>>,
    task: JoinHandle<()>,
    memory_scope: Option<crate::memory_scope::RequestScope>,
}
impl Drop for Turn {
    /// A turn given up stops its chat and speech at once.
    fn drop(&mut self) {
        self.memory_scope.take();
        self.task.abort();
    }
}

/// The second route's protocol state: at most one turn at a time.
pub struct Engine {
    shared: Arc<Shared>,
    output: mpsc::Receiver<ProviderOutput>,
    /// Events the engine itself says (`Started`, refusals); before any of
    /// the turn task's, so `Started` always precedes the turn's output.
    urgent: VecDeque<ProviderOutput>,
    turn: Option<Turn>,
    retired_through: u64,
    /// The request begun last, with its session.
    begun_last: Option<(u64, u64)>,
    /// `--sounds-checked`: a short sound is answered only when heard as a
    /// short word.
    sounds_checked: bool,
    /// The eye a look reads, when the eyes keep a look file.
    look: Option<Look>,
    /// The last spoken line's sound, for a line that goes on from it
    /// ([`KeptSound`], #1712).
    kept: Option<KeptSound>,
    /// A due reminder is said, not deferred ([`Engine::saying_reminders`]).
    reminders: bool,
    /// The room kept on this route when it is the lamp's only voice
    /// ([`Engine::keeping_room`]): told on the bus and kept in its file.
    room_tick: Option<Box<dyn FnMut(u64) + Send>>,
    memory: Option<(lamp_turn_owner::BootId, Arc<crate::memory_scope::FaceCache>)>,
    memory_camera_allowed: bool,
    memory_camera_generation: Option<u64>,
}

/// The busy room as this route hears it, in this process's one room
/// (`crate::room_told::room`), as the Live worker keeps its own: a line
/// heard and ended, the rule's verdict, the lamp speaking, doing something
/// or leaving a line unanswered. Read only when this route is the lamp's
/// only voice (`worker::Heeding::as_the_only_voice`); with Gemini's worker
/// beside it, the room is that worker's, read from the bus.
fn room(change: impl FnOnce(&mut crate::room_told::RoomTold)) {
    if let Ok(mut room) = crate::room_told::room().lock() {
        change(&mut room);
    }
}

impl Engine {
    /// `system` is asked for the system message at the start of each
    /// turn's chat (character, memory note and conversation so far).
    pub fn new(
        services: Arc<dyn Services>,
        system: impl Fn() -> Told + Send + Sync + 'static,
        tools: ToolRunner,
    ) -> Self {
        Self::with_clock(services, Arc::new(system), tools, Arc::new(Instant::now))
    }

    pub fn with_clock(
        services: Arc<dyn Services>,
        system: System,
        tools: ToolRunner,
        clock: Clock,
    ) -> Self {
        let (out, output) = mpsc::channel(OUTPUT_QUEUE);
        Self {
            shared: Arc::new(Shared {
                services,
                tools,
                scoped_tools: None,
                system,
                clock,
                answered_at: Mutex::new(None),
                sequence: AtomicU64::new(0),
                out,
                heed: None,
                asked: None,
                cues: None,
                down: None,
                announce: true,
                heard_last: Mutex::new(None),
                stop_untold: Mutex::new(None),
                staff: None,
                name_note: None,
                conversation_alone: None,
                call_note: None,
                task_waiting: None,
                did: None,
                hold: None,
                words_heard: None,
                look_by_call: false,
                prepare: false,
                begun: Mutex::new(None),
                in_flight: Mutex::new(None),
                answered_request: Mutex::new(None),
                cut: Mutex::new(None),
                first_words: None,
            }),
            output,
            urgent: VecDeque::new(),
            turn: None,
            retired_through: 0,
            begun_last: None,
            sounds_checked: false,
            look: None,
            kept: None,
            reminders: false,
            room_tick: None,
            memory: None,
            memory_camera_allowed: false,
            memory_camera_generation: None,
        }
    }

    /// Bind tools to the observation at admission, independent of later face changes.
    pub fn with_memory_scope(
        mut self,
        boot: lamp_turn_owner::BootId,
        cache: Arc<crate::memory_scope::FaceCache>,
        runner: ScopedToolRunner,
    ) -> Self {
        Arc::get_mut(&mut self.shared)
            .expect("configured before admission")
            .scoped_tools = Some(runner);
        self.memory = Some((boot, cache));
        self
    }

    fn memory_camera(&mut self, allowed: bool, generation: u64) {
        let changed = self
            .memory_camera_generation
            .is_some_and(|old| old != generation);
        self.memory_camera_generation = Some(generation);
        if (self.memory_camera_allowed != allowed || changed)
            && let Some((_, cache)) = &self.memory
        {
            cache.invalidate();
        }
        self.memory_camera_allowed = allowed;
        if (!allowed || changed)
            && let Some(scope) = self
                .turn
                .as_ref()
                .and_then(|turn| turn.memory_scope.as_ref())
            && scope
                .lease()
                .scope()
                .is_some_and(|scope| scope.person().is_some())
        {
            scope.revoke();
        }
    }

    fn memory_scope(
        &self,
        request: u64,
        session: u64,
        heard: bool,
    ) -> Option<crate::memory_scope::RequestScope> {
        self.memory.as_ref().map(|(worker, cache)| {
            let scope = if heard && self.memory_camera_allowed {
                cache.snapshot(std::time::Instant::now())
            } else {
                crate::memory_scope::ScopeSnapshot::room()
            };
            crate::memory_scope::RequestScope::new(
                crate::memory_scope::RequestKey {
                    worker: *worker,
                    session,
                    request,
                },
                scope,
            )
        })
    }

    /// Short sounds let through are checked (`--sounds-checked`, #800).
    pub fn checking_sounds(mut self, on: bool) -> Self {
        self.sounds_checked = on;
        self
    }

    /// A checked sound with a word in it is answered when `asked` says the
    /// lamp has just asked something (17 October). Before any turn only.
    pub fn answering_the_lamp(mut self, asked: Asked) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.asked = Some(asked);
        }
        self
    }

    /// A spoken turn's chat begins on the words its speech to text has
    /// before they are final (`LAMP_SECOND_PREPARED=on`, [`Prepared`]).
    /// Before any turn only.
    pub fn preparing(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.prepare = on;
        }
        self
    }

    /// A spoken turn's words are judged with `heed` before they are answered.
    /// Before any turn only: the turns share it.
    pub fn heeding(mut self, heed: Heed) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.heed = Some(heed);
        }
        self
    }

    /// A spoken request whose words name one of the staff is asked with the
    /// Live route's note beside them (`LAMP_STAFF_NOTE=on`,
    /// `crate::staff_note`). Before any turn only.
    pub fn noting_staff(mut self, staff: StaffNote) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.staff = Some(staff);
        }
        self
    }

    /// The room this route keeps, told on the bus and kept in its file by
    /// `tick` at each of the worker's ticks ([`Engine::tick_room`]), when
    /// this route is the lamp's only voice (#1712: with nothing publishing
    /// `room/state`, the route judged every line in an unknown room).
    pub fn keeping_room(mut self, tick: Box<dyn FnMut(u64) + Send>) -> Self {
        self.room_tick = Some(tick);
        self
    }

    /// At each of the worker's ticks: the room told and kept, if this route
    /// keeps it.
    pub fn tick_room(&mut self, now_us: u64) {
        if let Some(tick) = self.room_tick.as_mut() {
            tick(now_us);
        }
    }

    /// A spoken request whose line the lamp's name began is asked with the
    /// Live route's note beside it (`LAMP_NAME_NOTE=on`, `crate::called`).
    /// Before any turn only.
    pub fn noting_the_name(mut self, note: NameNote) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.name_note = Some(note);
        }
        self
    }

    /// What the lamp did is told to `did`: its work asked of its parts and
    /// its own turns heard, which the rule for whom it answers takes as
    /// the Live worker's does (#1712: after a hand-over and 104 s, every line
    /// was held as no conversation, where Live took a question about the
    /// work or a reply to its news). Before any turn only.
    pub fn telling_what_it_did(mut self, did: LampDid) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.did = Some(did);
        }
        self
    }

    /// Each spoken line is asked with `note`'s words beside it: answer it,
    /// when its words or the lamp's name called the lamp; otherwise, it may
    /// not be for the lamp, and then it is left alone (`worker::call_note`,
    /// #1712). Before any turn only.
    pub fn noting_whether_called(mut self, note: CallNote) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.call_note = Some(note);
        }
        self
    }

    /// A line the rule let out only as the conversation going on is asked
    /// with `note`'s words beside it (`worker::CONVERSATION_ALONE`): this
    /// route's model reads one line of words, not who said it to whom
    /// (#1712). Asked after the request is judged, with its request. Before
    /// any turn only.
    pub fn noting_the_conversation_alone(mut self, note: NameNote) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.conversation_alone = Some(note);
        }
        self
    }

    /// The answer's first piece is said once `words` words have come whole,
    /// if no sentence or clause ended sooner: for a voice asked one request
    /// a piece (`LAMP_SECOND_SPEECH=gemini`), whose first sound otherwise
    /// waits for the whole first sentence (#1712: through the relay, 1.89 s
    /// from the chat's first event to the first audio). Before any turn
    /// only.
    pub fn cutting_first_words(mut self, words: usize) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.first_words = Some(words);
        }
        self
    }

    /// The model looks by calling [`LOOK_CALL`], not only by its words: the
    /// coordinator is asked for the turn's full look (`LookAsked`, as for a
    /// skill's look step), the lamp says [`LOOKING`] if the model said
    /// nothing, and the turn ends there; its picture comes in the look turn
    /// after it (the first builder on #1712: a phrase rule is easy to trip).
    /// Before any turn only.
    pub fn looking_by_call(mut self) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.look_by_call = true;
        }
        self
    }

    /// `heard` is told each spoken request's words once they are in and it
    /// is to be answered, before its chat begins: a search can start while
    /// the model's first round runs (#1712). Before any turn only.
    pub fn hearing_words(mut self, heard: WordsHeard) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.words_heard = Some(heard);
        }
        self
    }

    /// A call `hold` says is not the lamp's is not run: the model is told
    /// it was not done ([`crate::policy_holds::HELD`]), as the Live route
    /// tells it of a call the Policy holds (#1712: on lamp-4ace on 19 Oct
    /// "Yes, give me five minutes", said to someone else, set a timer).
    /// Before any turn only.
    pub fn holding_calls(mut self, hold: CallHold) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.hold = Some(hold);
        }
        self
    }

    /// While a task this voice prepared waits for the person's yes, a spoken
    /// request is told so beside its words (`note`, with the tasks part):
    /// this route's history keeps the words of its last exchanges, not its
    /// calls, so without it a yes was taken as a new hand-over (#1712).
    /// Before any turn only.
    pub fn noting_a_task_waiting(mut self, note: TaskNote) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.task_waiting = Some(note);
        }
        self
    }

    /// A spoken turn whose first round is slow makes one soft sound
    /// (`LAMP_SECOND_WAIT_CUE=on`, sixth-P3). Before any turn only.
    pub fn waiting_with_a_sound(mut self, on: bool) -> Self {
        if on {
            let shared = Arc::get_mut(&mut self.shared);
            debug_assert!(shared.is_some(), "the wait sound is set before any turn");
            if let Some(shared) = shared {
                shared.cues = Some(Arc::default());
            }
        }
        self
    }

    /// A turn whose first round has not begun within its bound says
    /// [`DOWN_LINE`] (`LAMP_SECOND_DOWN_LINE=on`). Before any turn only.
    pub fn saying_when_down(mut self, on: bool) -> Self {
        if on {
            let shared = Arc::get_mut(&mut self.shared);
            debug_assert!(shared.is_some(), "the down line is set before any turn");
            if let Some(shared) = shared {
                shared.down = Some(Arc::default());
            }
        }
        self
    }

    /// Whether a turn after a quiet spell opens with [`ANNOUNCEMENT`], said
    /// because this voice cannot be Gemini's: off when it is the lamp's only
    /// voice (`LAMP_VOICE_ROUTE=second`, `--only-voice`), with no other voice
    /// to differ from. Before any turn only.
    pub fn announcing(mut self, on: bool) -> Self {
        let shared = Arc::get_mut(&mut self.shared);
        debug_assert!(shared.is_some(), "the announcement is set before any turn");
        if let Some(shared) = shared {
            shared.announce = on;
        }
        self
    }

    /// A look reads its picture with `look` (`LAMP_EYES_LOOK_FILE`).
    pub fn looking(mut self, look: Look) -> Self {
        self.look = Some(look);
        self
    }

    /// Take one input. Must run inside a tokio runtime: a turn is a task.
    /// Input for a retired request, or for a request other than the
    /// current one, is dropped; the worker loop has checked authority.
    pub fn submit(&mut self, input: ProviderInput) {
        match input {
            ProviderInput::Start {
                request,
                session,
                continues,
                continues_cut_off,
                after_stop,
                over_the_lamps_voice,
                ..
            } => {
                let previous = self.begun_last;
                if !self.begin(request, session) {
                    return;
                }
                room(|room| {
                    room.heard(request, monotonic_us());
                    if over_the_lamps_voice {
                        room.began_over_the_lamp(request);
                    }
                });
                // The voice's connection opened while the person speaks, not
                // by the answer's first sentence (#1712).
                tokio::spawn(self.shared.services.warm());
                // Going on from a request whose end went early and was given
                // up when the person spoke again: its words begin this one,
                // as Gemini, which heard them, is told (`CUT_OFF_NOTE`). Only
                // that request's own words, in this session: taken at every
                // start, so none are kept for a later one. With none heard
                // yet (given up before its speech to text finished), this
                // one is asked alone (the reviewer on #1168).
                let last = self
                    .shared
                    .heard_last
                    .lock()
                    .ok()
                    .and_then(|mut last| last.take());
                // The last line's sound, when this one goes on from it: the
                // coordinator ends a line at a short pause, so "Lamp, …
                // what's the weather?" came as pieces of about 1.3 s, each a
                // speech-to-text stream of its own, and the piece with the
                // name got no words (#1712, the owner on lamp-4ace on 19 Oct
                // at 16:37). Its sound goes into this line's stream first, so
                // the words are heard whole; its words then are not put
                // before this one's as well.
                let now = (self.shared.clock)();
                let answered = self
                    .shared
                    .answered_request
                    .lock()
                    .ok()
                    .and_then(|last| *last);
                let carried = self.kept.take().filter(|kept| {
                    let since = kept.ended.map(|end| now.saturating_duration_since(end));
                    kept.session == session
                        && kept.request < request
                        && !kept.samples.is_empty()
                        && since.is_none_or(|since| since <= KEEP_AFTER_END)
                        && (continues
                            || since.is_some_and(|since| since <= CARRY_AFTER_END)
                                && answered != Some(kept.request))
                });
                let before = last
                    .filter(|_| carried.is_none())
                    .filter(|(of, _)| continues_cut_off && previous == Some((*of, session)))
                    .map(|(_, words)| words);
                // A stop to tell: this request's own, or one left by the
                // request begun just before it in this session. Left when that
                // one was never asked (heard nothing, held, or replaced before
                // its chat), or, for a request going on from it, when it was
                // asked and then given up. Never past the coordinator's 2 min
                // (the reviewer on #1175).
                if let Ok(mut untold) = self.shared.stop_untold.lock() {
                    let now = tokio::time::Instant::now();
                    let within =
                        std::time::Duration::from_micros(crate::not_speech::TELL_STOP_WITHIN_US);
                    let left = untold.take().filter(|stop| {
                        previous == Some((stop.request, session))
                            && (!stop.asked || continues_cut_off)
                            && now.saturating_duration_since(stop.since) <= within
                    });
                    *untold = match (after_stop, left) {
                        (true, _) => Some(StopToTell {
                            request,
                            since: now,
                            asked: false,
                        }),
                        (false, Some(stop)) => Some(StopToTell {
                            request,
                            asked: false,
                            ..stop
                        }),
                        (false, None) => None,
                    };
                }
                let (audio, heard) = mpsc::channel(AUDIO_QUEUE);
                let sound = Arc::new(AtomicBool::new(false));
                let checked = self.sounds_checked.then(|| sound.clone());
                let said = Arc::new(Mutex::new((0, 0)));
                self.kept = Some(match carried {
                    Some(kept) => {
                        eprintln!(
                            "lamp-voice second: request {request} goes on from request {}: its {} ms of sound go first",
                            kept.request,
                            kept.samples.len() / 16
                        );
                        let _ = audio.try_send(kept.samples.iter().copied().collect());
                        // The line began with the sound carried: the name
                        // heard in it is this line's.
                        if let Ok(mut said) = said.lock() {
                            *said = (kept.said_from, kept.said_from);
                        }
                        KeptSound {
                            request,
                            ended: None,
                            ..kept
                        }
                    }
                    None => KeptSound {
                        request,
                        session,
                        samples: VecDeque::new(),
                        said_from: 0,
                        ended: None,
                    },
                });
                let memory_scope = self.memory_scope(request, session, true);
                let lease = memory_scope.as_ref().map(|scope| scope.lease());
                let task = tokio::spawn(spoken(
                    self.shared.clone(),
                    (request, lease),
                    heard,
                    checked,
                    said.clone(),
                    before,
                    continues
                        .then_some(previous)
                        .flatten()
                        .filter(|(_, of)| *of == session)
                        .map(|(from, _)| from),
                ));
                self.turn = Some(Turn {
                    request,
                    audio: Some(audio),
                    sound,
                    said,
                    task,
                    memory_scope,
                });
            }
            ProviderInput::Audio {
                request,
                samples,
                read_completed_at_us,
                ..
            } => {
                // No age check: replayed audio is older than live audio.
                let Some(turn) = self.turn.as_mut().filter(|turn| turn.request == request) else {
                    return;
                };
                let Some(audio) = turn.audio.as_ref() else {
                    return;
                };
                room(|room| room.hearing(monotonic_us()));
                if let Ok(mut said) = turn.said.lock() {
                    if said.0 == 0 {
                        said.0 = read_completed_at_us;
                    }
                    said.1 = read_completed_at_us;
                }
                if let Some(kept) = self.kept.as_mut().filter(|kept| kept.request == request) {
                    if kept.said_from == 0 {
                        kept.said_from = read_completed_at_us;
                    }
                    kept.samples.extend(samples.iter().copied());
                    let over = kept.samples.len().saturating_sub(KEPT_SAMPLES);
                    kept.samples.drain(..over);
                }
                match audio.try_send(samples) {
                    Ok(()) => {}
                    // The task already failed the turn itself.
                    Err(mpsc::error::TrySendError::Closed(_)) => turn.audio = None,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        self.turn = None;
                        self.urgent.push_back(ProviderOutput::TurnFailed {
                            request,
                            stage: FailedStage::InputOpen,
                            reason: Fault::Local,
                        });
                    }
                }
            }
            // A note for Gemini's model; this voice keeps none.
            ProviderInput::Called { .. } => {}
            ProviderInput::End { request, sound, .. } => {
                if let Some(kept) = self.kept.as_mut().filter(|kept| kept.request == request) {
                    kept.ended = Some((self.shared.clock)());
                }
                if let Some(turn) = self.turn.as_mut().filter(|turn| turn.request == request) {
                    room(|room| {
                        if sound {
                            room.ended_as_sound(request, monotonic_us());
                        } else {
                            room.ended(request, monotonic_us());
                        }
                    });
                    turn.sound.store(sound, Ordering::Release);
                    // Closing the queue is the end of input for the task.
                    turn.audio = None;
                }
            }
            ProviderInput::Written {
                request,
                session,
                text,
                look,
                ..
            } => {
                if !self.begin(request, session) {
                    return;
                }
                // The lamp taking a look: the eye's newest picture goes with
                // the words, as on the Live route. With none, the model is
                // told it saw nothing, never left to describe a picture it
                // was not given (sixth-P2, #1044).
                // A model that takes no picture is never sent one.
                let sees = self.shared.services.sees();
                if look && !sees {
                    eprintln!(
                        "lamp-voice second: the lamp looked: no picture with request {request}: the model takes none"
                    );
                }
                let picture = (look && sees)
                    .then(|| match self.look.as_ref().map(|look| look()) {
                        Some(Ok(jpeg)) => Some(jpeg),
                        Some(Err(why)) => {
                            eprintln!(
                                "lamp-voice second: the lamp looked: no picture with request {request}: {why}"
                            );
                            None
                        }
                        None => {
                            eprintln!(
                                "lamp-voice second: the lamp looked: no picture with request {request}: no look file"
                            );
                            None
                        }
                    })
                    .flatten();
                if let Some(jpeg) = &picture {
                    eprintln!(
                        "lamp-voice second: the lamp looked: a picture of {} bytes went with request {request}",
                        jpeg.len()
                    );
                }
                let text = if look && picture.is_none() {
                    crate::speak_up::NO_LOOK_NOTE.to_owned()
                } else {
                    text
                };
                // A full look whose still never came reaches here as the
                // no-look note without `look` (`sight::Look::turn`): a
                // look all the same, with its 5 s and its line (the reviewer
                // on #1154).
                let looking = look || text == crate::speak_up::NO_LOOK_NOTE;
                let memory_scope = self.memory_scope(request, session, false);
                let lease = memory_scope.as_ref().map(|scope| scope.lease());
                let shared = self.shared.clone();
                let begun = Instant::now();
                let task = tokio::spawn(async move {
                    answer(
                        &shared,
                        (request, lease),
                        text,
                        Asker::Written { looking },
                        picture,
                        Vec::new(),
                        Times::new(begun),
                        None,
                    )
                    .await
                });
                self.turn = Some(Turn {
                    request,
                    audio: None,
                    sound: Arc::default(),
                    said: Arc::default(),
                    task,
                    memory_scope,
                });
            }
            ProviderInput::Reminder {
                request,
                session,
                text,
                late,
                late_by_ms,
                ..
            } => {
                // Deferred unless this route is the lamp's only voice and the
                // lamp says its reminders: standing by, Gemini says them once
                // it is back.
                let note = if self.reminders {
                    crate::provider_worker::reminder_note(&text, late, late_by_ms)
                } else {
                    Err(ReminderDeferral::Disabled)
                };
                let note = match note {
                    Ok(note) => note,
                    Err(reason) => {
                        self.urgent.push_back(ProviderOutput::ReminderDeferred {
                            request,
                            session,
                            reason,
                        });
                        return;
                    }
                };
                if !self.begin(request, session) {
                    return;
                }
                // Reminder data is not something to remember because it
                // was said (as on the Live route).
                crate::remember::keep_out(request);
                // The room's memory, as for any turn the lamp writes (#1794).
                let memory_scope = self.memory_scope(request, session, false);
                let lease = memory_scope.as_ref().map(|scope| scope.lease());
                let shared = self.shared.clone();
                let task = tokio::spawn(async move {
                    answer(
                        &shared,
                        (request, lease),
                        note,
                        Asker::Reminder,
                        None,
                        Vec::new(),
                        Times::new(Instant::now()),
                        None,
                    )
                    .await
                });
                self.turn = Some(Turn {
                    request,
                    audio: None,
                    sound: Arc::default(),
                    said: Arc::default(),
                    task,
                    memory_scope,
                });
            }
        }
    }

    /// A due reminder is said, as the Live route says it, rather than
    /// deferred: when this route is the lamp's only voice
    /// (`LAMP_VOICE_ROUTE=second`) and the lamp says its reminders
    /// (`LAMP_SCHEDULE_SPEAK`). Before any turn only.
    pub fn saying_reminders(mut self, on: bool) -> Self {
        self.reminders = on;
        self
    }

    /// A new turn replaces any older one. False for a retired request.
    fn begin(&mut self, request: u64, session: u64) -> bool {
        if request <= self.retired_through {
            return false;
        }
        self.cut_short(request - 1);
        self.turn = None;
        self.begun_last = Some((request, session));
        if let Ok(mut begun) = self.shared.begun.lock() {
            *begun = Some((request, session));
        }
        self.urgent.push_back(ProviderOutput::Started {
            request,
            session,
            waiting_for_barrier: false,
        });
        true
    }

    /// Requests up to `through` are over (spoken over, completed or
    /// abandoned): their task stops and their unsent output is dropped.
    pub fn retire_through(&mut self, through: u64) {
        self.retired_through = self.retired_through.max(through);
        self.cut_short(self.retired_through);
        if self
            .turn
            .as_ref()
            .is_some_and(|turn| turn.request <= self.retired_through)
        {
            self.turn = None;
        }
        let retired = self.retired_through;
        self.urgent
            .retain(|event| output_request(event).is_none_or(|r| r > retired));
    }

    /// A spoken request up to `through` still being answered, none of its
    /// answer heard, is cut short: its words are kept for the next line, and
    /// the journal says so (#1712: on lamp-4ace on 19 Oct the weather and a
    /// race result were searched, then ended by the next line, which was
    /// held, and were never said).
    fn cut_short(&self, through: u64) {
        let Some(carried) = self
            .shared
            .in_flight
            .lock()
            .ok()
            .and_then(|mut in_flight| in_flight.take_if(|carried| carried.request <= through))
        else {
            return;
        };
        eprintln!(
            "lamp-voice second: request {}: ended before its answer: a newer line began; its words wait for that line's verdict",
            carried.request
        );
        if let Ok(mut cut) = self.shared.cut.lock() {
            *cut = Some(carried);
        }
    }

    /// The next output ready now, if any.
    pub fn try_output(&mut self) -> Option<ProviderOutput> {
        loop {
            let event = match self.urgent.pop_front() {
                Some(event) => event,
                None => self.output.try_recv().ok()?,
            };
            if output_request(&event).is_none_or(|r| r > self.retired_through) {
                return Some(event);
            }
        }
    }

    /// The next output, waiting for it.
    #[cfg(test)]
    async fn next(&mut self) -> ProviderOutput {
        loop {
            let event = match self.urgent.pop_front() {
                Some(event) => event,
                None => self.output.recv().await.expect("the engine holds a sender"),
            };
            if output_request(&event).is_none_or(|r| r > self.retired_through) {
                return event;
            }
        }
    }
}

/// The request an output belongs to, as the fixture has it: failures and
/// refusals are owed even for a retired request.
pub(crate) fn output_request(event: &ProviderOutput) -> Option<u64> {
    match event {
        // Owed even for a retired request, as a failure is: an A/B scores
        // every request (#1686's follow-up).
        ProviderOutput::Ready { .. }
        | ProviderOutput::TurnFailed { .. }
        | ProviderOutput::Stages { .. }
        | ProviderOutput::ReminderDeferred { .. }
        | ProviderOutput::InputDiscarded { .. }
        | ProviderOutput::OwnershipAssumed { .. }
        | ProviderOutput::Ended { .. } => None,
        ProviderOutput::Started { request, .. }
        | ProviderOutput::Audio { request, .. }
        | ProviderOutput::GenerationComplete { request }
        | ProviderOutput::Interrupted { request }
        | ProviderOutput::TurnComplete { request, .. }
        | ProviderOutput::HoldingAnswer { request, .. }
        | ProviderOutput::Unclear { request, .. }
        | ProviderOutput::LookAsked { request, .. }
        | ProviderOutput::NewsAnswered { request, .. } => Some(*request),
        ProviderOutput::Transcript { request, .. } => *request,
    }
}

/// The closest provider fault for a client error. No text is carried.
pub fn fault(error: &net::Error) -> Fault {
    match error {
        net::Error::Url => Fault::Local,
        net::Error::Transport(_) => Fault::Transport,
        net::Error::Timeout(step) if step.contains("connect") => Fault::ConnectTimeout,
        net::Error::Timeout(_) => Fault::ResponseTimeout,
        net::Error::Status(401 | 403) => Fault::Authentication,
        net::Error::Status(402) => Fault::QuotaExceeded,
        net::Error::Status(429) => Fault::RateLimited,
        net::Error::Status(500..) => Fault::ServerUnavailable,
        net::Error::Status(_) => Fault::ServerRejected,
        net::Error::Protocol(_) => Fault::Protocol,
    }
}

impl Shared {
    /// False once the engine is gone; the task then just ends.
    async fn emit(&self, event: ProviderOutput) -> bool {
        self.out.send(event).await.is_ok()
    }

    async fn fail(&self, request: u64, stage: FailedStage, error: &net::Error) {
        self.emit(ProviderOutput::TurnFailed {
            request,
            stage,
            reason: fault(error),
        })
        .await;
    }

    fn announcement_due(&self) -> bool {
        let now = (self.clock)();
        self.answered_at
            .lock()
            .map(|at| at.is_none_or(|at| now.saturating_duration_since(at) >= ANNOUNCE_AFTER))
            .unwrap_or(true)
    }
}

/// A spoken turn: listen until the queue closes, then answer the words.
/// With `checked` (the sound mark, when sounds are checked), a short sound
/// not heard as a short word is not answered; with a [`Heed`], words it
/// judges not said to the lamp are not either.
async fn spoken(
    shared: Arc<Shared>,
    context: (u64, Option<crate::memory_scope::ScopeLease>),
    mut heard: mpsc::Receiver<Vec<i16>>,
    checked: Option<Arc<AtomicBool>>,
    said: Arc<Mutex<(u64, u64)>>,
    before: Option<String>,
    continues_from: Option<u64>,
) {
    let (request, scope) = context;
    let begun = Instant::now();
    let mut listener = match shared.services.listen().await {
        Ok(listener) => listener,
        Err(error) => {
            shared.stages(request, &Times::new(begun)).await;
            return shared.fail(request, FailedStage::InputOpen, &error).await;
        }
    };
    // The words before they are final, for a chat begun on them.
    let mut early = if shared.prepare {
        listener.early()
    } else {
        None
    };
    let mut prepared: Option<Prepared> = None;
    loop {
        tokio::select! {
            samples = heard.recv() => {
                let Some(samples) = samples else { break };
                if let Err(error) = listener.send(&samples) {
                    shared.stages(request, &Times::new(begun)).await;
                    return shared.fail(request, FailedStage::InputOpen, &error).await;
                }
            }
            Some(said) = next_early(&mut early) => {
                prepared = prepared_on(&shared, request, prepared, said);
            }
        }
    }
    let finishing = Instant::now();
    let times = Times::new(finishing);
    let finish = listener.finish(request);
    tokio::pin!(finish);
    let finished = loop {
        tokio::select! {
            finished = &mut finish => break finished,
            Some(said) = next_early(&mut early) => {
                prepared = prepared_on(&shared, request, prepared, said);
            }
        }
    };
    // Every early word sent before the words were final counts: a
    // `TurnResumed` just before the end still takes them back.
    while let Some(said) = early.as_mut().and_then(|early| early.try_recv().ok()) {
        prepared = prepared_on(&shared, request, prepared, said);
    }
    let text = match finished {
        Ok(text) => {
            reached(&times, |times| &mut times.words);
            eprintln!(
                "lamp-voice second: request {request} heard {} words, the last {} ms after the line ended",
                text.split_whitespace().count(),
                finishing.elapsed().as_millis()
            );
            text
        }
        Err(error) => {
            eprintln!("lamp-voice second: request {request} speech to text failed: {error}");
            shared.stages(request, &times).await;
            return shared
                .fail(request, FailedStage::AwaitingResponse, &error)
                .await;
        }
    };
    if let Ok(mut last) = shared.heard_last.lock() {
        *last = Some((request, text.clone()));
    }
    if !shared
        .emit(ProviderOutput::Transcript {
            request: None,
            text: text.clone(),
            finished: true,
        })
        .await
    {
        return;
    }
    // What was heard in this request is told as heard; the model is asked
    // with the words of the one it goes on from before them.
    let text = match before {
        Some(before) if !before.trim().is_empty() => {
            eprintln!(
                "lamp-voice second: request {request} goes on from the request given up: its {} words begin it",
                before.split_whitespace().count()
            );
            format!("{} {}", before.trim(), text.trim())
        }
        _ => text,
    };
    let empty = text.trim().is_empty();
    let marked = checked.is_some_and(|sound| sound.load(Ordering::Acquire))
        && !crate::not_speech::short_word(&text);
    // A sound with a word in it, right after the lamp's own question in a
    // conversation going on, is the person's answer, as on the Live route
    // (lamp-4ace, 17 October 16:11: "Yes. Please start it." heard as
    // "bien"). The rule (`heed`) judges it as it does any line.
    let answers = marked
        && text.chars().any(char::is_alphanumeric)
        && shared.asked.as_ref().is_some_and(|asked| asked(request));
    if answers {
        eprintln!(
            "lamp-voice second: request {request}: a short sound with a word in it, right after the lamp's own question, in a conversation going on: judged as a line"
        );
    }
    let held = marked && !answers;
    if held && !empty {
        eprintln!(
            "lamp-voice second: request {request}: its answer is held back: a short sound, and nothing shows it was said to the lamp: not heard as a short word (this route has no ears or eyes)"
        );
    }
    let heeded = match &shared.heed {
        Some(heed) if !empty && !held => {
            let window = said.lock().map_or((0, 0), |said| *said);
            heed(request, window, &text, continues_from)
        }
        _ => None,
    };
    if let Some(why) = heeded {
        eprintln!("lamp-voice second: request {request}: its answer is held back: {why}");
    }
    // Words cut short by this line: answered in its place if it is held,
    // dropped if it is answered itself (#1712).
    let carried = shared
        .cut
        .lock()
        .ok()
        .and_then(|mut cut| cut.take())
        .filter(|carried| carried.request < request && carried.at.elapsed() <= CARRY_WITHIN);
    if empty || held || heeded.is_some() {
        // Left unanswered: a line held by the rule is not talk, nor are no
        // words at all, which are a sound (as a short sound already is).
        room(|room| {
            if empty {
                room.ended_as_sound(request, monotonic_us());
            }
            room.unanswered(request, monotonic_us());
        });
        if let Some(carried) = carried {
            eprintln!(
                "lamp-voice second: request {request} is held, so request {}'s words, cut short by it, are answered in its place",
                carried.request
            );
            if let Ok(mut in_flight) = shared.in_flight.lock() {
                *in_flight = Some(Carried {
                    request,
                    ..carried.clone()
                });
            }
            answer(
                &shared,
                (request, scope),
                carried.text,
                Asker::Spoken,
                None,
                Vec::new(),
                times,
                None,
            )
            .await;
            return;
        }
        if shared.stages(request, &times).await {
            shared
                .emit(ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                })
                .await;
        }
        return;
    }
    // The first request asked after a hand or the head stopped the lamp: the
    // model is told so, as Gemini is (`STOPPED_NOTE`, #1148), so the story
    // that was stopped is not begun again.
    let stopped = shared.stop_untold.lock().ok().and_then(|mut untold| {
        let stop = untold
            .as_mut()
            .filter(|stop| stop.request == request && !stop.asked)?;
        stop.asked = true;
        Some(crate::provider_worker::STOPPED_NOTE.to_owned())
    });
    let staff = shared
        .staff
        .as_ref()
        .and_then(|staff| staff(request, &text));
    // Its name heard as it began: told as the Live route tells it (#1712:
    // without it this route missed two calls this morning).
    let named = shared
        .name_note
        .as_ref()
        .and_then(|note| note(request))
        .map(str::to_owned);
    // A task prepared and waiting for a yes: this route's history keeps the
    // words of its last exchanges, not its calls, so the model is told, or a
    // yes is taken as a new hand-over (#1712).
    let waiting = shared.task_waiting.as_ref().and_then(|note| note(request));
    // Let out only as the conversation going on: it may be people's talk.
    let alone = shared
        .conversation_alone
        .as_ref()
        .and_then(|note| note(request))
        .map(str::to_owned);
    // Said by name: answer it; else it may not be for the lamp. The line let
    // out by a conversation alone has its own note for that.
    let call = alone
        .is_none()
        .then(|| {
            shared
                .call_note
                .as_ref()
                .and_then(|note| note(request, &text))
        })
        .flatten()
        .map(str::to_owned);
    let notes = stopped
        .into_iter()
        .chain(staff)
        .chain(named)
        .chain(call)
        .chain(alone)
        .chain(waiting)
        .collect();
    if let Some(carried) = carried {
        eprintln!(
            "lamp-voice second: request {}'s words, cut short, are let go: request {request} is answered",
            carried.request
        );
    }
    if let Ok(mut in_flight) = shared.in_flight.lock() {
        *in_flight = Some(Carried {
            request,
            text: text.clone(),
            at: Instant::now(),
        });
    }
    if let Some(heard) = &shared.words_heard {
        heard(request, &text);
    }
    answer(
        &shared,
        (request, scope),
        text,
        Asker::Spoken,
        None,
        notes,
        times,
        prepared,
    )
    .await;
}

/// Where the worker reads [`Engine::preparing`]: `on`, and nothing else.
pub const PREPARED_ENVIRONMENT: &str = "LAMP_SECOND_PREPARED";

/// Round 1 of a spoken turn's chat begun on the line's early words
/// (`LAMP_SECOND_PREPARED`, #1712, the first builder at 17:02: the route's
/// first word came about 3.4 s after a line, Live's 2.4 s). Its events are
/// kept as they come; nothing is said and no tool runs. When the line's
/// words are final it is kept if they are its words and nothing else asked
/// differs ([`Prepared::misfit`]), and dropped otherwise, or at once when
/// the person goes on. Neither the rule nor a tool runs early: both change
/// what the lamp knows or does.
struct Prepared {
    words: String,
    messages: Vec<Message>,
    events: mpsc::UnboundedReceiver<net::Result<ChatEvent>>,
    at: Instant,
    task: Option<JoinHandle<()>>,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Prepared {
    /// The chat asked on `words`, as round 1 would be asked: the turn's
    /// system message and history, then the words.
    fn begin(shared: &Shared, words: String) -> Self {
        let told = (shared.system)();
        let mut messages = vec![Message::system(told.text)];
        messages.extend(told.history);
        messages.push(Message::user(words.clone()));
        let mut chat = shared.services.chat(messages.clone());
        let (kept, events) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            while let Some(event) = chat.next().await {
                if kept.send(event).is_err() {
                    break;
                }
            }
        });
        Self {
            words,
            messages,
            events,
            at: Instant::now(),
            task: Some(task),
        }
    }

    /// Why it can't stand for round 1 asked with `messages`: `base`, its
    /// system message and history; `notes` beside the words, then `said`.
    /// `None` when it can: the same words (as letters, digits and spaces,
    /// whatever the case), the same conversation, and no note it lacks but
    /// the name note on words that name the lamp, which tells nothing new.
    fn misfit(&self, base: &[Message], notes: &[String], said: &str) -> Option<&'static str> {
        if plain_words(&self.words) != plain_words(said) {
            Some("the final words are not the early ones")
        } else if self.messages.get(..self.messages.len().saturating_sub(1)) != Some(base) {
            Some("the conversation changed")
        } else if !notes.iter().all(|note| {
            [crate::called::NOTE, crate::called::NOTE_HEARD_AS].contains(&note.as_str())
                && crate::attention::called_by_name(said)
        }) {
            Some("a note goes beside the words")
        } else {
            None
        }
    }

    /// Its events, those kept and those still to come, as round 1's.
    fn into_events(mut self) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
        let events = std::mem::replace(&mut self.events, mpsc::unbounded_channel().1);
        // The task goes with the stream: dropping it stops the chat.
        let task = AbortOnDrop(self.task.take());
        futures_util::stream::unfold((events, task), |(mut events, task)| async move {
            events.recv().await.map(|event| (event, (events, task)))
        })
        .boxed()
    }
}

/// A task stopped when this is dropped.
struct AbortOnDrop(Option<JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

/// `words` as letters, digits and single spaces, lower case: what the early
/// and the final words are compared as.
fn plain_words(words: &str) -> String {
    words
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The next of a listener's early words; never, once there are none.
async fn next_early(early: &mut Option<mpsc::UnboundedReceiver<net::Early>>) -> Option<net::Early> {
    let Some(receiver) = early.as_mut() else {
        return std::future::pending().await;
    };
    match receiver.recv().await {
        Some(said) => Some(said),
        None => {
            *early = None;
            std::future::pending().await
        }
    }
}

/// What a turn's prepared chat is once `said` comes: begun on new early
/// words, kept on the same, dropped when the person went on.
fn prepared_on(
    shared: &Shared,
    request: u64,
    prepared: Option<Prepared>,
    said: net::Early,
) -> Option<Prepared> {
    match said {
        net::Early::Words(words) => {
            if prepared
                .as_ref()
                .is_some_and(|prepared| prepared.words == words)
            {
                return prepared;
            }
            drop(prepared);
            eprintln!(
                "lamp-voice second: request {request}: its chat is begun on the line's early words"
            );
            Some(Prepared::begin(shared, words))
        }
        net::Early::Resumed => {
            if prepared.is_some() {
                eprintln!(
                    "lamp-voice second: request {request}: the chat begun on its early words is dropped: the person went on"
                );
            }
            None
        }
    }
}

/// Who asked the turn's chat, which sets what it may do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asker {
    /// A person spoke: the wait's sound and the down line may be heard, and
    /// a tool runs as heard.
    Spoken,
    /// The lamp wrote it (a hello, a "Sorry?", a skill's say), or a look
    /// with `looking`.
    Written { looking: bool },
    /// A due reminder, said as the Live route says it: a note to say it, and
    /// no tool runs (`provider_worker::reminder_note`).
    Reminder,
}

/// A request's times for [`ProviderOutput::Stages`], from `from` (a spoken
/// request's line end as this route got it, a written one's start).
struct Times {
    from: Instant,
    words: Option<u64>,
    cue: Option<u64>,
    chat: Option<u64>,
    audio: Option<u64>,
}

impl Times {
    fn new(from: Instant) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            from,
            words: None,
            cue: None,
            chat: None,
            audio: None,
        }))
    }
}

/// Mark the stage `pick` chooses as reached now, if it was not yet.
fn reached(times: &Mutex<Times>, pick: impl FnOnce(&mut Times) -> &mut Option<u64>) {
    if let Ok(mut times) = times.lock() {
        let ms = u64::try_from(times.from.elapsed().as_millis()).unwrap_or(u64::MAX);
        pick(&mut times).get_or_insert(ms);
    }
}

impl Shared {
    /// The request's times, before its last event.
    async fn stages(&self, request: u64, times: &Mutex<Times>) -> bool {
        let Some(stages) = times.lock().ok().map(|times| ProviderOutput::Stages {
            request,
            words_ms: times.words,
            cue_ms: times.cue,
            chat_ms: times.chat,
            audio_ms: times.audio,
        }) else {
            return true;
        };
        self.emit(stages).await
    }
}

/// How an answer stopped short.
enum Stop {
    /// `not_for_me`: nothing more is said.
    Silent,
    /// The first round had not begun within its bound ([`BEGIN_WITHIN`]):
    /// the route gave the turn up, not the model. It ends as a silent turn,
    /// but what was said is no talk the lamp left alone.
    GaveUp,
    Failed(Fault),
    /// The engine is gone.
    Gone,
}

/// One wait sound for `request`, as audio only: no words, so nothing of it
/// is kept in the thread, and it does not count as an answer for the
/// announcement's spell.
async fn cue(shared: &Shared, request: u64, clip: Vec<i16>) -> Result<(), Stop> {
    for part in clip.chunks(PACKET_SAMPLES) {
        let audio = ProviderOutput::Audio {
            request,
            sequence: shared.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            provider_event_at_us: monotonic_us(),
            source_frames: part.len(),
            source_offset: 0,
            samples: part.to_vec(),
        };
        if !shared.emit(audio).await {
            return Err(Stop::Gone);
        }
    }
    Ok(())
}

/// Chat, speech and the turn's last events, for the person's `said`, with
/// the picture of a look (`looking`). The picture goes in this request only,
/// never into the thread or a later turn's history.
// The prepared round is the turn's eighth input; a struct of them all would
// only rename the arguments.
#[allow(clippy::too_many_arguments)]
async fn answer(
    shared: &Shared,
    context: (u64, Option<crate::memory_scope::ScopeLease>),
    said: String,
    asked: Asker,
    picture: Option<Vec<u8>>,
    notes: Vec<String>,
    times: Arc<Mutex<Times>>,
    prepared: Option<Prepared>,
) {
    let (request, scope) = context;
    let heard = asked == Asker::Spoken;
    let looking = asked == Asker::Written { looking: true };
    // Started now, with the chat request, so it is ready first.
    let mut speech = JoinSet::new();
    let announcement = (shared.announce && shared.announcement_due())
        .then(|| start_speech(shared, &mut speech, ANNOUNCEMENT.to_owned()));
    let (sentences, mut queued) = mpsc::channel(SENTENCES_AHEAD);
    let spoke = AtomicBool::new(false);
    // The answer's own first audio went (not the announcement's).
    let answered = AtomicBool::new(false);
    let mut queue = Queue {
        shared,
        speech: &mut speech,
        announcement,
        sentences,
        started: false,
        request,
        heard_at: Instant::now(),
    };
    let began = Instant::now();
    // The bounds below are on the runtime's clock, which a test can step.
    let begun = tokio::time::Instant::now();
    if heard && let Some(cues) = &shared.cues {
        cues.render(&shared.services);
    }
    // The picture of a look goes in round 1 only: a later round after a
    // tool is asked with the words alone (the reviewer on #1075).
    let mut seeing = picture.is_some();
    let words = Message::user(said.clone());
    let prepared_for = prepared.map(|prepared| (prepared, said.clone()));
    let question = match picture {
        Some(jpeg) => Message::user_seeing(said, &jpeg),
        None => Message::user(said),
    };
    let writing_times = times.clone();
    let writing = async move {
        if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
            return Err(Stop::Gone);
        }
        let told = (shared.system)();
        let mut messages = vec![Message::system(told.text)];
        messages.extend(told.history);
        // A chat begun on the early words stands for round 1 if it fits.
        let mut prepared = prepared_for.and_then(|(prepared, said)| {
            match prepared.misfit(&messages, &notes, &said) {
                None => {
                    eprintln!(
                        "lamp-voice second: request {request}: the chat begun on its early words is kept, begun {} ms ago",
                        prepared.at.elapsed().as_millis()
                    );
                    Some(prepared)
                }
                Some(why) => {
                    eprintln!(
                        "lamp-voice second: request {request}: the chat begun on its early words is dropped: {why}"
                    );
                    None
                }
            }
        });
        // A note beside the request, never anyone's words (as on the Live
        // route, where a note goes beside what the model hears).
        messages.extend(notes.into_iter().map(Message::system));
        messages.push(question);
        // What round 1 was asked, kept for the rounds after it.
        if let Some(prepared) = &prepared {
            messages.clone_from(&prepared.messages);
        }
        let asked_at = messages.len() - 1;
        let mut round = 0;
        while round < MAX_ROUNDS {
            if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                return Err(Stop::Gone);
            }
            round += 1;
            let mut refused = false;
            let mut stream = match prepared.take() {
                Some(prepared) if round == 1 => prepared.into_events(),
                _ => shared.services.chat(messages.clone()),
            };
            let mut first = true;
            let mut split = Sentences::default();
            let mut text = String::new();
            let mut calls = Vec::new();
            let mut reasoning = None;
            loop {
                let next = if first && round == 1 {
                    let at = |after: Duration| begun + after;
                    let cues = shared.cues.as_ref().filter(|_| heard);
                    let first_wait = if cues.is_some() {
                        CUE_AFTER
                    } else if looking {
                        LOOK_BEGIN_WITHIN
                    } else {
                        BEGIN_WITHIN
                    };
                    let mut next = tokio::time::timeout_at(at(first_wait), stream.next()).await;
                    if next.is_err()
                        && let Some(cues) = cues
                    {
                        // One soft sound, then a longer wait; with no sound
                        // ready yet, the plain bound.
                        let bound = match cues.next() {
                            Some(clip) => {
                                eprintln!(
                                    "lamp-voice second: request {request} made a sound while its chat had not begun in {} ms",
                                    began.elapsed().as_millis()
                                );
                                reached(&writing_times, |times| &mut times.cue);
                                cue(shared, request, clip).await?;
                                BEGIN_WITHIN_AFTER_CUE
                            }
                            None => BEGIN_WITHIN,
                        };
                        next = tokio::time::timeout_at(at(bound), stream.next()).await;
                    }
                    match next {
                        Ok(next) => next,
                        Err(_) if looking => {
                            eprintln!(
                                "lamp-voice second: request {request} look round had not begun in {} ms; the lamp says it could not look",
                                began.elapsed().as_millis()
                            );
                            queue.push(COULD_NOT_LOOK.to_owned()).await?;
                            return Ok(());
                        }
                        Err(_) => {
                            eprintln!(
                                "lamp-voice second: request {request} chat round {round} had not begun in {} ms; the turn ends unanswered",
                                began.elapsed().as_millis()
                            );
                            // Not nothing, when switched on: the line, as
                            // audio only, so it is no answer in the thread.
                            // Only to someone who spoke: a lamp's own turn
                            // (a hello, a skill's say) never says it (the
                            // reviewer on #1195).
                            if let Some((down, clip)) = shared
                                .down
                                .as_ref()
                                .filter(|_| heard)
                                .and_then(|down| Some((down, down.due()?)))
                            {
                                cue(shared, request, clip).await?;
                                down.said();
                                eprintln!(
                                    "lamp-voice second: request {request} said it can't think right now"
                                );
                            }
                            return Err(Stop::GaveUp);
                        }
                    }
                } else {
                    stream.next().await
                };
                let Some(event) = next else { break };
                let event = match event {
                    // A picture the chat refuses is no failure of the look:
                    // asked once more, within the same bound, with the
                    // no-look note (#1044).
                    Err(net::Error::Status(status)) if seeing && first && round == 1 => {
                        eprintln!(
                            "lamp-voice second: request {request} the chat refused the look's picture ({status}); asked again without it"
                        );
                        messages[asked_at] = Message::user(crate::speak_up::NO_LOOK_NOTE);
                        seeing = false;
                        refused = true;
                        break;
                    }
                    event => event.map_err(|error| {
                        eprintln!("lamp-voice second: request {request} chat round {round} failed after {} ms: {error}", began.elapsed().as_millis());
                        Stop::Failed(fault(&error))
                    })?,
                };
                if first {
                    first = false;
                    if round == 1 {
                        reached(&writing_times, |times| &mut times.chat);
                    }
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round} first event in {} ms",
                        began.elapsed().as_millis()
                    );
                }
                match event {
                    ChatEvent::Text(piece) => {
                        text.push_str(&piece);
                        // The model has chosen to speak: the announcement
                        // need not wait for its first sentence.
                        // A call written as words opens nothing: no
                        // announcement for it (lamp-4ace, 17:12).
                        if piece.chars().any(char::is_alphanumeric)
                            && !crate::not_speech::names_a_function(&text)
                        {
                            queue.announce().await?;
                        }
                        let mut sentences = split.push(&piece);
                        if sentences.is_empty() && !queue.started {
                            sentences.extend(split.clause().or_else(|| {
                                shared.first_words.and_then(|words| split.words(words))
                            }));
                        }
                        for sentence in sentences {
                            queue.push(sentence).await?;
                        }
                    }
                    ChatEvent::ToolCalls(asked) => calls = asked,
                    ChatEvent::Reasoning(details) => reasoning = Some(details),
                    ChatEvent::Searched => eprintln!(
                        "lamp-voice second: request {request} chat round {round} searched the web in {} ms",
                        began.elapsed().as_millis()
                    ),
                    ChatEvent::Done => break,
                }
            }
            if refused {
                round -= 1;
                continue;
            }
            // Before the unfinished last words are said.
            if !calls.is_empty() {
                let names: Vec<&str> = calls.iter().map(|call| call.name.as_str()).collect();
                eprintln!(
                    "lamp-voice second: request {request} chat round {round} called {}",
                    names.join(", ")
                );
            }
            if calls.iter().any(|call| call.name == NOT_FOR_ME) {
                return Err(Stop::Silent);
            }
            // A look asked by its own call (#1712): the coordinator takes the
            // turn's full look, and the turn ends here; its picture comes in
            // the look turn after it. Only in a turn someone spoke.
            if shared.look_by_call && calls.iter().any(|call| call.name == LOOK_CALL) {
                if let Some(rest) = split.finish() {
                    queue.push(rest).await?;
                }
                let session = shared
                    .begun
                    .lock()
                    .ok()
                    .and_then(|begun| *begun)
                    .filter(|(of, _)| *of == request)
                    .map(|(_, session)| session);
                let dropped = if !heard {
                    Some("the turn is one the lamp wrote itself")
                } else if session.is_none() {
                    Some("its turn is not the one in hand")
                } else {
                    None
                };
                eprintln!(
                    "lamp-voice second: request {request} called {LOOK_CALL}: {}",
                    dropped.unwrap_or("the look is asked of the coordinator")
                );
                let asked_look = ProviderOutput::LookAsked {
                    request,
                    session: session.unwrap_or_default(),
                    dropped: dropped.map(str::to_owned),
                };
                if !shared.emit(asked_look).await {
                    return Err(Stop::Gone);
                }
                if dropped.is_none() && !queue.started {
                    queue.push(LOOKING.to_owned()).await?;
                }
                return Ok(());
            }
            if let Some(rest) = split.finish() {
                queue.push(rest).await?;
            }
            if calls.is_empty() {
                return Ok(());
            }
            // A reminder is only said: its words are data, never an ask
            // (as the Live route's tool gate holds it).
            if asked == Asker::Reminder {
                eprintln!(
                    "lamp-voice second: request {request} is a reminder: its calls are not run"
                );
                return Ok(());
            }
            if std::mem::take(&mut seeing) {
                messages[asked_at] = words.clone();
            }
            messages.push(Message::assistant_calls_with(
                Some(text),
                &calls,
                reasoning.take(),
            ));
            for call in &calls {
                if shared
                    .hold
                    .as_ref()
                    .is_some_and(|hold| hold(request, &call.name))
                {
                    eprintln!(
                        "lamp-voice second: request {request} asked for {}: held, nothing shows it was said to the lamp",
                        call.name
                    );
                    messages.push(Message::tool(
                        call.id.clone(),
                        crate::policy_holds::held().to_string(),
                    ));
                    continue;
                }
                // The lamp's own work, for a question about it later, as the
                // Live worker marks it for any turn not held (#829).
                if (crate::agent_tasks::is_tool(&call.name)
                    || crate::tools::is_schedule_tool(&call.name))
                    && let Some(did) = &shared.did
                {
                    did(Did::Worked(monotonic_us()));
                }
                let result = match (&shared.scoped_tools, &scope) {
                    (Some(run), Some(scope)) => {
                        run(
                            request,
                            heard,
                            call.name.clone(),
                            call.args.clone(),
                            scope.clone(),
                        )
                        .await
                    }
                    (Some(_), None) => crate::tools::unavailable(),
                    (None, _) => {
                        (shared.tools)(request, heard, call.name.clone(), call.args.clone()).await
                    }
                };
                if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                    return Err(Stop::Gone);
                }
                messages.push(Message::tool(call.id.clone(), result.to_string()));
            }
        }
        // ponytail: a model still calling tools after MAX_ROUNDS ends the
        // turn with what it has said; a spoken "I could not finish" if seen.
        Ok(())
    };
    let saying = async {
        let mut after = false;
        while let Some((text, mut packets)) = queued.recv().await {
            // The announcement is heard, not written: the coordinator reads
            // an answer's opening words for its look ("Let me look.") and
            // for the body's gesture, and they are the model's (#1075 on
            // lamp-52e6: "My voice sounds… Let me look." took no look).
            // Sentences come trimmed: each after the first is said after a
            // space, so the lamp's words read "just now. Hmm," (#1014).
            let answering = text != ANNOUNCEMENT;
            if answering {
                let words = ProviderOutput::Transcript {
                    request: Some(request),
                    text: if std::mem::replace(&mut after, true) {
                        format!(" {text}")
                    } else {
                        text
                    },
                    finished: false,
                };
                if !shared.emit(words).await {
                    return Err(Stop::Gone);
                }
            }
            while let Some(packet) = packets.recv().await {
                let samples = packet.map_err(|error| Stop::Failed(fault(&error)))?;
                for part in samples.chunks(PACKET_SAMPLES) {
                    let audio = ProviderOutput::Audio {
                        request,
                        sequence: shared.sequence.fetch_add(1, Ordering::Relaxed) + 1,
                        provider_event_at_us: monotonic_us(),
                        source_frames: part.len(),
                        source_offset: 0,
                        samples: part.to_vec(),
                    };
                    if !shared.emit(audio).await {
                        return Err(Stop::Gone);
                    }
                    spoke.store(true, Ordering::Relaxed);
                    // The answer's, never the announcement's or a cue's.
                    if answering && !answered.swap(true, Ordering::Relaxed) {
                        if let Ok(mut last) = shared.answered_request.lock() {
                            *last = Some(request);
                        }
                        // Some of its answer heard: no longer to be carried.
                        if let Ok(mut in_flight) = shared.in_flight.lock() {
                            in_flight.take_if(|carried| carried.request == request);
                        }
                        // An answer to someone, or the lamp's own turn or a
                        // reminder, which is no answer.
                        room(|room| {
                            if asked == Asker::Spoken {
                                room.spoke(request, monotonic_us());
                            } else {
                                room.spoke_unasked(request, monotonic_us());
                            }
                        });
                        // Its own turn heard: news someone may reply to.
                        if asked != Asker::Spoken
                            && let Some(did) = &shared.did
                        {
                            did(Did::Spoke(monotonic_us()));
                        }
                        reached(&times, |times| &mut times.audio);
                        eprintln!(
                            "lamp-voice second: request {request} first answer audio {} ms after the words were heard",
                            began.elapsed().as_millis()
                        );
                    }
                    if let Ok(mut at) = shared.answered_at.lock() {
                        *at = Some((shared.clock)());
                    }
                }
            }
        }
        // The lamp's words for the turn are over, said as the Live route
        // says its own are over: a last piece, empty and finished. Without
        // it, what reads the lamp's words line by line (the session's
        // records, `voice/said`) never had a line from this route (the
        // first builder, 19 Oct, lamp-4ace).
        if after {
            let done = ProviderOutput::Transcript {
                request: Some(request),
                text: String::new(),
                finished: true,
            };
            if !shared.emit(done).await {
                return Err(Stop::Gone);
            }
        }
        Ok(())
    };
    let ended = tokio::try_join!(writing, saying);
    let gave_up = matches!(ended, Err(Stop::GaveUp));
    let last = match ended {
        Ok(_) => {
            if !shared
                .emit(ProviderOutput::GenerationComplete { request })
                .await
            {
                return;
            }
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            }
        }
        Err(Stop::Silent | Stop::GaveUp) => ProviderOutput::TurnComplete {
            request,
            idle: true,
        },
        Err(Stop::Failed(reason)) => ProviderOutput::TurnFailed {
            request,
            stage: if spoke.load(Ordering::Relaxed) {
                FailedStage::Responding
            } else {
                FailedStage::AwaitingResponse
            },
            reason,
        },
        Err(Stop::Gone) => return,
    };
    // Its turn over: nothing of it is carried.
    if let Ok(mut in_flight) = shared.in_flight.lock() {
        in_flight.take_if(|carried| carried.request == request);
    }
    // A spoken line the turn ended without answering: what the model left
    // alone, the busy room's talk. Not one the route gave up itself, its
    // chat not begun in time: that was said to the lamp, and held as the
    // rule's holds are (#1712, the first builder at 16:04: on lamp-4ace on
    // 19 October the owner's lines without the name, just after the lamp
    // had answered them, were held in the busy room as no conversation).
    if asked == Asker::Spoken
        && matches!(last, ProviderOutput::TurnComplete { .. })
        && !answered.load(Ordering::Relaxed)
    {
        room(|room| {
            if gave_up {
                room.held(request);
            }
            room.unanswered(request, monotonic_us());
        });
    }
    if !shared.stages(request, &times).await {
        return;
    }
    shared.emit(last).await;
    // Rendered after a turn is out, never beside its chat and speech on a
    // slow relay (Nine on #1195).
    if let Some(down) = &shared.down {
        down.render(&shared.services);
    }
}

type Packets = mpsc::Receiver<net::Result<Vec<i16>>>;

/// Speech of `text` downloading into its own queue, in the turn's set so
/// that it stops with the turn.
fn start_speech(shared: &Shared, speech: &mut JoinSet<()>, text: String) -> Packets {
    let (packets, queued) = mpsc::channel(SENTENCE_PACKETS);
    let mut stream = shared.services.speak(text);
    speech.spawn(async move {
        while let Some(packet) = stream.next().await {
            let failed = packet.is_err();
            if packets.send(packet).await.is_err() || failed {
                break;
            }
        }
    });
    queued
}

/// Sentences on their way to being said, each with its speech started.
struct Queue<'a> {
    shared: &'a Shared,
    speech: &'a mut JoinSet<()>,
    /// Said before the first sentence of the answer, if due.
    announcement: Option<Packets>,
    sentences: mpsc::Sender<(String, Packets)>,
    /// Some of the answer itself is on its way.
    started: bool,
    /// Whose, and when its words were heard: for the journal's line when
    /// the first piece goes to the voice (#1712, the first builder at 17:27:
    /// the chat's first clause in about 0.9 s and the voice's first audio in
    /// about 0.3 s, but the first answer audio 1.4 to 3.9 s after the words).
    request: u64,
    heard_at: Instant,
}
impl Queue<'_> {
    async fn announce(&mut self) -> Result<(), Stop> {
        match self.announcement.take() {
            Some(packets) => self.send(ANNOUNCEMENT.to_owned(), packets).await,
            None => Ok(()),
        }
    }

    async fn push(&mut self, text: String) -> Result<(), Stop> {
        // A "sentence" with no word in it is not said: as the first words
        // of an answer, the coordinator would take it for silence.
        if !text.chars().any(char::is_alphanumeric) {
            return Ok(());
        }
        // A function call written as words is never said: `not_for_me{}`
        // is the call, so the turn ends in silence as for the real one; any
        // other is left out, and not run (its arguments are not read).
        if crate::not_speech::bare_call(&text) {
            if text.trim_start().starts_with(NOT_FOR_ME) {
                return Err(Stop::Silent);
            }
            return Ok(());
        }
        self.announce().await?;
        if !self.started {
            eprintln!(
                "lamp-voice second: request {} first piece to the voice {} ms after the words were heard, {} words",
                self.request,
                self.heard_at.elapsed().as_millis(),
                text.split_whitespace().count()
            );
        }
        self.started = true;
        let packets = start_speech(self.shared, self.speech, text.clone());
        self.send(text, packets).await
    }

    async fn send(&mut self, text: String, packets: Packets) -> Result<(), Stop> {
        // Closed only when the saying half stopped; its error is the one kept.
        self.sentences
            .send((text, packets))
            .await
            .map_err(|_| Stop::Gone)
    }
}
