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
mod services;
pub mod worker;

pub use services::{Listener, Net, Services};

#[cfg(test)]
mod tests;

/// Said once before the first answer after a quiet spell, so the person
/// knows the different voice is the lamp's.
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
/// The system message for a turn, built when the turn's chat begins, and
/// whose turn it is.
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

/// What every turn task shares.
struct Shared {
    services: Arc<dyn Services>,
    tools: ToolRunner,
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
    /// The words last heard, with their request: a request that goes on
    /// from one given up after an early end begins with them.
    heard_last: Mutex<Option<(u64, String)>>,
    /// The stop to be told (`after_stop`), with the request it is for.
    stop_untold: Mutex<Option<StopToTell>>,
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
}
impl Drop for Turn {
    /// A turn given up stops its chat and speech at once.
    fn drop(&mut self) {
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
                system,
                clock,
                answered_at: Mutex::new(None),
                sequence: AtomicU64::new(0),
                out,
                heed: None,
                asked: None,
                cues: None,
                down: None,
                heard_last: Mutex::new(None),
                stop_untold: Mutex::new(None),
            }),
            output,
            urgent: VecDeque::new(),
            turn: None,
            retired_through: 0,
            begun_last: None,
            sounds_checked: false,
            look: None,
        }
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

    /// A spoken turn's words are judged with `heed` before they are answered.
    /// Before any turn only: the turns share it.
    pub fn heeding(mut self, heed: Heed) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.heed = Some(heed);
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
                ..
            } => {
                let previous = self.begun_last;
                if !self.begin(request, session) {
                    return;
                }
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
                let before = last
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
                let task = tokio::spawn(spoken(
                    self.shared.clone(),
                    request,
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
                if let Ok(mut said) = turn.said.lock() {
                    if said.0 == 0 {
                        said.0 = read_completed_at_us;
                    }
                    said.1 = read_completed_at_us;
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
                if let Some(turn) = self.turn.as_mut().filter(|turn| turn.request == request) {
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
                let picture = look
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
                let shared = self.shared.clone();
                let task = tokio::spawn(async move {
                    answer(&shared, request, text, false, looking, picture, None).await
                });
                self.turn = Some(Turn {
                    request,
                    audio: None,
                    sound: Arc::default(),
                    said: Arc::default(),
                    task,
                });
            }
            ProviderInput::Reminder {
                request, session, ..
            } => self.urgent.push_back(ProviderOutput::ReminderDeferred {
                request,
                session,
                reason: ReminderDeferral::Disabled,
            }),
        }
    }

    /// A new turn replaces any older one. False for a retired request.
    fn begin(&mut self, request: u64, session: u64) -> bool {
        if request <= self.retired_through {
            return false;
        }
        self.turn = None;
        self.begun_last = Some((request, session));
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
        ProviderOutput::Ready { .. }
        | ProviderOutput::TurnFailed { .. }
        | ProviderOutput::ReminderDeferred { .. }
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
    request: u64,
    mut heard: mpsc::Receiver<Vec<i16>>,
    checked: Option<Arc<AtomicBool>>,
    said: Arc<Mutex<(u64, u64)>>,
    before: Option<String>,
    continues_from: Option<u64>,
) {
    let listener = match shared.services.listen().await {
        Ok(listener) => listener,
        Err(error) => return shared.fail(request, FailedStage::InputOpen, &error).await,
    };
    while let Some(samples) = heard.recv().await {
        if let Err(error) = listener.send(&samples) {
            return shared.fail(request, FailedStage::InputOpen, &error).await;
        }
    }
    let finishing = Instant::now();
    let text = match listener.finish(request).await {
        Ok(text) => {
            eprintln!(
                "lamp-voice second: request {request} heard {} words, the last {} ms after the line ended",
                text.split_whitespace().count(),
                finishing.elapsed().as_millis()
            );
            text
        }
        Err(error) => {
            eprintln!("lamp-voice second: request {request} speech to text failed: {error}");
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
    if empty || held || heeded.is_some() {
        shared
            .emit(ProviderOutput::TurnComplete {
                request,
                idle: true,
            })
            .await;
        return;
    }
    // The first request asked after a hand or the head stopped the lamp: the
    // model is told so, as Gemini is (`STOPPED_NOTE`, #1148), so the story
    // that was stopped is not begun again.
    let note = shared.stop_untold.lock().ok().and_then(|mut untold| {
        let stop = untold
            .as_mut()
            .filter(|stop| stop.request == request && !stop.asked)?;
        stop.asked = true;
        Some(crate::provider_worker::STOPPED_NOTE)
    });
    answer(&shared, request, text, true, false, None, note).await;
}

/// How an answer stopped short.
enum Stop {
    /// `not_for_me`: nothing more is said.
    Silent,
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
async fn answer(
    shared: &Shared,
    request: u64,
    said: String,
    heard: bool,
    looking: bool,
    picture: Option<Vec<u8>>,
    note: Option<&'static str>,
) {
    // Started now, with the chat request, so it is ready first.
    let mut speech = JoinSet::new();
    let announcement = shared
        .announcement_due()
        .then(|| start_speech(shared, &mut speech, ANNOUNCEMENT.to_owned()));
    let (sentences, mut queued) = mpsc::channel(SENTENCES_AHEAD);
    let spoke = AtomicBool::new(false);
    let mut queue = Queue {
        shared,
        speech: &mut speech,
        announcement,
        sentences,
        started: false,
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
    let asked = match picture {
        Some(jpeg) => Message::user_seeing(said, &jpeg),
        None => Message::user(said),
    };
    let writing = async move {
        let told = (shared.system)();
        let mut messages = vec![Message::system(told.text)];
        messages.extend(told.history);
        // A note beside the request, never anyone's words (as on the Live
        // route, where a note goes beside what the model hears).
        messages.extend(note.map(Message::system));
        messages.push(asked);
        let asked_at = messages.len() - 1;
        let mut round = 0;
        while round < MAX_ROUNDS {
            round += 1;
            let mut refused = false;
            let mut stream = shared.services.chat(messages.clone());
            let mut first = true;
            let mut split = Sentences::default();
            let mut text = String::new();
            let mut calls = Vec::new();
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
                            return Err(Stop::Silent);
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
                            sentences.extend(split.clause());
                        }
                        for sentence in sentences {
                            queue.push(sentence).await?;
                        }
                    }
                    ChatEvent::ToolCalls(asked) => calls = asked,
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
            if let Some(rest) = split.finish() {
                queue.push(rest).await?;
            }
            if calls.is_empty() {
                return Ok(());
            }
            if std::mem::take(&mut seeing) {
                messages[asked_at] = words.clone();
            }
            messages.push(Message::assistant_calls(Some(text), &calls));
            for call in &calls {
                let result =
                    (shared.tools)(request, heard, call.name.clone(), call.args.clone()).await;
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
            if text != ANNOUNCEMENT {
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
                    if !spoke.swap(true, Ordering::Relaxed) {
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
        Ok(())
    };
    let last = match tokio::try_join!(writing, saying) {
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
        Err(Stop::Silent) => ProviderOutput::TurnComplete {
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
